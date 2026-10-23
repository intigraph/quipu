//! Linux syscall spies prove help does not read config/credentials or network.
#![cfg(all(feature = "shacl", target_os = "linux"))]
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::Path,
    process::Command,
};

fn trace(root: &Path, name: &str, binary: &Path, args: &[&str], port: u16) -> String {
    let log = root.join(name);
    let output = Command::new("strace")
        .args(["-f", "-e", "trace=%file,%network", "-o"])
        .arg(&log)
        .arg(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("QUIPU_AUTH_TOKEN_FILE", root.join("home/dummy-credential"))
        .env("QUIPU_HELP_SPY_ROOT", root)
        .env("QUIPU_HELP_SPY_PORT", port.to_string())
        .current_dir(root.join("cwd"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(log).unwrap()
}
#[test]
fn help_io_spies_have_positive_file_and_network_controls() {
    if Command::new("strace").arg("--version").output().is_err() {
        eprintln!(
            "strace unavailable: syscall spy skipped; run this test on Linux with strace for IO acceptance"
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("home")).unwrap();
    fs::create_dir_all(root.join("cwd/.bobbin")).unwrap();
    fs::write(root.join("home/dummy-credential"), "public-test-value").unwrap();
    fs::write(root.join("cwd/.bobbin/config.toml"), "invalid TOML [").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Exercise the same instrument using an actual file read and loopback
    // connection. A broken/empty trace cannot pass as a clean help result.
    let positive = trace(
        root,
        "positive.trace",
        &std::env::current_exe().unwrap(),
        &["--exact", "spy_positive_helper", "--ignored", "--nocapture"],
        port,
    );
    assert!(positive.contains("dummy-credential"), "{positive}");
    assert!(positive.contains("connect("), "{positive}");
    listener.accept().unwrap();
    for (n, args) in [
        vec!["shapes", "load", "--help"],
        vec!["--help"],
        vec!["mcp", "--help"],
        vec!["hooks", "install", "-h"],
        vec!["import", "review", "notify", "--help"],
    ]
    .iter()
    .enumerate()
    {
        let log = trace(
            root,
            &format!("help-{n}.trace"),
            Path::new(env!("CARGO_BIN_EXE_quipu")),
            args,
            port,
        );
        assert!(!log.contains(".bobbin/config.toml"), "config access: {log}");
        assert!(
            !log.contains("dummy-credential"),
            "credential access: {log}"
        );
        assert!(!log.contains("socket("), "network socket: {log}");
        assert!(!log.contains("connect("), "network connect: {log}");
        assert!(!log.contains("quipu-server"), "MCP companion access: {log}");
    }
    assert!(!root.join("cwd/.bobbin/quipu").exists());
}
#[test]
#[ignore = "subprocess positive control for the syscall spy"]
fn spy_positive_helper() {
    let root = std::env::var("QUIPU_HELP_SPY_ROOT").unwrap();
    assert_eq!(
        fs::read(Path::new(&root).join("home/dummy-credential")).unwrap(),
        b"public-test-value"
    );
    let port: u16 = std::env::var("QUIPU_HELP_SPY_PORT")
        .unwrap()
        .parse()
        .unwrap();
    TcpStream::connect(("127.0.0.1", port)).unwrap();
}
