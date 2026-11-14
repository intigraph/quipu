"""Real old/new CLI gates on disposable public fixtures; no benchmark build."""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[3]
with tempfile.TemporaryDirectory(prefix="expected-results-demo-") as directory:
    work = Path(directory)
    measured = work / "measured"
    measured.mkdir()
    ledger = {"suite_revision": "a" * 40, "results": [
        {"class": "fixture", "manifest": "public", "id": "known-failure", "status": "failed"}]}
    baseline = work / "baseline.json"
    baseline.write_text(json.dumps(ledger))
    candidate = measured / "suite.json"
    candidate.write_text(json.dumps(ledger))
    expected = work / "expected.json"
    gate = [sys.executable, str(root / "benchmark/public/expected_results.py"),
            "--results-dir", str(measured), "--expected", str(expected)]
    subprocess.run([*gate, "--write"], check=True)
    ledger["results"][0]["status"] = "passed"
    candidate.write_text(json.dumps(ledger))
    before = subprocess.run([sys.executable, str(root / "benchmark/public/check_regression.py"),
                             "--baseline", str(baseline), "--candidate", str(candidate)], capture_output=True, text=True)
    after = subprocess.run(gate, capture_output=True, text=True)
    print(f"BEFORE unexpected pass: old regression gate exit={before.returncode} (accepted)")
    print(f"AFTER unexpected pass: strict expectation gate exit={after.returncode} (refused)")
    print(after.stderr.strip())
    assert before.returncode == 0 and after.returncode == 1
    subprocess.run([*gate, "--write"], check=True)  # explicit author action, outside the gate
    updated = subprocess.run(gate, capture_output=True, text=True)
    print(f"AFTER explicit expectation update: exit={updated.returncode}")
    print(updated.stdout.strip())
    assert updated.returncode == 0
