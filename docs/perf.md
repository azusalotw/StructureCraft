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

Known costs to revisit: deleting a node scans every frame and area for what uses it (12 ms
here); a node-to-element adjacency list would make it constant.
