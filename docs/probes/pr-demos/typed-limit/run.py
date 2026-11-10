import os
from pathlib import Path
import subprocess

p = Path(__file__).parent
print("BEFORE: base 360f8f49 with the same newly injected work-bound test", flush=True)
print((p / "before.txt").read_text(), flush=True)
print("AFTER: candidate native evaluator, same 10000 typed facts and canary query", flush=True)
subprocess.run([os.environ["QUIPU_TEST_BIN"], "sparql::triple::limited_tests", "--nocapture"], check=True)
