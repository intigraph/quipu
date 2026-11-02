# Release PR discovery: an API error is unknown

The [recording](output.log.gz), [timing](timing.log) and
[readable transcript](transcript.txt) execute the actual release-workflow step
from base `e3319d618fdaf9110fec2e1ecdcbe9eb3045af88` and candidate
`33b3b5e107f0cc6686917b2531e9deaf839e865e`.

Watch `api-error`: the base returns exit 0 and claims no open release PR;
the candidate returns exit 1 and makes no absence claim. The same successful
empty-discovery control remains exit 0 on both revisions.

The input is a disposable `gh` command fixture. Only PR discovery is available;
any other command fails. No live API, authentication, status publication,
release dispatch, tag or registry write participates. The broader workflow
controls also cover fetch/read failures, revision movement and exact status
publication; this recording isolates the discovery correction.

To reproduce, save the base workflow outside the checkout, then run:

```sh
git show e3319d618fdaf9110fec2e1ecdcbe9eb3045af88:.github/workflows/release.yml > /tmp/base-release.yml
BASE_RELEASE_WORKFLOW=/tmp/base-release.yml python3 docs/demos/release-pr-discovery/probe.py
```

Python requires PyYAML, as do the repository's existing workflow tests. The
small sanitized artifacts follow the existing `docs/demos` retention policy.
Download the recording and timing together for terminal playback:

```sh
gzip -dk output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

Playback was verified. The transcript normalizes CRLF line endings; compressed
recording and timing retain the original captured bytes. Later release-version
metadata does not change either demonstrated workflow body.
