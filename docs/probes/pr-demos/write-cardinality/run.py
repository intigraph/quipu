#!/usr/bin/env python3
"""Record real before/after CLI behavior using disposable databases only."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

SHAPES = """@prefix ex: <https://example.org/cardinality/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:S a sh:NodeShape; sh:targetClass ex:FactVersion;
  sh:property [sh:path ex:value; sh:maxCount 1].
"""
QUERY = "SELECT ?v WHERE { <https://example.org/cardinality/item> <https://example.org/cardinality/value> ?v }"


def command(binary, directory, arguments, expected):
    print("$ quipu " + " ".join(arguments), flush=True)
    time.sleep(0.6)
    result = subprocess.run(
        [str(binary), *arguments], cwd=directory, capture_output=True,
        text=True, timeout=30,
    )
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", flush=True)
    print(f"exit={result.returncode}", flush=True)
    assert result.returncode == expected, result
    return result


def demonstrate(side, binary, rejected):
    print(f"\n{side}: actual CLI, fresh disposable SQLite database", flush=True)
    with binary.open("rb") as source:
        print("executable SHA256=" + hashlib.file_digest(source, "sha256").hexdigest())
    print(subprocess.check_output([str(binary), "--version"], text=True), flush=True)
    with tempfile.TemporaryDirectory(prefix="quipu-cardinality-demo-") as name:
        directory = Path(name)
        (directory / "shapes.ttl").write_text(SHAPES)
        for file, value, subject in [("a.ttl", "A", "item"), ("b.ttl", "B", "item"),
                                     ("control.ttl", "control", "control")]:
            (directory / file).write_text(
                f'@prefix ex: <https://example.org/cardinality/> . '
                f'ex:{subject} a ex:FactVersion; ex:value "{value}".\n'
            )
        def knot(file, expected):
            return command(binary, directory,
                           ["knot", file, "--shapes", "shapes.ttl", "--db", "demo.db"], expected)
        knot("a.ttl", 0)
        second = knot("b.ttl", 1 if rejected else 0)
        if rejected:
            assert "SHACL validation passed" not in second.stdout
        result = command(binary, directory, ["query", QUERY, "--db", "demo.db"], 0)
        actual = [line for line in result.stdout.splitlines() if line.startswith('"')]
        assert actual == (['"A"'] if rejected else ['"A"', '"B"']), result.stdout
        knot("control.ttl", 0)
        print("Observed current values: " + ", ".join(actual), flush=True)


if __name__ == "__main__":
    if sys.stdout.isatty():
        # Keep the recorded stream LF-only, without postprocessing its output.
        import termios
        mode = termios.tcgetattr(sys.stdout.fileno())
        mode[1] &= ~termios.ONLCR
        termios.tcsetattr(sys.stdout.fileno(), termios.TCSANOW, mode)
    demonstrate("BEFORE", Path(os.environ["BEFORE_QUIPU"]).resolve(), False)
    demonstrate("AFTER", Path(os.environ["AFTER_QUIPU"]).resolve(), True)
