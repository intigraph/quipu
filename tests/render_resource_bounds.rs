//! Actual CLI memory falsifier under an OS limit, never against a service.
#![cfg(all(feature = "shacl", target_os = "linux"))]

use std::process::Command;

#[test]
fn doubling_template_is_refused_before_large_allocation() {
    let root = tempfile::tempdir().unwrap();
    let attack = format!(
        "{{% set s = 'x' %}}{}{{{{ s[0] }}}}",
        "{% set s = s ~ s %}".repeat(26)
    );
    for (template, success) in [("{{ 'x' }}", true), (attack.as_str(), false)] {
        // Parsing reserves a 256 MiB stack. Leave virtual-address room for
        // linked feature surfaces (notably LanceDB); RSS remains capped below.
        let result = Command::new("prlimit")
            .args([
                "--as=1073741824",
                "--",
                "/usr/bin/time",
                "-f",
                "MAX_RSS_KB=%M",
                env!("CARGO_BIN_EXE_quipu"),
                "render",
                "--query",
                "SELECT ?v WHERE { VALUES ?v { 'positive' } }",
                "--template",
                template,
                "--dest",
                root.path().join("owned").to_str().unwrap(),
                "--dry-run",
                "--db",
                root.path().join("private.db").to_str().unwrap(),
            ])
            .current_dir(root.path())
            .env("HOME", root.path())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert_eq!(result.status.success(), success, "{stderr}");
        if !success {
            assert!(
                stderr.contains("allocation operators/assignment"),
                "{stderr}"
            );
        }
        let rss = stderr
            .lines()
            .find_map(|line| line.strip_prefix("MAX_RSS_KB="))
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(rss < 96 * 1024, "unexpected RSS {rss} KiB");
        assert!(!root.path().join("owned").exists());
    }
}
