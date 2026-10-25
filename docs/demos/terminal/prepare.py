#!/usr/bin/env python3
"""Create a new private fixture directory, refusing an existing store/workspace."""
import argparse
from pathlib import Path
import shutil

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("workspace", type=Path)
a = p.parse_args()
a.workspace.mkdir()  # A rerender always starts with a fresh database.
(a.workspace / ".terminal-demo-sandbox").touch()
shutil.copy2(Path(__file__).parent / "fixture/episode.json", a.workspace / "episode.json")
