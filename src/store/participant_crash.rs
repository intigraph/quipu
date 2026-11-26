//! Test-only crash injection at participant-commit durable edges
//! (aegis-n0wc.5 Phase 3): an in-process early return, or, in an armed child
//! process, a blocking pause for the parent's SIGKILL. No-ops in other builds.
use super::*;

/// Durable edges where a test injects a crash (an early return that skips all
/// cleanup). Indexes are canonical participant positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    AfterJournal,
    AfterPrepare(usize),
    AfterDecision,
    AfterApply(usize),
    AfterPublish,
}

#[cfg(test)]
thread_local! {
    static CRASH: std::cell::Cell<Option<CrashPoint>> = const { std::cell::Cell::new(None) };
    static CRASHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `work` with one injected crash point armed (test only).
#[cfg(test)]
pub fn with_crash<T>(point: CrashPoint, work: impl FnOnce() -> T) -> T {
    CRASH.with(|slot| slot.set(Some(point)));
    CRASHED.with(|slot| slot.set(false));
    let result = work();
    CRASH.with(|slot| slot.set(None));
    result
}

pub(super) fn crash(point: CrashPoint) -> Result<()> {
    // A child test process armed by the parent reports here, then blocks
    // holding every writer and lease until it is SIGKILLed (test only).
    #[cfg(test)]
    if std::env::var(super::kill_tests::KILL_ENV).ok().as_deref() == Some(&format!("{point:?}")) {
        use std::io::Write;
        println!("{}", super::kill_tests::MARKER);
        let _ = std::io::stdout().flush();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    #[cfg(test)]
    if CRASH.with(std::cell::Cell::get) == Some(point) {
        CRASHED.with(|slot| slot.set(true));
        return Err(Error::PolicyDenied(format!(
            "injected participant crash at {point:?}"
        )));
    }
    let _ = point;
    Ok(())
}

/// True only after an injected crash: the simulated process skips cleanup.
pub(super) fn crashed() -> bool {
    #[cfg(test)]
    return CRASHED.with(std::cell::Cell::get);
    #[cfg(not(test))]
    false
}
