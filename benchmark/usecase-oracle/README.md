# Use-case oracle (source slice)

## Native temporal adapter (source, awaiting qualification)

`examples/merge_native_oracle` accepts the separate `native-c4-input.json`
recipe on stdin and runs the actual fork-promote path in a private temporary
store. Input is bounded to 64 KiB and rejects unknown fields, including oracle
answers and external database paths. Share and pendant paths are not accepted
by this first slice. No live database or endpoint is used.

```sh
cargo run --locked --example merge_native_oracle --features shacl \
  < benchmark/usecase-oracle/native-c4-input.json > <private-capture.json>
```

This is a future qualification command, not a result from this revision.
The capture contains actual ROOT physical rows, native transaction IDs and
source tags, current queries and independently executed transaction/valid-time
queries. `receive` and `decide` are bound from persisted native source tags,
not an assumed `tx - 1`. Missing or ambiguous role pins refuse. History is read
from the scratch database through a read-only SQLite connection; assertions,
empty receipts and duplicate rows remain distinct. Closure attribution uses
persisted `retracted_tx`, not a diff of final value sets. Scratch deletion is
required before a capture is returned successfully.

The `capture` object supplies `current`, `history`, `transactions`, `as_of` and
`report` to `oracle.evaluate(asof_cases.CASE, capture)`. The enclosing native
outcome must also be `promoted`; an error/refusal is not a passing observation.
The input recipe does not load the expected fixture. Conflict report kind is
normalized from the native conflict record, and the chosen label from the
resolution bound to that record. Clean-slot classification is unavailable and
is left absent, so it cannot silently satisfy an oracle requiring that report.

Five Rust source controls cover the two C4 receipt pins and earlier valid time,
an unknown pin, unknown input fields, an undecided conflict with no writes, and
foreign/unknown receipt bindings.
They are uncompiled and unrun. Share/pendant adapters, remaining cases, replay
identity normalization and all benchmark/paper reruns remain open.

The judge compares explicit fixture expectations with observed current facts and
history records. It reads no shapes and imports no merge operator. History
records include valid intervals and transaction roles, so preserving a triple
with the wrong interval does not satisfy the history expectation. List values
remain structured sequences, including repeated elements.
Current facts are a set; history is a multiset of records. Duplicate historical
records are counted, so an extra receipt or a missing repeated record cannot
disappear through set deduplication.

`current_facts_lost` and `history_records_missing` are separate counts. A rejected
incoming value can be correctly absent from current state while its receipt
must remain in history. Extra current facts and history records also fail.
An omitted observation view is invalid input, never a successful empty result.
When a fixture declares `expected_report`, every named report field is checked
as well. A graph matching the expected facts does not excuse a missing
convergent flag or a wrong held count. Adapters project native reports into the
fixture's canonical terms; this module does not claim those are native wire
enum names.

`list_cases.py` states C6a–C6e outcomes explicitly from the design. These are
pendant projections with no store history: held list conflicts retain base.
The fixtures do not run the merge or manufacture observations. Their positive
and defeated controls validate the judge, not any operator implementation.

`temporal_cases.py` adds explicit single-slot store projections for C1a–C1c,
C2a–C2c, C3a–C3d, all C4 decisions, C5a ours/theirs/base, C5b ours/theirs,
C5c and C7. Refused `both` and atomic `defer` receive nothing, including no
receipt history. C3 fixes ROOT's existing close
times and receives only incoming additions, avoiding duplicate current claims.
They include valid intervals and ordered
transaction traces. Where a fixture declares `expected_transactions`, an
observation must provide the trace: reversed receive/decide order, a missing
receipt transaction, or an extra transaction fails even when final facts and
history happen to match. Empty transactions are not invented. These are
normalized adapter observations, not commands to the system under test.

`replay_cases.py` states C8a–C8d after a second action. It requires the first
decision's transaction IDs on replay, unchanged transaction counts for no-op
or stale attempts, and separate identities and filtered logs for distinct
clean merges. Its fixture-local ID tokens describe identity relationships;
future adapters must map actual IDs consistently, never replace observed
relationships with the expected ones. No native status or ID encoding is
asserted here.

`asof_cases.py` adds explicit C4-gate answers at both transaction and valid-time
pins. A matching final graph, history and trace do not excuse an open receipt
at the receive pin, a stale answer at the decide pin, or rewritten earlier ROOT
history. Missing pinned answers are invalid input. Adapters must execute the
named reads against the system under test and preserve their transaction-ID
binding; reconstructing answers from expected history is not measurement.

Run the judge's independent positive and defeated controls with:

```sh
python3 -m unittest discover -s benchmark/usecase-oracle -p 'test_*.py'
```

This is the scoring boundary only. Complete C1–C8 fixtures, recorded later-repair
states, per-use-case competency questions, path-specific adapters, benchmark
reruns and paper tables remain open. An explicit empty history
expectation is appropriate only for a history-free path, such as a pendant;
store fixtures must provide their expected receipts and valid intervals.
No benchmark result or merge correctness is claimed by these judge controls.
