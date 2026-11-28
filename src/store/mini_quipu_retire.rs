//! Mini-Quipu slice M5 (docs/design/mini-quipu.md §2.4): retire a mini-Quipu
//! after its final merge, or when its owner abandons it. Native API only.
//!
//! Retirement runs in the design's order:
//! 1. Fence the mini-Quipu writer. The file records the retirement and gets
//!    SQL triggers that refuse every insert, update and delete on `facts` and
//!    `transactions`, so every write path on the file refuses, not only
//!    [`super::local::MiniQuipu::transact`]. The file stays readable.
//! 2. Mark the home registry entry retired, recording the last merge id or
//!    `abandoned`. This is a control write on the home owner. It writes
//!    nothing to home member graphs and no home transaction. Home triggers
//!    then refuse any update or delete of a retired entry, so a retired fork
//!    never reopens. A new fork is a new name and a new minted identity.
//! 3. Keep the file until [`verify_receipts`] confirms every merge receipt
//!    against home transactions. Deleting the file afterwards is an operator
//!    choice. Home history never reads the file.
//!
//! The fence goes first. If retirement stops between the steps, the file is
//! fenced while the entry is still open, and rerunning retirement finishes
//! it. The reverse order could leave a writable file behind a closed entry.
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use super::{GraphSetOwner, MiniForkEntry, Owner, has_table, merge::MergeReceipt};
use crate::{Error, Result, Store};

const ABANDONED: &str = "abandoned";
const FENCE: &str = "CREATE TABLE IF NOT EXISTS mini_quipu_fence (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    name       TEXT NOT NULL,
    last_merge TEXT NOT NULL,
    head       INTEGER NOT NULL
);";
const HOME: &str = "CREATE TABLE IF NOT EXISTS mini_quipu_retirements (
    name        TEXT PRIMARY KEY,
    store_id    TEXT NOT NULL,
    last_merge  TEXT NOT NULL,
    fenced_head INTEGER NOT NULL,
    retired_at  TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS mini_quipu_never_reopen BEFORE UPDATE ON mini_quipu_forks
WHEN OLD.status = 'retired' BEGIN
    SELECT RAISE(ABORT, 'mini-quipu retire refused (retired-never-reopens)');
END;
CREATE TRIGGER IF NOT EXISTS mini_quipu_retired_kept BEFORE DELETE ON mini_quipu_forks
WHEN OLD.status = 'retired' BEGIN
    SELECT RAISE(ABORT, 'mini-quipu retire refused (retired-never-reopens)');
END;
CREATE TRIGGER IF NOT EXISTS mini_quipu_retirement_kept_u BEFORE UPDATE ON mini_quipu_retirements
BEGIN SELECT RAISE(ABORT, 'mini-quipu retire refused (retired-never-reopens)'); END;
CREATE TRIGGER IF NOT EXISTS mini_quipu_retirement_kept_d BEFORE DELETE ON mini_quipu_retirements
BEGIN SELECT RAISE(ABORT, 'mini-quipu retire refused (retired-never-reopens)'); END;";

/// Why the fork is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetireReason {
    /// After the final merge. The file head must be merged with no held slots.
    FinalMerge,
    /// The owner abandons the fork. Unmerged local writes stay unmerged.
    Abandoned,
}

/// A retirement request.
#[derive(Debug, Clone, Copy)]
pub struct RetireRequest<'a> {
    /// Registry entry name.
    pub name: &'a str,
    /// Final merge or abandonment.
    pub reason: RetireReason,
    /// Registry timestamp.
    pub timestamp: &'a str,
}

/// The recorded retirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retirement {
    /// Registry entry name.
    pub name: String,
    /// The retired mini-Quipu identity.
    pub store_id: String,
    /// The last merge id, or `abandoned`.
    pub last_merge: String,
    /// The mini-Quipu head `local_tx` at the fence.
    pub fenced_head: i64,
}

/// The named refusals of this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetireRefusal {
    /// No such registry entry.
    NotRegistered,
    /// The entry is already retired. A retired fork never reopens.
    AlreadyRetired,
    /// `FinalMerge` with local writes after the last merge, or no merge.
    Unmerged,
    /// The last partial merge still holds unresolved local slots.
    HeldRemainder,
    /// The file is not this entry's mini-Quipu.
    ForeignFile,
}

impl RetireRefusal {
    /// The stable reason code.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotRegistered => "not-registered",
            Self::AlreadyRetired => "retired-never-reopens",
            Self::Unmerged => "unmerged-local-writes",
            Self::HeldRemainder => "held-remainder",
            Self::ForeignFile => "foreign-file",
        }
    }
}

fn refused(reason: RetireRefusal, detail: &str) -> Error {
    Error::PolicyDenied(format!(
        "mini-quipu retire refused ({}): {detail}; nothing was written",
        reason.code()
    ))
}

/// The retirement recorded in a mini-Quipu file, if its writer is fenced.
pub fn fenced(store: &Store) -> Result<Option<(String, String, i64)>> {
    if !has_table(store, "mini_quipu_fence")? {
        return Ok(None);
    }
    Ok(store
        .conn
        .query_row(
            "SELECT name, last_merge, head FROM mini_quipu_fence WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Step 1: record the retirement in the file and install the write fence.
/// Rerunning it for the same retirement is a no-op.
fn fence_writer(mini: &Store, r: &Retirement) -> Result<()> {
    if let Some(found) = fenced(mini)? {
        return if found == (r.name.clone(), r.last_merge.clone(), r.fenced_head) {
            Ok(())
        } else {
            Err(refused(
                RetireRefusal::AlreadyRetired,
                "the file is fenced under another retirement",
            ))
        };
    }
    let tx = mini.conn.unchecked_transaction()?;
    tx.execute_batch(FENCE)?;
    tx.execute(
        "INSERT INTO mini_quipu_fence (id, name, last_merge, head) VALUES (1, ?1, ?2, ?3)",
        params![r.name, r.last_merge, r.fenced_head],
    )?;
    let mut sql = String::new();
    for table in ["facts", "transactions", "mini_quipu_fence"] {
        for op in ["INSERT", "UPDATE", "DELETE"] {
            sql.push_str(&format!(
                "CREATE TRIGGER IF NOT EXISTS mini_quipu_fence_{table}_{op} BEFORE {op} ON {table} \
                 BEGIN SELECT RAISE(ABORT, 'mini-quipu write refused (retired-writer-fenced)'); END;\n",
            ));
        }
    }
    tx.execute_batch(&sql)?;
    tx.commit()?;
    Ok(())
}

fn last_merge(home: &Store, name: &str) -> Result<Option<String>> {
    if !has_table(home, "mini_quipu_merges")? {
        return Ok(None);
    }
    Ok(home
        .conn
        .query_row(
            "SELECT merge_id FROM mini_quipu_merges WHERE name = ?1 ORDER BY rowid DESC LIMIT 1",
            [name],
            |r| r.get(0),
        )
        .optional()?)
}

fn open_file(path: &Path, entry: &MiniForkEntry) -> Result<super::local::MiniQuipu> {
    super::local::MiniQuipu::open(path, entry).map_err(|e| {
        refused(
            RetireRefusal::ForeignFile,
            &format!("the file is not this entry's mini-Quipu: {e}"),
        )
    })
}

fn retire(
    home: &mut Store,
    path: &Path,
    req: &RetireRequest<'_>,
    fence: impl FnOnce(&Store, &Retirement) -> Result<()>,
) -> Result<Retirement> {
    let entry = super::mini_fork_entry(home, req.name)?
        .ok_or_else(|| refused(RetireRefusal::NotRegistered, "no such registry entry"))?;
    let status: String = home.conn.query_row(
        "SELECT status FROM mini_quipu_forks WHERE name = ?1",
        [req.name],
        |r| r.get(0),
    )?;
    if status != "open" {
        return Err(refused(
            RetireRefusal::AlreadyRetired,
            "a retired fork never reopens; a new fork is a new identity",
        ));
    }
    // A partial merge records its head too. Matching heads alone does not
    // establish that all local work reached home. Refuse before touching the
    // mini file or the home registry; abandonment is the explicit discard path.
    if req.reason == RetireReason::FinalMerge
        && let Some(last) = entry.last_merge.as_ref().filter(|last| last.held > 0)
    {
        return Err(refused(
            RetireRefusal::HeldRemainder,
            &format!(
                "{} held slot(s) remain; resolve the remainder or explicitly abandon",
                last.held
            ),
        ));
    }
    let mini = open_file(path, &entry)?;
    let head = mini.store().latest_tx_id()?;
    let last = match req.reason {
        RetireReason::Abandoned => ABANDONED.to_owned(),
        RetireReason::FinalMerge => match last_merge(home, req.name)? {
            Some(id) if entry.merged_head == Some(head) => id,
            _ => {
                return Err(refused(
                    RetireRefusal::Unmerged,
                    "the file has local writes after its last merge; merge them or abandon",
                ));
            }
        },
    };
    let retirement = Retirement {
        name: entry.name.clone(),
        store_id: entry.store_id.clone(),
        last_merge: last,
        fenced_head: head,
    };
    fence(mini.store(), &retirement)?;
    drop(mini);
    home.ownership_operation(|home| {
        home.ensure_ownership_writable()?;
        let tx = home.conn.unchecked_transaction()?;
        tx.execute_batch(HOME)?;
        tx.execute(
            "INSERT INTO mini_quipu_retirements (name, store_id, last_merge, fenced_head, retired_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                retirement.name,
                retirement.store_id,
                retirement.last_merge,
                retirement.fenced_head,
                req.timestamp
            ],
        )?;
        tx.execute(
            "UPDATE mini_quipu_forks SET status = 'retired' WHERE name = ?1 AND status = 'open'",
            [req.name],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    Ok(retirement)
}

/// Read a retirement back. `None` while the entry is open or absent.
pub fn retirement(home: &Store, name: &str) -> Result<Option<Retirement>> {
    if !has_table(home, "mini_quipu_retirements")? {
        return Ok(None);
    }
    Ok(home
        .conn
        .query_row(
            "SELECT name, store_id, last_merge, fenced_head FROM mini_quipu_retirements WHERE name = ?1",
            [name],
            |r| {
                Ok(Retirement {
                    name: r.get(0)?,
                    store_id: r.get(1)?,
                    last_merge: r.get(2)?,
                    fenced_head: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// Step 3: verify every merge receipt of `name` against home history alone.
/// Each nonzero `receive` and `decide` transaction must exist at home with
/// this merge id in its source. Returns the verified receipts, in merge
/// order. Once this succeeds, deleting the file is an operator choice.
pub fn verify_receipts(home: &Store, name: &str) -> Result<Vec<MergeReceipt>> {
    if !has_table(home, "mini_quipu_merges")? {
        return Ok(Vec::new());
    }
    let mut stmt = home
        .conn
        .prepare("SELECT receipt FROM mini_quipu_merges WHERE name = ?1 ORDER BY rowid")?;
    let receipts = stmt
        .query_map([name], |r| r.get::<_, String>(0))?
        .map(|row| {
            serde_json::from_str::<MergeReceipt>(&row?)
                .map_err(|e| Error::Serialization(e.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    for receipt in &receipts {
        for tx in receipt.receive_tx.iter().chain(&receipt.decide_tx) {
            if *tx == 0 {
                continue;
            }
            let source: Option<String> = home
                .conn
                .query_row("SELECT source FROM transactions WHERE id = ?1", [tx], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten();
            let tag = format!("merge={};", receipt.merge_id);
            if !source.is_some_and(|s| s.contains(&tag)) {
                return Err(Error::InvalidValue(format!(
                    "mini-quipu receipt unverified: home transaction {tx} does not carry merge {}",
                    receipt.merge_id
                )));
            }
        }
    }
    Ok(receipts)
}

/// Retire a mini-Quipu whose writable members are on the legacy file.
pub fn retire_on_legacy(
    legacy: &mut Store,
    path: &Path,
    req: &RetireRequest<'_>,
) -> Result<Retirement> {
    retire(legacy, path, req, fence_writer)
}

impl GraphSetOwner {
    /// Retire a mini-Quipu, closing its entry through this owner's writer.
    pub fn retire_mini_quipu(&self, path: &Path, req: &RetireRequest<'_>) -> Result<Retirement> {
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        retire(&mut store, path, req, fence_writer)
    }
}

#[cfg(test)]
#[path = "mini_quipu_retire_tests.rs"]
mod tests;
