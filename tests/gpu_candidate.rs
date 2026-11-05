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

/// An explicitly requested native qualification, never a CI download/model/GPU
/// side effect. Cargo admission and the operator's native scope apply.
#[test]
#[ignore = "requires an eligible NVIDIA GPU, an existing verified matched bundle and explicitly supplied local model"]
fn matched_gpu_loader_creates_a_strict_cuda_session() {
    let model = std::env::var_os("QUIPU_GPU_TEST_MODEL").expect("explicit local model");
    assert!(
        std::env::var_os("ORT_DYLIB_PATH").is_none(),
        "automatic-loader proof must not use an override"
    );
    assert!(
        std::env::var_os("LD_LIBRARY_PATH").is_none(),
        "prove absolute dependency preload without startup loader hints"
    );
    let candidate = quipu::onnx_runtime::gpu_candidate()
        .unwrap()
        .expect("existing verified GPU candidate");
    assert!(quipu::onnx_runtime::current().is_none());
    let loaded = quipu::onnx_runtime::initialize(Some(&candidate)).unwrap();
    assert!(loaded.available);
    assert_eq!(loaded.linkage, "dynamic-selected");
    // A repeated request for the original candidate resolves to the same
    // privately staged core; it cannot select or initialize a second runtime.
    assert_eq!(
        quipu::onnx_runtime::initialize(Some(&candidate))
            .unwrap()
            .path,
        loaded.path
    );
    let session = ort::session::Session::builder()
        .unwrap()
        .with_execution_providers([ort::ep::CUDA::default()
            .with_tf32(false)
            .build()
            .error_on_failure()])
        .unwrap()
        .commit_from_file(std::path::Path::new(&model))
        .unwrap();
    assert!(!session.outputs().is_empty());
    println!(
        "MATCHED_GPU_STRICT_SESSION:{}",
        serde_json::to_string(&loaded).unwrap()
    );
}
