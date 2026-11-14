# Breaking-change doctor fixture replay

Run `just breaking demo` with `jsonschema==4.23.0` available. The
[transcript](transcript.txt) shows the real detector changing from AFFECTED for
an old-only reader to NOT_AFFECTED for a dual reader, while retaining one exact
identity. Duplicate identity and missing control mutations return UNKNOWN.

All versions, receipts, digests and observations are synthetic test fixtures.
The checked-in real w3id record remains draft and refuses release readiness.
This demonstration performs no production reads, installation, correction,
signing or publication. The CLI test also proves `--apply` is refused and the
supplied input bytes and mtimes remain unchanged.

No replayable recording is supplied: the authoring environment has no asciinema.
The transcript is output from the actual source utility, not a video.
