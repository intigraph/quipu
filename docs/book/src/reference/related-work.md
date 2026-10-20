# Related work

Two recent preprints address evidence and maintenance in agent-accessible knowledge graphs.
The descriptions below are limited to their abstracts; their evaluations are not
benchmarks of Quipu.

- Vishisht Choudhary, [*Keep Your Receipts: Trustworthy Cleanup of Agent-Built
  Knowledge Graphs*](https://arxiv.org/abs/2610.17719) (2026), studies the consequences
  of withdrawing source evidence. PolyGraph records alternative evidence bundles
  for derived artifacts and audits those receipts by re-derivation. Its results
  motivate checking downstream dependencies rather than assuming that deleting
  directly extracted facts repairs every affected answer. This complements
  Quipu's [temporal record](../concepts/temporal-model.md) and
  [SHACL-governed reconciliation](../concepts/shacl-validation.md): a merge's
  structural validity does not establish the fidelity of its evidence receipts.
- Akash Raj, [*Plant Memory: Technical Language Processing and a Verified
  Maintenance Ontology for Agent-Accessible Failure Knowledge from SAP PM Work
  Orders*](https://arxiv.org/abs/2610.19131) (2026), describes rule-first extraction,
  optional LLM review, reified facts with confidence, evidence and verification
  state, technician review and MCP access. Its reported evaluation uses a
  synthetic 400-work-order dataset. It provides an application-specific example
  of reviewed agent-accessible knowledge; Quipu's store and merge work instead
  concerns a general RDF graph governed by explicit shapes. The abstract does
  not establish a comparison of their temporal or withdrawal semantics.

Official arXiv BibTeX: [Keep Your Receipts](https://arxiv.org/bibtex/2610.17719)
and [Plant Memory](https://arxiv.org/bibtex/2610.19131).
