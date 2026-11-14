# Comment author boundary

An outside author with a same-repository head previously reached the optional
sticky-comment writer. The candidate skips that writer; a maintainer-author
positive control still posts once. The read-only review report is unchanged.

[Readable transcript](transcript.jsonl) records the actual workflow shell against
a private fake forge, with no network or public comments. This is a text
transcript, not a screen recording or installed-service acceptance.

The before workflow is commit `7d3ba5d4a62e62e65f50197efd53bdf065046216`.
The candidate workflow SHA256 is `f6670c18ea9a5fc9176d939592d9fc7d09ca65cd22bd9f0bda50e859cf5cd223`.
The harness is `scripts/test-pendant-comment-guard.py`; its positive, outside,
unknown, fork, API-failure and job-condition controls reproduce the boundary:

```sh
python3 scripts/test-pendant-comment-guard.py
```

The test uses the actual write-step shell, a private `gh` stub and the real `jq`
expression. No Rust build, token, production store or live comment is needed.
