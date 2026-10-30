# StructureCraft: instructions for agents

StructureCraft is an open-source, pure-Rust structural analysis app: a 3D modeler for nodes,
frames (beams, columns, braces) and areas (slabs, walls), with an analysis engine to follow.
It aims to replace commercial tools such as SAP2000, ETABS and MIDAS. Architecture:
`docs/architecture.md`.

## Hard rules (what enforces each)
- Independent: never copy another product's code, icons, text, file formats or manuals; never drive or take data from commercial structural software.
- No GPL/LGPL/AGPL code: dependencies are MIT, Apache-2.0, BSD or similarly permissive.
- Never crash: no unwrap/expect/panic/unsafe in non-test code, errors are `Result`, input is hostile: clippy `-D warnings`, `hostile_params_never_panic`.
- Everything is a command (`crates/engine`), with an input schema; programmatic calls never open dialogs. The UI, CLI and MCP server all go through the registry.
- Layering (geom, render, model, io, engine, gpu/mcp, ui-egui, apps); UI and GPU crates only in L5, L6 and the apps: `cargo xtask layers`.
- One camera path (`crates/ui-egui/src/navigation.rs`), colours from theme tokens (`crates/ui-egui/src/theme.rs`, which also feeds the 3D scene's palette).
- Application code is Rust only and builds for wasm: `cargo xtask wasm`.
- Scale is a requirement: think about 500,000 nodes before adding a per-element allocation, a per-frame rebuild or an O(n²) loop.
- Sorted lists (the command registry, `mod` lines) stay sorted.
- Run `cargo xtask ci` before every commit.
