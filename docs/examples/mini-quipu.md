# Two file-backed forks, one conflict

The executable reference is
[`mini_quipu_example_tests.rs`](../../src/store/mini_quipu_example_tests.rs).
It uses disposable stores and the native APIs, with no application adapter.
The same lifecycle runs twice: first with `urn:example:*` graphs, then with
`https://example.org/{x,y,z}` graphs. Graph names are fixture parameters.

1. Home owns one atomic set containing a work graph and an evidence graph.
   An unrelated legacy graph stays outside that set. A shape makes the
   example value slot `card: one`.
2. A partial-set fork refuses before creating a file or registry entry (B1).
   Two whole-set forks then materialize the same home cut with fresh identities.
3. Each local writer replaces `present` with its own value. Home still reads
   `present`; fork A reads `alice`; fork B reads `bob`. Local `GRAPH ?g`
   discovers both members and excludes the unrelated graph (B3).
4. Home and fork cursors refuse in the other lineage, including between the
   two forks (B4). A local transaction number is never a home cursor.
5. Fork A merges through the ordinary fork merge core. Fork B's atomic merge
   holds one divergent slot and writes nothing. A resolution chooses `bob`,
   producing a home decision. Retrying returns the original receipt.
6. Each receipt retains the observed local author and origin transactions;
   home writes belong to the applier or reviewer. Undeclared local identity
   fields remain unknown. The merge invents no provenance (B7).
7. Both files retire after their final merge. Receipt verification succeeds;
   further merges and local writes refuse. Removing the disposable files
   leaves the home value and transaction head unchanged.

Run only this fixture from the repository root:

```sh
cargo test --locked --no-default-features --features shacl --lib m6_generic_worked_example -- --test-threads=2
```

The whole-set boundary is catalog driven; conflict cardinality comes from the
shape. Renaming every application graph requires no runtime code change.
This native example activates no served routing or production store.
