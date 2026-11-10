#!/usr/bin/env python3
"""Compare one isolated occupied-reader fixture on two server binaries."""
import importlib.util
from pathlib import Path
import sys

root = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("startup", root / "scripts/test-read-process-startup.py")
startup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(startup)
base, head, cli = map(lambda value: str(Path(value).resolve()), sys.argv[1:4])
print("BEFORE: one foreign listener occupies the configured reader port", flush=True)
try:
    startup.run(base, cli, "occupied")
except AssertionError as error:
    lines = [line for line in str(error).splitlines() if "cannot start read process pool:" in line]
    assert len(lines) == 1 and "Address already in use" in lines[0], str(error)
    print(lines[0], flush=True)
    print("Observed: primary unavailable (expected baseline failure)", flush=True)
else:
    raise AssertionError("baseline unexpectedly survived")
print("AFTER: same occupied-reader fixture", flush=True)
startup.run(head, cli, "occupied")
print("CONTROL: unoccupied reader port", flush=True)
startup.run(head, cli, "free")
