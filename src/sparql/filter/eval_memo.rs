//! Term lookups memoized for one streaming aggregate (aegis-xdvg.6).
//!
//! A count's FILTERs and BINDs look the same terms up once per row: a constant
//! datatype IRI such as `xsd:integer` (not a dictionary term, so every lookup
//! misses into SQL) and each subject's IRI for `STR(?s)` (past the term cache's
//! admission limit on a large board, so every resolve is SQL too). On the 2fa
//! stats aggregate that was a quarter of the remaining time.
//!
//! The memo exists only while [`Scope`] is held, around one read evaluation:
//! nothing writes to the store meanwhile, so an id read once stays right for
//! the whole evaluation. Failed lookups are not remembered, and the memo stops
//! admitting entries past a bound.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

const MEMO_ENTRIES: usize = 1 << 20;

#[derive(Default)]
struct Memo {
    refs: HashMap<String, i64>,
    iris: HashMap<i64, String>,
    /// Constant subexpressions by address: the algebra is borrowed, unmoved,
    /// for the whole evaluation. Shared, so a comparison reads the value
    /// without copying it (aegis-pu7).
    constants: HashMap<usize, Option<Rc<Value>>>,
    date_times: HashMap<String, Option<oxsdatatypes::DateTime>>,
}

thread_local! {
    static MEMO: RefCell<Option<Memo>> = const { RefCell::new(None) };
}

/// Memoize lookups until dropped; restores any enclosing scope's memo.
pub(in crate::sparql) struct Scope(Option<Memo>);

impl Scope {
    pub(in crate::sparql) fn install() -> Self {
        Self(MEMO.with(|memo| memo.replace(Some(Memo::default()))))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        MEMO.with(|memo| *memo.borrow_mut() = self.0.take());
    }
}

/// [`Store::query_ref`], memoized inside a [`Scope`].
pub(super) fn query_ref(store: &Store, iri: &str) -> Result<i64> {
    if let Some(id) = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .and_then(|m| m.refs.get(iri).copied())
    }) {
        return Ok(id);
    }
    let id = store.query_ref(iri)?;
    MEMO.with(|memo| {
        if let Some(m) = memo.borrow_mut().as_mut()
            && m.refs.len() < MEMO_ENTRIES
        {
            m.refs.insert(iri.to_owned(), id);
        }
    });
    Ok(id)
}

/// [`Store::resolve`], memoized inside a [`Scope`].
pub(super) fn resolve(store: &Store, id: i64) -> Result<String> {
    if let Some(iri) = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .and_then(|m| m.iris.get(&id).cloned())
    }) {
        return Ok(iri);
    }
    let iri = store.resolve(id)?;
    MEMO.with(|memo| {
        if let Some(m) = memo.borrow_mut().as_mut()
            && m.iris.len() < MEMO_ENTRIES
        {
            m.iris.insert(id, iri.clone());
        }
    });
    Ok(iri)
}

/// The value of a constant expression (no variables), computed once inside a
/// [`Scope`]; `compute` runs every time outside one.
pub(super) fn constant(
    expr: &spargebra::algebra::Expression,
    compute: impl FnOnce() -> Option<Value>,
) -> Option<Value> {
    match shared_constant(expr, compute) {
        Shared::Memo(value) => value.as_deref().cloned(),
        Shared::Computed(value) => value,
    }
}

/// A constant's value from [`shared_constant`]: the memo's shared copy, or
/// one computed outside any [`Scope`].
pub(super) enum Shared {
    Memo(Option<Rc<Value>>),
    Computed(Option<Value>),
}

/// [`constant`] without copying the memoized value out.
pub(super) fn shared_constant(
    expr: &spargebra::algebra::Expression,
    compute: impl FnOnce() -> Option<Value>,
) -> Shared {
    let key = std::ptr::from_ref(expr) as usize;
    let active = MEMO.with(|memo| memo.borrow().is_some());
    if !active {
        return Shared::Computed(compute());
    }
    if let Some(value) = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .and_then(|m| m.constants.get(&key).cloned())
    }) {
        return Shared::Memo(value);
    }
    let value = compute().map(Rc::new);
    MEMO.with(|memo| {
        if let Some(m) = memo.borrow_mut().as_mut()
            && m.constants.len() < MEMO_ENTRIES
        {
            m.constants.insert(key, value.clone());
        }
    });
    Shared::Memo(value)
}

/// An `xsd:dateTime` lexical form parsed once inside a [`Scope`].
pub(in crate::sparql) fn date_time(lexical: &str) -> Option<oxsdatatypes::DateTime> {
    let cached = MEMO.with(|memo| {
        memo.borrow()
            .as_ref()
            .map(|m| m.date_times.get(lexical).copied())
    });
    match cached {
        Some(Some(parsed)) => parsed,
        Some(None) => {
            let parsed = lexical.parse().ok();
            MEMO.with(|memo| {
                if let Some(m) = memo.borrow_mut().as_mut()
                    && m.date_times.len() < MEMO_ENTRIES
                {
                    m.date_times.insert(lexical.to_owned(), parsed);
                }
            });
            parsed
        }
        None => lexical.parse().ok(),
    }
}
