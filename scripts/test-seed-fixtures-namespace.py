#!/usr/bin/env python3
"""Check the actual generator against an archived, fixture-corrected baseline."""
import argparse
import json
import shutil
import sqlite3
import subprocess
import tempfile
from pathlib import Path

LEGACY = "http://aegis.gastown.local/ontology/"
CUSTOM = "https://example.org/fixture#"
ASSETS = ("seed-infra.ttl", "seed-agents.ttl", "seed-knowledge.ttl",
          "test-shapes.ttl", "test-episodes.json")


def generate(binary, namespace, explicit=True):
    source = Path(__file__).resolve().parents[1] / "test-fixtures"
    with tempfile.TemporaryDirectory(prefix="seed-namespace-") as temp:
        root = Path(temp)
        fixtures = root / "test-fixtures"
        fixtures.mkdir()
        for name in ASSETS:
            shutil.copyfile(source / name, fixtures / name)
        (root / ".bobbin").mkdir()
        (root / ".bobbin/config.toml").write_text(
            "[quipu]\nbase_ns = " + json.dumps(namespace) + "\n")
        command = [str(binary)] + (["--base-ns", namespace] if explicit else [])
        result = subprocess.run(command, cwd=root, capture_output=True,
                                text=True, timeout=120, check=False)
        assert result.returncode == 0, (result.returncode, result.stdout, result.stderr)
        legacy_values = []
        with sqlite3.connect(fixtures / "test-store.db") as connection:
            tables = list(connection.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"))
            for (table,) in tables:
                quoted = table.replace('"', '""')
                for row in connection.execute(f'SELECT * FROM "{quoted}"'):
                    legacy_values.extend([table, value] for value in row
                                         if isinstance(value, str) and LEGACY in value)
        return {"stdout": result.stdout, "legacy_values": legacy_values}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--baseline-json", type=Path, required=True)
    parser.add_argument("--record", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.record:
        args.baseline_json.write_text(json.dumps(generate(binary, LEGACY)))
        return
    baseline = json.loads(args.baseline_json.read_text())
    for explicit in (True, False):
        result = generate(binary, CUSTOM, explicit)
        assert not result["legacy_values"], result["legacy_values"][:2]
    assert generate(binary, LEGACY) == baseline, "legacy output differs from corrected baseline"
    with tempfile.TemporaryDirectory(prefix="seed-refusal-") as temp:
        root = Path(temp)
        (root / "test-fixtures").mkdir()
        database = root / "test-fixtures/test-store.db"
        database.write_bytes(b"preserve-existing-store")
        result = subprocess.run([str(binary), "--base-ns", ""], cwd=root,
                                capture_output=True, timeout=15, check=False)
        assert result.returncode != 0
        assert database.read_bytes() == b"preserve-existing-store"
    print("PASS: explicit/configured custom namespace, corrected legacy identity, invalid namespace refusal")


if __name__ == "__main__":
    main()
