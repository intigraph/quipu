# Pendant merge attributes

The same real `git check-attr merge` probe selects no driver for five pendant
paths on base `ec3b707d`, then selects `quipu` for all five on the candidate.
The legacy `qpack/export.nt` positive control remains `quipu`; an unrelated
`export.nt` remains unspecified.

[Terminal recording](demo.cast): download and run `asciinema play demo.cast`.
The [readable transcript](output.log) is also available. The local Asciinema
player is unavailable; the captured JSON output events were decoded and checked,
but native player playback was not tested. No inline playback is promised.
The recording contains actual Git output from two isolated repositories using
the base and candidate `.gitattributes`. It shows attribute selection, not a
driver installation, an executed semantic merge or deployed server behavior.

Reproduce the broader root, nested, space-containing, legacy and exclusion
controls with `python3 scripts/test_pendant_attributes.py`.
