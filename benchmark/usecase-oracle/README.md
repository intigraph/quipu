# Use-case oracle (source slice)

The judge compares explicit fixture expectations with observed current facts and
history records. It reads no shapes and imports no merge operator. History
records include valid intervals and transaction roles, so preserving a triple
with the wrong interval does not satisfy the history expectation. List values
remain structured sequences, including repeated elements.

`current_facts_lost` and `history_records_missing` are separate counts. A rejected
incoming value can be correctly absent from current state while its receipt
must remain in history. Extra current facts and history records also fail.
An omitted observation view is invalid input, never a successful empty result.

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
