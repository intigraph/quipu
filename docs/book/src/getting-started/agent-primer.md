<!-- This page is `quipu prime --static`. Edit src/primer.md, not this page. -->

{{#include ../../../../src/primer.md}}

The command above prints the same text, followed by a bounded, read-only
summary of the local store (named graphs, shapes, ontologies and the last
transaction). `quipu prime --brief` prints five to ten lines: version and job,
key live counts or a store diagnosis, then the command for the full primer.
`--brief --static` condenses the What section without opening a store.
`quipu prime --section <name>` prints one section.

`--json` follows the shared `stack-prime/1` contract: `tool`, `version`,
`primer.sections` and `primer.text`, `live.{status,elapsed_ms,summary}`, and
an overall `status`. `--brief --json` omits `primer.text`; store fields such as
`last_tx`, `graphs`, `shapes`, `ontologies` and `path` are under `live.summary`.
This replaces the earlier `{primer: string, store: object}` output.
`--static` reports `live.status: skipped` with an empty summary. An absent,
unreadable or partial store degrades overall status while prime still exits 0;
`ok` and `skipped` yield overall `ok`. Bad flags or sections exit 2.

To register Quipu with a client, see [Agent Setup](agent-setup.md).
