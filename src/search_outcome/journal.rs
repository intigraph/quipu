//! Private append journals. Retention never deletes unresolved evidence here.
use super::observation::Event;
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

enum Sink {
    Disabled,
    Failed,
    Ready(std::fs::File),
}

struct Journal {
    sink: Mutex<Sink>,
    written: AtomicU64,
    disabled: AtomicU64,
    failed: AtomicU64,
    oversized: AtomicU64,
    last_success: AtomicU64,
}
static JOURNAL: OnceLock<Journal> = OnceLock::new();

#[cfg(unix)]
fn claim(directory: &std::path::Path) -> std::io::Result<std::fs::File> {
    use ring::rand::SecureRandom;
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    // IaC/operator creates the directory. Never chmod or follow its final
    // symlink; openat then pins file creation to the verified directory inode.
    if !directory.is_absolute() {
        return Err(std::io::Error::other("journal path must be absolute"));
    }
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)?;
    let metadata = dir.metadata()?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(std::io::Error::other(
            "journal directory must be private and owned",
        ));
    }
    let mut random = [0_u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| std::io::Error::other("journal identity unavailable"))?;
    let file: std::fs::File = rustix::fs::openat(
        &dir,
        format!("search-{}.jsonl", hex::encode(random)),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::APPEND
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?
    .into();
    file.sync_all()?;
    dir.sync_all()?;
    Ok(file)
}

#[cfg(not(unix))]
fn claim(_: &std::path::Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::other("private journal unsupported"))
}

impl Journal {
    fn new(directory: Option<&std::path::Path>) -> Self {
        let sink = match directory {
            None => Sink::Disabled,
            Some(path) => claim(path).map_or(Sink::Failed, Sink::Ready),
        };
        Self {
            sink: Mutex::new(sink),
            written: AtomicU64::new(0),
            disabled: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            oversized: AtomicU64::new(0),
            last_success: AtomicU64::new(0),
        }
    }

    fn append(&self, event: &Event) {
        if event.logical_call_id.as_deref().is_none_or(str::is_empty) {
            self.failed.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let Ok(mut bytes) = serde_json::to_vec(event) else {
            self.failed.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if bytes.len() >= 16 * 1024 {
            self.oversized.fetch_add(1, Ordering::Relaxed);
            return;
        }
        bytes.push(b'\n');
        let mut sink = self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &mut *sink {
            Sink::Disabled => {
                self.disabled.fetch_add(1, Ordering::Relaxed);
            }
            Sink::Failed => {
                self.failed.fetch_add(1, Ordering::Relaxed);
            }
            Sink::Ready(file) => {
                if file
                    .write_all(&bytes)
                    .and_then(|()| file.sync_data())
                    .is_ok()
                {
                    self.written.fetch_add(1, Ordering::Relaxed);
                    self.last_success
                        .store(crate::time::epoch_secs(), Ordering::Relaxed);
                } else {
                    // Preserve a possibly partial line. A subsequent event must
                    // not be appended to that incomplete tail; restart a new file.
                    *sink = Sink::Failed;
                    self.failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn render(&self, out: &mut String) {
        out.push_str("# HELP quipu_search_capture_total Durable capture attempts, separate from semantic request success.\n# TYPE quipu_search_capture_total counter\n");
        for (outcome, count) in [
            ("written", &self.written),
            ("disabled", &self.disabled),
            ("error", &self.failed),
            ("oversized", &self.oversized),
        ] {
            let _ = writeln!(
                out,
                "quipu_search_capture_total{{outcome=\"{outcome}\"}} {}",
                count.load(Ordering::Relaxed)
            );
        }
        let last = self.last_success.load(Ordering::Relaxed);
        if last > 0 {
            out.push_str("# HELP quipu_search_capture_last_success_timestamp_seconds Last durable outcome append.\n# TYPE quipu_search_capture_last_success_timestamp_seconds gauge\n");
            let _ = writeln!(
                out,
                "quipu_search_capture_last_success_timestamp_seconds {last}"
            );
        }
    }
}

pub(super) fn append(event: &Event) {
    let journal = JOURNAL.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        let path = std::env::var_os("QUIPU_SEARCH_JOURNAL_DIR")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from);
        #[cfg(target_arch = "wasm32")]
        let path: Option<std::path::PathBuf> = None;
        Journal::new(path.as_deref())
    });
    journal.append(event);
}

pub(crate) fn render_health(out: &mut String) {
    // Uninitialized capture is absent, not a fabricated healthy zero producer.
    if let Some(journal) = JOURNAL.get() {
        journal.render(out);
    }
}

#[cfg(test)]
mod tests;
