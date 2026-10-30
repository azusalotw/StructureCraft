use serde_json::{Value, json};

use super::*;

fn run(s: &mut Session, id: &str, input: Value) -> Value {
    s.execute(id, input).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn count(s: &mut Session, kind: &str) -> u64 {
    run(s, "model.info", json!({}))["counts"][kind].as_u64().unwrap()
}

#[test]
fn command_ids_are_unique_and_sorted() {
    let ids: Vec<_> = registry().iter().map(|c| c.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted);
}

#[test]
fn hostile_params_never_panic() {
    let inputs = [
        json!(null),
        json!(1),
        json!("x"),
        json!([]),
        json!({}),
        json!({"name": 5}),
        json!({"name": "\u{0}".repeat(1000)}),
        json!({"points": [[1e308, 0, 0]]}),
        json!({"points": [[0, 0]]}),
        json!({"segments": [[[0, 0, 0], [0, 0, 0]]]}),
        json!({"polygons": [[[0, 0, 0], [1, 0, 0]]]}),
        json!({"nodes": ["N0:1", "N0:1", "X"], "by": [1, 0, 0]}),
        json!({"bays_x": 0, "bays_y": 1, "stories": 1}),
        json!({"bays_x": 100000, "bays_y": 100000, "stories": 100000}),
        json!({"kind": "nodes", "offset": u64::MAX, "limit": 0}),
        json!({"ids": ["N", "F1:1", "S99:7", ""]}),
        json!({"path": ""}),
        json!({"name": "", "shape": {"shape": "rectangle", "b": -1, "h": 0}}),
        json!({"stories": [{"name": "a", "elevation": 3}, {"name": "b", "elevation": 1}]}),
    ];
    for spec in registry() {
        assert!((spec.input_schema)().is_object(), "{} has no schema", spec.id);
        for input in &inputs {
            let mut s = Session::new();
            run(&mut s, "frame.draw", json!({"segments": [[[0, 0, 0], [0, 0, 3]]]}));
            let _ = s.execute(spec.id, input.clone());
        }
    }
    assert!(matches!(Session::new().execute("no.such", json!({})), Err(CommandError::Unknown(_))));
}

#[test]
fn drawing_reuses_nodes_and_undo_redo_round_trip() {
    let mut s = Session::new();
    let out = run(&mut s, "frame.draw", json!({"segments": [[[0, 0, 0], [0, 0, 3]], [[0, 0, 3], [6, 0, 3]], [[6, 0, 0], [6, 0, 3.0004]]]}));
    assert_eq!(out["nodes_created"], 4);
    assert_eq!(count(&mut s, "frames"), 3);
    let roles: Vec<_> =
        out["frames"].as_array().unwrap().iter().map(|id| run(&mut s, "element.get", json!({"ids": [id]}))["elements"][0]["role"].clone()).collect();
    assert_eq!(roles, [json!("column"), json!("beam"), json!("column")]);
    let after = s.model.clone();

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(count(&mut s, "nodes"), 0);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.model, after);
    assert!(matches!(s.execute("edit.redo", json!({})), Err(CommandError::NothingTo("redo"))));
}

#[test]
fn a_failing_command_leaves_no_trace() {
    let mut s = Session::new();
    run(&mut s, "node.add", json!({"points": [[0, 0, 0]]}));
    let before = s.model.clone();
    // The first segment is fine, the second has no length: nothing of it may stay.
    let bad = s.execute("frame.draw", json!({"segments": [[[0, 0, 0], [5, 0, 0]], [[9, 9, 9], [9, 9, 9]]]}));
    assert!(bad.is_err());
    assert_eq!(s.model, before);
    assert_eq!(s.history().0, Some("Add nodes"));
}

#[test]
fn deleting_a_node_deletes_what_is_on_it() {
    let mut s = Session::new();
    run(&mut s, "frame.draw", json!({"segments": [[[0, 0, 3], [6, 0, 3]], [[6, 0, 3], [6, 5, 3]]]}));
    run(&mut s, "area.draw", json!({"polygons": [[[0, 0, 3], [6, 0, 3], [6, 5, 3], [0, 5, 3]]]}));
    let corner = s.model.node_at(structurecraft_geom::DVec3::new(6.0, 0.0, 3.0)).unwrap();
    let out = run(&mut s, "element.delete", json!({"nodes": [format_id(corner)]}));
    assert_eq!(out["deleted"], json!({"nodes": 1, "frames": 2, "areas": 1}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!((count(&mut s, "frames"), count(&mut s, "areas"), count(&mut s, "nodes")), (2, 1, 4));
}

#[test]
fn moving_nodes_and_stale_ids() {
    let mut s = Session::new();
    let n = run(&mut s, "node.add", json!({"points": [[0, 0, 0]]}))["nodes"][0].clone();
    run(&mut s, "node.move", json!({"nodes": [n], "by": [1, 2, 3]}));
    assert_eq!(run(&mut s, "element.get", json!({"ids": [n]}))["elements"][0]["position"], json!([1.0, 2.0, 3.0]));
    run(&mut s, "element.delete", json!({"nodes": [n]}));
    let again = run(&mut s, "node.add", json!({"points": [[0, 0, 0]]}))["nodes"][0].clone();
    assert_ne!(again, n, "a reused slot gets a new generation");
    assert!(s.execute("node.move", json!({"nodes": [n], "by": [1, 0, 0]})).is_err());
}

#[test]
fn generated_building_counts() {
    let mut s = Session::new();
    let out = run(&mut s, "generate.building", json!({"bays_x": 3, "bays_y": 2, "stories": 4}));
    let (nx, ny, nz) = (4, 3, 5);
    assert_eq!(out["nodes_created"], nx * ny * nz);
    let columns = nx * ny * (nz - 1);
    let beams = ((nx - 1) * ny + nx * (ny - 1)) * (nz - 1);
    assert_eq!(out["frames"], columns + beams);
    assert_eq!(out["areas"], 3 * 2 * 4);
    assert_eq!(s.model.grid().stories.len(), 5);
    // Generating again on top merges every node.
    assert_eq!(run(&mut s, "generate.building", json!({"bays_x": 3, "bays_y": 2, "stories": 4}))["nodes_created"], 0);
}

#[test]
fn save_and_open() {
    let dir = std::env::temp_dir().join(format!("structurecraft-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("m.stc").to_string_lossy().into_owned();
    let mut s = Session::new();
    run(&mut s, "generate.building", json!({"bays_x": 2, "bays_y": 2, "stories": 2}));
    run(&mut s, "model.save", json!({"path": path}));
    let saved = s.model.clone();
    let mut t = Session::new();
    run(&mut t, "model.open", json!({"path": path}));
    assert_eq!(t.model, saved);
    assert!(!t.dirty);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn sections_and_materials() {
    let mut s = Session::new();
    let m =
        run(&mut s, "material.add", json!({"name": "Steel", "elastic_modulus": 200e9, "poisson_ratio": 0.3, "density": 7850}))["material"].clone();
    let sec =
        run(&mut s, "section.add", json!({"name": "W", "material": m, "shape": {"shape": "i_section", "h": 0.4, "b": 0.2, "tf": 0.015, "tw": 0.01}}))
            ["section"]
            .clone();
    let f = run(&mut s, "frame.draw", json!({"segments": [[[0, 0, 0], [0, 0, 4]]], "section": sec}))["frames"][0].clone();
    assert_eq!(run(&mut s, "element.get", json!({"ids": [f]}))["elements"][0]["section"], sec);
    let slab = run(&mut s, "element.list", json!({"kind": "sections"}))["items"][2]["id"].clone();
    assert!(s.execute("frame.draw", json!({"segments": [[[0, 0, 0], [1, 0, 0]]], "section": slab})).is_err());
}

#[test]
fn changing_frames_and_areas() {
    let mut s = Session::new();
    let f = run(&mut s, "frame.draw", json!({"segments": [[[0, 0, 3], [6, 0, 3]]]}))["frames"][0].clone();
    let rev = s.revision();
    run(&mut s, "frame.set", json!({"frames": [f], "role": "brace"}));
    assert!(s.revision() > rev);
    assert_eq!(run(&mut s, "element.get", json!({"ids": [f]}))["elements"][0]["role"], "brace");
    let slab = run(&mut s, "element.list", json!({"kind": "sections"}))["items"][2]["id"].clone();
    assert!(s.execute("frame.set", json!({"frames": [f], "section": slab})).is_err());
    let a = run(&mut s, "area.draw", json!({"polygons": [[[0, 0, 0], [1, 0, 0], [1, 0, 3]]]}))["areas"][0].clone();
    run(&mut s, "area.set", json!({"areas": [a], "role": "wall"}));
    assert_eq!(run(&mut s, "element.get", json!({"ids": [a]}))["elements"][0]["role"], "wall");
}
