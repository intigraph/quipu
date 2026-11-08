# Signed resolver observer consumer

Run `bash docs/demos/client-resolver-observation/run.sh` from the checkout.
The base source c2e3a34979ba4d34144363e01b2350ab623231af has no resolver artifact
module; this is a source absence check, not a live before-service measurement.
The containing commit adds the library consumer. The real disposable-store
writer produces conforming and violating observations. Watch their bound outcomes,
Unknown when no validator runs, and refusal of a validly signed substituted
manifest. Same-store ROOT/named and two-distinct-named graph inverse controls
refuse despite equal RDF and valid signatures. Only actual ROOT observations
can currently be bound; named-graph binding explicitly refuses. Fixtures use
ephemeral keys and no service calls.

[Output](output.log) and [timing](timing.log) form an output-only recording;
download both and use `scriptreplay --log-out output.log --log-timing timing.log`.
No inline playback, endpoint admission, full-graph conformance, commit receipt,
installed service or runtime acceptance is claimed. Full reproduction compiles
the native library; the recording shows the focused executions.
