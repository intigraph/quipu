# Use-case oracle (source slice)

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
