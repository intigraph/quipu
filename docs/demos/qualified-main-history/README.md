# Qualified MAIN history

The same isolated candidate contains a named-graph assertion and retraction.
Its legacy ROOT-history call returns zero rows; its new explicitly qualified
graph call returns both, with local transaction identity and separate declared
and authenticated provenance. `unit-host` remains a declaration, never an
authenticated physical host.

`response.json` is a real response transcript captured from the focused native
reproducer, not a server recording. The legacy result is measured through the
unchanged ROOT interface on the same candidate; no old binary was rerun.
`source.json` identifies the source base and exact source-file hashes. The PR
identifies the resulting published head. Both stores and inputs are synthetic.

Replay the saved response:

```sh
python3 docs/demos/qualified-main-history/replay.py
```

Reproduce with an appropriately admitted local build:

```sh
just test --locked --features full qualified_history_preserves_named_graph -- --nocapture
```

Watch the two history counts and the declared/authenticated split. The candidate
also tests range fences, reopen/topology/policy changes, attached exclusions,
alias refusal, type fidelity, overflow, deadline cleanup and nonce preservation.
The transcript proves only the isolated MAIN profile. It does not demonstrate
SDK adoption, live service deployment, complete archived history, a real Mac
request, or authenticated host origin. The small transcript is retained here
with the code for review; it contains no production identifiers or credentials.
