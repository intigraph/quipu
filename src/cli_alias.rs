//! Store-free installation of the supported `qp -> quipu` command name.
use std::{env, ffi::OsStr, fs, path::Path};

type Result<T> = std::result::Result<T, String>;

fn existing_alias(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot inspect {}: {e}", path.display())),
        Ok(meta) => {
            if meta.file_type().is_symlink()
                && fs::read_link(path).ok().as_deref() == Some(Path::new("quipu"))
                && path.parent().is_some_and(|p| p.join("quipu").is_file())
            {
                Ok(())
            } else {
                Err(format!(
                    "qp collision: refusing existing {}; only a relative qp -> quipu alias is accepted",
                    path.display()
                ))
            }
        }
    }
}

fn preflight_path(bin: &Path, path: Option<&OsStr>) -> Result<()> {
    existing_alias(&bin.join("qp"))?;
    if let Some(path) = path {
        for directory in env::split_paths(path) {
            let candidate = directory.join("qp");
            if candidate.is_file() {
                existing_alias(&candidate)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn preflight(bin: &Path) -> Result<()> {
    preflight_path(bin, env::var_os("PATH").as_deref())
}

pub(crate) fn ensure(bin: &Path) -> Result<()> {
    preflight(bin)?;
    let target = bin.join("quipu");
    if !target.is_file() {
        return Err(format!("install {} before creating qp", target.display()));
    }
    let alias = bin.join("qp");
    if fs::symlink_metadata(&alias).is_err() {
        #[cfg(unix)]
        std::os::unix::fs::symlink("quipu", &alias).map_err(|e| {
            format!(
                "create {} without replacing existing files: {e}",
                alias.display()
            )
        })?;
        #[cfg(not(unix))]
        return Err("relative qp aliases currently require a Unix host".into());
    }
    let link = fs::read_link(&alias).map_err(|e| format!("read {}: {e}", alias.display()))?;
    let resolved_alias =
        fs::canonicalize(&alias).map_err(|e| format!("resolve {}: {e}", alias.display()))?;
    let resolved_target =
        fs::canonicalize(&target).map_err(|e| format!("resolve {}: {e}", target.display()))?;
    if link != Path::new("quipu") || resolved_alias != resolved_target {
        return Err("qp alias identity read-back failed".into());
    }
    Ok(())
}

pub(crate) fn run(args: &[String]) {
    let result = (|| {
        let mut bin = env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|e| format!("resolve current Quipu executable: {e}"))?
            .parent()
            .ok_or("Quipu executable has no directory")?
            .to_path_buf();
        let mut check = false;
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--check" => check = true,
                "--bin-dir" if i + 1 < args.len() => {
                    i += 1;
                    bin = args[i].clone().into();
                }
                _ => return Err("usage: qp install-alias [--bin-dir DIR] [--check]".into()),
            }
            i += 1;
        }
        if check {
            preflight(&bin)?;
        } else {
            ensure(&bin)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let alias = fs::metadata(bin.join("qp")).map_err(|e| e.to_string())?;
                let target = fs::metadata(bin.join("quipu")).map_err(|e| e.to_string())?;
                if (alias.dev(), alias.ino()) != (target.dev(), target.ino()) {
                    return Err("qp and quipu inode verification failed".into());
                }
            }
            let version = |name: &str| -> Result<Vec<u8>> {
                let output = std::process::Command::new(bin.join(name))
                    .arg("--version")
                    .output()
                    .map_err(|e| format!("run {name} --version: {e}"))?;
                if !output.status.success() || output.stdout.is_empty() {
                    return Err(format!("{name} --version failed"));
                }
                Ok(output.stdout)
            };
            if version("qp")? != version("quipu")? {
                return Err("qp and quipu version outputs disagree".into());
            }
            println!("Verified same executable inode and version");
        }
        println!(
            "qp -> quipu in {}{}",
            bin.display(),
            if check { " (preflight only)" } else { "" }
        );
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        eprintln!("Error: {error}");
        std::process::exit(2);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn fixture() -> std::path::PathBuf {
        let dir = env::temp_dir().join(format!(
            "qp-alias-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn fresh_relative_alias_is_idempotent_and_tracks_replacement() {
        let dir = fixture();
        fs::write(dir.join("quipu"), b"old").unwrap();
        ensure(&dir).unwrap();
        ensure(&dir).unwrap();
        assert_eq!(fs::read_link(dir.join("qp")).unwrap(), Path::new("quipu"));
        fs::write(dir.join("next"), b"new").unwrap();
        fs::rename(dir.join("next"), dir.join("quipu")).unwrap();
        assert_eq!(fs::read(dir.join("qp")).unwrap(), b"new");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn foreign_destination_and_dangling_link_are_preserved() {
        let dir = fixture();
        fs::write(dir.join("qp"), b"foreign").unwrap();
        assert!(preflight_path(&dir, None).is_err());
        assert_eq!(fs::read(dir.join("qp")).unwrap(), b"foreign");
        fs::remove_file(dir.join("qp")).unwrap();
        symlink("missing", dir.join("qp")).unwrap();
        assert!(preflight_path(&dir, None).is_err());
        assert_eq!(fs::read_link(dir.join("qp")).unwrap(), Path::new("missing"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn foreign_qp_anywhere_on_path_refuses_before_alias_creation() {
        let dir = fixture();
        let foreign = fixture();
        fs::write(foreign.join("qp"), b"foreign").unwrap();
        let path = env::join_paths([&foreign, &dir]).unwrap();
        assert!(preflight_path(&dir, Some(&path)).is_err());
        assert!(!dir.join("qp").exists());
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(foreign).unwrap();
    }
}
