"""Derive the Arm E tables from the actual share-merge driver output."""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PAPER = HERE.parents[1] / "docs/paper-merge/sections"


def main():
    records = json.loads(Path(sys.argv[1]).read_text())
    assert len(records) == 25
    (HERE / "results.json").write_text(json.dumps(records, indent=2) + "\n")
    columns = [
        "tp",
        "fp",
        "fn",
        "data_questions",
        "context_questions",
        "triples_lost",
        "triples_extra",
        "shacl_violations",
    ]
    md = [
        "# ARM E results",
        "",
        "Five explicit fixtures repeated with seed identities 1, 7, 42, 99, 2026.",
        "Counts below are per seed; seed identity does not change the template counts.",
        "",
        "| case | TP | FP | FN | merge questions | Context questions | lost | extra | violations |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    tex = [
        "% Generated from benchmark/shapes/results.json; do not hand-edit counts.",
        "\\begin{table}[ht]\\centering\\small",
        "\\begin{tabular}{lrrrrrrrr}\\toprule",
        "Case & TP & FP & FN & Merge & Context & Lost & Extra & SHACL \\\\ \\midrule",
    ]
    for name in ("tighten", "loosen", "property-added", "datatype", "removed"):
        selected = [r for r in records if r["case"] == name]
        assert len(selected) == 5
        values = []
        for column in columns:
            numbers = {r[column] for r in selected}
            assert len(numbers) == 1
            values.append(str(numbers.pop()))
        md.append("| " + name + " | " + " | ".join(values) + " |")
        tex.append(name + " & " + " & ".join(values) + " \\\\")
    tex += [
        "\\bottomrule\\end{tabular}",
        "\\caption{Arm E, per seed. The datatype incompatibility is an oracle conflict "
        "missed by the cardinality detector and caught by post-merge SHACL validation. "
        "These are normalized automatic-output metrics, not partial deployment results.}",
        "\\label{tab:schema}\\end{table}",
    ]
    md += [
        "",
        "All shape-graph merges conformed to the meta-shape and recovered the right-side",
        "contract. A separate both-sides-bound-change control raised a real meta conflict.",
        "Every source branch was legal under its own contract; the datatype incompatibility",
        "arises after the contract changes. Validation must remain a gate, because the",
        "cardinality conflict detector alone misses that case. No production schema changed.",
    ]
    (HERE / "RESULTS.md").write_text("\n".join(md) + "\n")
    (PAPER / "06-shapes-tables.tex").write_text("\n".join(tex) + "\n")


if __name__ == "__main__":
    main()
