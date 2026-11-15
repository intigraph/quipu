//! Private-file reproducer for native binding writer ownership.
use quipu::bindings::Engine;

fn main() {
    let expect_open = std::env::args().any(|arg| arg == "--expect-open");
    let dir = tempfile::tempdir().expect("private fixture directory");
    let path = dir.path().join("demo.db");
    let path = path.to_str().unwrap();
    println!("core revision: {}", quipu::GIT_SHA);
    let owner = Engine::open(path).expect("first writer opens");
    println!("first writer: OPENED");
    let second = Engine::open(path);
    println!(
        "same-file second writer: {}",
        if second.is_ok() { "OPENED" } else { "REFUSED" }
    );
    assert_eq!(second.is_ok(), expect_open, "unexpected ownership result");
    if let Err(error) = &second {
        assert!(error.to_string().contains("single-writer lock"));
    }
    drop(second);
    drop(owner);
    let _reopened = Engine::open(path).expect("owner drop releases the file");
    println!("after owner closes: OPENED");
    println!("fixture: disposable database; no served store accessed");
}
