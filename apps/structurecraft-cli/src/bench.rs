//! `structurecraft-cli bench`: time the core operations on a large generated building.

use std::time::Instant;

use serde_json::{Value, json};
use structurecraft_engine::{Session, format_id};
use structurecraft_geom::DVec3;

fn timed<T>(label: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let start = Instant::now();
    let out = f()?;
    println!("{label:<40} {:>10.1} ms   rss {:>7} MB", start.elapsed().as_secs_f64() * 1e3, rss_mb().map_or("?".to_owned(), |m| m.to_string()));
    Ok(out)
}

/// Resident memory, where the system says (Linux).
fn rss_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024)
}

fn exec(s: &mut Session, id: &str, input: Value) -> Result<Value, String> {
    s.execute(id, input).map_err(|e| format!("{id}: {e}"))
}

pub fn run(bays: Option<&str>, stories: Option<&str>) -> Result<(), String> {
    let bays: usize = bays.map_or(Ok(70), str::parse).map_err(|e| format!("bays: {e}"))?;
    let stories: usize = stories.map_or(Ok(100), str::parse).map_err(|e| format!("stories: {e}"))?;
    let mut s = Session::new();
    println!("{bays} x {bays} bays, {stories} stories");
    timed("generate building", || exec(&mut s, "generate.building", json!({"bays_x": bays, "bays_y": bays, "stories": stories})))?;
    let info = timed("model info (bounds)", || exec(&mut s, "model.info", json!({})))?;
    println!("  {}", info["counts"]);

    let bytes = timed("save to bytes", || structurecraft_io::to_bytes(&s.model).map_err(|e| e.to_string()))?;
    println!("  {:.1} MB", bytes.len() as f64 / 1e6);
    let opened = timed("open from bytes (with checks)", || structurecraft_io::from_bytes(&bytes).map_err(|e| e.to_string()))?;
    if opened != s.model {
        return Err("the opened model differs from the saved one".to_owned());
    }
    drop(opened);

    let top = DVec3::new(0.0, 0.0, stories as f64 * 3.5);
    timed("draw one frame", || exec(&mut s, "frame.draw", json!({"segments": [[top.to_array(), (top + DVec3::new(-5.0, 0.0, 0.0)).to_array()]]})))?;
    let corner = s.model.node_at(top).ok_or("no node at the top corner")?;
    timed("delete one node (and what is on it)", || exec(&mut s, "element.delete", json!({"nodes": [format_id(corner)]})))?;
    timed("undo delete", || exec(&mut s, "edit.undo", json!({})))?;
    timed("undo draw", || exec(&mut s, "edit.undo", json!({})))?;
    timed("undo generate", || exec(&mut s, "edit.undo", json!({})))?;
    timed("redo generate", || exec(&mut s, "edit.redo", json!({})))?;
    Ok(())
}
