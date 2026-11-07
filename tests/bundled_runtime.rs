//! Native bundle acquisition, override and corruption controls without a model.
#![cfg(all(feature = "onnx", feature = "bundled-onnx"))]
use std::{path::Path, process::Command};

fn child(cache: &Path, mode: &str) -> std::process::Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "runtime_probe_child", "--nocapture"])
        .env("QUIPU_RUNTIME_TEST_CHILD", mode)
        .env("XDG_CACHE_HOME", cache)
        .env_remove("ORT_DYLIB_PATH");
    if mode == "explicit-missing" {
        command.env("ORT_DYLIB_PATH", cache.join("missing-explicit-runtime"));
    }
    command.output().unwrap()
}

#[test]
fn bundle_loads_without_override_and_refuses_corruption() {
    let temporary = tempfile::tempdir().unwrap();
    let cache = temporary.path().join("cache");
    let explicit = child(&cache, "explicit-missing");
    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    assert!(
        !cache.exists(),
        "failed explicit override must not stage a fallback"
    );
    let positive = child(&cache, "positive");
    assert!(
        positive.status.success(),
        "{}",
        String::from_utf8_lossy(&positive.stderr)
    );
    let stdout = String::from_utf8(positive.stdout).unwrap();
    let line = stdout
        .lines()
        .find_map(|line| line.split_once("RUNTIME_INFO:").map(|(_, payload)| payload))
        .unwrap();
    let info: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(info["available"], true);
    assert_eq!(info["linkage"], "bundled-dynamic-cpu");
    let path = Path::new(info["path"].as_str().unwrap());
    assert!(path.starts_with(&cache));
    let mut corrupted = std::fs::read(path).unwrap();
    let end = corrupted.len() - 1;
    corrupted[end] ^= 1;
    std::fs::write(path, &corrupted).unwrap();
    let negative = child(&cache, "corrupt");
    assert!(
        negative.status.success(),
        "{}",
        String::from_utf8_lossy(&negative.stderr)
    );
    assert_eq!(
        std::fs::read(path).unwrap(),
        corrupted,
        "no automatic repair or unchecked fallback"
    );
}

#[test]
fn runtime_probe_child() {
    let Ok(mode) = std::env::var("QUIPU_RUNTIME_TEST_CHILD") else {
        return;
    };
    let result = quipu::onnx_runtime::initialize(None);
    match mode.as_str() {
        "positive" => {
            let info = result.unwrap();
            assert!(info.available);
            println!("RUNTIME_INFO:{}", serde_json::to_string(&info).unwrap());
            // Force the real ABI query, not merely the presence of a file.
            assert!(!ort::info().is_empty());
        }
        "explicit-missing" => {
            assert!(result.unwrap_err().to_string().contains("load failed"));
            assert!(quipu::onnx_runtime::current().is_none());
        }
        "corrupt" => {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("checksum mismatch")
            );
            assert!(quipu::onnx_runtime::current().is_none());
        }
        _ => panic!("unknown isolated runtime probe"),
    }
}
