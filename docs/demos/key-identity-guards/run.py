#!/usr/bin/env python3
"""Compare the same key-identity fixture in two already built executables."""
import argparse
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("base", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--candidate-revision", required=True)
args = parser.parse_args()
for label, executable, flags in [
    ("BASE ed79bf881ec99e100d7fe3b047cb08d315bbdb59", args.base, []),
    ("CANDIDATE " + args.candidate_revision, args.candidate, ["--expect-guards"]),
]:
    print(label, flush=True)
    subprocess.run([str(executable.resolve(strict=True)), *flags], check=True)
    print(flush=True)
