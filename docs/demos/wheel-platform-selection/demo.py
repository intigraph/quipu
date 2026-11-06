#!/usr/bin/env python3
"""Compare frozen base workflow matrices with the selector on an isolated Git fixture."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

import yaml

ROOT = Path(__file__).resolve().parents[3]
BASE = "d52acf0315dd36141716674c218c3e6c08e13632"
base = yaml.safe_load(subprocess.check_output(
    ["git", "-C", str(ROOT), "show", BASE + ":.github/workflows/python-bindings.yml"], text=True,
))
spec = importlib.util.spec_from_file_location("selector", ROOT / "scripts/python-wheel-platforms.py")
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


def counts(mac):
    matrices = selector.matrices(mac)
    return {name: len(matrix["include"]) for name, matrix in matrices.items()}


with tempfile.TemporaryDirectory() as fixture:
    env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)

    def git(*args):
        return subprocess.check_output(["git", *args], cwd=fixture, env=env, text=True,
                                       stderr=subprocess.DEVNULL).strip()

    def commit():
        git("add", ".")
        git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")
        return git("rev-parse", "HEAD")

    git("init", "-q")
    (Path(fixture) / "src").mkdir()
    source = Path(fixture) / "src/generic.rs"
    source.write_text("fn shared() {}\n")
    before = commit()
    source.write_text("fn shared() { let value = 1; }\n")
    after = commit()
    previous = Path.cwd()
    try:
        os.chdir(fixture)
        mac, _ = selector.select("pull_request", before, after)
        wheels = base["jobs"]["wheels"]["strategy"]["matrix"]["include"]
        tests = base["jobs"]["test-wheels"]["strategy"]["matrix"]
        print("Target: isolated Git fixture; no hosted runs or installed services")
        print("Base workflow:", BASE)
        print("Selector source: b3317a35 (source is unchanged by this demonstration)")
        print("Input: pull_request, portable src/generic.rs edit")
        print("Before:", json.dumps({"wheels": len(wheels), "tests": len(tests["name"]) * len(tests["python"])}))
        print("After:", json.dumps(counts(mac)))
        assert counts(mac) == {"wheels": 2, "tests": 8}
        source.write_text('#[cfg(target_os = "macos")]\nfn shared() {}\n')
        platform = commit()
        print("macOS code control:", json.dumps(counts(selector.select("pull_request", after, platform)[0])))
        print("Scheduled control:", json.dumps(counts(selector.select("schedule", "", "")[0])))
        assert counts(selector.select("pull_request", after, platform)[0]) == {"wheels": 3, "tests": 12}
        print("Five matrix jobs avoided for the portable PR; platform coverage retained by both controls.")
        print("Limit: one new Linux selector job; no measured live queue-drain or runtime-speed claim.")
    finally:
        os.chdir(previous)
