# Shared signing distribution preparation

The shared `quipu-signing` 0.1.0 source package remains `publish = false`.
Quipu and Yupana still use their existing implementations. This extension
prepares an exact package and defines future registry readback checks; it does
not publish, establish package ownership, or authorize consumer migration.

## Routes

The existing `.github/actions/crates-publish` callers are unchanged. Its default
package is `quipu-ai`, with `Cargo.toml` and no shared opt-in. Existing root
publication steps, credentials and dry-run behavior remain the same. Before
those steps, an offline check refuses a future shared dependency until actual
distribution admission exists. The current root has no such dependency.

Shared preparation requires all of these inputs:

| Input | Required value |
| --- | --- |
| `package-name` | `quipu-signing` |
| `manifest-path` | `crates/quipu-signing/Cargo.toml` |
| `expected-version` | `0.1.0` |
| `shared-package-opt-in` | `prepare-quipu-signing-0.1.0` |
| `package-metadata` | Saved offline Cargo metadata JSON |
| `package-archive` | Saved offline `.crate` archive |

Unknown packages, arbitrary manifests, missing or incorrect opt-ins, version
mismatches and rehearsal refs refuse before any publication step. The selected
shared route runs only the preparation program. It does not authenticate,
fetch, publish, invoke a network dry-run, create a release, or retry an upload.
No workflow trigger or permission is added. A successful preparation report has
`distribution_verified: false` and `publication_authorized: false`.

## Prepare locally

Use a dependency-prepared isolated environment and a private Cargo target.
The following commands perform metadata and packaging only; `--no-verify`
deliberately does not claim a compiler/test qualification. Run meaningful
checks under the deployment's ordinary HOME escape guard.

```sh
cargo metadata --offline --locked --no-deps --format-version 1 \
  --manifest-path crates/quipu-signing/Cargo.toml > metadata.json
cargo package --offline --locked --no-verify \
  --manifest-path crates/quipu-signing/Cargo.toml
python3 scripts/ci/signing-package.py prepare \
  --name quipu-signing --manifest crates/quipu-signing/Cargo.toml \
  --opt-in prepare-quipu-signing-0.1.0 --expected-version 0.1.0 --ref main \
  --metadata metadata.json --archive "$CARGO_TARGET_DIR/package/quipu-signing-0.1.0.crate" \
  > preparation.json
```

Commit the proposed source before final packaging. During author qualification,
Cargo may require `--allow-dirty`; preserve that result as a draft package, not
a release artifact. Do not add that option to an eventual publication lane.
The preparation program verifies actual Cargo metadata, the original and
normalized manifests, every archive member against source, and the complete
tracked package file set. It refuses omitted, additional, changed, linked,
unsafe or oversized contents. The report binds the archive SHA-256, original
manifest SHA-256 and individual source hashes. It contains no private paths.

## Future distribution admission and readback

Publication remains a separate, irreversible decision. An admitted operator
must first establish the exact registry name, owner and authorized publisher,
the reviewed source/commit, intended version, packaging method and workflow
identity. Do not infer any of these from this source proposal or a mock test.
Changing `publish = false` requires a subsequent admitted source change and a
new exact package/plan; today's archive is not a publish-ready artifact.

After that separate admission, a future implementation must acquire both the
exact version response from `/api/v1/crates/quipu-signing/0.1.0` and the exact
registry archive using the admitted registry client. Preserve response status,
origin, retrieval time and source identity. Transport failure, missing data,
yanked versions and ambiguous upload outcomes remain UNKNOWN or refusal.
Do not automatically retry an ambiguous upload. A `max_version` headline or a
successful upload response alone is insufficient.

The offline verifier can compare retained bytes:

```sh
python3 scripts/ci/signing-package.py verify --plan preparation.json \
  --registry-version exact-version.json --archive registry.crate
```

It requires the exact name/version, explicit `yanked: false`, matching registry
and prepared archive checksums, and matching source witnesses. It reports only
`artifact_verified`, with caller-supplied provenance explicitly stated. It does
not authenticate how the files were acquired or declare distribution complete.
Synthetic fixtures prove comparisons, not registry acceptance.

`root-ready` currently refuses every shared dependency even after this offline
comparison. A subsequent admitted integration must establish actual acquisition
and publisher authority before lifting that boundary. Aliased, target-specific,
build, dev and workspace dependencies are inspected; registry requirements must
pin `=0.1.0`, and path/git/alternate-registry shortcuts refuse. This keeps a root
release from progressing against an unavailable or unqualified shared version.

Only after actual distribution qualification may reviewed consumer migrations
make Quipu, Yupana and the sync client use one exact crate, remove the Yupana
mirror, and prove the same envelope across real consumers. Those remain open
acceptance work. No key, registration, trust or runtime configuration changes
are implied by package preparation or artifact verification.

## Verification and rollback

```sh
python3 scripts/ci/test_signing_package.py
bash scripts/ci/crates-publish-guard.sh --selftest
```

The controls include a real execution of the composite action's shared shell
steps, unchanged legacy root-step comparison, positive preparation and exact
byte verification, missing opt-in and identity refusals, omitted/tampered source,
failed/UNKNOWN registry data and root dependency ordering. Registry fixtures are
synthetic and use no credentials or network. The CI pre-commit job runs them.

A source revert restores the old action and removes this inert preparation
route; existing callers need no edits. Preserve any generated plan and evidence
for review. A source revert cannot undo immutable registry publication. Any
future publication rollback, yanking or ownership action needs its own explicit
operator decision. No such action is performed here.
