# Native client changeset admission

The source-only `client_changeset::master` library admits conflict-free ROOT
proposals. It installs no HTTP/MCP route, profile, signing key or production
grant. Real grant issuance requires the operator's trust-anchor decision.
Named graphs and resolver-selected conflicts remain unsupported and refuse.
The existing client v1 envelope and public merge core are unchanged.

The master reserves its writer before reading current key registration,
lineage, shapes, historical base or operation receipt. It reconstructs the
base from retained master history and rejects a different base hash, epoch,
audience or shape registry. Historical rows closed after the frozen cursor
retain their original open interval. Originally bounded intervals refuse.
Both the base and current ROOT are limited to 10,000 rows before hydration;
larger scopes require a separate bounded admission design.

An owner's local `ClientWriteGrant` binds one registration, signer, public
key, store, epoch, ROOT and explicit predicate set to a validity window.
Newer revisions revoke or replace grants. The admission reader requires a
current governed registration holding that exact key. Artifact `attests`
alone never grants writes. There are no implicit predicates, wildcard graph
grants or fallback to a bearer. The local grant and receipt tables are not
RDF enrollment or share-import surfaces. No production grant is issued by
publishing this library.

The existing share merge core computes and persists the three-way plan.
Unresolved slots write nothing. Normal ownership, merge-risk, governance and
incremental SHACL gates remain active. Admission also validates the actual
full merged ROOT against current shapes inside the same writer reservation.
A missing validator execution, error or nonconformance refuses and rolls back.

Facts, provenance and the native typed `ChangesetReceipt` commit in one outer
transaction. The receipt is protected local data; this slice invents no RDF
receipt class. Its operation ID is unique. An identical signed fingerprint
returns the original receipt after verifying the original key; it does not
exercise current write authority again. Changed content under that ID refuses.
A lost response can therefore be retried after reopening the store. An API
call inside someone else's transaction refuses rather than claiming a durable
acceptance before that transaction commits.

This source slice does not establish full P3 acceptance, production routing,
real grant issuance, browser admission, named-graph validation or signed
conflict-resolution admission. Keep journals and committed receipts during
rollback; withdraw future routing rather than rewind accepted history.
