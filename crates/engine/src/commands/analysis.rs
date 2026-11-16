//! Running the linear static analysis and reading its results.
//!
//! Results belong to the model revision they were computed from: any edit makes them stale,
//! and `analysis.results` refuses stale results rather than report numbers for a model that is
//! gone.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_analysis::{Analysis, AnalysisError, CaseResult, DOF_NAMES};
use structurecraft_model::{CaseId, CombinationKind, Model};

use super::loads::{case, combination};
use super::{Ref, ids, ids_schema, parse};
use crate::ids::format_id;
use crate::{AnalysisState, CommandError, Session};

pub const RUN_SUMMARY: &str =
    "Run the linear static analysis of every load case: frames carry the load, slabs hand their loads to the frames on their edges.";
pub const RESULTS_SUMMARY: &str =
    "Read analysis results for a load case or combination: node displacements, support reactions or frame end forces, a page at a time.";

const PAGE_LIMIT: usize = 10_000;

/// Describe an analysis error with the ids commands use.
pub fn describe(m: &Model, e: AnalysisError) -> CommandError {
    let text = match e {
        AnalysisError::Frame { frame, reason } => format!("frame {}: {reason}", format_id(frame)),
        AnalysisError::Unresisted { node, dof } => {
            let what = DOF_NAMES.get(dof).copied().unwrap_or("?");
            let why = if m.frames().iter().any(|(_, f)| f.i == node || f.j == node) {
                "nothing there resists it (a released end or a pin?)"
            } else {
                "no frame reaches the node"
            };
            format!("node {} is loaded along {what} but {why}", format_id(node))
        }
        other => other.to_string(),
    };
    CommandError::Analysis(text)
}

pub fn run(session: &mut Session, _input: Value) -> Result<Value, CommandError> {
    let analysis = structurecraft_analysis::analyze(&session.model).map_err(|e| describe(&session.model, e))?;
    let summary = summary(&session.model, &analysis);
    let revision = session.revision();
    session.set_analysis(revision, analysis);
    Ok(summary)
}

fn summary(m: &Model, a: &Analysis) -> Value {
    let cases: Vec<Value> = a
        .cases
        .iter()
        .map(|c| {
            let mut largest = (0.0f64, None);
            for (id, _) in m.nodes().iter() {
                if let Some(u) = c.displacement(id) {
                    let d = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
                    if d > largest.0 {
                        largest = (d, Some(id));
                    }
                }
            }
            let mut reaction = [0.0; 3];
            for (id, _) in m.nodes().iter().filter(|(_, n)| n.restraint.0 != 0) {
                if let Some(r) = c.reaction(id) {
                    for k in 0..3 {
                        reaction[k] += r[k];
                    }
                }
            }
            json!({
                "case": format_id(c.case),
                "name": m.cases().get(c.case).map_or("", |c| c.name.as_str()),
                "max_displacement": largest.0,
                "max_displacement_node": largest.1.map(format_id),
                "reaction_total": reaction,
            })
        })
        .collect();
    json!({"cases": cases, "dofs": a.dofs, "held_dofs": a.held, "nonzeros": a.nonzeros})
}

pub fn results_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "of": {"type": "string", "description": "Load case or combination, by id (C0:1, K0:1) or name"},
            "kind": {"enum": ["displacements", "reactions", "frame_forces"], "description": "displacements: ux uy uz (m) rx ry rz (rad) along global axes; reactions: fx fy fz (N) mx my mz (N·m) at supports; frame_forces: P V2 V3 T M2 M3 at end i then end j (N, N·m), local axes, tension and sagging positive"},
            "nodes": ids_schema("node"),
            "frames": ids_schema("frame"),
            "offset": {"type": "integer", "minimum": 0},
            "limit": {"type": "integer", "minimum": 1, "maximum": PAGE_LIMIT},
        },
        "required": ["of", "kind"],
        "additionalProperties": false,
    })
}

/// Where the values come from: one case, or a combination of cases.
enum Source {
    Case(CaseId),
    Linear(Vec<(CaseId, f64)>),
    Envelope(Vec<(CaseId, f64)>),
}

impl Source {
    /// The values for one element, as JSON: `{"values": [...]}`, or `{"max", "min"}` for an
    /// envelope. `None` when the element has no results.
    fn values<const N: usize>(&self, a: &Analysis, get: impl Fn(&CaseResult) -> Option<[f64; N]>) -> Option<Value> {
        let of = |id: CaseId| a.case(id).and_then(&get);
        match self {
            Self::Case(id) => of(*id).map(|v| json!({"values": v.to_vec()})),
            Self::Linear(terms) => {
                let mut sum = [0.0; N];
                for (id, f) in terms {
                    let v = of(*id)?;
                    for k in 0..N {
                        sum[k] += f * v[k];
                    }
                }
                Some(json!({"values": sum.to_vec()}))
            }
            Self::Envelope(terms) => {
                let (mut max, mut min) = ([f64::NEG_INFINITY; N], [f64::INFINITY; N]);
                for (id, f) in terms {
                    let v = of(*id)?;
                    for k in 0..N {
                        max[k] = max[k].max(f * v[k]);
                        min[k] = min[k].min(f * v[k]);
                    }
                }
                if terms.is_empty() {
                    (max, min) = ([0.0; N], [0.0; N]);
                }
                Some(json!({"max": max.to_vec(), "min": min.to_vec()}))
            }
        }
    }
}

pub fn results(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        of: Ref,
        kind: Kind,
        #[serde(default)]
        nodes: Option<Vec<Ref>>,
        #[serde(default)]
        frames: Option<Vec<Ref>>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "page")]
        limit: usize,
    }
    #[derive(Deserialize, Clone, Copy)]
    #[serde(rename_all = "snake_case")]
    enum Kind {
        Displacements,
        Reactions,
        FrameForces,
    }
    fn page() -> usize {
        1000
    }
    let Input { of, kind, nodes, frames, offset, limit } = parse(input)?;
    let limit = limit.clamp(1, PAGE_LIMIT);
    let m = &session.model;
    let a = match session.analysis_state() {
        AnalysisState::Current => session.analysis().ok_or(CommandError::NothingTo("read"))?,
        AnalysisState::Stale => {
            return Err(CommandError::InvalidInput("the model changed since the last analysis; run analysis.run again".to_owned()));
        }
        AnalysisState::None => return Err(CommandError::InvalidInput("no analysis results yet; run analysis.run first".to_owned())),
    };
    let (source, header) = match case(m, &of) {
        Ok(id) => (Source::Case(id), json!({"case": format_id(id), "name": m.cases().get(id).map_or("", |c| c.name.as_str())})),
        Err(_) => {
            let id = combination(m, &of).map_err(|_| CommandError::InvalidInput(format!("no load case or combination `{}`", of.as_str())))?;
            let c = m.combinations().get(id).ok_or_else(|| CommandError::InvalidInput("the combination is gone".to_owned()))?;
            let source = match c.kind {
                CombinationKind::Linear => Source::Linear(c.cases.clone()),
                CombinationKind::Envelope => Source::Envelope(c.cases.clone()),
            };
            (source, json!({"combination": format_id(id), "name": c.name, "kind": c.kind}))
        }
    };

    let mut total = 0;
    let mut items = Vec::new();
    let mut push = |item: Option<Value>| {
        if let Some(item) = item {
            if total >= offset && items.len() < limit {
                items.push(item);
            }
            total += 1;
        }
    };
    let with = |key: &str, id: String, v: Option<Value>| {
        v.map(|mut v| {
            if let Some(o) = v.as_object_mut() {
                o.insert(key.to_owned(), Value::String(id));
            }
            v
        })
    };
    let order: Vec<String> = match kind {
        Kind::Displacements | Kind::Reactions => {
            let reactions = matches!(kind, Kind::Reactions);
            let wanted = match &nodes {
                Some(refs) => {
                    let list = ids(refs)?;
                    if let Some(bad) = list.iter().find(|id| !m.nodes().contains(**id)) {
                        return Err(CommandError::InvalidInput(format!("no node {}", format_id(*bad))));
                    }
                    list
                }
                None => m.nodes().ids().filter(|id| if reactions { a.is_supported(*id) } else { a.has_node(*id) }).collect(),
            };
            for id in wanted {
                let v = if reactions {
                    a.is_supported(id).then(|| source.values(a, |c| c.reaction(id))).flatten()
                } else {
                    a.has_node(id).then(|| source.values(a, |c| c.displacement(id))).flatten()
                };
                push(with("node", format_id(id), v));
            }
            let names = if reactions { ["fx", "fy", "fz", "mx", "my", "mz"] } else { DOF_NAMES };
            names.iter().map(|s| (*s).to_owned()).collect()
        }
        Kind::FrameForces => {
            let wanted = match &frames {
                Some(refs) => {
                    let list = ids(refs)?;
                    if let Some(bad) = list.iter().find(|id| !m.frames().contains(**id)) {
                        return Err(CommandError::InvalidInput(format!("no frame {}", format_id(*bad))));
                    }
                    list
                }
                None => m.frames().ids().collect(),
            };
            for id in wanted {
                push(with("frame", format_id(id), source.values(a, |c| c.frame_forces(id))));
            }
            ["i", "j"].iter().flat_map(|end| ["p", "v2", "v3", "t", "m2", "m3"].map(|f| format!("{f}_{end}"))).collect()
        }
    };
    let mut out = header;
    if let Some(o) = out.as_object_mut() {
        o.insert("order".to_owned(), json!(order));
        o.insert("total".to_owned(), json!(total));
        o.insert("offset".to_owned(), json!(offset));
        o.insert("items".to_owned(), Value::Array(items));
    }
    Ok(out)
}
