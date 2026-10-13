# ARM E results

Five explicit fixtures repeated with seed identities 1, 7, 42, 99, 2026.
Counts below are per seed; seed identity does not change the template counts.

| case | TP | FP | FN | merge questions | Context questions | lost | extra | violations |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| tighten | 1 | 0 | 0 | 1 | 1 | 0 | 0 | 0 |
| loosen | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 |
| property-added | 1 | 0 | 0 | 1 | 1 | 0 | 0 | 0 |
| datatype | 0 | 0 | 1 | 0 | 1 | 0 | 2 | 1 |
| removed | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 |

All shape-graph merges conformed to the meta-shape and recovered the right-side
contract. A separate both-sides-bound-change control raised a real meta conflict.
Every source branch was legal under its own contract; the datatype incompatibility
arises after the contract changes. Validation must remain a gate, because the
cardinality conflict detector alone misses that case. No production schema changed.
