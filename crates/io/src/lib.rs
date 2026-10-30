//! Model files (L3): `.stc`.
//!
//! A file is a header and a list of chunks, each a four-byte tag, a byte length and a body, so
//! a reader skips chunks it does not know and later versions can add some. The small parts
//! (name, units, grid, materials, sections) are JSON; nodes, frames and areas are fixed-width
//! little-endian records, written and read in one pass without building a tree, so models of
//! hundreds of thousands of elements open in well under a second.
//!
//! Every table is stored slot by slot, free slots included, so element ids survive a save and
//! open. Reading treats the bytes as hostile: every length is checked before it is trusted, and
//! the model checks every reference before it is accepted.

mod bytes;

use serde::{Deserialize, Serialize};
use structurecraft_geom::DVec3;
use structurecraft_model::{
    Area, AreaRole, Corners, Frame, FrameRole, Grid, Id, Material, Model, ModelError, Node, Parts, Restraint, Section, SectionShape, Table, Units,
};

use bytes::{Reader, Writer};

/// The file extension of a StructureCraft model.
pub const EXTENSION: &str = "stc";

/// The newest format version this build writes. Older versions still read.
pub const FORMAT_VERSION: u32 = 1;

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
                    slots.push((g, Frame { i, j, section, role, angle: body.f64()? }));
                }
                frames = Some(Table::from_slots(slots));
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
            // A chunk from a later version: skip it.
            _ => {}
        }
    }
    let meta = meta.ok_or(Error::MissingChunk("META"))?;
    Ok(Model::from_parts(Parts {
        name: meta.name,
        display_units: meta.display_units,
        grid: meta.grid,
        nodes: nodes.unwrap_or_default(),
        frames: frames.unwrap_or_default(),
        areas: areas.unwrap_or_default(),
        materials: materials.unwrap_or_default(),
        sections: sections.unwrap_or_default(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use structurecraft_model::{Change, Op};

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
        m
    }

    #[test]
    fn round_trips_with_ids() {
        let model = sample();
        let back = from_bytes(&to_bytes(&model).unwrap()).unwrap();
        assert_eq!(back, model);
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
