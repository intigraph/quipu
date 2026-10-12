# Quipu command alias demo

This terminal recording shows a copied candidate executable before and after
`quipu install-alias`: the missing command becomes a relative `qp -> quipu`
link with identical inode and version. A repeated invocation succeeds, and
an existing foreign command is refused with its bytes preserved.

Candidate source: `659fa8cec235b08532f7a3d9d02317427b60cd79`, based on
`44518894b37d14ae770b7754c0b6bee365562b3a`. This is an isolated Linux source
build demonstration, not an installed release or service deployment.
The before state represents the existing binary-only install contract.

Replay the downloaded files with util-linux:

```bash
gzip -dc output.log.gz > output.log
scriptreplay --log-out output.log --log-timing timing.log
```

Reproduce with an absolute path to a built candidate:

```bash
bash reproduce.sh /path/to/quipu
```

These small recording files remain in this directory with the repository's
other PR demos. Playback requires downloading and a local terminal; no inline
player is claimed. Paths and credentials are absent from the recorded output.
