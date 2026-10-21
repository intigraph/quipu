# quipu

**Quipu is a governed, bitemporal knowledge store for AI coding agents and the
people who run them.** It can check writes against your rules, records where each fact came from, and answers what was true when. *Bitemporal* means keeping both
when a fact was recorded and, when supplied, when it was true.

Write enforcement is opt-in: `shacl.validate_on_write` and `governance.enforce_on_write`
default to `false` ([configuration](https://scbrown.github.io/quipu/getting-started/configuration.html)).
The [60-second demo](docs/demos/governed-memory-v0.13.0/) shows an enabled gate,
a refused write, corrected provenance, and a valid-time read.

Quipu answers standard SPARQL 1.1 as a CLI, REST server and Rust library, with its own MCP server (`quipu mcp` or `quipu-server` over HTTP).

[Domain shapes in practice](docs/book/src/concepts/domain-shapes.md) describes
producer-owned Camayoc profiles, the Seeds work profile and the staged
load-before-remove transition from bundled domain examples.

A [quipu](https://en.wikipedia.org/wiki/Quipu) is the Andean knotted-cord record: cords are entities, knots are facts.

mcp-name: io.github.scbrown/quipu

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/scbrown/quipu/main/install.sh | sh
```

Python 3.9+, curl, and a Linux or macOS shell. Downloads the latest published archive, verifies SHA-256 and the executable
version before publishing command links in `~/.local/bin`. Add it to `PATH`.
The complete versioned bundle stays in `~/.local/lib/quipu/TAG` for rollback.
Published targets: Linux and macOS, x86_64 and arm64.

Set `STACK_INSTALL_ROOT` on the receiving shell for another prefix.
Pinned releases, alternatives, configuration and removal are in the
[installation and usage reference](docs/installation-reference.md).

## Try it

Write a rule (every `Person` needs a name), two people who follow it, and one
who does not:

```bash
mkdir quipu-demo && cd quipu-demo
printf '@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix ex: <http://example.org/> .\nex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;\n  sh:property [ sh:path ex:name ; sh:minCount 1 ] .\n' > shapes.ttl
printf '@prefix ex: <http://example.org/> .\nex:alice a ex:Person ; ex:name "Alice" ; ex:knows ex:bob .\nex:bob   a ex:Person ; ex:name "Bob" .\n' > people.ttl
printf '@prefix ex: <http://example.org/> .\nex:carol a ex:Person .\n' > carol.ttl
```

Then load the good facts, ask a question, and try the bad one:

```bash
quipu knot people.ttl --shapes shapes.ttl --db demo.db
quipu read 'SELECT ?who ?name WHERE { ?who <http://example.org/name> ?name }' --db demo.db
quipu knot carol.ttl --shapes shapes.ttl --db demo.db
```

<!-- markdownlint-disable MD010 -->
<!-- quipu's real output is tab-separated; the block below is verbatim. -->

```text
SHACL validation passed
knotted 5 facts from people.ttl (tx 1)
who	name
----------------------------------------
http://example.org/alice	"Alice"
http://example.org/bob	"Bob"

2 results
SHACL validation failed: 1 violation(s)
  Violation on http://example.org/carol: MinCount(1) not satisfied
```

<!-- markdownlint-enable MD010 -->

The last command exits 1 and writes nothing: Carol breaks the rule, so she never
enters the graph. That refusal at write time, with the reason attached, is what
Quipu is for.

## Next steps

- [Tambo](https://github.com/scbrown/tambo): guided stack installation and verification.
- [Camayoc](https://github.com/scbrown/camayoc): governed memory and vocabulary.
- [Cross-repository pendant diagram (source-only)](https://github.com/scbrown/quipu/blob/5f51e5d2cda5cee0f3ca37016b312c308fce33eb/docs/book/src/sharing/cross-repo-pendants.svg).
- [Detailed examples and agent setup](docs/installation-reference.md).

**Load this repository’s graph:** the [latest release](https://github.com/scbrown/quipu/releases/latest) includes a repository pendant, checksum and provenance. Verify those together, extract the share directory, then use `quipu import SHARE_DIR --db repository.db`; import stages knowledge and leaves ROOT unchanged. See the [verified import workflow](https://scbrown.github.io/quipu/sharing/index.html).

## The stack

<!-- stack-table:start -->

The Quipu stack is built around a governed, bitemporal knowledge store: shared
facts, decisions and work history your agents can query. Quipu can check writes
against your rules, records where each fact came from, and answers what was true
when.

| tool | what it gives your agents |
|---|---|
| [tambo](https://github.com/scbrown/tambo) | 🧰 Agent Platform Toolkit - installs and configures the quipu stack |
| [quipu](https://github.com/scbrown/quipu) | stores shared facts with provenance and history; can check writes against your rules |
| [camayoc](https://github.com/scbrown/camayoc) | prepares knowledge and vocabulary for Quipu, with checks before it is admitted |
| [quechua](https://github.com/scbrown/quechua) | shared vocabulary and shapes for interoperable knowledge |
| [chaski](https://github.com/scbrown/chaski) | reacts to graph changes and delivers governed notifications |
| [bobbin](https://github.com/scbrown/bobbin) | searches your code and can include related knowledge from Quipu |
| [yupana](https://github.com/scbrown/yupana) | shows code impact and can read policies from Quipu to check changes |
| [desire-path](https://github.com/scbrown/desire-path) | turns agent mistakes into feature requests; works on its own without Quipu |
| [seeds](https://github.com/scbrown/seeds) | a work item tracker backed by the Quipu knowledge base |
| [shantytown](https://github.com/scbrown/shantytown) | runs crews of AI agents on work items, triggered by hand, on a schedule or by events; can share work and memory through Quipu |
| [rikuy](https://github.com/scbrown/rikuy) | live brief: status, graphs, watches and signed decisions over Quipu |
| [shuttle](https://github.com/scbrown/shuttle) | runs signed agent workflows and keeps their records in Quipu |

<!-- stack-table:end -->

[License](LICENSE).
