#!/usr/bin/env python3
"""Refuse new stack vocabulary in Quipu's source.

Quipu core names no vocabulary of the stack that runs on it: no deployment
namespace, hostname or vocabulary-pack term list in src/. Vocabularies are
data a store loads (see src/term_vocabulary.rs).

Literals that predate this guard are listed, per file, with the number of
matching lines in scripts/stack-vocabulary-allowlist.tsv. The allowlist is a
ratchet:

  * a file not in the allowlist with any match       -> FAIL (new leak)
  * a listed file with MORE matching lines than listed -> FAIL (new leak)
  * a listed file with FEWER matching lines            -> FAIL until the
    allowlist is lowered to the new count (so it only ever shrinks)
  * a listed file that no longer exists or matches     -> FAIL until removed

Usage:
  scripts/check-stack-vocabulary.py            check the tracked src/ tree
  scripts/check-stack-vocabulary.py --write    rewrite the allowlist to the
                                               current counts (shrink only:
                                               refuses if any count would grow)
  scripts/check-stack-vocabulary.py --self-test
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path

PATTERN = re.compile(r"aegis|camayoc|gastown", re.IGNORECASE)
# A work-item reference such as `(aegis-pu7)` or `aegis-xdvg.6` is provenance,
# not vocabulary: it names no namespace, host or term the code depends on.
# Such tokens are removed before matching, so citing an item in a comment does
# not redden every open PR. A hostname-shaped token (`aegis-x.example`) or a longer
# hyphenated name (`aegis-crew-...`) is not an item ID and still matches.
ITEM_ID = re.compile(
    r"(?<![a-z0-9/._-])aegis-[a-z0-9]{2,8}(?:\.[0-9]+)*(?![a-z0-9-]|\.[a-z0-9])",
    re.IGNORECASE,
)


def names_stack_vocabulary(line: str) -> bool:
    return bool(PATTERN.search(ITEM_ID.sub("", line)))
ROOT = Path(__file__).resolve().parents[1]
ALLOWLIST = ROOT / "scripts" / "stack-vocabulary-allowlist.tsv"


def tracked_sources(root: Path) -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--", "src"],
        check=True,
        capture_output=True,
    ).stdout
    return sorted(p for p in out.decode().split("\0") if p)


def count_matches(root: Path, paths: list[str]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for rel in paths:
        text = (root / rel).read_text(encoding="utf-8", errors="replace")
        n = sum(1 for line in text.splitlines() if names_stack_vocabulary(line))
        if n:
            counts[rel] = n
    return counts


def read_allowlist(path: Path) -> dict[str, int]:
    allowed: dict[str, int] = {}
    if not path.exists():
        return allowed
    for lineno, line in enumerate(path.read_text().splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        rel, _, n = line.partition("\t")
        if not n.strip().isdigit() or rel in allowed:
            raise SystemExit(f"{path}:{lineno}: malformed or duplicate entry: {line!r}")
        allowed[rel] = int(n)
    return allowed


def compare(actual: dict[str, int], allowed: dict[str, int]) -> list[str]:
    problems: list[str] = []
    for rel in sorted(set(actual) | set(allowed)):
        have, may = actual.get(rel, 0), allowed.get(rel)
        if may is None:
            problems.append(f"NEW  {rel}: {have} line(s) name stack vocabulary; none allowed")
        elif have > may:
            problems.append(f"GREW {rel}: {have} line(s), allowlist permits {may}")
        elif have < may:
            problems.append(
                f"SHRANK {rel}: {have} line(s), allowlist still says {may}; "
                "lower it (scripts/check-stack-vocabulary.py --write)"
            )
    return problems


def render(actual: dict[str, int]) -> str:
    header = (
        "# Pre-existing stack-vocabulary literals in src/, per file: path<TAB>matching lines.\n"
        "# A ratchet: counts may only fall. Regenerate with\n"
        "# scripts/check-stack-vocabulary.py --write after removing literals.\n"
    )
    return header + "".join(f"{rel}\t{n}\n" for rel, n in sorted(actual.items()))


def run(root: Path, allowlist: Path, write: bool = False) -> int:
    actual = count_matches(root, tracked_sources(root))
    allowed = read_allowlist(allowlist)
    if write:
        grown = [p for p in compare(actual, allowed) if not p.startswith("SHRANK")]
        if grown and allowed:
            print("refusing to grow the allowlist:", *grown, sep="\n  ", file=sys.stderr)
            return 1
        allowlist.write_text(render(actual))
        print(f"wrote {allowlist}: {len(actual)} file(s), {sum(actual.values())} line(s)")
        return 0
    problems = compare(actual, allowed)
    if problems:
        print("stack vocabulary in src/ (quipu core names none; load it as data):")
        print(*problems, sep="\n")
        return 1
    print(
        f"stack vocabulary guard: {len(actual)} allowlisted file(s), "
        f"{sum(actual.values())} pre-existing line(s), nothing new"
    )
    return 0


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        (root / "src").mkdir()
        old = root / "src" / "old.rs"
        old.write_text("// legacy: Aegis\nfn x() {}\n// camayoc\n")
        (root / "src" / "clean.rs").write_text("fn y() {}\n")
        subprocess.run(["git", "-C", str(root), "add", "src"], check=True)
        allow = root / "allow.tsv"
        allow.write_text("src/old.rs\t2\n")
        cases = []
        cases.append(("exact allowlist passes", run(root, allow) == 0))
        (root / "src" / "clean.rs").write_text("const NS: &str = \"http://GASTOWN/\";\n")
        cases.append(("new file leak fails", run(root, allow) == 1))
        (root / "src" / "clean.rs").write_text("fn y() {}\n")
        old.write_text("// legacy: Aegis\n// aegis again\n// camayoc\n")
        cases.append(("grown count fails", run(root, allow) == 1))
        old.write_text("// legacy: Aegis\n")
        cases.append(("shrunk count fails until lowered", run(root, allow) == 1))
        cases.append(("--write lowers it", run(root, allow, write=True) == 0))
        (root / "src" / "clean.rs").write_text(
            "// fixed in (aegis-pu7), see aegis-xdvg.6 and AEGIS-67p0lj.\nfn y() {}\n"
        )
        cases.append(("item references are not vocabulary", run(root, allow) == 0))
        for leak in (
            '"http://aegis-node.example/"',
            "// aegis-crew-process-rules",
            "// fixed (aegis-pu7) on aegis",
        ):
            (root / "src" / "clean.rs").write_text(leak + "\n")
            cases.append((f"still refused: {leak}", run(root, allow) == 1))
        (root / "src" / "clean.rs").write_text("fn y() {}\n")
        cases.append(("then passes", run(root, allow) == 0))
        old.write_text("// legacy: Aegis\n// aegis again\n")
        cases.append(("--write refuses to grow", run(root, allow, write=True) == 1))
    failed = [name for name, ok in cases if not ok]
    for name, ok in cases:
        print(f"  {'ok  ' if ok else 'FAIL'} {name}")
    if failed:
        print("self-test FAILED:", ", ".join(failed))
        return 1
    print(f"self-test: {len(cases)} case(s) passed")
    return 0


def main(argv: list[str]) -> int:
    if argv[1:] == ["--self-test"]:
        return self_test()
    if argv[1:] == ["--write"]:
        return run(ROOT, ALLOWLIST, write=True)
    if argv[1:]:
        print(__doc__, file=sys.stderr)
        return 2
    if not ALLOWLIST.exists():
        print(f"missing {ALLOWLIST}; refusing a vacuous pass", file=sys.stderr)
        return 1
    return run(ROOT, ALLOWLIST)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
