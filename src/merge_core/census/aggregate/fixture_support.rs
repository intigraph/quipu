//! Test-only contention controls; the parent census module is cfg(test).
use super::AggregateSink;

impl AggregateSink {
    pub(crate) fn fixture_busy<T>(&self, body: impl FnOnce() -> T) -> T {
        let _guard = self.state.lock().unwrap();
        body()
    }

    pub(crate) fn fixture_poison(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.state.lock().unwrap();
            panic!("fixture-only sink poison");
        }));
    }
}
