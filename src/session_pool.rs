//! A fixed pool of exclusively held inference sessions (aegis-wjo7).
//!
//! An ONNX `Session::run` needs `&mut Session`, so one session serializes every
//! embedding in the process: `/resolve`, `/hybrid_search`, `/search` and the
//! write-embedding drain queue behind each other on one lock. A pool of N lets
//! up to N embeddings run at once.
//!
//! - `N = 1` (the default) is today's code path: one `Mutex`, locked directly
//!   by every caller, with no lane bookkeeping.
//! - `N >= 2`: a caller takes any free session. [`Lane::Background`] (bulk and
//!   write-embedding drains) may hold at most ONE session at a time, so writes
//!   can never take every session away from interactive reads.
//!
//! Each session holds its own copy of the model weights, so N multiplies the
//! model's resident memory; size N from a measured per-session RSS.
use parking_lot::{Condvar, Mutex, MutexGuard};

/// Largest accepted pool size.
pub const MAX_SESSIONS: usize = 8;

/// Who is asking for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// A request a caller is waiting on (a query embedding).
    Interactive,
    /// Bulk or deferred write embedding; at most one session at a time.
    Background,
}

struct State {
    free: Vec<usize>,
    background: bool,
}

/// N sessions; see the module docs.
pub struct SessionPool<T> {
    slots: Vec<Mutex<T>>,
    state: Mutex<State>,
    released: Condvar,
}

/// One checked-out session. Dropping it returns the session to the pool.
pub struct Checkout<'a, T> {
    /// Always `Some` until drop, which releases it BEFORE returning the index.
    session: Option<MutexGuard<'a, T>>,
    release: Option<(&'a SessionPool<T>, usize, Lane)>,
}

impl<T> std::ops::Deref for Checkout<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.session.as_ref().expect("held until drop")
    }
}

impl<T> std::ops::DerefMut for Checkout<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.session.as_mut().expect("held until drop")
    }
}

impl<T> Checkout<'_, T> {
    /// Index of the session held (stable for the pool's lifetime).
    pub fn index(&self) -> usize {
        self.release.map_or(0, |(_, index, _)| index)
    }
}

impl<T> Drop for Checkout<'_, T> {
    fn drop(&mut self) {
        // Unlock the session first, so the next holder of this index never
        // waits on it.
        self.session = None;
        if let Some((pool, index, lane)) = self.release.take() {
            let mut state = pool.state.lock();
            state.free.push(index);
            if lane == Lane::Background {
                state.background = false;
            }
            drop(state);
            pool.released.notify_all();
        }
    }
}

impl<T> SessionPool<T> {
    /// A pool over `sessions` (1..=[`MAX_SESSIONS`]).
    ///
    /// # Errors
    /// Refuses an empty or oversized pool.
    pub fn new(sessions: Vec<T>) -> crate::Result<Self> {
        if sessions.is_empty() || sessions.len() > MAX_SESSIONS {
            return Err(crate::Error::InvalidValue(format!(
                "embedding sessions must be 1..={MAX_SESSIONS}, got {}",
                sessions.len()
            )));
        }
        let free = (0..sessions.len()).rev().collect();
        Ok(Self {
            slots: sessions.into_iter().map(Mutex::new).collect(),
            state: Mutex::new(State {
                free,
                background: false,
            }),
            released: Condvar::new(),
        })
    }

    /// Number of sessions.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Never true: a pool always holds at least one session.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Wait for a session for `lane`.
    pub fn checkout(&self, lane: Lane) -> Checkout<'_, T> {
        if self.slots.len() == 1 {
            // Today's path: one lock, every caller, no lane state.
            return Checkout {
                session: Some(self.slots[0].lock()),
                release: None,
            };
        }
        let mut state = self.state.lock();
        loop {
            if let Some(checkout) = self.take(&mut state, lane) {
                return checkout;
            }
            self.released.wait(&mut state);
        }
    }

    /// A session for `lane` if one is available now, else `None`.
    pub fn try_checkout(&self, lane: Lane) -> Option<Checkout<'_, T>> {
        if self.slots.len() == 1 {
            return self.slots[0].try_lock().map(|session| Checkout {
                session: Some(session),
                release: None,
            });
        }
        let mut state = self.state.lock();
        self.take(&mut state, lane)
    }

    fn take(&self, state: &mut State, lane: Lane) -> Option<Checkout<'_, T>> {
        if lane == Lane::Background && state.background {
            return None;
        }
        let index = state.free.pop()?;
        if lane == Lane::Background {
            state.background = true;
        }
        // The index is exclusively ours, so this lock never contends.
        let session = self.slots[index].lock();
        Some(Checkout {
            session: Some(session),
            release: Some((self, index, lane)),
        })
    }
}

#[cfg(test)]
#[path = "session_pool_tests.rs"]
mod tests;
