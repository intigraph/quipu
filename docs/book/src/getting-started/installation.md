# Installation, upgrades and removal

Choose one installation owner for each executable directory. Mixing Cargo,
Homebrew, apt and manual copies can leave a different version first on `PATH`.
The CLI command is `quipu`; `qp` is its relative alias. Native MCP also needs
`quipu-server` from the same release beside the CLI.

| Method | Prerequisites | Executable location |
|---|---|---|
| Release installer | Python 3.9+, curl, Linux/macOS | `$STACK_INSTALL_ROOT/bin`, default `~/.local/bin` |
| Release archive | curl, tar, mktemp, SHA-256 utility, matching platform | Your selected directory |
| Homebrew | macOS, Homebrew | `$(brew --prefix)/bin` links into the Cellar |
| apt | Debian/Ubuntu amd64 or arm64, sudo, curl, GnuPG | `/usr/bin` |
| Cargo | Rust toolchain, native build prerequisites | `$CARGO_INSTALL_ROOT/bin` or `~/.cargo/bin` |
| Source checkout | Rust toolchain, Git, just | Selected directory, default `~/.local/bin` |
| Tambo | Tambo and a reviewed plan | The installer selected by the plan |
| Rust dependency | Rust toolchain and Cargo project | Project dependency, not a CLI installation |

Binary installers do not create your knowledge database, register an agent or
start a service. Running Quipu can create data; see
[configuration and data removal](#remove-configuration-and-data) separately.

## Verification scope

The release installer and archive commands have been exercised with the
published Linux x86_64 bundles, including an upgrade and removal. The Python
client has been installed and removed in a dedicated virtual environment.
Cargo installation of `quipu-ai` 0.15.0 with `full`, alias creation, all three
version checks, uninstall and owned-alias removal passed on Linux x86_64 in
a clean HOME with a copied Rust 1.98 toolchain. Unrelated data was preserved.

The Homebrew, apt, source-checkout, Rust-library, Tambo apply/removal and
container procedures below are **not yet independently exercised for this
reference revision**. Their commands are derived from the current source and
package definitions; they are not a claim of a successful installation on your
platform. Use the listed version and removal checks to verify that boundary.

## Release installer

Supported release targets are Linux x86_64/aarch64 and macOS x86_64/arm64.
Download the installer for inspection, then run it:

```sh
curl --fail --show-error --location \
  https://raw.githubusercontent.com/intigraph/quipu/main/install.sh -o install-quipu.sh
sh install-quipu.sh
```

Set variables on the **receiving shell**, not on curl, for a pinned release or
custom prefix:

```sh
STACK_VERSION=quipu-ai-v0.15.0 STACK_INSTALL_ROOT="$HOME/.local" sh install-quipu.sh
"$HOME/.local/bin/quipu" --version
"$HOME/.local/bin/qp" --version
"$HOME/.local/bin/quipu-server" --version
```

The installer verifies the archive checksum, safe extraction and executable
version before publishing links. The full bundle and `.release.json` receipt
live under `PREFIX/lib/quipu/TAG`; `PREFIX/bin/{quipu,qp,quipu-server}` point into
that bundle. Add `PREFIX/bin` to `PATH`.

**Upgrade:** rerun the installer without `STACK_VERSION` for the latest stable
release, or select another published tag. Previous bundle directories remain
available. Verify all three commands after changing versions.

**Remove:** set `prefix` to the prefix actually used. Inspect the links first;
remove only commands owned by this installation:

```sh
prefix="$HOME/.local"
ls -l "$prefix/bin/quipu" "$prefix/bin/qp" "$prefix/bin/quipu-server"
rm -- "$prefix/bin/quipu" "$prefix/bin/qp" "$prefix/bin/quipu-server"
rm -r -- "$prefix/lib/quipu"
```

This removes every retained Quipu release in that prefix, not user databases.
Remove a dedicated PATH entry from your shell configuration if no other tool
uses the directory. Do not remove the whole `~/.local` directory.

## Release archive

Use the archive matching your operating system and architecture. For Linux
x86_64, with an explicit published version:

```sh
(
  set -eu
  V=0.15.0
  T=x86_64-unknown-linux-gnu
  A="quipu-quipu-ai-v$V-$T"
  BASE="https://github.com/intigraph/quipu/releases/download/quipu-ai-v$V"
  download_dir=$(mktemp -d)
  trap 'rm -rf "$download_dir"' EXIT
  curl --fail --show-error --location "$BASE/$A.tar.gz" -o "$download_dir/$A.tar.gz"
  curl --fail --show-error --location "$BASE/$A.tar.gz.sha256" -o "$download_dir/$A.tar.gz.sha256"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$download_dir" && sha256sum -c "$A.tar.gz.sha256")
  else
    (cd "$download_dir" && shasum -a 256 -c "$A.tar.gz.sha256")
  fi
  tar -xzf "$download_dir/$A.tar.gz"
  "./$A/quipu" --version
  "./$A/quipu-server" --version
  "./$A/qp" --version
)
```

The subshell stops on a failed download or checksum and removes its temporary
downloads on exit. It extracts the verified bundle into your current directory;
a successful run keeps that extracted directory.

Other target names are `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin` and
`aarch64-apple-darwin`. Keep the extracted bundle intact: it contains the
companion server and supporting files, not just the CLI. Add its absolute
directory to `PATH`, or use the release installer to manage command links.

**Upgrade:** download and verify the next bundle into a different directory;
change your PATH or dedicated links after all three version checks succeed.
Do not extract a new archive over a running installation.

**Remove:** remove your PATH entry or links, then the exact extracted directory
and any archive/checksum files you separately retained. The example above
cleans its temporary downloads automatically. No service is installed by
extraction. Database removal is independent.

## Homebrew

The tap name `scbrown/tap` resolves to the `scbrown/homebrew-tap` repository.
The Quipu formula supplies macOS Intel and Apple Silicon archives:

```sh
brew install scbrown/tap/quipu
brew_prefix=$(brew --prefix)
"$brew_prefix/bin/quipu" --version
"$brew_prefix/bin/qp" --version
"$brew_prefix/bin/quipu-server" --version
brew test scbrown/tap/quipu
brew list --verbose quipu
```

Use the explicit Homebrew prefix for these checks so a Cargo or manually
installed executable earlier on `PATH` cannot impersonate the formula.
The formula test checks the CLI version and the relative `qp` alias.

**Upgrade:**

```sh
brew update
brew upgrade quipu
brew_prefix=$(brew --prefix)
"$brew_prefix/bin/quipu" --version
"$brew_prefix/bin/qp" --version
"$brew_prefix/bin/quipu-server" --version
brew test scbrown/tap/quipu
```

Homebrew owns its Cellar paths and command links; do not manually overwrite them.
The current formula defines no Homebrew service.

**Remove and verify:**

```sh
brew_prefix=$(brew --prefix)
brew uninstall quipu
brew cleanup quipu
brew list --formula
for name in quipu qp quipu-server; do
  test ! -e "$brew_prefix/bin/$name" && test ! -L "$brew_prefix/bin/$name" || exit 1
done
```

Confirm that `quipu` is absent from the formula list. The executable/link checks
above refer to the Homebrew prefix: they do not remove or certify the absence
of a separate installation elsewhere on `PATH`. Use `brew untap scbrown/tap`
only if no other installed stack formula uses the tap. Homebrew removal does
not erase knowledge stores or shared config.

**Verification status: UNVERIFIED on macOS.** The commands match the current
formula, but have not been exercised for this reference on a clean macOS
installation. A Linux archive test is not a Homebrew installation test.

## apt

Requires Debian or Ubuntu, amd64 or arm64, curl, GnuPG and administrator rights.
Check the key fingerprint **before** adding the repository:

```bash
(
  set -euo pipefail
  key_file=$(mktemp)
  trap 'rm -f "$key_file"' EXIT
  curl -fsSL https://scbrown.github.io/apt/key.gpg -o "$key_file"
  fingerprints=$(gpg --batch --show-keys --with-colons "$key_file" \
    | awk -F: '$1 == "pub" { primary=1; next } primary && $1 == "fpr" { print $10; primary=0 }')
  test "$fingerprints" = AB5625C5612F643BF76464DF0DC99924994EC959
  sudo install -D -m 0644 "$key_file" /etc/apt/keyrings/scbrown-stack.gpg
  echo 'deb [signed-by=/etc/apt/keyrings/scbrown-stack.gpg] https://scbrown.github.io/apt stable main' \
    | sudo tee /etc/apt/sources.list.d/scbrown-stack.list
  sudo apt update
  sudo apt install quipu
)
quipu --version
quipu-server --version
dpkg-query -L quipu
```

Use the package file list to check whether your package provides `qp` and any
service unit. Do not create a manual alias inside a package-owned directory.

**Upgrade:** `sudo apt update && sudo apt install --only-upgrade quipu`, then
repeat the checks. Adding this repository does not itself enable unattended
third-party updates. [Repository channels and pinning](https://github.com/scbrown/apt#channels-and-pinning)
are controlled separately.

**Remove:** stop a Quipu service you explicitly configured, then
`sudo apt purge quipu`. Only if no other stack package uses this repository,
remove `/etc/apt/sources.list.d/scbrown-stack.list` and
`/etc/apt/keyrings/scbrown-stack.gpg`, then run `sudo apt update`.
Package removal does not remove project databases or shared configuration.
**Verification status: UNVERIFIED.** A clean-room Debian/Ubuntu install and
purge is still required; the disposable guest bootstrap is currently blocked.
Unpacking a `.deb` alone does not exercise package-manager scripts.

## Cargo executable installation

The package is **`quipu-ai`**, while the library and CLI are named `quipu`.
Use `full` to include the companion server:

```sh
cargo install quipu-ai --locked --features full
quipu install-alias --check
quipu install-alias
quipu --version
qp --version
quipu-server --version
cargo install --list
```

Cargo installs binaries into `CARGO_INSTALL_ROOT/bin`, `ROOT/bin` with `--root ROOT`, or
`~/.cargo/bin`. `install-alias` creates `qp` beside the running executable and
refuses another tool's conflicting name. A plain build without the server's
required features can omit `quipu-server`.

**Upgrade:** rerun `cargo install quipu-ai --locked --features full`; add
`--version VERSION` to select a published version, or `--force` only when you
intend to rebuild an already installed version. Repeat the alias and version
checks.

**Remove:** `cargo uninstall quipu-ai`, using the same `--root ROOT` if supplied
at installation. Cargo does not own an alias created afterward: inspect the
remaining `ROOT/bin/qp` and remove it only if it is the `quipu` symlink from this
installation. Do not remove other Cargo packages or the shared Cargo directory.

## Source checkout

```sh
git clone https://github.com/intigraph/quipu.git
cd quipu
just install
quipu --version
qp --version
quipu-server --version
```

`just install` builds both release executables with `full`, checks for alias
collisions, then installs the pair and relative alias. Destination resolution:
its explicit directory argument, then `QUIPU_INSTALL_ROOT/bin`, then
`CARGO_INSTALL_ROOT/bin`, then `~/.local/bin`. For example:
`just install "$HOME/quipu-tools/bin"`.

**Upgrade:** fetch the desired reviewed source revision and rerun `just install`
with the same destination. Keep a known-good binary pair for rollback.

**Remove:** remove the installed `quipu`, `quipu-server` and owned `qp` alias
from that destination. This method has no versioned `lib/quipu` bundle. Remove
the checkout and its `target/` only if you no longer need the source or build
cache. It does not install a service.

## Tambo and the stack wrapper

Tambo installs selected tools, not just Quipu. Start in a directory you will
retain and review the generated plan before applying it:

```sh
tambo --version
tambo init --guided
tambo apply
tambo verify
```

Select the knowledge-graph (`kg`) profile in the guided interview. For an
unattended plan, first provide a completed `caboodle-intent.toml`, then run
`tambo plan --profile kg`; `plan` refuses a fresh directory without that intent.

Read `caboodle-plan.toml` before `apply`. Preserve it and `.caboodle/` to resume
the same installation. The plan determines the package manager and installed
paths: inspect its Quipu entry rather than assuming Cargo or Homebrew.

**Upgrade:** use the installed Tambo version's supported update/apply commands
against that retained plan; verify the resulting tool versions and functional
checks. **Remove Quipu:** follow the removal procedure for the package manager
or binary prefix in the plan. Removing the plan is not an uninstall. Remove a
Tambo-created store only after backing it up and checking its exact path.
Do not delete `.caboodle/` if you intend to keep managing other installed tools.

The repository also contains `scripts/install-stack.sh`. Its current executable
contract invokes `caboodle plan/apply/verify` and can load explicitly supplied
pendants. Inspect `--help` and `--dry-run` before use; it is not an alias for
arbitrary Tambo subcommands. Its `--yes` path applies the whole selected plan
and can write the selected database. No separate uninstall command exists in
that wrapper: use the same per-tool owners and data-removal procedure above.

## Rust library dependency

In a Cargo project:

```toml
[dependencies]
quipu = { package = "quipu-ai", version = "0.15" }
```

Verify with `cargo check`. Enable required capabilities with Cargo features;
`shacl` supplies validation support, while `full` also includes the
server-related capabilities. **Upgrade:** update the version requirement deliberately and run
`cargo update -p quipu-ai && cargo check` with your application's tests.
**Remove:** remove the dependency entry and its imports, then run `cargo check`
and let Cargo update the lockfile. A dependency does not install CLI commands,
aliases, services or a global database. Do not use `cargo uninstall` for it.

## Container and MCP Registry

`server.json` declares `ghcr.io/intigraph/quipu:<version>` for the MCP Registry,
stamped by the release that published it; that registry pin is independent of the
latest native release. A compatible
Docker runtime is required. For a deliberately selected image:

```sh
docker run --name quipu-mcp -i -v quipu-data:/data ghcr.io/intigraph/quipu:<version>
```

Verify the image/version before configuring an agent; use the
[Agent Setup connection control](agent-setup.md#verify-the-clients-connection)
afterward. **Upgrade:** select and verify another published image, stop the
old container, and recreate it with the same intended volume and configuration.
**Remove:** `docker stop quipu-mcp && docker rm quipu-mcp`. Container removal
retains the `quipu-data` volume. `docker volume rm quipu-data` **permanently
deletes its database**; run it only after a verified backup and after checking
that no other container uses that volume. Remove the client registration too.

## Python client

The optional zero-dependency REST client is under `python/` and requires
Python 3.11+. Install it in a dedicated virtual environment with
`python -m pip install ./python`. Verify with `python -m pip show quipu-client`.
Upgrade by installing the desired checkout into that environment. Remove with
`python -m pip uninstall quipu-client`, or remove the dedicated virtual
environment after disconnecting its users. The client does not install a
server, alias or local graph. It does not erase data held by a remote server.

## Remove configuration and data

First stop your own Quipu processes and disconnect agents. None of the binary
methods above automatically creates a systemd or launchd service. If you added
one, stop and disable **that exact unit**, remove its unit/plist file, then
reload the relevant manager. Check both user and system managers as applicable;
do not guess a service name or stop an unrelated shared service.

Configuration can come from project `.bobbin/config.toml`, global
`~/.config/bobbin/config.toml`, explicit flags and environment. These files can
also configure other stack tools: remove the **Quipu sections** you own, not the
whole shared file. Remove dedicated token files and agent registrations only
after their consumers are disconnected.

The default store is `.bobbin/quipu/quipu.db` relative to the working directory;
`--db` or configuration can select another location. Record the
actual paths before uninstalling. Store files, SQLite sidecars, attachments,
embeddings and other configured data are separate from executables.

**Deleting a store is irreversible without a backup.** Back it up first,
confirm no server or client still has it open, and remove only its dedicated
data directory. Never delete `.bobbin/` wholesale: other tools may use it.
Finally run `command -v quipu`, `command -v qp` and `command -v quipu-server` in
a new shell. Any surviving command must be attributed to another installation,
not treated as proof that the removed copy remains installed.
