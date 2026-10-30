# Performance

`cargo run --release -p structurecraft-cli -- bench [BAYS] [STORIES]` generates a regular
building and times the core operations. The default, 70 x 70 bays and 100 stories, is 509,141
nodes, 1,498,100 frames and 490,000 slabs.

Measured on the Linux CI container (release build), 2026-10-30:

| Operation | Time |
|---|---|
| Generate the building (one undoable command) | 0.53 s |
| Model info (bounds over every node) | 3 ms |
| Save to bytes (93 MB) | 55 ms |
| Open from bytes, checking every reference | 0.55 s |
| Draw one frame | 5 ms |
| Delete one node and what is on it | 12 ms |
| Undo the generation / redo it | 0.20 s / 0.14 s |

The model holds about 220 MB for these 2.5 million elements.

## Viewport

At 70 x 70 bays and 60 stories (307,501 nodes, 898,860 frames, 294,000 slabs), packing the
GPU buffers takes 49 ms (63 MB of vertex data). That happens once per edit, not per frame; a
frame writes one 80-byte uniform and issues three instanced draws. The desktop app opens and
draws this model; the status bar shows the last packing time.

Frame rates on real GPUs are not measured yet: CI and the development container render with
a software rasteriser.

Known costs to revisit:
- Deleting a node scans every frame and area for what uses it (12 ms here); a
  node-to-element adjacency list would make it constant.
- Every edit repacks the whole GPU scene (about 50 ms at 300,000 nodes). Updating only the
  changed slots would make edits on large models instant.
