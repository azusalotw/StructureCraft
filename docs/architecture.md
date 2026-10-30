# Architecture

StructureCraft is a pure-Rust Cargo workspace. Crates sit in layers; a crate depends only on
crates in lower layers, and only layer 5 and the apps may use a UI toolkit or GPU crate.
`cargo xtask layers` enforces this.

| Layer | Crates | Role |
|---|---|---|
| L0 | `geom` | points, vectors, bounding boxes (f64 metres) |
| L1 | `render` | view math: the orbit camera and projections, headless |
| L2 | `model` | the structural model: nodes, frames, areas, grids, materials, sections |
| L3 | `io` | model files (`.stc`), later import and export |
| L4 | `engine` | the session and the command registry (everything is a command) |
| L5 | `ui-egui`, `mcp` | the egui front end shared by desktop and browser; the MCP server |
| apps | `structurecraft`, `structurecraft-web`, `structurecraft-cli` | desktop app, browser app, headless CLI |

The analysis solver will be its own crate below `model`'s consumers (it reads the model's
arrays directly and never depends on the UI), and the GPU renderer joins L5.

## Performance from the start

The target is models of hundreds of thousands of nodes at interactive frame rates.

- **Storage:** struct-of-arrays with generational ids, not a graph of boxed objects, so
  rendering, picking and assembling the stiffness matrix walk contiguous memory.
- **Precision:** the model is f64; the GPU gets f32 positions relative to a local origin.
- **Drawing:** instanced draws for nodes and frames, batched meshes for areas, buffers
  updated in chunks when the model changes rather than rebuilt per frame.
- **Picking:** a GPU id buffer for clicks and a BVH for box selection.
- **The window thread never waits:** long work (file loads, meshing, analysis) runs in the
  background and reports progress.
