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

This produces one runtime executable, `target/release/quipu`. Start HTTP with
`quipu serve`, or native stdio MCP with `quipu mcp`. Both enter the same compiled
server and cannot select a stale companion. `just install` and release archives
retain `quipu-server` as a relative alias for one transition release.

The `full` bundle enables the HTTP and embedding features. A default CLI-only
build remains supported and refuses `serve` and native MCP explicitly before
opening configuration or a store. The ONNX runtime is loaded only when embedding
work needs it; pure version/help inspection requires no runtime dylib.

`quipu --version` and `quipu serve --version` report the same compiled `git_sha`
and `git_dirty` without opening a database or loading configuration. A source
archive without Git metadata reports `git_sha: unknown`; a dirty build reports
`git_dirty: true`. HTTP `/version` retains its existing response format.

## With Cargo

Cargo installs the executable targets; finish the installation with the alias
command to provide both supported names:

```bash
cargo install --git https://github.com/scbrown/quipu --locked --features full
quipu install-alias
qp --version
```

`install-alias` uses the directory containing the running Quipu executable,
including Cargo's `--root` destination. It refuses conflicting `qp` names and
verifies the same inode and exact version output. `--check` only checks for
collisions and does not create the link.

`just install` resolves its root as `QUIPU_INSTALL_ROOT`, then
`CARGO_INSTALL_ROOT`, then `$HOME/.local`, and installs into that root's `bin`
directory. Its optional directory argument overrides this resolution.

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
