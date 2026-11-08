# Separately signed client resolver artifact

This source-only wire boundary leaves the `quipu-changeset-v1` envelope, signature,
body digest and replay fingerprint unchanged. It adds no route, key enrollment
or admission authority. The writer integration is not implemented in this slice.

`ResolverArtifact` version 1 carries the original proposal fingerprint, a public
`ProspectiveCandidate` and a public `ValidatorManifest`. Its message consists of
`quipu-client-resolver-artifact-v1` followed by NUL, the four-byte big-endian
version, then three length-framed strings: proposal fingerprint, candidate
canonical bytes, manifest canonical bytes. Each length is eight-byte big-endian.
The public encoders retain input and decision order and bind validator contents.
JSON whitespace does not contribute to the message.

Structural checks require exact agreement on store, epoch, graph, base cursor,
base slice digest and loaded combined-shapes digest. The retained client proposal
covers one complete graph, so a partial or explicitly empty resolver scope is
refused. The candidate's validator digest must match the manifest. These checks
do not establish that supplied rows or validator observations are true.

`ResolverArtifact::sign` takes a caller-owned key and requires that it verify
the original proposal. `SignedResolverArtifact::verify` verifies both messages
with the same supplied key. Neither method loads a key, consults a registry or
asserts current permission. Current master registration, graph and predicate
capabilities, revocation and epoch checks remain admission requirements.

The admission adapter must independently reconstruct all inputs under its writer
reservation, obtain actual validator observations, compare the signed identities,
assign admission time and validate the actual staged graph again. It must bind
fresh evidence to an atomic, idempotent receipt. Unsupported or unknown context
must refuse; a valid signature cannot turn it into conformance. `SignedResolverArtifact::bind_observation` consumes exactly one actual execution
captured by public `Store::observe_validator`. It compares the signed candidate
with the caller's independently reconstructed candidate, compares the complete
signed manifest with the actual observation, compares candidate RDF rows with
the actual complete staged graph using the existing parser, and binds the raw validator outcome
through the existing DTO. Binding currently requires both the signed graph label
`ROOT` and the actual observation's ROOT graph ID. The public observation exposes
local named-graph IDs without their IRIs, so named-graph evidence binding refuses
rather than trusting a caller mapping. Equal RDF and valid re-signatures cannot
substitute a different graph. Named proposals/artifacts can still be serialized
and verified; that does not make their observations bindable.
Missing/ambiguous observations and mismatched artifacts refuse. Unexecuted validation or a gate error cannot produce Conforms; the DTO
retains Unknown for incomplete context and RequiresAdmissionTime for unresolved
times. A violation remains Violates even though the write rolled back.

The caller still owns candidate reconstruction and reservation. This method does
not establish source authenticity, current key authority, full staged-graph
conformance or transaction commit. It adds no endpoint/admission caller or core
export and copies no selection logic. Runtime and writer integration remain held
under s24 c120 and the source-only disposition.
