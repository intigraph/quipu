#!/usr/bin/env python3
"""Opt-in CI mirror acquisition; pins remain in the native runtime catalogue."""
import importlib.util
import argparse
import hashlib
import os
from pathlib import Path
import platform
import re
import shutil
import tempfile
from urllib.parse import urlsplit


def distribution(catalogue, target):
    entries = re.findall(r'"([^"\n]+)" => Distribution \{(.*?)\n        \}', catalogue, re.S)
    matches = [body for name, body in entries if name == target]
    if len(matches) != 1:
        raise ValueError("target lacks one pinned ONNX distribution")
    fields = {}
    for name in ("url", "archive_sha256"):
        values = re.findall(r'\b' + name + r': "([^"\n]+)"', matches[0])
        if len(values) != 1:
            raise ValueError("runtime catalogue field is missing or ambiguous")
        fields[name] = values[0]
    return fields


def native_target():
    targets = {
        ("Linux", "x86_64"): "x86_64-unknown-linux-gnu",
        ("Linux", "aarch64"): "aarch64-unknown-linux-gnu",
        ("Darwin", "arm64"): "aarch64-apple-darwin",
        ("Darwin", "x86_64"): "x86_64-apple-darwin",
        ("Windows", "AMD64"): "x86_64-pc-windows-msvc",
    }
    try:
        return targets[(platform.system(), platform.machine())]
    except KeyError:
        raise ValueError("unsupported native CI platform") from None


def prepare(root, environment):
    mirror = environment.get("CI_ARTIFACT_MIRROR_URL", "")
    cache = environment.get("CI_ARTIFACT_CACHE_DIR", "")
    if not mirror and not cache:
        print("CI artifact mirror unset; native acquisition unchanged")
        return
    pin = distribution((root / "build/onnx_catalogue.rs").read_text(), native_target())
    spec = importlib.util.spec_from_file_location("pinned_asset", root / "scripts/fetch-pinned-ci-asset.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if cache:
        cached = Path(cache) / "sha256" / pin["archive_sha256"] / urlsplit(pin["url"]).path.rsplit("/", 1)[-1]
        if not cached.exists():
            # Existing Cargo caches can warm the new dedicated archive cache.
            # Filenames alone prove nothing: select only the current exact pin.
            candidates = list((root / "target/debug/build").glob("quipu-ai-*/out/onnxruntime-source.archive"))
            if len(candidates) > 32:
                raise ValueError("too many candidate cached runtime archives")
            for candidate in candidates:
                if candidate.stat().st_size > module.MAX_BYTES:
                    continue
                with candidate.open("rb") as stream:
                    digest = hashlib.sha256()
                    while chunk := stream.read(1024 * 1024):
                        digest.update(chunk)
                if digest.hexdigest() == pin["archive_sha256"]:
                    cached.parent.mkdir(parents=True, exist_ok=True)
                    with tempfile.NamedTemporaryFile(dir=cached.parent, delete=False) as saved:
                        saved_path = Path(saved.name)
                    try:
                        shutil.copyfile(candidate, saved_path)
                        os.replace(saved_path, cached)
                    finally:
                        saved_path.unlink(missing_ok=True)
                    break
    output = Path(environment["RUNNER_TEMP"]) / "pinned-onnx-runtime.archive"
    receipt = module.fetch(pin["url"], pin["archive_sha256"], output, mirror, cache=cache)
    # GitHub's environment-file delimiter is a newline; refuse before writing
    # rather than allowing a runner path to inject another environment entry.
    if "\n" in str(output) or "\r" in str(output):
        raise ValueError("invalid runner temporary path")
    with open(environment["GITHUB_ENV"], "a", encoding="utf-8") as stream:
        stream.write("QUIPU_ORT_ARCHIVE=" + str(output) + "\n")
    print("verified CI ONNX archive: %(source)s SHA256 %(sha256)s" % receipt)


if __name__ == "__main__":
    try:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument("--metadata", action="store_true")
        args = parser.parse_args()
        root = Path(__file__).resolve().parents[1]
        if args.metadata:
            pin = distribution((root / "build/onnx_catalogue.rs").read_text(), native_target())
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
                stream.write("key=pinned-onnx-" + native_target() + "-" + pin["archive_sha256"] + "\n")
        else:
            prepare(root, os.environ)
    except (ValueError, KeyError, OSError) as error:
        # No URLs, response bodies, or runner configuration in public logs.
        raise SystemExit("pinned CI ONNX acquisition refused (" + type(error).__name__ + ")")
