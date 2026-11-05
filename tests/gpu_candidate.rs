//! Discovery controls run in subprocesses: no global environment races, GPU,
//! network, model or ORT initialization in these tests.
#![cfg(feature = "onnx")]
use std::process::Command;

#[test]
fn discovery_override_missing_cache_and_corruption_controls() {
    let temporary = tempfile::tempdir().unwrap();
    for mode in ["missing", "explicit", "optout", "malformed"] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "discovery_child", "--nocapture"])
            .env("QUIPU_GPU_DISCOVERY_CHILD", mode)
            .env("QUIPU_GPU_CACHE", temporary.path().join(mode))
            .env("XDG_CACHE_HOME", temporary.path().join("private-cache"))
            .env_remove("ORT_DYLIB_PATH")
            .env_remove("QUIPU_GPU")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(
        !temporary.path().join("private-cache").exists(),
        "discovery must not stage bytes"
    );
}

#[test]
fn discovery_child() {
    let Ok(mode) = std::env::var("QUIPU_GPU_DISCOVERY_CHILD") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("QUIPU_GPU_CACHE").unwrap());
    if mode != "missing" {
        let bundle = root.join("onnx-1.23.2-cuda12-v1");
        std::fs::create_dir_all(bundle.join("lib")).unwrap();
        std::fs::write(bundle.join("receipt.json"), b"{}").unwrap();
    }
    if mode == "malformed" {
        assert!(quipu::onnx_runtime::gpu_candidate().is_err());
    } else {
        // A separate child is needed to set policy before any runtime code.
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "discovery_leaf", "--nocapture"]);
        if mode == "explicit" {
            command.env("ORT_DYLIB_PATH", "/does-not-exist");
        }
        if mode == "optout" {
            command.env("QUIPU_GPU", "0");
        }
        assert!(command.status().unwrap().success());
    }
    assert!(quipu::onnx_runtime::current().is_none());
}

#[test]
fn discovery_leaf() {
    if std::env::var_os("QUIPU_GPU_DISCOVERY_CHILD").is_none() {
        return;
    }
    assert!(quipu::onnx_runtime::gpu_candidate().unwrap().is_none());
    assert!(quipu::onnx_runtime::current().is_none());
}
