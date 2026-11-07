# Expiring memory-only credentials

This is an additive credential-registry contract. It does not enable file
memory, issue a credential, reload a running registry, or change existing
four-field writer credentials.

A version-1 registry may contain an entry with audience `quipu-memory`, the
existing `credential_id`, `principal` and `token_sha256` fields, and both:

- `expires_at_epoch_secs`: a positive unsigned UTC epoch second;
- `memory_scope`: an object containing exactly `graph`, `deployment` and
  `project`, each an absolute IRI.

Neither field may be null or omitted for this audience. They are refused on
ordinary audience entries. Unknown fields, duplicate credential IDs or verifier
hashes, and malformed scope IRIs refuse the registry. The registry still contains
verifiers only; never put bearer plaintext in it.

The memory credential admits exactly POST `/memory/commit`,
`/memory/projection` and `/memory/receipt`, before its expiry. The configured
memory graph, deployment and project must exactly match its recorded scope.
The existing handlers additionally require the authenticated principal to equal
the requested owner. Ordinary reads, generic writes, bulk export and replication
are not granted by this credential. Read-only mode still refuses commits.
Expired credentials remain recognizable and are refused, including on otherwise
open read routes; they do not fall through to shared or open-write authority.

Expiry is checked when authorizing a request, and again before dispatching its
named identity. It does not cancel an already-admitted in-flight operation.

## Deployment and revocation

Use the existing reviewed issuer and registry installation path. Stage a new
candidate privately, review it independently, and verify that the actually
serving verifier supports this contract before using the credential. No request
header supplied by a client establishes the registry principal.

Registries are immutable startup snapshots. Removing an entry from a file is
not immediate revocation: install the reviewed replacement and use the normal
service reload/restart path, then prove the old credential is refused before
reissuing. Preserve existing entries and shared credentials. A source revert is
not a complete rollback of an active registry containing the new audience;
remove its entries through the reviewed registry lane before reverting the
verifier contract.

The credential restriction does **not** retrofit per-owner secrecy onto the
existing shared memory-read/export routes or ordinary query policy. Those
policies remain unchanged for other callers. Private native files must not be
uploaded unless the deployment's overall read-access boundary is separately
qualified. Synthetic policy and router tests are not live credential or native
round-trip acceptance.
