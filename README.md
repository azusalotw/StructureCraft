# StructureCraft

**An open-source structural analysis application built in pure Rust.**

StructureCraft aims to be a free, open alternative to SAP2000, ETABS and MIDAS: model a
building in 3D with nodes, beams, columns and slabs, then analyse it. It runs on Windows,
macOS and Linux, in the browser, and AI agents can model through its built-in MCP server.

> Early days: the workspace, the window and the command layer are in place. The structural
> model, the GPU viewport and the modeling tools come next; the analysis engine after that.

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
