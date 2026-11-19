# Scoped machine writers

Named credentials in `crew_credentials_file` identify **unrestricted operators**.
They do not restrict writes to an application graph. A machine that must write
only one graph needs the separate `scoped_writer_credentials_file` profile and
an owner-enrolled write grant. Declared actors and provenance headers cannot
grant authority. Read grants and human WebAuthn enrollment are separate.

## Enrollment

This is local owner administration, not an HTTP or MCP operation. Keep database
files, policy records, and verifier files under owner control. Configure a
normal operator `auth_token` and a separate verifier-only credential registry:

```toml
[quipu.server]
scoped_writer_credentials_file = "/private/scoped-writers.json"
```

The registry uses the same version-1 format as the ordinary named credential
registry: `credential_id`, absolute `principal`, audience `quipu`, and lowercase
`token_sha256`. Never put bearer plaintext in a registry or grant file. Empty,
unreadable, malformed, or overlapping scoped registries refuse startup.
Credential identifiers and verifiers must be disjoint from ordinary named,
replication, current-shared, and active previous-shared credentials. This
profile cannot be combined with scoped reads, strict session keys, a replica,
or a relay. Existing operator profiles remain unrestricted.

Register the intended committed graph using normal owner tools. Observe its
instance from the **actual destination database**:

```sh
quipu graph writer-instance 'https://example.org/graphs/answers' --db data.db
```

Copy the returned object verbatim into the grant's `instance`. Do not infer
identity from a graph IRI or creation timestamp. A version-1 grant has these
fields:

```json
{
  "version": 1,
  "id": "answers-writer",
  "principal": "https://example.org/agents/curator",
  "credential_id": "curator-1",
  "instance": {
    "store_id": "<observed store_id>",
    "graph": "https://example.org/graphs/answers",
    "generation": "<observed generation>"
  },
  "operations": ["update"],
  "valid_from": 1793836800,
  "valid_until": 1793923200,
  "revision": 1,
  "revoked": false
}
```

Choose a current, bounded validity interval in UTC Unix seconds, then apply the
owner-reviewed file:

```sh
quipu graph writer-grant grant.json --db data.db
```

Only a strictly higher revision can replace a grant. To withdraw it, apply a
higher revision with `revoked: true`; withdrawal also works after the graph has
disappeared. It takes effect without restarting the listener. A write already
inside the commit barrier may finish before withdrawal completes. Once
withdrawal returns, an older authorization cannot commit afterward. Reusing a
withdrawn identifier at an older revision is refused. Credential registry
changes themselves require a listener restart.

## Request behavior

Invalid credentials return 401. Authenticated scoped writes without a matching
current grant return 403, regardless of `governance.enforce_authority`. Matching
requires the authenticated principal, credential identifier, operation, store
identity, graph instance, and validity interval. Ordinary governance, SHACL,
ownership, and human-attestation protection still apply.

`POST /update` accepts statically named graph targets in INSERT/DELETE data or
conditional DELETE/INSERT templates. Every mutation target must be granted,
including a foreign template whose WHERE would match nothing. One grant must cover the entire nonempty mutation target set. Each write grant
pins one graph instance, so writes across distinct graphs are refused together
before evaluation or term interning; separate requests can use separate grants. Default-graph,
variable-graph, graph-management, and unprovable mutation targets are refused.
This is write authorization; it does not grant or restrict read access.

`POST /episode` and `POST /knot` require an explicit, already registered, granted
committed destination graph. Snapshot replacement cannot widen it. Scoped
episodes resolve inside the writer barrier. Scoped writes do not perform a
second global embedding update after their graph transaction; index maintenance
remains an operator operation.

`/set` currently writes ROOT only and is refused for scoped credentials, even
when supplied a `graph` field. All other mutation routes, including graph
creation, deletion, labeling, import, patch, and policy enrollment, are refused.
ROOT, overlay graphs, attached graphs, and frozen graphs cannot be enrolled.

Conditional `/update` retains its ordinary report: asserted/retracted counts,
per-graph transactions, and a null transaction for no change. Bearers are not
one-shot signatures. Retrying the same properly conditional update after a lost
response reports no change; use its conditional state test for idempotency.
Human challenge, nonce, replay, and receipt rules are unchanged. Server-generated
signed policy verdicts and metadata-only refusal events wait for the outer data
transaction to resolve and can persist after rejected data rolls back. They are
server audit records, not a caller capability to write ROOT; caller actor/source
strings cannot select that internal path.

## Graph identity and recovery

An additive migration creates random local graph-instance generations and empty
policy tables; it grants nothing. The published generation also binds the local store epoch, so explicit owner
adoption invalidates old grants even when the lineage store ID is preserved.
Registry replacement, deletion/recreation,
relabeling, and other registry updates invalidate the pin. A repeat registration
that does not change a registry row keeps the pin. Re-observe and explicitly
re-enroll after an intentional identity change.

Grants and instance pins are excluded from reconstruction shares. Restored or
respaced stores require fresh observation and enrollment; imported RDF cannot
install authority. A raw filesystem copy retains policy records and lineage identity, but normal
writer ownership checks refuse the copy and explicit adoption changes the local
epoch, invalidating its old graph grants. Re-enroll explicitly after adoption.
Do not distribute a raw database as an untrusted data share.

Source rollback is an ordinary reviewed revert. Never move a restricted bearer
into an unrestricted registry to recover service. Withdraw its grant or remove
its verifier and restart, then use separately authorized operator recovery.
