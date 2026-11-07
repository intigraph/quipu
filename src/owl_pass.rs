//! Candidate sinks shared by inline and durable snapshot enumeration.
use crate::error::Result;
use crate::{Datum, Fact, Op, Store, Value};
use std::collections::HashSet;

/// Accumulates one derivation pass: a fact is staged only if it is neither in
/// the store already nor staged earlier in this pass, so counts stay honest
/// and the fixpoint loop terminates.
pub(super) struct Pass<'a> {
    pub(super) seen: HashSet<(i64, i64, Vec<u8>)>,
    pub(super) datums: Vec<Datum>,
    timestamp: &'a str,
    limit: Option<usize>,
    pub(super) raw: usize,
    pub(super) known: usize,
    pub(super) repeated: usize,
    disk: Option<&'a Store>,
    pub(super) exhausted: bool,
}

impl<'a> Pass<'a> {
    pub(super) fn from_facts(facts: &[Fact], timestamp: &'a str, limit: Option<usize>) -> Self {
        let mut seen = HashSet::new();
        for f in facts {
            seen.insert((f.entity, f.attribute, f.value.to_bytes()));
        }
        Self {
            seen,
            datums: Vec::new(),
            timestamp,
            limit,
            disk: None,
            raw: 0,
            known: 0,
            repeated: 0,
            exhausted: false,
        }
    }

    pub(super) fn disk(store: &'a Store, timestamp: &'a str) -> Self {
        let mut pass = Self::from_facts(&[], timestamp, None);
        pass.disk = Some(store);
        pass
    }

    pub(super) fn push(
        &mut self,
        entity: i64,
        attribute: i64,
        value: Value,
        counter: &mut usize,
    ) -> Result<()> {
        let key = (entity, attribute, value.to_bytes());
        if let Some(store) = self.disk {
            self.raw += 1;
            if self.raw % 1024 == 1 {
                crate::owl::snapshot::pages::check_budget(store)?;
            }
            let known: bool = store.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM facts WHERE e=?1 AND a=?2 AND v=?3 AND op=1 AND valid_to IS NULL)",
                rusqlite::params![entity,attribute,key.2], |r| r.get(0))?;
            if known {
                self.known += 1;
                return Ok(());
            }
            let inserted = store.conn.execute(
                "INSERT OR IGNORE INTO owl_page_queue(e,a,v) VALUES(?1,?2,?3)",
                rusqlite::params![entity, attribute, key.2],
            )?;
            *counter += inserted;
            if inserted == 0 {
                self.repeated += 1;
            }
            return Ok(());
        }
        if self.seen.contains(&key) {
            return Ok(());
        }
        if self.limit.is_some_and(|limit| self.datums.len() >= limit) {
            self.exhausted = true;
            return Ok(());
        }
        if self.seen.insert(key) {
            self.datums.push(Datum {
                entity,
                attribute,
                value,
                valid_from: self.timestamp.to_string(),
                valid_to: None,
                op: Op::Assert,
            });
            *counter += 1;
        }
        Ok(())
    }
}
