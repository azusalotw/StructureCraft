# StructureCraft

**An open-source structural analysis application built in pure Rust.**

StructureCraft aims to be a free, open alternative to SAP2000, ETABS and MIDAS: model a
building in 3D with nodes, beams, columns and slabs, then analyse it. It runs on Windows,
macOS and Linux, in the browser, and AI agents can model through its built-in MCP server.

> Early days: you can model (nodes, beams, columns, braces, slabs, walls, sections, stories),
> view models of hundreds of thousands of nodes, and drive everything from the CLI or MCP. The
> analysis engine comes next.

## Using the desktop app

- **Navigate:** right drag orbits, middle drag (or Shift + right drag) pans, the wheel zooms.
  Plan, Front, Side and 3D set the view; F fits the model.
- **Tools:** Select (click, Shift-click, drag a box), Node, Beam (click points in turn), Column
  (click a plan point: from the story below to the working story), Slab (click corners, then
  the first corner, a double-click or Enter). Points snap to nodes and grid intersections.
- **Edit:** Delete removes the selection, Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z redoes. The
  Properties panel changes supports, sections and frame roles.
- **Start a model:** Model > Generate building, or `structurecraft path/to/model.stc`.

## Build

```sh
cargo run -p structurecraft            # desktop app
cargo run -p structurecraft-cli -- commands
cargo run -p structurecraft-cli -- mcp # MCP server on stdin/stdout
cd apps/structurecraft-web && trunk serve   # browser app
```

Before a commit, run `cargo xtask ci` (format, layering, clippy, tests, wasm check).

## Licence

MIT or Apache-2.0, at your option.
