# Session-capture credential guidance demo

The same Stop event with no credential environment variables prompts the actual
CLI once. Before, it offers environment-only bearer guidance. After, it prefers
authenticated tooling, names token-file fallback, and tells the caller to stop
before HTTP when no non-empty credential is available. Both retain the capture
solicitation and skip option.

The recorded targets are local CLI binaries: installed baseline
`19d2765ebda0618744a900e88318380d2d84bd97` and clean candidate
`046a8567ec5d13d409978fd83cf51419fbe72ade`, both version 0.14.1. The source
comparison base is `21bd8e8e42c0a238a3c5d4af1d4a0b5f2bcfe8f6`; the focused
regression contract also failed on that base and passed after the change.
Subsequent commits adding this recording do not change the demonstrated source.

[Recording/transcript](output.log) and [timing](timing.log) are a small util-linux
`script` recording retained with this probe. Download both and replay in a
terminal (playback was checked locally):

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Reproduce with binaries built from the stated revisions:

```sh
./run.sh /path/to/before/quipu /path/to/after/quipu
```

The script supplies identical synthetic Stop JSON, isolates hook state, unsets
token environment variables, and uses `https://example.invalid`. It performs
no HTTP. The output demonstrates emitted instructions, not token loading,
installation, the identity of a historical unauthenticated caller, or a reduction
in authentication refusals. The recorder was `script`; asciinema was unavailable.
