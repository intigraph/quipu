# Named graph creation through MCP

[Replayable terminal recording](assets/named-graph-create.cast) · [Actual transcript](assets/named-graph-create-transcript.txt)

The installed baseline exposes 50 native MCP tools, with no graph create or label tool; the actual graph-create call returns unknown tool. The candidate exposes 52 tools, registers a committed graph with the declared actor/time, and preserves that original provenance when another actor repeats the call. Each process uses a fresh private database. No live graph or service is modified.

Baseline binary: Quipu 0.13.0, clean `f5cd1bb38381696bc57b265380339607fb94f4e9`. Candidate binary: clean `1cea235b355a8e85fab3d9c522df8ecec934d33a`, a base refresh of product commit `813a2dc22c9a97a9b510f2aba835c8563e42414e` onto `0f23ef3be3cc6ed32d62dacd4cf7c0986fc803bb`. This proves candidate source behavior, not deployment.

For the broader repeatable recipe, run `just mcp test`: it executes the seven exact MCP requests in the book, validates unknown graph/type refusals, pairs ROOT isolation with a named-graph positive control, restarts, and verifies original provenance and facts persist. The scoped parity audit retains the remaining share/import/reason/update native MCP gaps; this change does not claim every transport gap is closed.

The recording preserves actual output chunks and timing. It demonstrates an API/CLI change, not a browser interaction or notification.
