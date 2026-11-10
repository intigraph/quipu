"""Controlled unavailable-upstream demonstration, with no remote network calls."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
from urllib.error import HTTPError

root = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("pinned_asset", root / "scripts/fetch-pinned-ci-asset.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
data = b"independently pinned demonstration archive"
pin = hashlib.sha256(data).hexdigest()
url = "https://upstream.example/archive.tgz"
calls = []


def unavailable(url, timeout):
    calls.append(url)
    raise HTTPError(url, 504, "controlled upstream outage", {}, None)


with tempfile.TemporaryDirectory() as directory:
    temporary = Path(directory)
    print("Before: unavailable upstream, no dedicated archive cache")
    try:
        module.fetch(url, pin, temporary / "output", opener=unavailable)
    except ValueError as error:
        print(error)
    print("After: same unavailable upstream, independently verified cache seed")
    cached = temporary / "cache/sha256" / pin / "archive.tgz"
    cached.parent.mkdir(parents=True)
    cached.write_bytes(data)
    calls.clear()
    receipt = module.fetch(url, pin, temporary / "output", cache=temporary / "cache", opener=unavailable)
    print(receipt)
    print("Upstream calls:", len(calls))
    cached.write_bytes(b"corrupt")
    try:
        module.fetch(url, pin, temporary / "output", cache=temporary / "cache", opener=unavailable)
    except ValueError as error:
        print("Corrupt cache:", error)
    print("Upstream calls after corrupt cache:", len(calls))
