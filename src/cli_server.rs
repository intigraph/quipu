//! Shared server dispatch, including the one-release legacy command alias.
/// Pure capability marker for reviewed single-artifact installers.
pub(crate) const LAYOUT: &str = "quipu-runtime-layout/1;single-executable=1;server=1";

pub(crate) fn legacy_name(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .is_some_and(|name| name == "quipu-server" || name == "quipu-server.exe")
}

pub(crate) fn run(args: &[String]) {
    #[cfg(all(feature = "server", feature = "onnx"))]
    crate::server::run(args);
    #[cfg(not(all(feature = "server", feature = "onnx")))]
    {
        let _ = args;
        eprintln!(
            "serve and native MCP require a full build: cargo install quipu-ai --features full"
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_alias_matches_only_the_command_name() {
        assert!(legacy_name("/private/bin/quipu-server"));
        assert!(legacy_name("quipu-server.exe"));
        assert!(!legacy_name("/quipu-server/quipu"));
        assert!(!legacy_name("other-quipu-server"));
    }
}
