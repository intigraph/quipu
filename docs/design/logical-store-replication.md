# Logical full-store replication source components

The scoped live-fact bootstrap does not reproduce a whole queryable store:
it limits explicit graph scope, projects current facts, and omits stored
queries, parameters and datasets. A fact transaction cursor also misses
metadata writes that do not create a fact transaction. The logical profile
preserves typed table rows and follows those writes through one journal.

These are library components. The HTTP producer, follower process,
credential provisioning, freshness protocol and read routing are not wired
to this profile. Existing scoped endpoints retain their current contract.
Nothing initializes the journal when a store opens or a read is served.

| Component | Contract |
| --- | --- |
| `store_stream::initialize` | Explicit local writer action; atomic journal/triggers, fresh public generation, exact schema digest. |
| `store_stream::write_snapshot` | One SQLite read snapshot; typed JSONL to a private staging writer; complete receipt only after success. |
| `store_replica::FullStoreManifest` | Exact source/epoch, journal generation/revision, fact head, catalog, row/byte counts and spool digest. |
| `SnapshotTrust` | Independently pinned source key, lineage, epoch, generation, catalog and exact cut hash; never derived from the response. |
| `Applier::bootstrap_logical_store` | Source signature, locally recognized schema and full spool verification; data and composite progress publish atomically. |
| `Applier::apply_logical_revision` | Complete signed committed prefix; exact keys and typed values, atomic data/progress, checked replay and gap refusal. |

The source catalog covers all local graph registrations, including later
graphs, and preserves dictionary IDs, transaction IDs, full fact intervals,
operations and history. Queries, parameters, datasets, shapes and other
explicitly classified data travel as rows. Received SQL is never executed:
the receiver compares schema against its local build and binds typed values
into locally constructed statements. Unknown tables, attachments and active
frozen-pack closure refuse instead of silently disappearing.

Source-local ownership, credentials, authentication judgments, nonces and
replication state do not become receiver authority. Historical transaction
actor/source content remains data. The catalog and profile are explicit;
they do not establish support for an external provider or its configuration.

The receiver must match this build's untouched initial data, including its
ROOT/meta/dictionary seeds. Existing non-fact data or replication evidence
refuses before any replacement. Bootstrap cannot adopt a legacy cursor.
Logical mode refuses legacy fact-only frames, and reopen requires the same
admitted descriptor. Journal position and global fact head live in the same
persisted progress row; metadata-only changes advance only the former.

Snapshot output is staging until its receipt, signature and independent
admission are verified. An output error or cap leaves an unusable partial
spool. Source rows are capped at one MiB before Rust decoding, encoded records
at eight MiB, and total output at the caller's explicit byte budget. Journal
frames cap rows and canonical bytes; a gap or oversized prefix requires a
fresh bootstrap, never partial transaction publication. Production spool
headroom, cut deadlines and journal retention still need their transport and
resource contracts before activation.

Private controls compare the scoped baseline with the logical bootstrap,
exercise actual parameterized named queries and datasets, preserve raw table
and temporal history, and cover metadata-only updates, later graphs,
signature/descriptor tampering, interrupted spool rollback, malformed signed
tail rollback, exact redelivery and destination-data preservation. A source
signature or heartbeat alone does not establish freshness. Runtime activation
requires separately admitted auth, trust, serving configuration, rollback and
routing; source checks do not grant any of those.
