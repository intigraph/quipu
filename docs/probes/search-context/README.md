# Search context demo

A private in-memory store contains one entity. Both queries use the same explicit
embedding, so even the missing-subject query has a nearest neighbour. The
candidate API preserves that diagnostic hit but admits zero context for the
missing subject. The present-subject control keeps one admitted hit.

This demonstrates diagnostic versus admitted views in the candidate, not two
live server runs. The retained pre-change API baseline had ranked hits and no
context decision. The toy embedding intentionally isolates admission from an
embedding model's quality. It is not a calibration or a whole-store absence proof.

Implementation source: 1ed662dcbcea10fbea94dbce80a4db940c2b04cc. The later
example and recording additions preserve that admission implementation.

[Terminal recording / transcript](output.txt), [timing](timing.txt).
Repeat with `bash docs/probes/search-context/run-demo.sh`. Replay here with:

```sh
scriptreplay --log-out output.txt --log-timing timing.txt
```

This is a terminal recording, not an inline video. No production data, external
HTTP, notification, installation or deployment is involved. The small recording
is retained in Git with its reproducer.
