# CI/CD and Releases

> **Implementation status (2026-07-23, kelly):** ✅ **Implemented.** `.github/workflows/ci.yml`
> has correctness, invariant, and housekeeping jobs; `release.yml` uses
> `release-plz-action@v0.5` behind the exact-SHA correctness aggregate;
> `docs.yml` builds the mdBook and deploys to Pages. `release-plz.toml`, `cliff.toml`,
> `.pre-commit-config.yaml`, and `justfile` all present. (A `changelog-check.yml`
> guard now exists too — additive.) Verified by reading the workflows.

Quipu uses GitHub Actions for continuous integration and release-plz for
automated versioning and changelog generation.

## CI Pipeline

Every push to `main`, pull request and combined merge-queue tree runs the seven
required checks with unchanged names: **Format**, **Clippy (default)**,
**Clippy (shacl)**, **Test (default)**, **Test (shacl)**, **Build** and
**Release correctness**. The independent `review/approved` requirement is unchanged.
Release correctness still requires generated-doc drift, SHACL write-path checks,
pre-commit controls and the fast conformance expectation controls.

Pendant review, shared signing, installer controls, Markdown lint, changelog
checks and source/shape invariants remain PR checks. In particular,
**Generated docs drift** always checks committed completions and man pages.
PR runs cancel superseded heads; main and merge-queue verdicts finish.

| Expensive surface | PR coverage | Nightly coverage |
|-------------------|-------------|------------------|
| Extended features, full build, three-OS ONNX, string-filter and mixed-load ratchets | Nightly | Every run |
| Seeded merge load and full W3C conformance re-derivation | Nightly | Every run |
| Python wheels, sdist and installed-artifact tests | Python/binding paths and their workflow controls | Full platform matrix |
| Wasm harness and browser bundle | Wasm sources, shared actions, lockfiles and their controls | Every run |
| Book build | Documentation sources and browser assets | Main source changes also rebuild Pages |

The CI schedule at 05:17 UTC is the single heavy-suite orchestrator. Python,
merge-load and conformance share their existing definitions through reusable
workflows, so nightly coverage cannot diverge into another copy. A manual CI
run exercises the same gate without publishing. `just ci test` checks this topology
and the release guards offline, including failure and missing-evidence cases.

### Experimental nightly channel

Only a scheduled run on `main` can publish a nightly GitHub prerelease. All
nightly suites must succeed: failed, cancelled, missing or skipped suites refuse
**Nightly correctness**. Publication consumes the Linux x86_64 full-feature
CLI/server package already built in that run, verifies its checksum and embedded
source SHA, and requires the gate to name that same SHA. It never rebuilds after
the gate. Tags are immutable `nightly-<run-id>-<full-commit-sha>` prereleases, excluded from
`latest`; an existing different package is refused rather than overwritten.

To opt in, select the exact experimental nightly in the repository's Releases,
download `quipu-nightly-linux-x86_64.tar.gz` and its `.sha256`, verify with
`sha256sum -c`, then extract into a separate directory. Keep the previous
installation and a store backup before trying it. Consult `FEATURES.txt` for
ONNX runtime/model requirements. Experimental compatibility and schema changes
can break running systems; rollback may require the pre-upgrade store backup.
This channel does not publish crates, PyPI packages or Homebrew formulae.

A red scheduled gate or publication updates the single **Nightly correctness
failure** issue with the source SHA, failed-stage results and run URL. It does
not silently ship a partial nightly. Stable promotion is a separate release
review after a seven-day soak; this workflow never promotes a nightly or changes
the stable release workflow. A deferred optional failure is therefore discovered
on nightly rather than during an unrelated PR; fixes retain their PR-local tests.

## Release Automation

Pushes to `main` trigger the release workflow (`.github/workflows/release.yml`).
Before release-plz runs, it waits for CI's `Release correctness` check on the
exact pushed SHA. Formatting and Markdown lint remain visible CI checks but do
not block release. Publication uses
[release-plz](https://release-plz.ieni.dev/) after a reviewed release PR merges.
A separate daily workflow prepares the version and changelog candidate at
07:00 UTC, so ordinary main merges do not rewrite that review window.
The two commands together:

1. Analyze conventional commits since the last release
2. Bump the version in `Cargo.toml`
3. Generate a changelog from commit messages
4. Create a GitHub release with a git tag

If the wait budget expires while correctness is queued, running, absent or
unreadable, the waiter reports **DEFERRED**, emits `ready=false`, and skips
release-plz. This is neither a code failure nor permission to publish. A genuine
completed non-success check still fails the waiter. `release-ci-ready.yml`
observes both CI and Release completion, then rearms only an initially deferred
Release whose exact SHA is still main and whose genuine correctness check is
terminal. It retries once, using the existing gate again. Superseded commits,
pull requests, foreign repositories and already retried runs are never rearmed.
If that one retry remains unready, inspect the recorded reason before an operator
reruns it; no deadline or completion event substitutes for a successful check.

### Conventional Commits

Commit messages drive changelog categories:

| Prefix | Category |
|--------|----------|
| `feat:` | Added |
| `fix:` | Fixed |
| `refactor:` | Changed |
| `doc:` | Documentation |
| `test:` | Testing |
| `chore:` | Miscellaneous |
| `ci:` | CI/CD |

Commits with `security` in the body get a **Security** category.

### Configuration

- **release-plz.toml** -- enables git releases and tags, disables crates.io
  publishing
- **cliff.toml** -- [git-cliff](https://git-cliff.org/) template for
  changelog formatting with GitHub links and commit SHAs

## Documentation Deployment

The docs workflow (`.github/workflows/docs.yml`) builds the mdbook and
deploys to GitHub Pages on pushes to `main`:

1. Build: `mdbook build docs/book`
2. Deploy: upload to GitHub Pages (main branch only)

## Pre-commit Hooks

Local development uses pre-commit hooks (`.pre-commit-config.yaml`):

- Trailing whitespace and end-of-file fixes
- YAML and JSON validation
- Merge conflict detection
- Large file checks
- Markdown linting (markdownlint-cli2)
- File size limits (warn at 400 lines, error at 500 for Rust source)

Install hooks with:

```bash
just setup
```

## Quality Gate

Before pushing, run the full quality gate:

```bash
just check       # All pre-commit hooks
just docs check  # Markdown lint + mdbook build
```
