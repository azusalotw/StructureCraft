//! Model files (L3): `.stc`.
//!
//! A file is a header and a list of chunks, each a four-byte tag, a byte length and a body, so
//! a reader skips chunks it does not know and later versions can add some. The small parts
//! (name, units, grid, materials, sections, load patterns, cases and combinations) are JSON;
//! nodes, frames, areas and the loads on them are fixed-width little-endian records, written
//! and read in one pass without building a tree, so models of hundreds of thousands of
//! elements open in well under a second.
//!
//! Version 2 added frame end releases (FREL, only the frames that have some) and loads (LPAT,
//! LCAS, COMB, NLOD, FLOD, ALOD). A version 1 file opens with no releases and the default Dead
//! and Live patterns.
//!
//! Every table is stored slot by slot, free slots included, so element ids survive a save and
//! open. Reading treats the bytes as hostile: every length is checked before it is trusted, and
//! the model checks every reference before it is accepted.

mod bytes;

use serde::{Deserialize, Serialize};
use structurecraft_geom::DVec3;
use structurecraft_model::{
    Area, AreaLoad, AreaRole, Combination, CombinationKind, Corners, Frame, FrameLoad, FrameLoadShape, FrameRole, Grid, Id, LoadCase, LoadDirection,
    LoadPattern, Material, Model, ModelError, Node, NodeLoad, Parts, Releases, Restraint, Section, SectionShape, SlabSpan, Table, Units,
};

use bytes::{Reader, Writer};

/// The file extension of a StructureCraft model.
pub const EXTENSION: &str = "stc";

/// The newest format version this build writes. Older versions still read.
pub const FORMAT_VERSION: u32 = 2;

const MAGIC: &[u8; 8] = b"STRCRAFT";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a StructureCraft model")]
    WrongMagic,
    #[error("made by a newer StructureCraft (format {0}, this build reads up to {FORMAT_VERSION})")]
    TooNew(u32),
    #[error("the file is damaged: {0}")]
    Damaged(String),
    #[error("the file is missing its {0} chunk")]
    MissingChunk(&'static str),
    #[error("the file's model is inconsistent: {0}")]
    Model(#[from] ModelError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Serialize, Deserialize)]
struct Meta {
    name: String,
    display_units: Units,
    grid: Grid,
}

#[derive(Serialize, Deserialize)]
struct SectionRecord {
    name: String,
    material: [u32; 2],
    #[serde(flatten)]
    shape: SectionShape,
}

#[derive(Serialize, Deserialize)]
struct CaseRecord {
    name: String,
    patterns: Vec<([u32; 2], f64)>,
}

#[derive(Serialize, Deserialize)]
struct CombinationRecord {
    name: String,
    kind: CombinationKind,
    cases: Vec<([u32; 2], f64)>,
}

/// Record sizes, for the reader's count checks.
const NODE_LOAD_RECORD: usize = 4 + 8 + 8 + 6 * 8;
const FRAME_LOAD_RECORD: usize = 4 + 8 + 8 + 2 + 4 * 8;
const AREA_LOAD_RECORD: usize = 4 + 8 + 8 + 8 + 1;

/// A table's slots as JSON: `[generation, row or null]` each.
fn json_slots<T, R: Serialize>(table: &Table<T>, record: impl Fn(&T) -> R) -> Vec<(u32, Option<R>)> {
    table.slots().map(|(g, row)| (g, (g & 1 == 1).then(|| record(row)))).collect()
}

fn from_json_slots<T: Default, R: for<'de> Deserialize<'de>>(bytes: &[u8], row: impl Fn(R) -> T) -> Result<Table<T>, Error> {
    let slots: Vec<(u32, Option<R>)> = serde_json::from_slice(bytes)?;
    Ok(Table::from_slots(slots.into_iter().map(|(g, r)| (g, r.map(&row).unwrap_or_default()))))
}

fn id<T>([index, generation]: [u32; 2]) -> Id<T> {
    Id::new(index, generation)
}

fn pair<T>(id: Id<T>) -> [u32; 2] {
    [id.index(), id.generation()]
}

pub fn to_bytes(model: &Model) -> Result<Vec<u8>, Error> {
    let mut w = Writer::with_capacity(64 + model.nodes().capacity() * 29 + model.frames().capacity() * 37 + model.areas().capacity() * 48);
    w.bytes(MAGIC);
    w.u32(FORMAT_VERSION);

    let meta = Meta { name: model.name().to_owned(), display_units: model.display_units(), grid: model.grid().clone() };
    w.chunk(b"META", |w| w.json(&meta))?;
    w.chunk(b"MATL", |w| {
        let slots: Vec<(u32, Option<&Material>)> = model.materials().slots().map(|(g, m)| (g, (g & 1 == 1).then_some(m))).collect();
        w.json(&slots)
    })?;
    w.chunk(b"SECT", |w| {
        let slots: Vec<(u32, Option<SectionRecord>)> = model
            .sections()
            .slots()
            .map(|(g, s)| (g, (g & 1 == 1).then(|| SectionRecord { name: s.name.clone(), material: pair(s.material), shape: s.shape })))
            .collect();
        w.json(&slots)
    })?;
    w.chunk(b"NODE", |w| {
        w.len(model.nodes().capacity())?;
        for (g, n) in model.nodes().slots() {
            w.u32(g);
            w.f64(n.position.x);
            w.f64(n.position.y);
            w.f64(n.position.z);
            w.u8(n.restraint.0);
        }
        Ok::<_, Error>(())
    })?;
    w.chunk(b"FRAM", |w| {
        w.len(model.frames().capacity())?;
        for (g, f) in model.frames().slots() {
            w.u32(g);
            w.id(f.i);
            w.id(f.j);
            w.id(f.section);
            w.u8(f.role.to_u8());
            w.f64(f.angle);
        }
        Ok::<_, Error>(())
    })?;
    w.chunk(b"AREA", |w| {
        w.len(model.areas().capacity())?;
        for (g, a) in model.areas().slots() {
            w.u32(g);
            w.id(a.section);
            w.u8(a.role.to_u8());
            let n = u16::try_from(a.corners.len()).map_err(|_| Error::Damaged("an area has too many corners".to_owned()))?;
            w.u16(n);
            for c in &a.corners {
                w.id(*c);
            }
        }
        Ok::<_, Error>(())
    })?;
    w.chunk(b"FREL", |w| {
        let released: Vec<(u32, u16)> =
            model.frames().iter().filter(|(_, f)| f.releases != Releases::NONE).map(|(id, f)| (id.index(), f.releases.0)).collect();
        w.len(released.len())?;
        for (index, bits) in released {
            w.u32(index);
            w.u16(bits);
        }
        Ok::<_, Error>(())
    })?;
    w.chunk(b"LPAT", |w| w.json(&json_slots(model.patterns(), Clone::clone)))?;
    w.chunk(b"LCAS", |w| {
        w.json(&json_slots(model.cases(), |c| CaseRecord {
            name: c.name.clone(),
            patterns: c.patterns.iter().map(|(p, f)| (pair(*p), *f)).collect(),
        }))
    })?;
    w.chunk(b"COMB", |w| {
        w.json(&json_slots(model.combinations(), |c| CombinationRecord {
            name: c.name.clone(),
            kind: c.kind,
            cases: c.cases.iter().map(|(k, f)| (pair(*k), *f)).collect(),
        }))
    })?;
    w.chunk(b"NLOD", |w| {
        w.len(model.node_loads().capacity())?;
        for (g, l) in model.node_loads().slots() {
            w.u32(g);
            w.id(l.node);
            w.id(l.pattern);
            for v in l.values {
                w.f64(v);
            }
        }
        Ok::<_, Error>(())
    })?;
    w.chunk(b"FLOD", |w| {
        w.len(model.frame_loads().capacity())?;
        for (g, l) in model.frame_loads().slots() {
            w.u32(g);
            w.id(l.frame);
            w.id(l.pattern);
            w.u8(l.shape.to_u8());
            w.u8(l.direction.to_u8());
            for v in [l.start, l.end, l.value_start, l.value_end] {
                w.f64(v);
            }
        }
        Ok::<_, Error>(())
    })?;
    w.chunk(b"ALOD", |w| {
        w.len(model.area_loads().capacity())?;
        for (g, l) in model.area_loads().slots() {
            w.u32(g);
            w.id(l.area);
            w.id(l.pattern);
            w.f64(l.pressure);
            w.u8(l.span.to_u8());
        }
        Ok::<_, Error>(())
    })?;
    Ok(w.finish())
}

pub fn from_bytes(bytes: &[u8]) -> Result<Model, Error> {
    let mut r = Reader::new(bytes);
    if r.take(MAGIC.len()).ok() != Some(&MAGIC[..]) {
        return Err(Error::WrongMagic);
    }
    let version = r.u32()?;
    if version > FORMAT_VERSION {
        return Err(Error::TooNew(version));
    }
    let (mut meta, mut materials, mut sections, mut nodes, mut frames, mut areas) = (None, None, None, None, None, None);
    let (mut patterns, mut cases, mut combinations) = (None, None, None);
    let (mut node_loads, mut frame_loads, mut area_loads) = (None, None, None);
    let mut releases = Vec::new();
    while !r.is_empty() {
        let tag = r.tag()?;
        let mut body = Reader::new(r.take_len()?);
        match &tag {
            b"META" => meta = Some(serde_json::from_slice::<Meta>(body.rest())?),
            b"MATL" => {
                let slots: Vec<(u32, Option<Material>)> = serde_json::from_slice(body.rest())?;
                materials = Some(Table::from_slots(slots.into_iter().map(|(g, m)| (g, m.unwrap_or_default()))));
            }
            b"SECT" => {
                let slots: Vec<(u32, Option<SectionRecord>)> = serde_json::from_slice(body.rest())?;
                sections = Some(Table::from_slots(
                    slots
                        .into_iter()
                        .map(|(g, s)| (g, s.map(|s| Section { name: s.name, material: id(s.material), shape: s.shape }).unwrap_or_default())),
                ));
            }
            b"NODE" => {
                let n = body.count(29)?;
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    let g = body.u32()?;
                    let position = DVec3::new(body.f64()?, body.f64()?, body.f64()?);
                    slots.push((g, Node { position, restraint: Restraint::from_bits(body.u8()?) }));
                }
                nodes = Some(Table::from_slots(slots));
            }
            b"FRAM" => {
                let n = body.count(37)?;
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    let g = body.u32()?;
                    let (i, j, section) = (body.id()?, body.id()?, body.id()?);
                    let role = FrameRole::from_u8(body.u8()?).unwrap_or_default();
                    slots.push((g, Frame { i, j, section, role, angle: body.f64()?, ..Frame::default() }));
                }
                frames = Some(slots);
            }
            b"AREA" => {
                let n = body.count(15)?;
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    let g = body.u32()?;
                    let section = body.id()?;
                    let role = AreaRole::from_u8(body.u8()?).unwrap_or_default();
                    let corners = usize::from(body.u16()?);
                    let corners = (0..corners).map(|_| body.id()).collect::<Result<Corners, _>>()?;
                    slots.push((g, Area { corners, section, role }));
                }
                areas = Some(Table::from_slots(slots));
            }
            b"FREL" => {
                let n = body.count(6)?;
                releases = (0..n).map(|_| Ok((body.u32()?, body.u16()?))).collect::<Result<_, Error>>()?;
            }
            b"LPAT" => patterns = Some(from_json_slots(body.rest(), |p: LoadPattern| p)?),
            b"LCAS" => {
                cases = Some(from_json_slots(body.rest(), |c: CaseRecord| LoadCase {
                    name: c.name,
                    patterns: c.patterns.into_iter().map(|(p, f)| (id(p), f)).collect(),
                })?)
            }
            b"COMB" => {
                combinations = Some(from_json_slots(body.rest(), |c: CombinationRecord| Combination {
                    name: c.name,
                    kind: c.kind,
                    cases: c.cases.into_iter().map(|(k, f)| (id(k), f)).collect(),
                })?)
            }
            b"NLOD" => {
                let n = body.count(NODE_LOAD_RECORD)?;
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    let g = body.u32()?;
                    let (node, pattern) = (body.id()?, body.id()?);
                    let mut values = [0.0; 6];
                    for v in &mut values {
                        *v = body.f64()?;
                    }
                    slots.push((g, NodeLoad { node, pattern, values }));
                }
                node_loads = Some(Table::from_slots(slots));
            }
            b"FLOD" => {
                let n = body.count(FRAME_LOAD_RECORD)?;
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    let g = body.u32()?;
                    let (frame, pattern) = (body.id()?, body.id()?);
                    let shape = FrameLoadShape::from_u8(body.u8()?).ok_or_else(|| Error::Damaged("unknown frame load shape".to_owned()))?;
                    let direction = LoadDirection::from_u8(body.u8()?).ok_or_else(|| Error::Damaged("unknown load direction".to_owned()))?;
                    let (start, end, value_start, value_end) = (body.f64()?, body.f64()?, body.f64()?, body.f64()?);
                    slots.push((g, FrameLoad { frame, pattern, shape, direction, start, end, value_start, value_end }));
                }
                frame_loads = Some(Table::from_slots(slots));
            }
            b"ALOD" => {
                let n = body.count(AREA_LOAD_RECORD)?;
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    let g = body.u32()?;
                    let (area, pattern, pressure) = (body.id()?, body.id()?, body.f64()?);
                    let span = SlabSpan::from_u8(body.u8()?).ok_or_else(|| Error::Damaged("unknown slab span".to_owned()))?;
                    slots.push((g, AreaLoad { area, pattern, pressure, span }));
                }
                area_loads = Some(Table::from_slots(slots));
            }
            // A chunk from a later version: skip it.
            _ => {}
        }
    }
    let meta = meta.ok_or(Error::MissingChunk("META"))?;
    let mut frames = frames.unwrap_or_default();
    for (index, bits) in releases {
        let (_, frame) = frames.get_mut(index as usize).ok_or_else(|| Error::Damaged("a release names no frame".to_owned()))?;
        frame.releases = Releases(bits);
    }
    if version < 2 && patterns.is_none() {
        let defaults = Model::new(String::new());
        patterns = Some(defaults.patterns().clone());
        cases = Some(defaults.cases().clone());
    }
    Ok(Model::from_parts(Parts {
        name: meta.name,
        display_units: meta.display_units,
        grid: meta.grid,
        nodes: nodes.unwrap_or_default(),
        frames: Table::from_slots(frames),
        areas: areas.unwrap_or_default(),
        materials: materials.unwrap_or_default(),
        sections: sections.unwrap_or_default(),
        patterns: patterns.unwrap_or_default(),
        cases: cases.unwrap_or_default(),
        combinations: combinations.unwrap_or_default(),
        node_loads: node_loads.unwrap_or_default(),
        frame_loads: frame_loads.unwrap_or_default(),
        area_loads: area_loads.unwrap_or_default(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use structurecraft_model::{Change, Op, PatternKind};

    fn sample() -> Model {
        let mut m = Model::new("Tower A");
        let slab = m.sections().iter().find(|(_, s)| s.shape.is_shell()).map(|(id, _)| id).unwrap();
        let beam = m.sections().iter().find(|(_, s)| !s.shape.is_shell()).map(|(id, _)| id).unwrap();
        let pts = [(0.0, 0.0), (6.0, 0.0), (6.0, 5.0), (0.0, 5.0)];
        let undo = m.apply(Change::Nodes(Op::Add(pts.iter().map(|(x, y)| Node { position: DVec3::new(*x, *y, 3.0), ..Node::default() }).collect())));
        let Ok(Change::Nodes(Op::Remove(ids))) = undo else { panic!() };
        m.apply(Change::Frames(Op::Add(vec![Frame { i: ids[0], j: ids[1], section: beam, angle: 0.25, ..Frame::default() }]))).unwrap();
        m.apply(Change::Areas(Op::Add(vec![Area { corners: ids.iter().copied().collect(), section: slab, role: AreaRole::Slab }]))).unwrap();
        // A free slot in the middle of the node table.
        let extra = m.apply(Change::Nodes(Op::Add(vec![Node::default(), Node { position: DVec3::X * 9.0, restraint: Restraint::FIXED }])));
        let Ok(Change::Nodes(Op::Remove(extra))) = extra else { panic!() };
        m.apply(Change::Nodes(Op::Remove(vec![extra[0]]))).unwrap();
        // Loads and releases.
        let frame = m.frames().ids().next().unwrap();
        let mut f = *m.frames().get(frame).unwrap();
        f.releases = Releases::new(Releases::MOMENTS, Releases::M3);
        m.apply(Change::Frames(Op::Set(vec![(frame, f)]))).unwrap();
        let Ok(Change::Patterns(Op::Remove(wind))) =
            m.apply(Change::Patterns(Op::Add(vec![LoadPattern { name: "Wind X".to_owned(), kind: PatternKind::Wind, self_weight: 0.0 }])))
        else {
            panic!()
        };
        let dead = m.patterns().ids().next().unwrap();
        let Ok(Change::Cases(Op::Remove(case))) =
            m.apply(Change::Cases(Op::Add(vec![LoadCase { name: "Wind X".to_owned(), patterns: vec![(wind[0], 1.0)] }])))
        else {
            panic!()
        };
        let first_case = m.cases().ids().next().unwrap();
        let combo = Combination { name: "1.2D+1.0W".to_owned(), kind: CombinationKind::Linear, cases: vec![(first_case, 1.2), (case[0], 1.0)] };
        m.apply(Change::Combinations(Op::Add(vec![combo]))).unwrap();
        m.apply(Change::NodeLoads(Op::Add(vec![NodeLoad { node: ids[2], pattern: wind[0], values: [10e3, 0.0, 0.0, 0.0, 0.0, 5e3] }]))).unwrap();
        let udl = FrameLoad { frame, pattern: dead, start: 0.0, end: 1.0, value_start: 12e3, value_end: 12e3, ..FrameLoad::default() };
        let point = FrameLoad {
            frame,
            pattern: dead,
            shape: FrameLoadShape::Point,
            direction: LoadDirection::Local2,
            start: 0.5,
            value_start: -3e3,
            ..FrameLoad::default()
        };
        m.apply(Change::FrameLoads(Op::Add(vec![udl, point]))).unwrap();
        let area = m.areas().ids().next().unwrap();
        m.apply(Change::AreaLoads(Op::Add(vec![AreaLoad { area, pattern: dead, pressure: 2.5e3, span: SlabSpan::TwoWay }]))).unwrap();
        m
    }

    /// The file without the chunks named, as a given version.
    fn rewrite(bytes: &[u8], version: u32, drop: &[&[u8; 4]]) -> Vec<u8> {
        let mut out = bytes[..8].to_vec();
        out.extend_from_slice(&version.to_le_bytes());
        let mut at = 12;
        while at < bytes.len() {
            let len = u64::from_le_bytes(bytes[at + 4..at + 12].try_into().unwrap()) as usize;
            let end = at + 12 + len;
            if !drop.iter().any(|t| &bytes[at..at + 4] == *t) {
                out.extend_from_slice(&bytes[at..end]);
            }
            at = end;
        }
        out
    }

    #[test]
    fn version_one_files_open_with_default_patterns() {
        let mut model = sample();
        let loads = model.node_loads().ids().collect();
        model.apply(Change::NodeLoads(Op::Remove(loads))).unwrap();
        let v1 = rewrite(&to_bytes(&model).unwrap(), 1, &[b"FREL", b"LPAT", b"LCAS", b"COMB", b"NLOD", b"FLOD", b"ALOD"]);
        let back = from_bytes(&v1).unwrap();
        assert_eq!(back.nodes(), model.nodes());
        assert_eq!(back.frames().iter().filter(|(_, f)| f.releases != Releases::NONE).count(), 0);
        assert_eq!(back.patterns(), Model::new("").patterns());
        assert_eq!(back.cases(), Model::new("").cases());
        assert!(back.frame_loads().is_empty());
    }

    #[test]
    fn round_trips_with_ids() {
        let model = sample();
        let back = from_bytes(&to_bytes(&model).unwrap()).unwrap();
        assert_eq!(back, model);
        assert_eq!(back.frame_loads().len(), 2);
        assert_eq!(back.combinations().len(), 1);
        for (id, n) in model.nodes().iter() {
            assert_eq!(back.nodes().get(id), Some(n));
            assert_eq!(back.node_uses(id), model.node_uses(id));
        }
    }

    #[test]
    fn hostile_bytes_are_errors_not_panics() {
        let good = to_bytes(&sample()).unwrap();
        for bytes in [&b""[..], b"\xff\xfe", b"STRCRAFT", b"STRCRAFT\x01\x00\x00\x00NODE\xff\xff\xff\xff\xff\xff\xff\xff"] {
            assert!(from_bytes(bytes).is_err());
        }
        // Every truncation and a flipped byte everywhere: errors or a valid model, never a panic.
        for len in 0..good.len() {
            let _ = from_bytes(&good[..len]);
        }
        for at in 0..good.len() {
            let mut bad = good.clone();
            bad[at] ^= 0x5a;
            let _ = from_bytes(&bad);
        }
        let mut newer = good.clone();
        newer[8] = 99;
        assert!(matches!(from_bytes(&newer), Err(Error::TooNew(99))));
    }

    #[test]
    fn unknown_chunks_are_skipped() {
        let mut bytes = to_bytes(&sample()).unwrap();
        bytes.extend_from_slice(b"FUTR");
        bytes.extend_from_slice(&3u64.to_le_bytes());
        bytes.extend_from_slice(b"abc");
        assert_eq!(from_bytes(&bytes).unwrap(), sample());
    }
}
