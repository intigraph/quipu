# Required session keys

The optional `[quipu.server]` setting below uses the existing protected session
registry and request-attestation format. It is disabled by default for staged
migration; adding this source does not enroll users or change a running server.

```toml
session_keys = { required = true, onboarding_read_paths = ["/health"] }
```

Required mode refuses unsigned requests even if they carry a valid shared or
named bearer. The only configurable onboarding exemptions are unique exact
`/health` and `/version` paths for GET/HEAD. Wildcards, query/write endpoints and
duplicates fail startup validation. OPTIONS is refused outside CORS: migrate
browser clients through the same origin or an authenticated broker before
activation. An unsigned CORS preflight must not become an implicit exception.

A valid signature resolves the principal through the protected binding. Supplied
agent/session declarations must match that binding. POST `/identity` returns
only the verified principal, key ID, session and authentication class; a bearer
cannot manufacture its result. Read requests use the same signature/replay
checks as the four audited mutations (`/knot`, `/update`, `/episode`,
`/graph/create`). Other signed mutations and URL query strings currently refuse;
this is a migration constraint, not full client compatibility.

Install server and client support before enforcement. Inventory required callers,
prepare distinct OS users and private homes, enroll public bindings using the
existing privileged introducer, and prove own-key identity, wrong-key rejection,
replay rejection and actual cross-UID key denial with synthetic controls first.
Do not copy live private keys or grant the model a root/broker credential. A
shared Unix UID with mode 0600 files provides no cross-agent isolation.

Keep the previous server/client configuration and public registry inventory as
the named rollback. During the reviewed security migration, test an actual signed
read/write and an unsigned refusal before continuing. If smoke fails, restore
the prior enforcement configuration without deleting keys, bindings or graph
data. No automatic registration, key issuance/revocation or host-user migration
is performed by this option. Existing operator and release holds still apply.
