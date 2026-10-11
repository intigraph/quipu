# Installation

## As a Rust Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
quipu = { package = "quipu-ai", git = "https://github.com/scbrown/quipu" }
```

To use SHACL validation (enabled by default):

```toml
[dependencies]
quipu = { package = "quipu-ai", git = "https://github.com/scbrown/quipu", features = ["shacl"] }
```

To exclude SHACL (smaller binary, faster compile):

```toml
[dependencies]
quipu = { package = "quipu-ai", git = "https://github.com/scbrown/quipu", default-features = false }
```

## From Source

`qp` is the preferred command name; `quipu` remains supported. Install both
names plus the companion server with:

```bash
just install
# Optional destination directory:
just install "$HOME/.local/bin"
qp --version
quipu --version
```

`qp` is a relative symlink to `quipu`, so both execute the same binary after
upgrades. The installer refuses an existing conflicting `qp` in the destination
or on PATH. It never replaces another tool's command name. Release archives
also carry this relative link beside both binaries.

To add the command name to an existing installation without opening a store:

```bash
quipu install-alias --check
quipu install-alias
```

```bash
git clone https://github.com/scbrown/quipu
cd quipu
cargo build --release --features full
```

This produces two binaries:

- `target/release/quipu` -- CLI tool
- `target/release/quipu-server` -- REST API server

`quipu-server` requires **both** `onnx` (the embedding runtime it uses to
auto-embed queries) and `server` (axum, tower-http and tokio — the HTTP stack is
feature-gated so the library does not carry a web server; `server` also implies
`remote`, the federation client). The `full` bundle is what releases are built
with and enables both. Neither is a default feature, so a
plain `cargo build --release` produces only the `quipu` CLI and **silently omits
the server** (cargo skips a bin whose `required-features` are off). If you only
want the CLI, `cargo build --release` is enough. Verify the server built:

```bash
ls target/release/quipu-server
```

Keep the CLI and server from the same build beside each other for `quipu mcp`.
Both `quipu --version` and `quipu-server --version` report `git_sha` and
`git_dirty` without opening a database or loading configuration. A source archive
without Git metadata reports `git_sha: unknown`; a dirty build reports
`git_dirty: true`. The HTTP `/version` response retains its existing format.

## The Full Stack (caboodle)

To install Quipu as part of the whole knowledge stack —
[caboodle](https://github.com/scbrown/caboodle) interviews, plans, applies,
and verifies the installation of every stack tool — use the wrapper script:

```bash
# Phase one: install caboodle if absent, write a reviewable plan, STOP.
scripts/install-stack.sh --profile kg

# Phase two: after reviewing caboodle-plan.toml, apply + verify,
# then verify and load knowledge packs into the target store.
scripts/install-stack.sh --profile kg --yes \
  --pendant domain.pendant.db --db my.db
```

The two-phase gate is deliberate and mirrors caboodle's own doctrine: nothing
installs until the written plan has been reviewed (or `--yes` given
explicitly). Every `--pendant` is checked with `quipu pack --verify` before
`quipu unpack` — a content-hash mismatch refuses the pack rather than
installing silently corrupted knowledge. `--dry-run` prints every command the
script would run and executes nothing; `--profile` selects the caboodle
profile (default `kg`).

## Python Client

The REST API has a thin, zero-dependency Python client under `python/`:

```bash
pip install ./python
```

See the [Python Client reference](../reference/python-client.md).

## Requirements

- Rust 1.85+ (edition 2024)
- SQLite is bundled via rusqlite -- no system dependency needed
- Python >= 3.11 for the optional Python client (stdlib only)
