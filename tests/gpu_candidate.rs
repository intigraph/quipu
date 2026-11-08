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
    // Cargo injects loader hints into test processes. The actual proof must
    // start a fresh process without them, before the ELF loader runs.
    if std::env::var_os("QUIPU_GPU_NATIVE_CHILD").is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "matched_gpu_loader_creates_a_strict_cuda_session",
                "--nocapture",
            ])
            .env("QUIPU_GPU_NATIVE_CHILD", "1")
            .env_remove("LD_LIBRARY_PATH")
            .env_remove("ORT_DYLIB_PATH")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "child status {}\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        print!("{}", String::from_utf8_lossy(&result.stdout));
        return;
    }
    let model = std::env::var_os("QUIPU_GPU_TEST_MODEL").expect("explicit local model");
    assert!(
        std::env::var_os("ORT_DYLIB_PATH").is_none(),
        "automatic-loader proof must not use an override"
    );
    assert!(
        std::env::var_os("LD_LIBRARY_PATH").is_none(),
        "prove absolute dependency preload without startup loader hints"
    );
    eprintln!("native stage: discovery");
    let candidate = if std::env::var_os("QUIPU_GPU_TEST_ACQUIRE").is_some() {
        quipu::onnx_runtime::acquire_gpu_candidate()
            .unwrap()
            .expect("fresh verified GPU candidate")
    } else {
        quipu::onnx_runtime::gpu_candidate()
            .unwrap()
            .expect("existing verified GPU candidate")
    };
    assert!(quipu::onnx_runtime::current().is_none());
    eprintln!("native stage: initialize selected");
    let loaded = quipu::onnx_runtime::initialize(Some(&candidate)).unwrap();
    eprintln!("native stage: runtime initialized");
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
    // Exercise the real consumer, not just another explicit bootstrap call.
    // Its initialize(None) must retain the selected core before Session::builder.
    let tokenizer = std::env::var_os("QUIPU_GPU_TEST_TOKENIZER")
        .expect("explicit local tokenizer for actual-provider regression");
    let provider = quipu::OnnxEmbeddingProvider::load(
        std::path::Path::new(&model),
        std::path::Path::new(&tokenizer),
        384,
        128,
    )
    .expect("actual provider must preserve the selected runtime");
    drop(provider);
    assert_eq!(quipu::onnx_runtime::current().unwrap().path, loaded.path);
    assert!(
        quipu::onnx_runtime::initialize(Some(&candidate.with_file_name("conflicting-core.so")))
            .unwrap_err()
            .to_string()
            .contains("different core"),
        "an explicit conflicting selection must still refuse"
    );
    eprintln!("native stage: strict CUDA session");
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
