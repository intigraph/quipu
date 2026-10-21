# Optional alignment evidence in a pendant

A producer may carry one optional `alignments.ttl` beside the graph, shapes and
stored queries. `ShareOptions.alignments_turtle` and the corresponding share
payload request accept exact UTF-8 Turtle. The CLI uses
`quipu share --alignments FILE --output DIR`; it reads a bounded regular file.
Unix opens also refuse symlinks and avoid blocking on a raced FIFO. The input is
limited to 2 MiB and parsed as Turtle before export. An explicit empty carrier
is valid and differs from omitting the member.

The canonical manifest contains `alignments_hash` (the existing `sha256:` digest
encoding) and `files.alignments = "alignments.ttl"` together. Both fields are
included in `share_id`. Exact bytes are sealed; changing only a comment changes
the identity. Omitting the carrier omits both fields, retaining legacy manifest
bytes and identities. Old readers may refuse a new carrier; they must not be
claimed compatible by changing the archive allowlist alone.

Directory, HTTP-directory and tar readers use the same fixed member name.
Archives retain duplicate, unsafe-path and unknown-file refusals. HTTP directory
reads fetch the member only when the manifest declares it; this does not prove
that an unlisted remote file does not exist. A declared missing, misnamed,
malformed, oversized or hash-mismatched carrier refuses before staging. A present
but undeclared member also refuses. The ordinary outward scrub includes these
bytes, including when receiving a share marked internal into an outward store.

Transport is not an alignment decision. Import retains exact verified bytes
separately from graph data, reachable through
`share_alignments::imported_member(store, share_id)`. It neither loads shapes nor
asserts identity, and ROOT promotion does not apply the ancillary mappings.
Full-store reconstruction carries the retained evidence table verbatim; it does
not turn those bytes into applied mappings or receiver authority. Ordinary graph
shares include only the explicitly selected carrier, rather than aggregating
every previously received carrier.
Existing alignment propose/decide/apply remains the curation route. An empty
carrier claims no accepted mapping dataset. Delta shares carry the complete
result member; the browser archive reader verifies it through the same core,
and Explorer export/delta preserve the originally imported evidence.

Repository jobs opt in explicitly with their reviewed carrier. They must package
and verify the physical member and engine identity; storing a source file or
appending empty prefix declarations to graph data is not that proof. Model
recipe verification, staging and deliberate promotion remain separate contracts.
