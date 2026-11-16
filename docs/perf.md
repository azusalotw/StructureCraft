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

Loads, measured 2026-11-05 on the same model:

| Operation | Time |
|---|---|
| Put a 10 kN/m load on all 1,498,100 frames (one command) | 0.26 s |
| Put a 3 kPa load on all 490,000 slabs | 39 ms |
| Pack the arrows of the 1.5 million frame loads (48 MB) | 0.13 s |
| Save with loads (188 MB) / open with loads | 0.11 s / 0.45 s |

Above 20,000 loads in a pattern, each load draws as a single line without an arrow head,
so a fully loaded model packs 32 bytes a load. Deleting elements now also scans the load
tables for loads on them.

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

## Analysis

Linear static analysis of generated buildings (frames only, with the default 10 kN/m Dead
load on every frame plus self-weight, and 3 kPa Live on every slab), measured 2026-11-16 on the
4-core Linux container (release build). Both cases are solved from one factorisation.

| Building | Nodes | Degrees of freedom | Analysis |
|---|---|---|---|
| 10 x 10 bays, 10 stories | 1,331 | 7,260 | 0.07 s |
| 10 x 10 bays, 60 stories | 7,381 | 43,560 | 0.80 s |
| 20 x 20 bays, 20 stories | 9,261 | 52,920 | 1.4 s |
| 30 x 30 bays, 30 stories | 29,791 | 172,980 | 12.5 s |

Almost all of the time is the sparse Cholesky factorisation; numbering, assembly, the loads
and the end forces take tens of milliseconds. Its cost grows with the size of the largest
"separator" (roughly one vertical slice through the building), so it rises much faster than
the node count: wide plans cost more than tall towers with the same number of nodes.

The default 509,141-node building is not analysed by the benchmark yet (it skips analysis above
50,000 nodes). Making that size practical is the next solver step: a nested-dissection
ordering, rigid floor diaphragms (three degrees of freedom a floor instead of six a node), and
measuring the factor's memory.
