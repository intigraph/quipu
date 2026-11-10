# Receipt/outbox fixture demonstration

This source fixture connects native attested transactions to a test-only outbox
and a disposable idempotent sink. It does not register a production producer,
change a running writer, or collect a live corpus.

Run the controls in a disposable build environment:

```sh
just test --locked --offline --lib census_fixture -- --nocapture
```

The [captured default-feature transcript](transcript.txt) shows three durable
native receipts drained after one hint, three unique exports, and no additional
export on a repeat drain. Seven deliberately wrong fixture policies invoke the
same assertions and are rejected: trusting the client result, treating a
reservation as a commit, publishing at inner release, treating a receipt as a
data commit, counting one write per wake, using a volatile-only outbox, and
propagating optional observer failure into the writer result. Their caught
panics are expected negative-control evidence; the enclosing test must pass.

The positive controls also cover authority refusal, outer rollback,
receipt-only settlement, close/reopen with a lost sink acknowledgement,
capacity/contention/poison/nonfatal SQL failure, invalid bindings, tampered
receipts, and an unarmed writer. Receipt-only settlement uses the private atomic
primitive because the public attested route always emits provenance RDF.
Close/reopen proves persistent replay in these fixture databases; it does not
simulate an operating-system crash or promise exactly-once in-memory metrics.

This transcript qualifies the native default-feature controls only. SHACL,
Wasm, the full source gate, hosted CI, and independent review have separate
receipts. None establishes production activation or whole-census acceptance.
