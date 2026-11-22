# Shared structural views: before and after

Yupana emits shared CodeModule, Document and Section structure without adding
presentation fields owned by another producer. Requiring those fields rejects
new shared views. This fixture fails with four violations under the frozen prior
shape contract and passes under the compatible contract.

Run `sh docs/demos/structural-shape-compat/run.sh` from the repository root with
an installed Quipu binary. It reads files only, with no server or database write.
The terminal recording is output.log with timing.log (`scriptreplay` playback).

Five native tests also verify that present labels/headings retain string types,
structural requirements remain required and CodeSymbol/Chunk label floors remain.
A Section dual-typed as Chunk still needs its label. This admits structural facts;
it does not claim an owner has supplied presentation metadata.

Qualification also validated the exact retained failed publisher payloads offline.
Runtime acceptance requires deployment through the reviewed shape source path,
then a successful natural producer run with its receipt and writer-hold delta.
No production shape-registry override or marker reset is part of this repair.
Rollback reverts the shape commit through the same deployment path; do not change
producer keys, delete partial-write evidence or advance failed publication markers.
