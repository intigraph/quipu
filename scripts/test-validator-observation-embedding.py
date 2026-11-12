#!/usr/bin/env python3
"""Actual Rust cdylib consumers under one host; never a live database."""

import argparse
import ctypes
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


SOURCE = r'''
use quipu::{Datum, Error, Op, Store, Value};
use std::ffi::{CString, c_char};
use std::sync::OnceLock;

fn capture() -> String {
    let ts = "2026-01-01T00:00:00Z";
    let mut store = Store::open_in_memory().unwrap();
    store.shacl_config_mut().validate_on_write = true;
    store.load_shapes("one", "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:maxCount 1].", ts).unwrap();
    let datum = Datum {
        entity: store.intern("urn:e").unwrap(),
        attribute: store.intern("urn:p").unwrap(),
        value: Value::Str("synthetic".into()),
        valid_from: ts.into(), valid_to: None, op: Op::Assert,
    };
    let (write, observations) = store.observe_validator(|s| s.transact(&[datum], ts, None, None));
    write.unwrap();
    assert_eq!(observations.len(), 1);
    match observations[0].manifest() {
        Ok(manifest) => manifest.implementation.artifact.0.iter().map(|b| format!("{b:02x}")).collect(),
        Err(Error::CannotVerify(_)) => "unproved".into(),
        Err(error) => format!("error:{error}"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn observed_identity() -> *const c_char {
    static RESULT: OnceLock<CString> = OnceLock::new();
    RESULT.get_or_init(|| CString::new(std::panic::catch_unwind(capture)
        .unwrap_or_else(|_| "error:panic".into())).unwrap()).as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn image_variant() -> u32 { __VARIANT__ }
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if sys.platform != "linux":
        print("SKIP: Linux shared-image control; unsupported targets refuse identity")
        return
    repo = Path(__file__).resolve().parents[1]
    host = hashlib.sha256(Path("/proc/self/exe").read_bytes()).hexdigest()
    results = []
    # Cargo's normal wrapper still governs each build and routes its target.
    with tempfile.TemporaryDirectory(prefix="validator-embedding-") as temporary:
        fixture = Path(temporary)
        (fixture / "src").mkdir()
        (fixture / "Cargo.toml").write_text(
            '[package]\nname="validator-embedding-probe"\nversion="0.0.0"\nedition="2024"\n'
            '[lib]\nname="embedding_probe"\ncrate-type=["cdylib"]\n'
            '[dependencies]\nquipu={package="quipu-ai",path='
            + json.dumps(str(repo))
            + ',default-features=false,features=["shacl"]}\n'
        )
        # Retain the repository's pinned resolution; Cargo adds only this
        # synthetic consumer to its private lockfile, without network access.
        shutil.copyfile(repo / "Cargo.lock", fixture / "Cargo.lock")
        for variant in (1, 2):
            (fixture / "src/lib.rs").write_text(SOURCE.replace("__VARIANT__", str(variant)))
            build = subprocess.run(
                ["cargo", "build", "--offline", "--manifest-path", str(fixture / "Cargo.toml"),
                 "--message-format=json-render-diagnostics"],
                capture_output=True, text=True,
            )
            if args.output:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.with_suffix(f".build-{variant}.log").write_text(build.stdout + build.stderr)
            if build.returncode:
                print(build.stderr, file=sys.stderr)
                raise SystemExit(build.returncode)
            artifacts = []
            for line in build.stdout.splitlines():
                try:
                    message = json.loads(line)
                except ValueError:
                    continue
                if message.get("reason") == "compiler-artifact" and message["target"]["name"] == "embedding_probe":
                    artifacts.extend(Path(p) for p in message["filenames"] if p.endswith(".so"))
            assert len(artifacts) == 1, artifacts
            image = fixture / f"image-{variant}.so"
            shutil.copyfile(artifacts[0], image)
            image_hash = hashlib.sha256(image.read_bytes()).hexdigest()
            library = ctypes.CDLL(str(image))
            library.image_variant.restype = ctypes.c_uint32
            assert library.image_variant() == variant
            library.observed_identity.restype = ctypes.c_char_p
            identity = library.observed_identity().decode()
            results.append({"variant": variant, "image_sha256": image_hash, "identity": identity})
        assert results[0]["image_sha256"] != results[1]["image_sha256"]
        assert hashlib.sha256(Path("/proc/self/exe").read_bytes()).hexdigest() == host
    report = {"host_sha256": host, "shared_images": results}
    if args.output:
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    assert all(row["identity"] == "unproved" for row in results), (
        "Shared-image extraction must refuse unproved executable linkage; "
        "an unchanged host hash cannot identify different validator images"
    )


if __name__ == "__main__":
    main()
