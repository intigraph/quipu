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

A privileged native store owner can call `client_base_journal()` and
`admit_client_changeset()`. Admission reserves the writer before checking current
trust-root-governed registration, signature and every touched graph/predicate
capability on that same registration. It independently reconstructs the base,
runs the pure string-term three-way merge, then applies normal graph gates and
validates the complete staged candidate against current shapes. The accepted
facts and immutable SQL `ChangesetReceipt` commit together. Identical signed
retries recover the receipt without another transaction; changed content under
the same operation UUID refuses. Conflicts commit nothing and return decision
records; the caller retains the overlay and journal.

This slice supports one explicit complete graph with open valid intervals and
canonical skolemized terms. Bounded or proposed intervals, multi-graph/context
scopes and conflict-resolution signatures are not supported: they must refuse,
not silently fall back. `preview()` composes canonical catch-up with the private
delta using the shared pure slot operator, returning either a complete graph or
explicit conflicts. It retains both the frozen journal and proposals, refuses
foreign lineage, stale cursors, changed shapes and bounded intervals, and makes
no SHACL or authority decision. The compiled Wasm bridge exposes the same preview.
Rebase, derived local graphs and the full resolver workflow remain implementation work. This
library slice does not establish full P3 acceptance or production readiness.

The portable Wasm bridge opens/replaces serialized overlays, prepares the exact
shared signing message and verifies a caller-supplied signature. Cryptographic
verification is not a registration or authorization decision. Only the master
admission path may make that decision.

Tests exercise the actual signed cut and stream applier, private proposals,
master admission, receipt recovery and replica convergence in disposable stores.
They include current key revocation, scope/content tampering, conflict retention,
full-candidate SHACL and receipt-insert rollback. The shared explorer smoke action
also runs the real compiled Wasm bridge with ephemeral caller-owned signing keys.
