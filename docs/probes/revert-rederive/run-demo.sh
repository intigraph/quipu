#!/usr/bin/env bash
set -eu
python3 - <<'PYTHON'
import contextlib
import importlib.util
import pathlib
import subprocess
import sys
import types

path = pathlib.Path("benchmark/public/test_conformance_report.py")
spec = importlib.util.spec_from_file_location("demo_tests", path)
tests = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tests)
candidate = tests.REPORT
preceding = types.ModuleType("preceding_report")
preceding.__file__ = str(path.with_name("conformance_report.py"))
sys.modules[preceding.__name__] = preceding
source = subprocess.check_output(["git", "show", "91aae714:benchmark/public/conformance_report.py"], text=True)
exec(compile(source, preceding.__file__, "exec"), preceding.__dict__)
probe = tests.LedgerProvenancePrModeTest()
with contextlib.ExitStack() as stack:
    root, run, measured = probe._repo(stack)
    for name in ("mine.rs", "theirs.rs"):
        (root / "src" / name).write_text("// v2\n")
    run("add", "-A")
    run("commit", "-qm", "main changes")
    run("checkout", "-qb", "revert")
    (root / "src/mine.rs").write_text("// v1\n")
    run("add", "-A")
    run("commit", "-qm", "restore measured file")
    print("Private Git fixture: own file restored; unrelated main file remains changed")
    for label, module, expected in [("preceding", preceding, 0), ("candidate", candidate, 1)]:
        tests.REPORT = module
        pr = probe._provenance(root, {"quipu_revision": measured}, "main")[0]
        strict = probe._provenance(root, {"quipu_revision": measured})[0]
        unknown = probe._provenance(root, {"quipu_revision": "0" * 40}, "main")[0]
        print(f"{label}: PR stale={pr} strict stale={strict} unknown={unknown}")
        assert (pr, strict, unknown) == (expected, 1, 2)
    tests.REPORT = candidate
    probe.test_pr_mode_ignores_a_src_change_that_came_from_main()
    print("candidate: docs-only PR remains fresh; no production calls")
PYTHON
