"""Generate the merge paper's schema-evolution note from published measurements."""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    schema = json.loads((ROOT / "benchmark/shapes/results.json").read_text())
    assert len(schema) == 25
    lines = [
        "# My merge experiments: schema evolution",
        "",
        "Research results note, 2026-10-13. Counts are per seed; five identities repeat",
        "balanced synthetic fixtures rather than sample production conflict rates.",
        "",
    ]
    first = [r for r in schema if r["seed"] == 1]
    tp = sum(r["tp"] for r in first)
    fn = sum(r["fn"] for r in first)
    decisions = sum(r["data_questions"] for r in first)
    context = sum(r["context_questions"] for r in first)
    lines += [
        "**ARM E — actual library merges, shapes first.** Tighten, loosen, added property,",
        "datatype change and removed shape each have five identity variants. A fixed meta-shape",
        "merges named nodes before data; all source branches are individually valid.",
        f"Per seed, {decisions} merge decisions recover {tp} oracle conflicts versus {context} Context questions;",
        f"{fn} datatype conflict is missed by cardinality detection and caught by validation.",
        "Validation remains a gate. This is research orchestration, not deployed schema negotiation.",
        "",
        "**Limits retained.** Arbitrary SHACL and blank-node alignment are untested.",
        "The 0/93 alias blindness, negative Arm D and single-deployment evidence remain.",
        "Held-at-base loss counts deferred",
        "valid changes, not irreversible deletion.",
        "",
        "Reproduction commands: benchmark/shapes/README.md.",
        "Run `just bench merge-note` to regenerate this note from the published shape results.",
    ]
    (ROOT / "docs/paper-merge/ARM-E-F-RESULTS.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
