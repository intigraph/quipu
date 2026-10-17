use super::*;

#[test]
fn memory_allocation_changes_actual_writer_and_reader_pragmas_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.db");
    let path = path.to_str().unwrap();
    let writer = Store::open(path).unwrap();
    writer
        .conn
        .execute_batch("CREATE TABLE write_control (value INTEGER);")
        .unwrap();
    let reader = Store::open_read_only(path).unwrap();
    let config = crate::config::ServerConfig {
        memory_budget_mib: Some(320),
        ..Default::default()
    };
    let allocation = crate::memory_budget::MemoryAllocation::for_server(&config)
        .unwrap()
        .unwrap();
    for store in [&writer, &reader] {
        store.apply_memory_allocation(allocation).unwrap();
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA cache_size", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            -(allocation.cache_kib as i64)
        );
        assert_eq!(
            store
                .conn
                .query_row("PRAGMA mmap_size", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            allocation.mmap_bytes as i64
        );
    }
    assert!(
        reader
            .conn
            .execute("INSERT INTO write_control VALUES (1)", [])
            .is_err()
    );
    writer
        .conn
        .execute("INSERT INTO write_control VALUES (2)", [])
        .unwrap();
    assert_eq!(
        reader
            .conn
            .query_row("SELECT value FROM write_control", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
