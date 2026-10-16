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
            return toml::from_str::<ConfigFile>(&content)
                .map(|file| file.quipu)
                .map_err(|_| {
                    crate::Error::InvalidValue(format!(
                        "invalid server configuration: {}",
                        path.display()
                    ))
                });
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
}
