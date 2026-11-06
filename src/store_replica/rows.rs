use rusqlite::{params_from_iter, types::Value};

use super::{Cell, Table, catalog::quote, invalid};
use crate::{Result, Store};

fn seed_rows(store: &Store, table: &Table, limit: usize) -> Result<Vec<Vec<Value>>> {
    let columns = table
        .columns
        .iter()
        .map(|c| quote(&c.name))
        .collect::<Vec<_>>();
    let order = table
        .key_indices()
        .iter()
        .map(|&i| columns[i].clone())
        .collect::<Vec<_>>();
    let bytes = columns
        .iter()
        .map(|c| format!("coalesce(length(cast({c} AS BLOB)),0)"))
        .collect::<Vec<_>>()
        .join("+");
    let sql = format!(
        "SELECT {bytes},{} FROM {} ORDER BY {} LIMIT {limit}",
        columns.join(","),
        quote(&table.name),
        order.join(",")
    );
    let mut statement = store.conn.prepare(&sql)?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        if row.get::<_, i64>(0)? > 16 * 1024 {
            return Err(invalid("logical target seed row exceeds resource cap"));
        }
        result.push(
            (0..columns.len())
                .map(|i| row.get::<_, Value>(i + 1))
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
    }
    Ok(result)
}

pub(crate) fn require_fresh_target(store: &Store, catalog: &[Table]) -> Result<()> {
    // A fact-only emptiness check would erase custom graph labels, shapes,
    // vectors and queries. Compare this build's deterministic initial data;
    // source-local random identity and epoch tables are not in the catalog.
    let fresh = Store::open_in_memory()?;
    for table in catalog {
        let expected = seed_rows(&fresh, table, 1025)?;
        let differs = if expected.is_empty() {
            store
                .conn
                .prepare(&format!("SELECT 1 FROM {} LIMIT 1", quote(&table.name)))?
                .exists([])?
        } else {
            expected.len() > 1024 || seed_rows(store, table, expected.len() + 1)? != expected
        };
        if differs {
            return Err(invalid(
                "logical bootstrap would overwrite non-fresh local data",
            ));
        }
    }
    for table in [
        "replica_tx_map",
        "replica_applied_tx",
        "replica_bootstrap_admission",
        "replica_source_end",
        "replica_coverage",
        "replica_stop",
    ] {
        let present = store
            .conn
            .prepare("SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1")?
            .exists([table])?;
        if present
            && store
                .conn
                .prepare(&format!("SELECT 1 FROM {} LIMIT 1", quote(table)))?
                .exists([])?
        {
            return Err(invalid(
                "logical bootstrap would overwrite existing replication evidence",
            ));
        }
    }
    Ok(())
}

fn keys(table: &Table, row: &[Cell]) -> Result<Vec<Value>> {
    if row.len() != table.columns.len() {
        return Err(invalid("logical row width differs from local schema"));
    }
    let keys = table
        .key_indices()
        .into_iter()
        .map(|i| row[i].to_sql())
        .collect::<Result<Vec<_>>>()?;
    if keys.is_empty() || keys.contains(&Value::Null) {
        return Err(invalid("logical row needs a complete non-null primary key"));
    }
    Ok(keys)
}

pub(crate) fn put(store: &Store, table: &Table, row: &[Cell]) -> Result<()> {
    write(store, table, row, true)
}

pub(crate) fn insert(store: &Store, table: &Table, row: &[Cell]) -> Result<()> {
    write(store, table, row, false)
}

fn write(store: &Store, table: &Table, row: &[Cell], upsert: bool) -> Result<()> {
    let key_values = keys(table, row)?;
    let values = row.iter().map(Cell::to_sql).collect::<Result<Vec<_>>>()?;
    let columns = table
        .columns
        .iter()
        .map(|c| quote(&c.name))
        .collect::<Vec<_>>();
    let key_columns = table
        .key_indices()
        .iter()
        .map(|&i| columns[i].clone())
        .collect::<Vec<_>>();
    let update = columns
        .iter()
        .map(|c| format!("{c}=excluded.{c}"))
        .collect::<Vec<_>>();
    let conflict = if upsert {
        format!(
            " ON CONFLICT ({}) DO UPDATE SET {}",
            key_columns.join(","),
            update.join(",")
        )
    } else {
        String::new()
    };
    store.conn.execute(
        &format!(
            "INSERT INTO {} ({}) VALUES ({}){}",
            quote(&table.name),
            columns.join(","),
            vec!["?"; values.len()].join(","),
            conflict,
        ),
        params_from_iter(&values),
    )?;
    // SQLite affinity must not silently alter a signed storage class/value.
    let condition = key_columns
        .iter()
        .map(|c| format!("{c} IS ?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let stored = store.conn.query_row(
        &format!(
            "SELECT {} FROM {} WHERE {condition}",
            columns.join(","),
            quote(&table.name),
        ),
        params_from_iter(&key_values),
        |r| {
            (0..columns.len())
                .map(|i| r.get::<_, Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        },
    )?;
    if stored != values {
        return Err(invalid("SQLite changed a logical row value/storage class"));
    }
    Ok(())
}

pub(crate) fn delete(store: &Store, table: &Table, key: &[Cell]) -> Result<()> {
    let columns = table
        .key_indices()
        .iter()
        .map(|&i| quote(&table.columns[i].name))
        .collect::<Vec<_>>();
    if key.len() != columns.len() || key.is_empty() {
        return Err(invalid("logical deletion primary key width differs"));
    }
    let values = key.iter().map(Cell::to_sql).collect::<Result<Vec<_>>>()?;
    if values.contains(&Value::Null) {
        return Err(invalid("logical deletion has a null primary key"));
    }
    let condition = columns
        .iter()
        .map(|c| format!("{c} IS ?"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let count = store.conn.execute(
        &format!("DELETE FROM {} WHERE {condition}", quote(&table.name),),
        params_from_iter(&values),
    )?;
    if count != 1 {
        return Err(invalid(
            "logical deletion does not match exactly one replica row",
        ));
    }
    Ok(())
}
