# IRIs which are not in the dictionary

A fresh in-memory store contains a known IRI. A VALUES table includes that IRI,
an unknown IRI, and a string with the same spelling as the unknown IRI. The
unknown IRI must remain an IRI and agree with BIND, while the string remains a
literal. A CONSTRUCT template must produce two distinct RDF triples for an
unknown IRI object and its equal-looking string literal.

[Before transcript](before.txt) and [after transcript](after.txt) retain actual
output from the same `cargo test --locked --no-default-features --lib unknown_iri`
probe. The base is `ee35d837897bbfe75061234bad725427d851a37c` plus the three new
regressions; the candidate adds the two fallback fixes in this change. Three
existing controls passed in both runs; all three regressions failed before and
passed after. The tests include SQLite query_only and unchanged dictionary/fact
controls, plus N-Triples serialization with an explicit literal control.

These are source-fixture transcripts, not recordings or deployment evidence.
Timing for the original baseline was not retained as a replayable recording.
The PR caption identifies the exact committed candidate. Existing ignored tests
and unrelated conformance failures are not reclassified by this demonstration.
