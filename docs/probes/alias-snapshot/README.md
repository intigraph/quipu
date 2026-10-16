# Bounded alias snapshots

A query can visit many small subject batches. Each leaf may try to snapshot the
composed term aliases. The preceding implementation decoded and hashed 4,097
rows before discarding a map larger than 4,096 entries. Repeating that work for
every batch made a large alias registry expensive even when its provider had no
facts in the selected graph.

The candidate first checks for a 4,097th row using a bounded SQLite probe. A
large registry retains the existing indexed canonicalization path. A complete
map of at most 4,096 entries retains the preceding behavior; no partial map is
used. The probe does not count or enumerate the whole registry. Budget checks
run before and after it, and normal SQLite interruption remains in force.

The indexed fallback memoizes at most 512 canonical-ID results for one leaf.
Every uncached ID still uses the preceding lookup, including after the memo is
full. No memo survives a leaf or crosses stores. Errors are not cached. Main
identity resolution and complete alias maps retain their existing paths. This
is an entry bound, not a total RSS cap.

Run from the repository root:

```sh
bash docs/probes/alias-snapshot/run-demo.sh
```

The demonstration uses a private in-memory SQLite registry and a projection
counter. It compares the preceding loop and candidate on the same 4,097 aliases:
both must decline the snapshot, but only the preceding loop decodes the prefix.
A 4,096-entry positive control must retain every mapping. A separate actual
attachment fixture explicitly exercises indexed fallback and counts repeated
physical-alias lookups, while checking that both paths return the canonical ID.
The preceding loop is a test-only reference implementation; it is not compiled into production.
These measurements describe the metadata operation, not production recovery or
a total RSS bound. Recording and exact revision caption follow after verification.

Rollback: revert the actual merge commit.
