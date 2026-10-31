use std::{path::PathBuf,collections::BTreeMap};
use rusqlite::{Connection,OpenFlags};

fn history_hashes(db:&Connection)->BTreeMap<String,String>{
    let mut hashes=BTreeMap::new();
    for table in ["jobs","reviews","status_history","review_triage","review_triage_events","application_details","application_events","followup_tasks","followup_events"]{
        let mut stmt=db.prepare(&format!("SELECT * FROM {table} ORDER BY rowid")).unwrap();
        let columns=stmt.column_count();let mut rows=stmt.query([]).unwrap();let mut hasher=blake3::Hasher::new();
        while let Some(row)=rows.next().unwrap(){for column in 0..columns{hasher.update(format!("{:?}\n",row.get_ref(column).unwrap()).as_bytes());}}
        hashes.insert(table.to_string(),hasher.finalize().to_hex().to_string());
    }
    hashes
}

#[test]
#[ignore="Requires an explicitly supplied private SQLite safety snapshot; never use the live database"]
fn private_snapshot_migration_and_recovery(){
    let source=PathBuf::from(std::env::var("MASAR_SAFETY_SNAPSHOT").expect("Set MASAR_SAFETY_SNAPSHOT to a verified backup file"));
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir=std::env::temp_dir().join(format!("masar-recovery-{stamp}"));std::fs::create_dir(&dir).unwrap();
    let copy=dir.join("migration.sqlite3");std::fs::copy(&source,&copy).unwrap();
    let before=Connection::open_with_flags(&source,OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();let hashes=history_hashes(&before);drop(before);
    let mut db=crate::database::open(&copy).unwrap();crate::review_step2::migrate(&mut db).unwrap();crate::applications_step3::migrate(&mut db).unwrap();crate::followups_step4::migrate(&mut db).unwrap();
    assert_eq!(history_hashes(&db),hashes,"Migration changed application/review/follow-up history");
    let backup=dir.join("backup.sqlite3");crate::database::backup(&db,&backup).unwrap();drop(db);
    let recovered=crate::database::open(&backup).unwrap();assert_eq!(history_hashes(&recovered),hashes);
    let integrity:String=recovered.query_row("PRAGMA integrity_check",[],|r|r.get(0)).unwrap();assert_eq!(integrity,"ok");
    let violations:i64=recovered.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check",[],|r|r.get(0)).unwrap();assert_eq!(violations,0);
    drop(recovered);
    // Delete only files generated in this uniquely created test directory.
    for entry in std::fs::read_dir(&dir).unwrap(){let entry=entry.unwrap();assert!(entry.path().starts_with(&dir));std::fs::remove_file(entry.path()).unwrap();}std::fs::remove_dir(dir).unwrap();
}
