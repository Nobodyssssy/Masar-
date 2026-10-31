//! Explicit, backup-first reset of Jobup user data for a fresh test run.
//! Schema and migration history remain; everything else is cleared transactionally.
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct ResetReport {
    pub backup_path: String,
    pub tables_cleared: usize,
    pub rows_cleared: usize,
}

pub fn execute(db: &mut Connection, backup_path: &Path) -> Result<ResetReport, String> {
    // Refuse a reset unless we first have a complete, consistent snapshot.
    crate::database::backup(db, backup_path)?;
    let backup = Connection::open_with_flags(backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Backup verification failed, database unchanged: {e}"))?;
    let integrity: String = backup.query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| format!("Backup verification failed, database unchanged: {e}"))?;
    if integrity != "ok" { return Err(format!("Backup integrity check failed, database unchanged: {integrity}")); }
    let marker: i64 = backup.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'", [], |r| r.get(0))
        .map_err(|e| format!("Backup schema check failed: {e}"))?;
    if marker != 1 { return Err("Backup is not a recognizable Jobup database; nothing deleted".into()); }
    drop(backup);

    // Discover the actual installed tables, including additions from future steps.
    // The schema_migrations table must survive, otherwise startup migrations may
    // incorrectly rebuild tables and reimport data on the next launch.
    let names = {
        let mut stmt = db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name != 'schema_migrations' ORDER BY name")
            .map_err(|e| e.to_string())?;
        // Materialize the mapped rows before `stmt` is dropped (Rust E0597).
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let table_names = rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        table_names
    };
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
    // All rows are deleted in ONE transaction. Deferring FK checks avoids
    // depending on an undocumented order among the application's tables.
    tx.execute_batch("PRAGMA defer_foreign_keys=ON;").map_err(|e| e.to_string())?;
    let mut cleared = 0usize;
    for table in &names {
        // Identifiers are schema-provided, never provided by a user. Quote anyway.
        let escaped = table.replace('"', "\"\"");
        cleared += tx.execute(&format!("DELETE FROM \"{escaped}\""), []).map_err(|e| format!("Unable to clear table {table}; transaction rolled back: {e}"))?;
    }
    tx.commit().map_err(|e| format!("Reset rolled back: {e}"))?;
    Ok(ResetReport {backup_path: backup_path.display().to_string(), tables_cleared: names.len(), rows_cleared: cleared})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("jobup-reset-test-{}-{nanos}.sqlite3",std::process::id()))
    }
    #[test]
    fn backup_first_reset_clears_rows_and_preserves_schema() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); INSERT INTO schema_migrations VALUES (1); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT REFERENCES jobs(id)); INSERT INTO jobs VALUES('one'); INSERT INTO reviews VALUES('one'); CREATE TABLE settings(key TEXT PRIMARY KEY); INSERT INTO settings VALUES ('prefs');").unwrap();
        let file=path();
        let report=execute(&mut db,&file).unwrap();
        assert_eq!(report.rows_cleared,3);
        for table in ["jobs","reviews","settings"] {
            let n: i64=db.query_row(&format!("SELECT COUNT(*) FROM {table}"),[],|r|r.get(0)).unwrap();assert_eq!(n,0);
        }
        let n: i64=db.query_row("SELECT COUNT(*) FROM schema_migrations",[],|r|r.get(0)).unwrap();assert_eq!(n,1);
        let saved=Connection::open(&file).unwrap();
        let n: i64=saved.query_row("SELECT COUNT(*) FROM jobs",[],|r|r.get(0)).unwrap();assert_eq!(n,1);
        drop(saved);std::fs::remove_file(file).unwrap();
    }
    #[test]
    fn backup_failure_does_not_clear_any_data() {
        let mut db=Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT); INSERT INTO jobs VALUES ('stay');").unwrap();
        let file=path();std::fs::write(&file,"already here").unwrap();
        assert!(execute(&mut db,&file).is_err());
        let n:i64=db.query_row("SELECT COUNT(*) FROM jobs",[],|r|r.get(0)).unwrap();assert_eq!(n,1);
        std::fs::remove_file(file).unwrap();
    }
}
