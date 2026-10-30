# Shared signing distribution and consumer acquisition

The shared `quipu-signing` 0.1.0 package was published to crates.io on
2026-10-30. The immutable archive checksum is
`9590d3ae3ad7383f3b3325fd7649e873f26026460fc9767251028aa2a00c6575`;
its VCS receipt identifies commit `3943cd305e8d271a7ed481493ba22a307fa675a8`.
All eight tracked package files match that source. Quipu now depends on the
exact registry version and re-exports its cryptographic primitives through the
existing signing adapter. The native replication client reaches the same
verification primitive without changing its domain-separated wire message.
Yupana migration and full three-consumer parity remain separate acceptance work.

The preparation route below is retained for the historical unpublished artifact.
It deliberately still requires `publish = false`; it is not a way to republish
or modify the immutable 0.1.0 release.

## Routes

The existing `.github/actions/crates-publish` callers are unchanged. Its default
package is `quipu-ai`, with `Cargo.toml` and no shared opt-in. Existing root
publication steps, credentials and dry-run behavior remain the same. Before
those steps, the offline check requires the exact admitted registry version and
checksum in Cargo.lock for the shared dependency. The current root uses that
qualified route; existing callers need no input changes.

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

## Historical preparation admission and current readback

The following preparation contract predates the published release described
above. It remains a historical, inert route rather than authority for another
upload. Publication is a separate, irreversible decision. An admitted operator
must first establish the exact registry name, owner and authorized publisher,
the reviewed source/commit, intended version, packaging method and workflow
identity. Do not infer any of these from this source proposal or a mock test.
Changing `publish = false` requires a subsequent admitted source change and a
new exact package/plan; today's archive is not a publish-ready artifact.

Actual distribution qualification acquired both the
exact version response from `/api/v1/crates/quipu-signing/0.1.0` and the exact
registry archive through normal public registry acquisition. Such evidence
must preserve response status,
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

`root-ready` admits this one qualified dependency only when the manifest pins
`=0.1.0` and Cargo.lock resolves exactly one `quipu-signing` package from the
standard crates.io registry with the archive checksum above. Cargo validates the
downloaded bytes against that checksum. Missing locks, other versions or checksums,
path/git/alternate-registry substitutions and ambiguous package entries refuse.
Aliased, target-specific, build, dev and workspace dependencies retain the same
manifest checks. A caller-supplied preparation or registry fixture cannot replace
this reviewed immutable acquisition identity. The preflight remains offline and
does not publish, fetch, register a publisher or grant trust.

Use `just signing locks` for the root, Python binding and browser consumer lockfiles;
`just signing package-controls` runs the positive and negative acquisition gates.
No dependency-wide upgrade, release event or production key is needed.

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

Reverting the consumer migration restores the local signing implementation and
removes the root registry dependency; retain coherent lockfiles. The historical
preparation route remains inert. Preserve any generated plan and evidence
for review. A source revert cannot undo immutable registry publication. Any
future publication rollback, yanking or ownership action needs its own explicit
operator decision. No such action is performed here.
