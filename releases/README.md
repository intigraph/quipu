# Breaking-change records and read-only doctor plans

A record describes who is affected, how to detect the problem, prerequisites,
ordered corrections, validation and rollback limits. It does not grant migration
or release authority. The first record is the **draft** w3id vocabulary change:
affected releases, platform bindings and native command packets remain unknown.
It must not ship as a complete correction record.

From this source checkout, install the pinned authoring dependency in an isolated
Python environment (`jsonschema==4.23.0`). CI and the pre-commit hook declare it.
For example, prefix the following commands with
`uv run --no-project --with jsonschema==4.23.0 --` if it is not installed.

```sh
just breaking validate
just breaking validate releases/changes/w3id-writer-vocabulary-transition.json --release
just breaking render --write
just breaking render --check
just breaking test
just breaking demo
```

The second command deliberately exits 1 while the record is incomplete. Validation
checks the local JSON Schema, rejects extra fields and duplicate JSON keys, and
supports only known detection/correction operations. Ready records must identify
all affected **exact three-component SemVer versions** and their supported platforms, independent review
and approval references, and content-bound correction and rollback packets.
No version-range guessing is performed. `--release` checks structural completeness;
it never authenticates a receipt or authorizes publication. The promotion gate
must independently verify the record digest and external approval receipts.
Every breaking release requires Stiwi's approval under the release design.

Candidate `payload.frozen.plan.breaking_records` entries are ordered `{id, sha256}`
file bindings. For each selected entry, consumers can run:

```sh
just breaking validate record.json --release \
  --expected-id RECORD_ID --expected-sha256 RECORD_FILE_SHA256
```

Supply both expected fields together. ID or digest mismatch refuses, including a
whitespace-only file change. Output includes the observed file SHA256; approval
authentication remains false and belongs to the promotion gate. Candidate order
and envelope authenticity are verified by that consumer, not reimplemented here.


One deterministic renderer produces [the correction changelog](BREAKING-CHANGES.md)
and the book's breaking-change page. The root changelog links to these records.
`render --check` and the behavioural controls run in the existing required
Pre-commit checks path; stale output fails. `render --write` is an explicit local
authoring operation that writes those two documentation files only.

## Collect evidence before planning

The doctor is a **source utility**, not a new installed `quipu doctor` or Tambo
subcommand. Tambo/promotion consumers can use its versioned JSON response. It
makes no network calls, writes no files, and has no `--apply` option. It consumes
a locally supplied native-reader snapshot; evidence is reported as
`supplied-unverified`, never as independently authenticated.

```sh
just breaking doctor record.json observation.json \
  --candidate-id SELECTED_CANDIDATE_SHA256 \
  --artifact-sha256 INSTALLED_ARTIFACT_SHA256
```

Both digests come from the caller's independently verified candidate/inventory
manifest. The record itself does not contain its enclosing candidate digest or
the digest of a binary that might embed it: that would create a digest cycle.
Candidate manifests bind record IDs and SHA256 separately. Artifact verification,
transport and authorization stay with those consumers.

A `breaking-observation/v1` JSON object must contain exactly these fields:

| Field | Required evidence |
|-------|-------------------|
| `record_id`, `record_sha256` | Exact selected ID and SHA256 of the record's file bytes |
| `candidate_id` | Selected 64-character lowercase SHA256 |
| `observed_at` | Timezone-aware ISO timestamp, at most 300 seconds old, never future |
| `scope` | Exact `tool`, `installed_version`, `artifact_sha256`, `platform`, known `subject_iri`, unique explicit `datasets` |
| `read_status`, `complete`, `truncated` | `success`, `true`, `false`; anything uncertain refuses |
| `positive_controls` | One `{dataset, count}` per selected dataset, integer count at least 1 |
| `expected_event_id` | Known input event ID, independent of the response |
| `reader_terms` | Observed supported canonical and historical reader predicates |
| `rows` | Exactly one `{dataset, subject, predicate, value}` native identity binding |

Include `schema_version: "breaking-observation/v1"`. Do not infer complete graph
coverage from a default/ROOT query, or report an empty control as healthy. A
collector must establish the exact dataset and bounded response coverage before
setting `complete=true`. Duplicate rows remain ambiguous even with equal literals.
Observations bind the selected installed artifact, version and platform; a version
outside the declared record coverage is UNKNOWN. The detector covers the selected
identity and datasets, not every record or every service on the fleet.

Exit 0 means `NOT_AFFECTED`: the known scoped identity reads correctly and both
historical and canonical term coverage is present. Exit 1 means `AFFECTED`: a
reader lacks that coverage. Exit 2 means `UNKNOWN`: incomplete draft, missing,
denied, malformed, stale, duplicate or unbound evidence. Neither 0 nor 1 establishes
release readiness. `--at` supplies an explicit timezone-aware replay time for
offline evidence; it does not make an old snapshot a current observation.

Only AFFECTED produces a plan. Its fixed local operation is
`managed-reader-first-writer-upgrade/v1`, with packet reference/digest and scoped
steps. Plans always say `executable=false`, `requires_separate_admission=true` and
`release_authorized=false`. Record prose and URLs are never evaluated as code.
UNKNOWN and NOT_AFFECTED produce no plan. New detector/operation kinds require a
reviewed source change rather than a remotely supplied command.

The w3id correction preserves instance IRIs and signing bytes. Install dual readers
and required shapes before writers. After canonical writes exist, rolling back to
an old-only reader is unsafe: retain dual readers, stop the scoped writer, and fix
forward or use a separately admitted restore with its stated data-loss limit.
No replay of uncertain effects or global namespace replacement is authorized.
