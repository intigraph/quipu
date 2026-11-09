#!/usr/bin/env python3
"""Offline controls for mirror order, immutable pins, fallback and CI opt-in."""
import hashlib
import importlib.util
import io
import shutil
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError, URLError

ROOT = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


asset = load("pinned_asset", "fetch-pinned-ci-asset.py")
prepare = load("prepare_onnx", "prepare-ci-onnx.py")
DATA = b"pinned archive fixture"
PIN = hashlib.sha256(DATA).hexdigest()
URL = "https://upstream.example/releases/archive.tgz"
MIRROR = "https://mirror.example/assets"


class Acquisition(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.output = Path(self.temporary.name) / "archive"
        self.calls = []

    def opener(self, responses):
        def open_url(url, timeout):
            self.assertEqual(timeout, 60)
            self.calls.append(url)
            response = responses.pop(0)
            if isinstance(response, Exception):
                raise response
            return io.BytesIO(response)
        return open_url

    def test_mirror_first_writes_only_verified_bytes(self):
        receipt = asset.fetch(URL, PIN, self.output, MIRROR, opener=self.opener([DATA]))
        self.assertEqual(receipt, {"source": "mirror", "sha256": PIN, "bytes": len(DATA)})
        self.assertEqual(self.calls, [MIRROR + "/sha256/" + PIN + "/archive.tgz"])
        self.assertEqual(self.output.read_bytes(), DATA)

    def test_unset_mirror_uses_original_url(self):
        asset.fetch(URL, PIN, self.output, opener=self.opener([DATA]))
        self.assertEqual(self.calls, [URL])

    def test_cache_hit_is_verified_without_upstream(self):
        cache = self.output.parent / "cache"
        cached = cache / "sha256" / PIN / "archive.tgz"
        cached.parent.mkdir(parents=True)
        cached.write_bytes(DATA)
        receipt = asset.fetch(URL, PIN, self.output, cache=cache, opener=self.opener([]))
        self.assertEqual(receipt["source"], "cache")
        self.assertEqual(self.calls, [])
        self.assertEqual(self.output.read_bytes(), DATA)

    def test_mirror_unavailable_then_cache_hit_never_calls_upstream(self):
        cache = self.output.parent / "cache"
        cached = cache / "sha256" / PIN / "archive.tgz"
        cached.parent.mkdir(parents=True)
        cached.write_bytes(DATA)
        receipt = asset.fetch(URL, PIN, self.output, MIRROR, cache=cache,
                              opener=self.opener([URLError("unavailable")]))
        self.assertEqual(receipt["source"], "cache")
        self.assertEqual(len(self.calls), 1)

    def test_corrupt_cache_refuses_without_upstream(self):
        cache = self.output.parent / "cache"
        cached = cache / "sha256" / PIN / "archive.tgz"
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
            asset.fetch(URL, PIN, self.output, cache=cache, opener=self.opener([]))
        self.assertEqual(self.calls, [])
        self.assertFalse(self.output.exists())

    def test_cold_cache_is_populated_only_after_pin_verification(self):
        cache = self.output.parent / "cache"
        asset.fetch(URL, PIN, self.output, cache=cache, opener=self.opener([DATA]))
        self.assertEqual((cache / "sha256" / PIN / "archive.tgz").read_bytes(), DATA)
        self.assertEqual(self.calls, [URL])

    def test_unavailable_mirror_falls_back_once(self):
        for failure in (URLError("offline"), TimeoutError(),
                        HTTPError(MIRROR, 404, "absent", {}, None),
                        HTTPError(MIRROR, 504, "unavailable", {}, None)):
            with self.subTest(failure=type(failure).__name__):
                self.calls.clear()
                receipt = asset.fetch(URL, PIN, self.output, MIRROR,
                                      opener=self.opener([failure, DATA]))
                self.assertEqual(receipt["source"], "upstream")
                self.assertEqual(len(self.calls), 2)
                self.assertEqual(self.calls[-1], URL)

    def test_bad_mirror_pin_refuses_without_fallback_or_replacing_destination(self):
        self.output.write_bytes(b"prior bytes")
        with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
            asset.fetch(URL, PIN, self.output, MIRROR, opener=self.opener([b"wrong", DATA]))
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.output.read_bytes(), b"prior bytes")
        self.assertEqual(list(self.output.parent.iterdir()), [self.output])

    def test_access_refusal_is_not_a_fallback(self):
        for code in (401, 403):
            self.calls.clear()
            with self.assertRaisesRegex(ValueError, "access refused"):
                asset.fetch(URL, PIN, self.output, MIRROR, opener=self.opener([
                    HTTPError(MIRROR, code, "private response body", {}, None), DATA]))
            self.assertEqual(len(self.calls), 1)
            self.assertFalse(self.output.exists())

    def test_upstream_mismatch_and_unavailability_never_publish(self):
        for result in (b"wrong", HTTPError(URL, 504, "private", {}, None)):
            with self.assertRaises(ValueError):
                asset.fetch(URL, PIN, self.output, opener=self.opener([result]))
            self.assertFalse(self.output.exists())

    def test_byte_budget_is_enforced_and_temporary_removed(self):
        with patch.object(asset, "MAX_BYTES", 4), self.assertRaisesRegex(ValueError, "budget"):
            asset.fetch(URL, PIN, self.output, MIRROR, opener=self.opener([DATA]))
        self.assertEqual(list(self.output.parent.iterdir()), [])

    def test_invalid_pin_and_credential_urls_fail_before_network(self):
        for url, pin, mirror in ((URL, "bad", MIRROR), ("http://example/a", PIN, MIRROR),
                                 (URL, PIN, "https://user:secret@example"),
                                 (URL + "?token=private", PIN, MIRROR)):
            with self.assertRaises(ValueError):
                asset.fetch(url, pin, self.output, mirror, opener=self.opener([]))
        self.assertEqual(self.calls, [])

    def test_redirect_never_downgrades_transport(self):
        with self.assertRaisesRegex(ValueError, "unsafe asset redirect"):
            asset.HTTPSRedirects().redirect_request(None, None, 302, "", {}, "http://example/a")

    def test_unconfigured_ci_does_not_read_catalogue_or_write_environment(self):
        prepare.prepare(Path(self.temporary.name) / "absent", {})
        self.assertEqual(list(self.output.parent.iterdir()), [])

    def test_ci_warms_from_verified_cargo_cache_and_exports_only_after_verification(self):
        root = self.output.parent
        (root / "scripts").mkdir()
        shutil.copyfile(ROOT / "scripts/fetch-pinned-ci-asset.py",
                        root / "scripts/fetch-pinned-ci-asset.py")
        (root / "build").mkdir()
        (root / "build/onnx_catalogue.rs").write_text(
            '"fixture-target" => Distribution {\n            url: "' + URL
            + '",\n            archive_sha256: "' + PIN + '",\n        }')
        seed = root / "target/debug/build/quipu-ai-fixture/out/onnxruntime-source.archive"
        seed.parent.mkdir(parents=True)
        seed.write_bytes(DATA)
        env_file = root / "environment"
        with patch.object(prepare, "native_target", return_value="fixture-target"):
            prepare.prepare(root, {"CI_ARTIFACT_CACHE_DIR": str(root / "cache"),
                                   "RUNNER_TEMP": str(root), "GITHUB_ENV": str(env_file)})
        self.assertEqual((root / "pinned-onnx-runtime.archive").read_bytes(), DATA)
        self.assertEqual(env_file.read_text(), "QUIPU_ORT_ARCHIVE="
                         + str(root / "pinned-onnx-runtime.archive") + "\n")

    def test_all_native_catalogue_pins_are_selected_without_a_second_pin_list(self):
        catalogue = (ROOT / "build/onnx_catalogue.rs").read_text()
        for target in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
                       "x86_64-apple-darwin", "aarch64-apple-darwin", "x86_64-pc-windows-msvc"):
            pin = prepare.distribution(catalogue, target)
            asset.safe_url(pin["url"])
            self.assertRegex(pin["archive_sha256"], r"^[0-9a-f]{64}$")
        with self.assertRaises(ValueError):
            prepare.distribution(catalogue, "wasm32-unknown-unknown")


if __name__ == "__main__":
    unittest.main()
