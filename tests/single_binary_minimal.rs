//! CLI-only builds refuse server paths before configuration or filesystem work.
#![cfg(all(feature = "shacl", not(all(feature = "server", feature = "onnx"))))]
use std::process::Command;

#[test]
fn unavailable_server_and_mcp_leave_the_store_directory_absent() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".bobbin")).unwrap();
    std::fs::write(root.path().join(".bobbin/config.toml"), "invalid TOML [").unwrap();
    for mode in ["serve", "mcp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
            .args([mode, "--db"])
            .arg(root.path().join("absent/store.db"))
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("require a full build"));
        assert!(!root.path().join("absent").exists());
    }
}
