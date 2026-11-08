# Signed client changeset library

The first P3 library slice is deliberately unwired. It adds no HTTP admission
route, key enrollment, credentials, installation or automatic client activation.
The full local-sync requirements remain in [local-sync-mode.md](local-sync-mode.md).

`ClientOverlay` keeps an immutable `BaseJournal` independently of the canonical
replica. Local replacement edits only its proposal set. `prepare()` produces the
existing unsigned ChangeSet body and the canonical `quipu-changeset-v1` signing
message; `sign()` uses the shared `quipu-signing` crate with a caller-held key.
The envelope binds source store/epoch, audience, persistent fork identity, graph
scope, base transaction and journal digest, shapes, delta and exact body digests,
registration, signer, operation UUID, issued time and nonce. Native and Wasm use
the same serializer. Private keys never enter serialized overlay state.

Master-side base reconstruction, current registration/capabilities, three-way
admission and atomic idempotent receipts are separate work. This source slice
exposes no admission method or endpoint. A signed proposal is not permission.

This slice supports one explicit complete graph with open valid intervals and
canonical skolemized terms. Bounded or proposed intervals, multi-graph/context
scopes and conflict-resolution signatures are not supported: they must refuse,
not silently fall back. `preview()` composes canonical catch-up with the private
delta using the shared pure slot operator, returning either a complete graph or
explicit conflicts. It retains both the frozen journal and proposals, refuses
foreign lineage, stale cursors, changed shapes and bounded intervals, and makes
no SHACL or authority decision.
`rebase()` verifies that the previous signed operation describes the exact old
overlay, composes canonical catch-up using the same preview, and prepares a new
operation with a fresh UUID and nonce. It returns a new frozen journal, the
remaining private delta, the unchanged previous wire object and local
supersession fingerprints. The caller persists this result and signs the new
request separately. A conflict returns decision records without a replacement
proposal or supersession; neither input is mutated. Reusing the old signature,
operation ID or nonce is refused.
This first rebase supports the same lineage, epoch, shapes and complete-graph
scope only. Restore/epoch migration, derived local graphs and the full resolver
workflow remain implementation work. This library slice does not establish full
P3 acceptance or production readiness.

The portable library opens/replaces overlays, prepares exact signing bytes and
verifies caller-supplied signatures. Cryptographic verification is not a current
registration or authorization decision. Tests cover signatures, private overlay
preservation, preview/rebase refusal and canonical wire round trips. The observer
consumer and its actual native-writer tests are described in
[client-resolver-artifact.md](client-resolver-artifact.md). Compiled explorer
bindings and runtime admission are outside this source slice.
