//! Disposable library probe for the enrolled startup boundary, never enrollment.
use quipu::Store;
use rusqlite::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("probe.db");
    let path = path.to_str().ok_or("non-UTF8 fixture path")?;
    drop(Store::open(path)?);
    println!("fixture=private-disposable-sqlite witness=NONE enrollment_api=NONE");
    println!("unenrolled_startup=ADMITTED");
    let raw = Connection::open(path)?;
    raw.execute_batch("CREATE TABLE ownership_handoff_enrollment(payload TEXT)")?;
    let before: i64 = raw.query_row("PRAGMA schema_version", [], |r| r.get(0))?;
    println!("input=database-with-reserved-enrollment-marker");
    println!(
        "writable_startup={}",
        if Store::open(path).is_ok() {
            "ADMITTED"
        } else {
            "REFUSED"
        }
    );
    let after: i64 = raw.query_row("PRAGMA schema_version", [], |r| r.get(0))?;
    println!("schema_unchanged={}", before == after);
    println!(
        "explicit_adoption={}",
        if Store::adopt(path).is_ok() {
            "ADMITTED"
        } else {
            "REFUSED"
        }
    );
    println!(
        "readonly_inspection={}",
        if Store::open_read_only(path).is_ok() {
            "AVAILABLE"
        } else {
            "UNAVAILABLE"
        }
    );
    Ok(())
}
