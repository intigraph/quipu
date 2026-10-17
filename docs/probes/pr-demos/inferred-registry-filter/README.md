# Registered inference remains explicit

The fixture registers an asserted graph and its inferred companion, then writes
one fact into each. Implicit named-graph membership must include the assertion
and exclude its companion; explicit inferred graph access must still work.

The [terminal recording](output.log) and [timing](timing.log) show the same native
Rust regressions with the SQL leaf exclusion removed and with all exclusions
present. Watch the actual query's two rows versus its expected one row in the
sabotage arm. The independent registry control still passes there. Both tests
pass with the guard restored, including explicit inferred-access controls.
This is a source-fixture demonstration, not a production query or deployment.
The separate base-source run failed both regressions before the repair.

The fixture builds on base `be9f52b40b5801450a1c323cb8eecb92b3bf96ca`.
The before binary is the candidate with only the SQL exclusion line removed;
the after binary contains the complete filter-first proposal. Companion minting
and registration are unchanged. Repeat by building the test binaries with
`just test --lib smdlnm_`, preserving each binary as a copy, then setting
`DEMO_BEFORE` and `DEMO_AFTER` to those copies and running `python3 run.py`.

Replay the downloaded recording with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The output is also the readable transcript. This is a terminal recording, not an
inline video. Its literal PTY CRLF format has a path-specific whitespace rule;
other whitespace checks remain active. The small recording is retained beside
the reproducer in the existing source demo tree.
