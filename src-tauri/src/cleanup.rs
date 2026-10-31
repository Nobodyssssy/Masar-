use rusqlite::{Connection,OpenFlags,TransactionBehavior};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct CleanupReport {pub removed:usize,pub backup_path:String}

pub fn execute(db:&mut Connection,backup_path:&Path,days:u32)->Result<CleanupReport,String>{
    if !(30..=3650).contains(&days){return Err("Cleanup age must be between 30 and 3650 days".into());}
    crate::database::backup(db,backup_path)?;
    let backup=Connection::open_with_flags(backup_path,OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
    let integrity:String=backup.query_row("PRAGMA integrity_check",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if integrity!="ok"{return Err("Cleanup backup integrity check failed; nothing deleted".into());}
    drop(backup);
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    tx.execute_batch("CREATE TEMP TABLE cleanup_candidates(source TEXT,source_id TEXT,PRIMARY KEY(source,source_id));").map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO cleanup_candidates SELECT s.source,s.source_id FROM source_listings s
        WHERE s.retrieved_at < datetime('now',?1)
        AND NOT EXISTS(SELECT 1 FROM jobs j WHERE j.id=CASE WHEN s.source='jobup' THEN s.source_id ELSE s.source||':'||s.source_id END OR j.link=s.listing_url)
        AND NOT EXISTS(SELECT 1 FROM review_triage t WHERE t.source=s.source AND t.source_id=s.source_id)
        AND NOT EXISTS(SELECT 1 FROM review_triage_events t WHERE t.source=s.source AND t.source_id=s.source_id)
        AND NOT EXISTS(SELECT 1 FROM opportunity_links o WHERE o.source=s.source AND o.source_id=s.source_id)",
        [format!("-{days} days")]).map_err(|e|e.to_string())?;
    for table in ["source_listing_revisions","source_eligibility"] {
        tx.execute(&format!("DELETE FROM {table} WHERE (source,source_id) IN (SELECT source,source_id FROM cleanup_candidates)"),[]).map_err(|e|e.to_string())?;
    }
    let removed=tx.execute("DELETE FROM source_listings WHERE (source,source_id) IN (SELECT source,source_id FROM cleanup_candidates)",[]).map_err(|e|e.to_string())?;
    tx.execute_batch("DROP TABLE cleanup_candidates;").map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(CleanupReport{removed,backup_path:backup_path.display().to_string()})
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn preserves_linked_and_reviewed_records(){
        let mut db=Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE source_listings(source TEXT,source_id TEXT,retrieved_at TEXT,listing_url TEXT);CREATE TABLE jobs(id TEXT,link TEXT);CREATE TABLE review_triage(source TEXT,source_id TEXT);CREATE TABLE review_triage_events(source TEXT,source_id TEXT);CREATE TABLE opportunity_links(source TEXT,source_id TEXT);CREATE TABLE source_listing_revisions(source TEXT,source_id TEXT);CREATE TABLE source_eligibility(source TEXT,source_id TEXT);
        INSERT INTO source_listings VALUES ('jobup','old','2020-01-01','https://example.com/old'),('jobup','applied','2020-01-01','https://example.com/applied'),('jobup','noted','2020-01-01','https://example.com/noted'),('jobup','recent',datetime('now'),'https://example.com/recent');INSERT INTO jobs VALUES('applied','https://example.com/applied');INSERT INTO review_triage_events VALUES('jobup','noted');").unwrap();
        let path=std::env::temp_dir().join(format!("masar-cleanup-{}.sqlite3",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let report=execute(&mut db,&path,90).unwrap();assert_eq!(report.removed,1);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM source_listings",[],|r|r.get::<_,usize>(0)).unwrap(),3);
        let saved=Connection::open(&path).unwrap();assert_eq!(saved.query_row("SELECT COUNT(*) FROM source_listings",[],|r|r.get::<_,usize>(0)).unwrap(),4);drop(saved);std::fs::remove_file(path).unwrap();
    }
    #[test] fn invalid_age_does_not_mutate(){let mut db=Connection::open_in_memory().unwrap();assert!(execute(&mut db,Path::new("unused"),0).is_err());}
}
