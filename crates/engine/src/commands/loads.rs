//! Loads: patterns, cases and combinations, and the loads put on nodes, frames and slabs.
//!
//! Patterns, cases and combinations are named by id (`P0:1`, `C0:1`, `K0:1`) or by name. A
//! load command puts one load on each element listed; by default it first removes the loads
//! the same pattern already has there, so running it again changes the load instead of
//! doubling it.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_model::{
    Area, AreaLoad, AreaRole, Change, Combination, CombinationKind, Frame, FrameLoad, FrameLoadShape, Id, LoadCase, LoadDirection, LoadPattern,
    LoadsOn, Model, Node, NodeLoad, Op, PatternId, PatternKind, SlabSpan, Table,
};

use super::{Ref, check_batch, ids, ids_schema, parse};
use crate::ids::{ElementRef, format_id, parse_id};
use crate::{CommandError, Session, Tx};

pub const PATTERN_ADD_SUMMARY: &str =
    "Add a load pattern (a source of load such as dead, live or wind), by default with a load case of the same name that applies it once.";
pub const CASE_SET_SUMMARY: &str = "Add a linear static load case, or replace one: the sum of load patterns, each times a factor.";
pub const COMBO_SET_SUMMARY: &str = "Add a load combination, or replace one: load cases times factors, summed (linear) or enveloped.";
pub const DELETE_SUMMARY: &str = "Delete load patterns, cases and combinations; deleting a pattern deletes its loads and takes it out of cases, deleting a case takes it out of combinations.";
pub const LIST_SUMMARY: &str = "List load patterns, cases, combinations, or the loads on nodes, frames or areas, a page at a time.";
pub const NODE_SUMMARY: &str = "Put forces (N) and moments (N·m) along the global axes on nodes, in one load pattern.";
pub const FRAME_SUMMARY: &str =
    "Put a distributed (N/m) or point (N) load on frames, in one load pattern; positions are fractions of the length from end i.";
pub const AREA_SUMMARY: &str = "Put a uniform downward pressure (Pa) on slabs, in one load pattern; the slab hands it to the frames along its edges.";

const PAGE_LIMIT: usize = 10_000;

fn mode_schema() -> Value {
    json!({"enum": ["replace", "add", "delete"], "description": "replace (default): remove this pattern's loads on these elements first; add: keep them; delete: only remove them"})
}

fn named_schema(what: &str) -> Value {
    json!({"type": "string", "description": format!("{what} id or name")})
}

/// A pattern, case or combination by id or, failing that, by name.
fn find<T: ElementRef>(table: &Table<T>, r: &Ref, name: impl Fn(&T) -> &str, what: &str) -> Result<Id<T>, CommandError> {
    if let Some(id) = parse_id::<T>(r.as_str()).filter(|id| table.contains(*id)) {
        return Ok(id);
    }
    table
        .iter()
        .find(|(_, row)| name(row) == r.as_str())
        .map(|(id, _)| id)
        .ok_or_else(|| CommandError::InvalidInput(format!("no {what} `{}`", r.as_str())))
}

fn pattern(m: &Model, r: &Ref) -> Result<PatternId, CommandError> {
    find(m.patterns(), r, |p| &p.name, "load pattern")
}

fn case(m: &Model, r: &Ref) -> Result<Id<LoadCase>, CommandError> {
    find(m.cases(), r, |c| &c.name, "load case")
}

fn combination(m: &Model, r: &Ref) -> Result<Id<Combination>, CommandError> {
    find(m.combinations(), r, |c| &c.name, "load combination")
}

/// Refuse a name another row of the table has (`except` is the row being replaced).
fn unique<T>(table: &Table<T>, name: &str, except: Option<Id<T>>, of: impl Fn(&T) -> &str, what: &str) -> Result<(), CommandError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CommandError::InvalidInput(format!("a {what} needs a name")));
    }
    match table.iter().find(|(id, row)| Some(*id) != except && of(row).trim().eq_ignore_ascii_case(name)) {
        Some(_) => Err(CommandError::InvalidInput(format!("there is already a {what} named `{name}`"))),
        None => Ok(()),
    }
}

/// Replace the row `id` names, or add `row` through `add` when there is none.
fn add_or_set<'a, T>(
    tx: &mut Tx<'a>,
    id: Option<Id<T>>,
    row: T,
    change: fn(Op<T>) -> Change,
    add: impl FnOnce(&mut Tx<'a>, Vec<T>) -> Result<Vec<Id<T>>, structurecraft_model::ModelError>,
) -> Result<Id<T>, CommandError> {
    if let Some(id) = id {
        tx.apply(change(Op::Set(vec![(id, row)])))?;
        return Ok(id);
    }
    add(tx, vec![row])?.first().copied().ok_or_else(|| CommandError::InvalidInput("lost an id".to_owned()))
}

pub fn pattern_add_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "kind": {"enum": ["dead", "superimposed_dead", "live", "roof_live", "snow", "wind", "seismic", "other"], "description": "Default: other"},
            "self_weight": {"type": "number", "description": "Times the structure's own weight this pattern adds; default 0"},
            "case": {"type": "boolean", "description": "Also add a load case of the same name that applies this pattern once; default true"},
        },
        "required": ["name"],
        "additionalProperties": false,
    })
}

pub fn pattern_add(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        name: String,
        #[serde(default = "other")]
        kind: PatternKind,
        #[serde(default)]
        self_weight: f64,
        #[serde(default = "yes")]
        case: bool,
    }
    fn other() -> PatternKind {
        PatternKind::Other
    }
    fn yes() -> bool {
        true
    }
    let Input { name, kind, self_weight, case } = parse(input)?;
    let name = name.trim().to_owned();
    unique(session.model.patterns(), &name, None, |p| &p.name, "load pattern")?;
    let with_case = case && unique(session.model.cases(), &name, None, |c| &c.name, "load case").is_ok();
    session.edit("Add load pattern", |tx| {
        let p = add_or_set(tx, None, LoadPattern { name: name.clone(), kind, self_weight }, Change::Patterns, Tx::add_patterns)?;
        let c = if with_case { Some(add_or_set(tx, None, LoadCase { name, patterns: vec![(p, 1.0)] }, Change::Cases, Tx::add_cases)?) } else { None };
        Ok(json!({"pattern": format_id(p), "case": c.map(format_id)}))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Term {
    #[serde(alias = "case")]
    pattern: Ref,
    factor: f64,
}

fn terms_schema(of: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "object", "properties": {of: named_schema(of), "factor": {"type": "number"}}, "required": [of, "factor"], "additionalProperties": false},
    })
}

pub fn case_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "case": named_schema("Load case to replace; omit to add a new one"),
            "name": {"type": "string"},
            "patterns": terms_schema("pattern"),
        },
        "required": ["name", "patterns"],
        "additionalProperties": false,
    })
}

pub fn case_set(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        case: Option<Ref>,
        name: String,
        patterns: Vec<Term>,
    }
    let Input { case: replace, name, patterns } = parse(input)?;
    let m = &session.model;
    let replace = replace.as_ref().map(|r| case(m, r)).transpose()?;
    unique(m.cases(), &name, replace, |c| &c.name, "load case")?;
    unique(m.combinations(), &name, None, |c| &c.name, "load combination")?;
    let patterns = patterns.iter().map(|t| Ok((pattern(m, &t.pattern)?, t.factor))).collect::<Result<Vec<_>, CommandError>>()?;
    let row = LoadCase { name: name.trim().to_owned(), patterns };
    session.edit(if replace.is_some() { "Change load case" } else { "Add load case" }, |tx| {
        Ok(json!({"case": format_id(add_or_set(tx, replace, row, Change::Cases, Tx::add_cases)?)}))
    })
}

pub fn combo_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "combination": named_schema("Load combination to replace; omit to add a new one"),
            "name": {"type": "string"},
            "kind": {"enum": ["linear", "envelope"], "description": "Default: linear"},
            "cases": terms_schema("case"),
        },
        "required": ["name", "cases"],
        "additionalProperties": false,
    })
}

pub fn combo_set(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        combination: Option<Ref>,
        name: String,
        #[serde(default)]
        kind: CombinationKind,
        cases: Vec<Term>,
    }
    let Input { combination: replace, name, kind, cases } = parse(input)?;
    let m = &session.model;
    let replace = replace.as_ref().map(|r| combination(m, r)).transpose()?;
    unique(m.combinations(), &name, replace, |c| &c.name, "load combination")?;
    unique(m.cases(), &name, None, |c| &c.name, "load case")?;
    let cases = cases.iter().map(|t| Ok((case(m, &t.pattern)?, t.factor))).collect::<Result<Vec<_>, CommandError>>()?;
    let row = Combination { name: name.trim().to_owned(), kind, cases };
    session.edit(if replace.is_some() { "Change load combination" } else { "Add load combination" }, |tx| {
        Ok(json!({"combination": format_id(add_or_set(tx, replace, row, Change::Combinations, Tx::add_combinations)?)}))
    })
}

pub fn delete_schema() -> Value {
    let list = |what: &str| json!({"type": "array", "items": named_schema(what)});
    json!({
        "type": "object",
        "properties": {"patterns": list("Load pattern"), "cases": list("Load case"), "combinations": list("Load combination")},
        "additionalProperties": false,
    })
}

pub fn delete(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        patterns: Vec<Ref>,
        #[serde(default)]
        cases: Vec<Ref>,
        #[serde(default)]
        combinations: Vec<Ref>,
    }
    let input: Input = parse(input)?;
    let m = &session.model;
    let mut patterns = input.patterns.iter().map(|r| pattern(m, r)).collect::<Result<Vec<_>, _>>()?;
    let mut cases = input.cases.iter().map(|r| case(m, r)).collect::<Result<Vec<_>, _>>()?;
    let mut combos = input.combinations.iter().map(|r| combination(m, r)).collect::<Result<Vec<_>, _>>()?;
    patterns.sort_unstable();
    patterns.dedup();
    cases.sort_unstable();
    cases.dedup();
    combos.sort_unstable();
    combos.dedup();
    // Loads of the deleted patterns, and the cases and combinations that keep the rest.
    let node_loads: Vec<_> = m.node_loads().iter().filter(|(_, l)| patterns.contains(&l.pattern)).map(|(id, _)| id).collect();
    let frame_loads: Vec<_> = m.frame_loads().iter().filter(|(_, l)| patterns.contains(&l.pattern)).map(|(id, _)| id).collect();
    let area_loads: Vec<_> = m.area_loads().iter().filter(|(_, l)| patterns.contains(&l.pattern)).map(|(id, _)| id).collect();
    let kept_cases: Vec<_> = m
        .cases()
        .iter()
        .filter(|(id, c)| !cases.contains(id) && c.patterns.iter().any(|(p, _)| patterns.contains(p)))
        .map(|(id, c)| (id, LoadCase { name: c.name.clone(), patterns: c.patterns.iter().copied().filter(|(p, _)| !patterns.contains(p)).collect() }))
        .collect();
    let kept_combos: Vec<_> = m
        .combinations()
        .iter()
        .filter(|(id, c)| !combos.contains(id) && c.cases.iter().any(|(k, _)| cases.contains(k)))
        .map(|(id, c)| (id, Combination { cases: c.cases.iter().copied().filter(|(k, _)| !cases.contains(k)).collect(), ..c.clone() }))
        .collect();
    let counts = json!({
        "patterns": patterns.len(), "cases": cases.len(), "combinations": combos.len(),
        "loads": node_loads.len() + frame_loads.len() + area_loads.len(),
    });
    session.edit("Delete loads", |tx| {
        let steps = [
            (!combos.is_empty()).then(|| Change::Combinations(Op::Remove(combos))),
            (!kept_combos.is_empty()).then(|| Change::Combinations(Op::Set(kept_combos))),
            (!cases.is_empty()).then(|| Change::Cases(Op::Remove(cases))),
            (!kept_cases.is_empty()).then(|| Change::Cases(Op::Set(kept_cases))),
            (!node_loads.is_empty()).then(|| Change::NodeLoads(Op::Remove(node_loads))),
            (!frame_loads.is_empty()).then(|| Change::FrameLoads(Op::Remove(frame_loads))),
            (!area_loads.is_empty()).then(|| Change::AreaLoads(Op::Remove(area_loads))),
            (!patterns.is_empty()).then(|| Change::Patterns(Op::Remove(patterns))),
        ];
        for change in steps.into_iter().flatten() {
            tx.apply(change)?;
        }
        Ok(json!({"deleted": counts}))
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    Replace,
    Add,
    Delete,
}

/// Remove `existing` unless adding, then add `rows` unless deleting.
fn assign(tx: &mut Tx<'_>, mode: Mode, existing: LoadsOn, rows: Change) -> Result<Value, CommandError> {
    let removed = existing.node_loads.len() + existing.frame_loads.len() + existing.area_loads.len();
    if mode != Mode::Add {
        let LoadsOn { node_loads, frame_loads, area_loads } = existing;
        for change in [
            (!node_loads.is_empty()).then(|| Change::NodeLoads(Op::Remove(node_loads))),
            (!frame_loads.is_empty()).then(|| Change::FrameLoads(Op::Remove(frame_loads))),
            (!area_loads.is_empty()).then(|| Change::AreaLoads(Op::Remove(area_loads))),
        ]
        .into_iter()
        .flatten()
        {
            tx.apply(change)?;
        }
    }
    let added = match (&rows, mode) {
        (_, Mode::Delete) => 0,
        (Change::NodeLoads(Op::Add(r)), _) => r.len(),
        (Change::FrameLoads(Op::Add(r)), _) => r.len(),
        (Change::AreaLoads(Op::Add(r)), _) => r.len(),
        _ => 0,
    };
    if added > 0 {
        tx.apply(rows)?;
    }
    Ok(json!({"added": added, "removed": if mode == Mode::Add { 0 } else { removed }}))
}

fn label(mode: Mode) -> &'static str {
    if mode == Mode::Delete { "Delete loads" } else { "Assign loads" }
}

pub fn node_schema() -> Value {
    let triple = |d: &str| json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": d});
    json!({
        "type": "object",
        "properties": {
            "nodes": ids_schema("node"),
            "pattern": named_schema("Load pattern"),
            "force": triple("[Fx, Fy, Fz] in N; default zero"),
            "moment": triple("[Mx, My, Mz] in N·m; default zero"),
            "mode": mode_schema(),
        },
        "required": ["nodes", "pattern"],
        "additionalProperties": false,
    })
}

pub fn node(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        nodes: Vec<Ref>,
        pattern: Ref,
        #[serde(default)]
        force: [f64; 3],
        #[serde(default)]
        moment: [f64; 3],
        #[serde(default)]
        mode: Mode,
    }
    let Input { nodes, pattern: p, force, moment, mode } = parse(input)?;
    check_batch(nodes.len())?;
    let nodes = ids::<Node>(&nodes)?;
    let p = pattern(&session.model, &p)?;
    let [fx, fy, fz] = force;
    let [mx, my, mz] = moment;
    let values = [fx, fy, fz, mx, my, mz];
    let existing = session.model.loads_on(&nodes, &[], &[], Some(p));
    let rows = nodes.iter().map(|node| NodeLoad { node: *node, pattern: p, values }).collect();
    session.edit(label(mode), |tx| assign(tx, mode, existing, Change::NodeLoads(Op::Add(rows))))
}

pub fn frame_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "frames": ids_schema("frame"),
            "pattern": named_schema("Load pattern"),
            "shape": {"enum": ["distributed", "point"], "description": "Default: distributed"},
            "direction": {"enum": ["gravity", "x", "y", "z", "local1", "local2", "local3"], "description": "Gravity (default) acts down: a positive value pushes along −Z"},
            "value": {"type": "number", "description": "N/m for a distributed load (at start), N for a point load"},
            "value_end": {"type": "number", "description": "N/m at the end of a distributed load; default: value (uniform)"},
            "start": {"type": "number", "minimum": 0, "maximum": 1, "description": "Fraction of the length from end i; default 0 (for a point load: where it acts, default 0.5)"},
            "end": {"type": "number", "minimum": 0, "maximum": 1, "description": "Default 1"},
            "mode": mode_schema(),
        },
        "required": ["frames", "pattern"],
        "additionalProperties": false,
    })
}

pub fn frame(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        frames: Vec<Ref>,
        pattern: Ref,
        #[serde(default)]
        shape: FrameLoadShape,
        #[serde(default)]
        direction: LoadDirection,
        #[serde(default)]
        value: f64,
        #[serde(default)]
        value_end: Option<f64>,
        #[serde(default)]
        start: Option<f64>,
        #[serde(default)]
        end: Option<f64>,
        #[serde(default)]
        mode: Mode,
    }
    let input: Input = parse(input)?;
    check_batch(input.frames.len())?;
    let frames = ids::<Frame>(&input.frames)?;
    let p = pattern(&session.model, &input.pattern)?;
    let point = input.shape == FrameLoadShape::Point;
    let load = FrameLoad {
        frame: Id::default(),
        pattern: p,
        shape: input.shape,
        direction: input.direction,
        start: input.start.unwrap_or(if point { 0.5 } else { 0.0 }),
        end: if point { input.start.unwrap_or(0.5) } else { input.end.unwrap_or(1.0) },
        value_start: input.value,
        value_end: if point { input.value } else { input.value_end.unwrap_or(input.value) },
    };
    let existing = session.model.loads_on(&[], &frames, &[], Some(p));
    let rows = frames.iter().map(|frame| FrameLoad { frame: *frame, ..load }).collect();
    session.edit(label(input.mode), |tx| assign(tx, input.mode, existing, Change::FrameLoads(Op::Add(rows))))
}

pub fn area_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "areas": ids_schema("area"),
            "pattern": named_schema("Load pattern"),
            "pressure": {"type": "number", "description": "Pa, acting down"},
            "span": {"enum": ["auto", "one_way", "two_way"], "description": "How the slab hands the load to its edge frames; auto (default): one-way when the long side is over twice the short"},
            "mode": mode_schema(),
        },
        "required": ["areas", "pattern"],
        "additionalProperties": false,
    })
}

pub fn area(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        areas: Vec<Ref>,
        pattern: Ref,
        #[serde(default)]
        pressure: f64,
        #[serde(default)]
        span: SlabSpan,
        #[serde(default)]
        mode: Mode,
    }
    let Input { areas, pattern: p, pressure, span, mode } = parse(input)?;
    check_batch(areas.len())?;
    let areas = ids::<Area>(&areas)?;
    let m = &session.model;
    let p = pattern(m, &p)?;
    if mode != Mode::Delete && areas.iter().any(|a| m.areas().get(*a).is_some_and(|a| a.role == AreaRole::Wall)) {
        return Err(CommandError::InvalidInput("area loads go on slabs, not walls".to_owned()));
    }
    let existing = m.loads_on(&[], &[], &areas, Some(p));
    let rows = areas.iter().map(|area| AreaLoad { area: *area, pattern: p, pressure, span }).collect();
    session.edit(label(mode), |tx| assign(tx, mode, existing, Change::AreaLoads(Op::Add(rows))))
}

pub fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {"enum": ["patterns", "cases", "combinations", "node_loads", "frame_loads", "area_loads"]},
            "pattern": named_schema("Only loads of this pattern"),
            "offset": {"type": "integer", "minimum": 0},
            "limit": {"type": "integer", "minimum": 1, "maximum": PAGE_LIMIT},
        },
        "required": ["kind"],
        "additionalProperties": false,
    })
}

pub fn list(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        kind: Kind,
        #[serde(default)]
        pattern: Option<Ref>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "page")]
        limit: usize,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Kind {
        Patterns,
        Cases,
        Combinations,
        NodeLoads,
        FrameLoads,
        AreaLoads,
    }
    fn page() -> usize {
        1000
    }
    let Input { kind, pattern: only, offset, limit } = parse(input)?;
    let limit = limit.clamp(1, PAGE_LIMIT);
    let m = &session.model;
    let only = only.as_ref().map(|r| pattern(m, r)).transpose()?;
    let of = |p: PatternId| only.is_none_or(|q| q == p);
    let name = |p: PatternId| m.patterns().get(p).map_or("", |p| p.name.as_str());
    let terms = |t: &[(PatternId, f64)]| t.iter().map(|(p, f)| json!({"pattern": name(*p), "factor": f})).collect::<Vec<_>>();
    fn page_of<'a, T: 'a>(rows: impl Iterator<Item = T>, offset: usize, limit: usize, f: impl Fn(T) -> Value) -> (usize, Vec<Value>) {
        let mut total = 0;
        let mut items = Vec::new();
        for row in rows {
            if total >= offset && items.len() < limit {
                items.push(f(row));
            }
            total += 1;
        }
        (total, items)
    }
    let (total, items) = match kind {
        Kind::Patterns => page_of(
            m.patterns().iter(),
            offset,
            limit,
            |(id, p)| json!({"id": format_id(id), "name": p.name, "kind": p.kind, "self_weight": p.self_weight}),
        ),
        Kind::Cases => {
            page_of(m.cases().iter(), offset, limit, |(id, c)| json!({"id": format_id(id), "name": c.name, "patterns": terms(&c.patterns)}))
        }
        Kind::Combinations => page_of(m.combinations().iter(), offset, limit, |(id, c)| {
            let cases: Vec<_> =
                c.cases.iter().map(|(k, f)| json!({"case": m.cases().get(*k).map_or("", |c| c.name.as_str()), "factor": f})).collect();
            json!({"id": format_id(id), "name": c.name, "kind": c.kind, "cases": cases})
        }),
        Kind::NodeLoads => page_of(m.node_loads().iter().filter(|(_, l)| of(l.pattern)), offset, limit, |(_, l)| {
            let [fx, fy, fz, mx, my, mz] = l.values;
            json!({"node": format_id(l.node), "pattern": name(l.pattern), "force": [fx, fy, fz], "moment": [mx, my, mz]})
        }),
        Kind::FrameLoads => page_of(m.frame_loads().iter().filter(|(_, l)| of(l.pattern)), offset, limit, |(_, l)| {
            json!({
                "frame": format_id(l.frame), "pattern": name(l.pattern), "shape": l.shape, "direction": l.direction,
                "start": l.start, "end": l.end, "value": l.value_start, "value_end": l.value_end,
            })
        }),
        Kind::AreaLoads => page_of(
            m.area_loads().iter().filter(|(_, l)| of(l.pattern)),
            offset,
            limit,
            |(_, l)| json!({"area": format_id(l.area), "pattern": name(l.pattern), "pressure": l.pressure, "span": l.span}),
        ),
    };
    Ok(json!({"total": total, "offset": offset, "items": items}))
}
