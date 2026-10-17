# Request command attribution demo

The same synthetic completion previously had no command attribution. The
candidate keeps the request identity and usage fields while adding
`declared_source: "st-anchor"` and `declared_operation: "list"`.

- Base: `b7068fdf6772d88e0f45dcdd63f156ef86abd1e4`.
- Demonstrated source: `c52b8cca`.
- Target: local library/middleware fixture, not an installed server.
- Input: one synthetic successful `/query` completion and two fixed headers.
- Reproduce: `just test --features onnx,server --bin quipu-server demo_command_attribution_before_and_after -- --nocapture`.

The [output recording](output.txt) and [timing](timing.txt) are a replayable
terminal recording, not a video. Download both and run
`scriptreplay --log-out output.txt --log-timing timing.txt`.
The output also serves as the readable transcript. Build diagnostics were
retained separately; the recorded command's exit status was zero.

The before line comes from the unchanged base log producer; the after line
passes that result through the new bounded declaration annotator. Separate
controls cover missing, unsupported and duplicate values, cancellation,
response completion, and unchanged metric labels. This does not demonstrate
installation, log-reader coverage, a traffic reduction, or a production census.
