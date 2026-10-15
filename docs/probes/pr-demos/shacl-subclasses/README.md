# Subclass target validation demo

The same invalid fixture changes from `valid`/exit0 to one minCount violation/exit1.
The direct-parent refusal and valid-child positive control remain unchanged.
Only private CLI fixtures run; no server, production data or installation is involved.

[Download the actual terminal output](output.log) and [timing](timing.log), then replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The before binary is clean0.14.1 at `f13fc1bf1dc938a703a85e7f6bac2519392d5fbe`.
The candidate is a pre-commit build from that base plus this proposal, so its
embedded version reports the base SHA and `git_dirty: true`. [Source hashes](source-hashes.json)
bind the four changed code/dependency files and candidate binary; those file
hashes must match the proposal being reviewed. This is source-fixture evidence,
not a claim about a deployed release.

Reproduce with private copies of the before and candidate CLI, passing their
paths via `BEFORE_QUIPU` and `AFTER_QUIPU`, then run `python3 run.py`.
The script checks expected exit codes and retains actual validator output.
The regression tests additionally cover transitive/cyclic/blank-node/diamond
hierarchies, compiled-validator reuse, explicit-type constraints, and a rejected
knot's commit absence followed by a valid positive commit.
