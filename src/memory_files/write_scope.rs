//! A private in-process capability for exact protocol rows, not request metadata.
use crate::{Datum, Error, Result, Store};
use std::cell::RefCell;

struct Permit {
    graph: i64,
    rows: Vec<Datum>,
}
thread_local! { static CURRENT: RefCell<Option<Permit>> = const { RefCell::new(None) }; }
struct Restore(Option<Permit>);
impl Drop for Restore {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

// Only this library's validated core can mint the capability. Server adapters
// are separate crates; generic RPC/CLI actor or source fields cannot create it.
pub(super) fn protocol<T>(graph: i64, rows: &[Datum], work: impl FnOnce() -> T) -> T {
    let old = CURRENT.with(|slot| {
        slot.replace(Some(Permit {
            graph,
            rows: rows.to_vec(),
        }))
    });
    let _restore = Restore(old);
    work()
}

pub(crate) fn check(store: &Store, graph: i64, rows: &[Datum]) -> Result<()> {
    for row in rows {
        if !store.resolve(row.entity)?.starts_with("urn:quipu:memory:")
            && !store
                .resolve(row.attribute)?
                .starts_with("https://quipu.dev/memory#")
        {
            continue;
        }
        let allowed = CURRENT.with(|slot| {
            slot.borrow().as_ref().is_some_and(|permit| {
                permit.graph == graph
                    && permit.rows.iter().any(|expected| {
                        expected.entity == row.entity
                            && expected.attribute == row.attribute
                            && expected.value == row.value
                            && expected.op == row.op
                    })
            })
        });
        if !allowed {
            return Err(Error::PolicyDenied(
                "reserved file-memory records require the validated file protocol".into(),
            ));
        }
    }
    Ok(())
}
