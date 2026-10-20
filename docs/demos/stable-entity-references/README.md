# Stable-reference CLI demonstration

This recording uses the candidate CLI against a disposable synthetic local
store. A configured public namespace is listed, explicitly persisted, then used
to mint a typed opaque identity. Resolution returns that same identity with an
`asOf` descriptor; an unknown namespace is refused. The fixture w3id base is not
a claim that its redirect exists. No network requests or installed-store writes
occur. Random identities and timestamps differ on replay.

Base revision: `ccbf48b4ef4ec8558305fdee4932d63c14258b8e`. That base has no
namespace commands; the recording demonstrates the new candidate commands only,
not an executed base-versus-head comparison. The PR identifies the exact
candidate revision. This is source-fixture evidence, not deployment acceptance.

Reproduce with the built candidate:

```sh
QUIPU_DEMO_BIN=/absolute/path/to/quipu bash run.sh
```

The small output/timing pair is retained with this source demonstration. Download
both [output.log](output.log) and [timing.log](timing.log), then replay with
util-linux `scriptreplay`:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The output file is also a readable transcript. Terminal replay was verified
locally; no inline web player is claimed. The org-to-second-store transfer and
same-export-to-public refusal are separately exercised by
`namespace_publication::tests` in the library suite.
