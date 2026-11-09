//! Loading [`QuipuConfig`] from host config files — the fs-touching half of
//! `config.rs`, split for the file-size ratchet and native-only by nature
//! (on wasm there is no file to search; configure programmatically —
//! `docs/design/wasm-support.md` §4.4).

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::QuipuConfig;

/// Top-level config file structure — we only care about the `[quipu]` section.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ConfigFile {
    #[serde(default)]
    pub(crate) quipu: QuipuConfig,
}

impl QuipuConfig {
    /// Load configuration, searching standard locations.
    ///
    /// Resolution: `.bobbin/config.toml` in `project_dir`, then `~/.config/bobbin/config.toml`.
    /// Returns defaults if no config file is found.
    pub fn load(project_dir: &Path) -> Self {
        // Try project-local config first.
        let local_path = project_dir.join(".bobbin/config.toml");
        if let Some(cfg) = Self::load_from(&local_path) {
            return cfg;
        }

        // Try user-level config.
        if let Some(home) = std::env::var_os("HOME") {
            let user_path = PathBuf::from(home).join(".config/bobbin/config.toml");
            if let Some(cfg) = Self::load_from(&user_path) {
                return cfg;
            }
        }

        Self::default()
    }

    /// Server loading fails closed on an existing invalid/unreadable config.
    /// In particular a malformed replica declaration must not open a writer.
    /// Error messages deliberately omit parser excerpts, which may hold secrets.
    pub fn load_strict(project_dir: &Path) -> crate::Result<Self> {
        let mut paths = vec![project_dir.join(".bobbin/config.toml")];
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".config/bobbin/config.toml"));
        }
        for path in paths {
            let content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    return Err(crate::Error::InvalidValue(format!(
                        "unreadable server configuration: {}",
                        path.display()
                    )));
                }
            };
            let parsed = toml::from_str::<ConfigFile>(&content)
                .map(|file| file.quipu)
                .map_err(|_| {
                    crate::Error::InvalidValue(format!(
                        "invalid server configuration: {}",
                        path.display()
                    ))
                })?;
            refuse_misplaced_scoped_keys(&content, &path)?;
            return Ok(parsed);
        }
        Ok(Self::default())
    }

    /// Load from a specific TOML file. Returns `None` if file doesn't exist or has no `[quipu]` section.
    fn load_from(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        let file: ConfigFile = toml::from_str(&content).ok()?;
        Some(file.quipu)
    }
}

/// A scoped-read profile is opt-in through exactly `[quipu.server.scoped_reads]`.
/// `ServerConfig` ignores unknown keys, so a misspelled or misplaced profile
/// would otherwise parse as "no profile" and start the legacy open-read
/// server, the one outcome the profile exists to prevent (aegis-44r G5).
/// Apart from the existing server writer-credential key, any other key in
/// `[quipu]` or `[quipu.server]` whose name contains "scoped" refuses startup.
/// Unrelated unknown keys stay accepted, so this
/// does not tighten parsing for existing configurations. The message names
/// neither the key nor any value.
fn refuse_misplaced_scoped_keys(content: &str, path: &Path) -> crate::Result<()> {
    let refuse = || {
        crate::Error::InvalidValue(format!(
            "unrecognized scoped read configuration key in {}; refusing startup",
            path.display()
        ))
    };
    let looks_scoped = |key: &str| {
        key.to_ascii_lowercase()
            .replace(['-', '_', ' '], "")
            .contains("scoped")
    };
    let value: toml::Table = toml::from_str(content).map_err(|_| refuse())?;
    let Some(quipu) = value.get("quipu").and_then(toml::Value::as_table) else {
        return Ok(());
    };
    if quipu.keys().any(|key| looks_scoped(key)) {
        return Err(refuse());
    }
    if let Some(server) = quipu.get("server").and_then(toml::Value::as_table)
        && server.keys().any(|key| {
            !matches!(
                key.as_str(),
                "scoped_reads" | "scoped_writer_credentials_file"
            ) && looks_scoped(key)
        })
    {
        return Err(refuse());
    }
    Ok(())
}

#[cfg(test)]
mod strict_tests {
    use super::*;
    #[test]
    fn malformed_replica_config_cannot_fall_back_to_a_writer_or_echo_values() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
        let path = dir.path().join(".bobbin/config.toml");
        for content in [
            "[quipu.server.replica]\n",
            "[quipu.server.replica]\nprimary_url = 123",
            "primary_url = \"private-sentinel\"\n[",
        ] {
            std::fs::write(&path, content).unwrap();
            let error = QuipuConfig::load_strict(dir.path()).unwrap_err();
            assert!(!error.to_string().contains("private-sentinel"));
        }
        std::fs::write(
            &path,
            "[quipu.server.replica]\nprimary_url = \"http://primary.example\"",
        )
        .unwrap();
        assert!(
            QuipuConfig::load_strict(dir.path())
                .unwrap()
                .server
                .replica
                .is_some()
        );
    }

    #[test]
    fn misspelled_or_misplaced_scoped_profile_refuses_instead_of_open_reads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
        let path = dir.path().join(".bobbin/config.toml");
        for content in [
            "[quipu.server.scoped_read]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
            "[quipu.server.scopedreads]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
            "[quipu.server.scoped-reads]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
            "[quipu.server.Scoped_Reads]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
            "[quipu.scoped_reads]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
            "[quipu.server]\nscoped = true\n",
            "[quipu.server]\nscoped_writer_credential_file = \"private-sentinel\"\n",
            "[quipu]\nscoped_writer_credentials_file = \"private-sentinel\"\n",
        ] {
            std::fs::write(&path, content).unwrap();
            let error = QuipuConfig::load_strict(dir.path())
                .unwrap_err()
                .to_string();
            assert!(error.contains("refusing startup"), "{content}: {error}");
            assert!(
                !error.contains("urn:o") && !error.contains("/p\""),
                "{content}"
            );
        }
        // The correct section still parses into an explicit profile.
        std::fs::write(
            &path,
            "[quipu.server.scoped_reads]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
        )
        .unwrap();
        assert!(
            QuipuConfig::load_strict(dir.path())
                .unwrap()
                .server
                .scoped_reads
                .is_some()
        );
        // Unrelated unknown keys are not newly refused.
        std::fs::write(&path, "[quipu.server]\nfuture_flag = true\n").unwrap();
        assert!(QuipuConfig::load_strict(dir.path()).is_ok());
    }

    #[test]
    fn existing_scoped_writer_credentials_remain_valid_with_read_profile_guard() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
        let path = dir.path().join(".bobbin/config.toml");
        for profile in [
            "",
            "\n[quipu.server.scoped_reads]\npolicy_path = \"/p\"\nowner = \"urn:o\"\n",
        ] {
            std::fs::write(
                &path,
                format!(
                    "[quipu.server]\nscoped_writer_credentials_file = \"/writer-credentials\"\n{profile}"
                ),
            )
            .unwrap();
            let config = QuipuConfig::load_strict(dir.path()).unwrap();
            assert_eq!(
                config.server.scoped_writer_credentials_file,
                Some("/writer-credentials".to_owned())
            );
            assert_eq!(config.server.scoped_reads.is_some(), !profile.is_empty());
        }
    }
}
