"""Generate the short E/F hand-off note from published measurements."""

import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    memory = json.loads((ROOT / "benchmark/memory/results/metrics.json").read_text())
    schema = json.loads((ROOT / "benchmark/shapes/results.json").read_text())
    assert len(memory) == 5 and len(schema) == 25
    lines = [
        "# My merge experiments: schema evolution and Memory metadata",
        "",
        "Research results note, 2026-10-13. Counts are per seed; five identities repeat",
        "balanced synthetic fixtures rather than sample production conflict rates.",
        "",
        "**ARM F — real Dolt 2.1.8.** Each trial has 200 Memories and 60 true conflict slots.",
        "Native JSON already merges independent keys; whole-cell TEXT is an ablation.",
        "Typed rows union distinct set additions but create same-value provenance conflicts.",
        "",
        "| representation | native decisions | entirely false decisions | collateral FP slots | missed slots |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    for arm in ("json-native", "whole-cell-text", "typed-rows"):
        fields = ("decision_units", "false_decision_units", "fp_slots", "fn_slots")
        values = [memory[0]["arms"][arm][key] for key in fields]
        assert all([r["arms"][arm][key] for key in fields] == values for r in memory)
        lines.append("| " + arm + " | " + " | ".join(map(str, values)) + " |")
    lines += [
        "",
        "Rows reduce collateral slots, not native false decisions, in this workload.",
        "Five invalid candidates per seed were refused by the prototype writer before mutation;",
        "native JSON accepted them. A JSON writer can implement the same declaration guard.",
        "",
        "**SQL probes.** Each seed has 2,000 Memories, 10,000 facts and 2,000 Links.",
        "Every repeated answer matches independently constructed expectations; full plans/samples",
        "are published. Pooled medians over 35 warm executions, in milliseconds:",
        "",
        "| query | native JSON | typed rows |",
        "| --- | ---: | ---: |",
    ]
    for name in ("status", "tag", "traversal"):
        values = [
            statistics.median(
                [
                    sample
                    for r in memory
                    for sample in r["query"]["queries"][name][arm]["samples_ms"]
                ]
            )
            for arm in ("json-native", "typed-rows")
        ]
        lines.append(f"| {name} | {values[0]:.3f} | {values[1]:.3f} |")
    first = [r for r in schema if r["seed"] == 1]
    tp = sum(r["tp"] for r in first)
    fn = sum(r["fn"] for r in first)
    decisions = sum(r["data_questions"] for r in first)
    context = sum(r["context_questions"] for r in first)
    lines += [
        "",
        "**ARM E — actual library merges, shapes first.** Tighten, loosen, added property,",
        "datatype change and removed shape each have five identity variants. A fixed meta-shape",
        "merges named nodes before data; all source branches are individually valid.",
        f"Per seed, {decisions} merge decisions recover {tp} oracle conflicts versus {context} Context questions;",
        f"{fn} datatype conflict is missed by cardinality detection and caught by validation.",
        "Validation remains a gate. This is research orchestration, not deployed schema negotiation.",
        "",
        "**Limits retained.** Body edits, ordered arrays, arbitrary SHACL and blank-node alignment",
        "are untested. Timings use one fixture host and fixed query order. The 0/93 alias blindness,",
        "negative Arm D and single-deployment evidence remain. Held-at-base loss counts deferred",
        "valid changes, not irreversible deletion. Every Dolt fixture stopped.",
        "",
        "Reproduction commands: benchmark/memory/README.md and benchmark/shapes/README.md.",
        "Run `just bench merge-note` to regenerate this note from their published results.",
    ]
    (ROOT / "docs/paper-merge/ARM-E-F-RESULTS.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
