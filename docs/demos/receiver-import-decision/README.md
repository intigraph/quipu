# Receiver admission diagnostic

The same synthetic quarantined JSON comes from a real child exiting zero.
Before, the admission predicate raises a generic non-stub error and retains
no decision. After, it emits fixed refusal reasons, retains the full decision
in a private 0600 receipt and still makes no promote call.

Baseline: `bb28bb63a3d1420adcf16a8dd96007f90710f455`; candidate implementation:
`845a2ec7cfa37f5941a35734ec5e96bdde1f5c71`. The baseline probe executes only
the three actual AST statements from import through promote, with a local
child stand-in. The candidate runs the new helper with that same child.
This is not a full production build or a reproduction of the historical
failure's unknown predicate arm. It proves diagnostic retention and preserved
refusal on this fixture.

[Recording](output.log.gz), [timing](timing.txt), [reproducer](probe.py).
Run `python3 probe.py` here. Download and decompress the output, then replay
with `scriptreplay --log-out output.log --log-timing timing.txt`; no inline
player is claimed. The raw output was checked for private infrastructure
paths. This small recording remains in the repository for review.
