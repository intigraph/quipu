# Signer operator CLI demo

The trigger is an operator trying to register a public test signer. The retained
baseline executable (0.16.2, commit `4cd9e48d`) refuses `signer --help`. The
candidate adds the command, enrolls the fixture, reads back its exact fingerprint
and graph scope, then revokes it. The store remains disabled throughout.

This is a real terminal recording against a temporary local store, with a
sandbox HOME; no service, real credential, graph protection or activation is
involved. The candidate is an unoptimized working-tree build over base
`8fa2947349878b9968821e9914538a8959b15ce2`, containing the signer implementation
in this proposal. The before executable predates the exact base; source
inspection at that base also confirms there is no signer dispatch. This is
source qualification, not evidence of installation or a complete browser flow.

Download [the output](output.typescript) and [its timing](timing.txt), then replay:

```bash
scriptreplay --log-out output.typescript --log-timing timing.txt
```

The output is also the readable transcript; inline playback is not promised.
Reproduce using [demo.sh](demo.sh) with `BEFORE_BIN` and `AFTER_BIN` pointing to
the two executables. Run in a private HOME. The generated credential is a fixed
public test key; the script computes an expiry one day ahead. No signing secret
is present. The recording is retained with the source in this small probe
folder; it does not require an external artifact host.
