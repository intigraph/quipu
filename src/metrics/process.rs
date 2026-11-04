//! Cheap process memory sampling, kept separate from metric exposition.

/// Current process (resident, virtual) memory in bytes from `/proc/self/statm`
/// on Linux; `(0, 0)` elsewhere. Fields are pages (4096 on the served `x86_64`).
#[must_use]
pub fn process_memory() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            let mut it = statm.split_whitespace();
            let vsz_pages: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let rss_pages: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            const PAGE: u64 = 4096;
            return (rss_pages * PAGE, vsz_pages * PAGE);
        }
    }
    (0, 0)
}
