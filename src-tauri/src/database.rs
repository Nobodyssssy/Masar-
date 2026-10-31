use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub reviewed: Value,
    pub jobs: Vec<Value>,
    pub settings: Value,
    pub saved_at: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub conflicts: usize,
    pub source_files: usize,
}

pub fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let mut db = Connection::open(path).map_err(|e| e.to_string())?;
    db.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;").map_err(|e| e.to_string())?;
    migrate(&mut db)?;
    Ok(db)
}

fn migrate(db: &mut Connection) -> Result<(), String> {
    let tx=db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT '', company TEXT NOT NULL DEFAULT '',
            place TEXT NOT NULL DEFAULT '', link TEXT NOT NULL DEFAULT '', grades_json TEXT NOT NULL DEFAULT '[]',
            source TEXT NOT NULL DEFAULT '', retrieved_at TEXT NOT NULL DEFAULT '',
            description TEXT, eligibility TEXT, raw_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE TABLE IF NOT EXISTS reviews (
            job_id TEXT PRIMARY KEY, status TEXT NOT NULL DEFAULT 'new',
            applied_date TEXT, follow_up_date TEXT, note TEXT NOT NULL DEFAULT '',
            reviewed_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS status_history (
            job_id TEXT NOT NULL, ordinal INTEGER NOT NULL, entry_json TEXT NOT NULL,
            PRIMARY KEY(job_id, ordinal)
        );
        CREATE TABLE IF NOT EXISTS seen (
            key TEXT PRIMARY KEY, job_id TEXT, original_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS import_sources (
            source_key TEXT PRIMARY KEY, digest TEXT NOT NULL, imported_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS import_conflicts (
            id INTEGER PRIMARY KEY, source_key TEXT NOT NULL, record_id TEXT NOT NULL,
            existing_json TEXT NOT NULL, incoming_json TEXT NOT NULL,
            UNIQUE(source_key,record_id,incoming_json)
        );
        INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
    "#).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    crate::source_store::migrate(db)
}

pub fn read(db: &Connection) -> Result<AppState, String> {
    let mut reviewed = serde_json::Map::new();
    let mut stmt=db.prepare("SELECT job_id, reviewed_json FROM reviews").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
    for row in rows { let (id,text)=row.map_err(|e|e.to_string())?;reviewed.insert(id,serde_json::from_str(&text).map_err(|e|e.to_string())?); }
    let mut jobs=vec![];
    let mut stmt=db.prepare("SELECT raw_json FROM jobs ORDER BY retrieved_at DESC, id").map_err(|e|e.to_string())?;
    for row in stmt.query_map([],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())? {
        jobs.push(serde_json::from_str(&row.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?);
    }
    let settings=db.query_row("SELECT value_json FROM settings WHERE key='ui'",[],|r|r.get::<_,String>(0)).optional().map_err(|e|e.to_string())?
        .and_then(|s|serde_json::from_str(&s).ok()).unwrap_or(json!({"dailyGoal":3}));
    let saved_at=db.query_row("SELECT value_json FROM settings WHERE key='saved_at'",[],|r|r.get::<_,String>(0)).optional().map_err(|e|e.to_string())?.unwrap_or_default();
    Ok(AppState {reviewed:Value::Object(reviewed), jobs, settings, saved_at})
}

fn upsert_job(tx:&rusqlite::Transaction<'_>, j:&Value, source:&str) -> Result<bool,String> {
    let id=j.get("id").and_then(Value::as_str).ok_or("Job without id")?;
    let company=j.get("company").and_then(|v|v.as_str().or_else(||v.get("name").and_then(Value::as_str))).unwrap_or("");
    let grades=j.get("grades").or_else(||j.get("employmentGrades")).cloned().unwrap_or(json!([]));
    let changed=tx.execute("INSERT OR IGNORE INTO jobs(id,title,company,place,link,grades_json,source,retrieved_at,description,raw_json) VALUES (?,?,?,?,?,?,?,?,?,?)",
    params![id,j.get("title").and_then(Value::as_str).unwrap_or(""),company,j.get("place").and_then(Value::as_str).unwrap_or(""),j.get("link").and_then(Value::as_str).unwrap_or(""),grades.to_string(),source,j.get("_date").and_then(Value::as_str).unwrap_or(""),j.get("description").and_then(Value::as_str),j.to_string()]).map_err(|e|e.to_string())?;
    Ok(changed>0)
}

fn merge_review(tx:&rusqlite::Transaction<'_>, id:&str, v:&Value, source:&str, report:&mut ImportReport)->Result<(),String>{
    if !v.is_object(){report.skipped+=1;return Ok(())}
    let incoming=v.to_string();
    let old:Option<String>=tx.query_row("SELECT reviewed_json FROM reviews WHERE job_id=?",[id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    if let Some(old)=old {
        if serde_json::from_str::<Value>(&old).ok().as_ref()==Some(v){report.skipped+=1;return Ok(())}
        tx.execute("INSERT OR IGNORE INTO import_conflicts(source_key,record_id,existing_json,incoming_json) VALUES (?,?,?,?)",params![source,id,old,incoming]).map_err(|e|e.to_string())?;
        report.conflicts+=1; return Ok(())
    }
    let status=v.get("status").and_then(Value::as_str).unwrap_or("new");
    tx.execute("INSERT INTO reviews(job_id,status,applied_date,follow_up_date,note,reviewed_json) VALUES (?,?,?,?,?,?)",params![id,status,v.get("appliedDate").and_then(Value::as_str),v.get("followUpDate").and_then(Value::as_str),v.get("note").and_then(Value::as_str).unwrap_or(""),incoming]).map_err(|e|e.to_string())?;
    if let Some(history)=v.get("history").and_then(Value::as_array){
        for (n,h) in history.iter().enumerate(){tx.execute("INSERT OR IGNORE INTO status_history(job_id,ordinal,entry_json) VALUES (?,?,?)",params![id,n,h.to_string()]).map_err(|e|e.to_string())?;}
    }
    report.imported+=1;Ok(())
}

pub fn import_reviewed(db:&mut Connection, value:&Value, source:&str)->Result<ImportReport,String>{
    let records= if value.get("version").is_some() {value.get("reviewed").unwrap_or(value)} else {value};
    let mut report=ImportReport::default();
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    if let Some(ids)=records.get("done").and_then(Value::as_array){
        for id in ids.iter().filter_map(Value::as_str){merge_review(&tx,id,&json!({"status":"done","note":"","date":""}),source,&mut report)?;}
    }else{
        for (id,v) in records.as_object().ok_or("Reviewed state must be a JSON object")? {merge_review(&tx,id,v,source,&mut report)?;}
    }
    tx.commit().map_err(|e|e.to_string())?;Ok(report)
}

// The separate history table is authoritative; UI-provided history may add events
// but can never delete or rewrite events already committed to SQLite.
pub fn replace_review(db:&mut Connection,id:&str, entry:Option<&Value>)->Result<(),String>{
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let old:Option<String>=tx.query_row("SELECT reviewed_json FROM reviews WHERE job_id=?",[id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    let mut stmt=tx.prepare("SELECT entry_json FROM status_history WHERE job_id=? ORDER BY ordinal") .map_err(|e|e.to_string())?;
    let stored:Vec<Value>=stmt.query_map([id],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?
        .map(|r|r.map_err(|e|e.to_string()).and_then(|s|serde_json::from_str(&s).map_err(|e|e.to_string())))
        .collect::<Result<_,_>>()?;
    drop(stmt);
    let previous=old.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(|e|e.to_string())?;
    let mut updated=entry.cloned().unwrap_or_else(||json!({"status":"new"}));
    if !updated.is_object(){return Err("Review must be an object".into())}
    let old_status=previous.as_ref().and_then(|v|v.get("status")).and_then(Value::as_str).unwrap_or("new");
    let new_status=updated.get("status").and_then(Value::as_str).unwrap_or("new").to_owned();
    let existing_date=previous.as_ref().and_then(|v|v.get("appliedDate")).and_then(Value::as_str).filter(|v|!v.is_empty());
    if let Some(d)=existing_date {updated["appliedDate"]=json!(d);}
    let supplied=updated.get("history").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut final_history=stored.clone();
    // Migrating first write to an existing record may include history previously
    // stored only in reviewed_json; don't lose that history.
    if stored.is_empty() {final_history=supplied.clone();}
    else if supplied.len()>=stored.len() && supplied[..stored.len()]==stored[..] {
        final_history.extend_from_slice(&supplied[stored.len()..]);
    }else if !supplied.is_empty() && supplied!=stored {
        // An undo/import attempted to rewrite history: retain the competing value.
        tx.execute("INSERT OR IGNORE INTO import_conflicts(source_key,record_id,existing_json,incoming_json) VALUES ('review-history-rewrite',?,?,?)",
            params![id,json!(stored).to_string(),json!(supplied).to_string()]).map_err(|e|e.to_string())?;
    }
    if old_status!=new_status && final_history==stored {
        final_history.push(json!({"from":old_status,"to":new_status,"date":updated.get("date").and_then(Value::as_str).unwrap_or(""),"kind":"native-status-change"}));
    }
    // None corresponds to the UI's undo-to-new action, NOT deletion of the audit trail.
    updated["history"]=json!(final_history);
    tx.execute("INSERT INTO reviews(job_id,status,applied_date,follow_up_date,note,reviewed_json) VALUES (?,?,?,?,?,?) ON CONFLICT(job_id) DO UPDATE SET status=excluded.status,applied_date=excluded.applied_date,follow_up_date=excluded.follow_up_date,note=excluded.note,reviewed_json=excluded.reviewed_json",
        params![id,new_status,updated.get("appliedDate").and_then(Value::as_str),updated.get("followUpDate").and_then(Value::as_str),updated.get("note").and_then(Value::as_str).unwrap_or(""),updated.to_string()]).map_err(|e|e.to_string())?;
    for (i,item) in final_history.iter().enumerate().skip(stored.len()){
        tx.execute("INSERT INTO status_history(job_id,ordinal,entry_json) VALUES (?,?,?)",params![id,i,item.to_string()]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e|e.to_string())
}

pub fn add_jobs(db:&mut Connection,jobs:&[Value],source:&str)->Result<usize,String>{
    let tx=db.transaction().map_err(|e|e.to_string())?;
    let mut n=0;for j in jobs {if upsert_job(&tx,j,source)? {n+=1}}
    tx.commit().map_err(|e|e.to_string())?;Ok(n)
}

pub fn write_settings(db:&Connection,value:&Value,saved_at:&str)->Result<(),String>{
    db.execute("INSERT INTO settings(key,value_json) VALUES ('ui',?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",[value.to_string()]).map_err(|e|e.to_string())?;
    db.execute("INSERT INTO settings(key,value_json) VALUES ('saved_at',?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",[saved_at]).map_err(|e|e.to_string())?;
    Ok(())
}

pub fn import_legacy(db:&mut Connection,root:&Path,archive_base:&Path)->Result<ImportReport,String>{
    let data=if root.join("data").is_dir(){root.join("data")}else{root.to_path_buf()};
    let mut files=Vec::<PathBuf>::new();
    fn collect(p:&Path,out:&mut Vec<PathBuf>)->Result<(),String>{for entry in fs::read_dir(p).map_err(|e|e.to_string())?{let p=entry.map_err(|e|e.to_string())?.path();if p.is_dir(){if p.file_name().and_then(|v|v.to_str())!=Some("backups"){collect(&p,out)?}}else if p.extension().and_then(|v|v.to_str())==Some("json"){files_push(out,p)}}Ok(())}
    fn files_push(out:&mut Vec<PathBuf>,p:PathBuf){let name=p.file_name().and_then(|v|v.to_str()).unwrap_or("");if name=="reviewed.json"||name.starts_with("jobs_")||name.starts_with("filtered_"){out.push(p)}}
    collect(&data,&mut files)?;files.sort();
    let mut report=ImportReport::default();
    // Never write into legacy source directory. Import failures roll back each file.
    for file in files {
        let bytes=fs::read(&file).map_err(|e|e.to_string())?;
        let original_digest=blake3::hash(&bytes).to_hex().to_string();
        let archive=archive_base.join("phase2-sources");
        fs::create_dir_all(&archive).map_err(|e|e.to_string())?;
        let archived=archive.join(format!("{}.{}", original_digest, file.file_name().and_then(|v|v.to_str()).unwrap_or("source.json")));
        if !archived.exists(){fs::copy(&file,&archived).map_err(|e|e.to_string())?;}
        let digest=blake3::hash(&bytes).to_hex().to_string();
        let key=file.to_string_lossy().to_string();
        if db.query_row("SELECT digest FROM import_sources WHERE source_key=?",[&key],|r|r.get::<_,String>(0)).optional().map_err(|e|e.to_string())?.as_deref()==Some(digest.as_str()){report.skipped+=1;continue}
        let value:Value=serde_json::from_slice(&bytes).map_err(|e|format!("{}: {e}",file.display()))?;
        let filename=file.file_name().and_then(|v|v.to_str()).unwrap_or("");
        let r=if filename=="reviewed.json"{import_reviewed(db,&value,&key)?}else {
            let arr=value.get("jobs").and_then(Value::as_array).ok_or_else(||format!("Invalid jobs file: {}",file.display()))?;
            let added=add_jobs(db,arr,&key)?;
            ImportReport{imported:added,skipped:arr.len()-added,..Default::default()}
        };
        report.imported+=r.imported;report.skipped+=r.skipped;report.conflicts+=r.conflicts;report.source_files+=1;
        db.execute("INSERT INTO import_sources(source_key,digest) VALUES (?,?) ON CONFLICT(source_key) DO UPDATE SET digest=excluded.digest,imported_at=datetime('now')",params![key,digest]).map_err(|e|e.to_string())?;
    }
    let seen_path=data.join("jobup_seen.db");
    if seen_path.exists(){
        let archive=archive_base.join("phase2-sources");
        fs::create_dir_all(&archive).map_err(|e|e.to_string())?;
        let source_bytes=fs::read(&seen_path).map_err(|e|e.to_string())?;
        let archived=archive.join(format!("{}.jobup_seen.db",blake3::hash(&source_bytes).to_hex()));
        let old=Connection::open_with_flags(&seen_path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
        if !archived.exists(){
            // SQLite snapshot includes any committed WAL transactions.
            let mut out=Connection::open(&archived).map_err(|e|e.to_string())?;
            let snapshot=rusqlite::backup::Backup::new(&old,&mut out).map_err(|e|e.to_string())?;
            snapshot.run_to_completion(100,std::time::Duration::from_millis(20),None).map_err(|e|e.to_string())?;
        }
        let mut stmt=old.prepare("SELECT key,id,title,company,place,grades,link,seen_at FROM seen").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],|r|Ok((0..8).map(|i|r.get::<_,Option<String>>(i).unwrap_or(None)).collect::<Vec<_>>())) .map_err(|e|e.to_string())?;
        let tx=db.transaction().map_err(|e|e.to_string())?;
        for row in rows {let parts=row.map_err(|e|e.to_string())?;let key=parts[0].as_deref().unwrap_or("");
            let added=tx.execute("INSERT OR IGNORE INTO seen(key,job_id,original_json) VALUES (?,?,?)",params![key,parts[1],json!(parts).to_string()]).map_err(|e|e.to_string())?;
            if added>0 {report.imported+=1}else{report.skipped+=1}
        }
        tx.commit().map_err(|e|e.to_string())?;
        report.source_files+=1;
    }
    Ok(report)
}

pub fn backup(db:&Connection,destination:&Path)->Result<(),String>{
    if destination.exists(){return Err("Backup destination already exists".into())}
    if let Some(parent)=destination.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?}
    // VACUUM INTO creates a consistent snapshot, including WAL content.
    db.execute("VACUUM INTO ?",[destination.to_string_lossy().as_ref()]).map_err(|e|e.to_string())?;
    Ok(())
}

// A verified SQLite snapshot is restored with SQLite's online backup API, never
// by copying over a live database (which could have a WAL sidecar).
pub fn restore(db:&mut Connection,source:&Path)->Result<(),String>{
    if !source.is_file(){return Err("Restore file does not exist".into())}
    let src=Connection::open_with_flags(source,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e|e.to_string())?;
    let ok:String=src.query_row("PRAGMA integrity_check",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if ok!="ok"{return Err(format!("Backup integrity check failed: {ok}"))}
    let exists:i64=src.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if exists!=1{return Err("Not a Jobup SQLite backup".into())}
    let copy=rusqlite::backup::Backup::new(&src,db).map_err(|e|e.to_string())?;
    copy.run_to_completion(100,std::time::Duration::from_millis(25),None).map_err(|e|e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn migration_and_review_history(){let dir=tempfile::tempdir().unwrap();let path=dir.path().join("state.db");let mut db=open(&path).unwrap();let v=json!({"x":{"status":"applied","appliedDate":"2020-01-01","history":[{"to":"applied"}],"note":"hello"}});assert_eq!(import_reviewed(&mut db,&v,"test").unwrap().imported,1);assert_eq!(import_reviewed(&mut db,&v,"test").unwrap().skipped,1);let mut r=read(&db).unwrap().reviewed["x"].clone();r["status"]=json!("interview");r["history"].as_array_mut().unwrap().push(json!({"to":"interview"}));replace_review(&mut db,"x",Some(&r)).unwrap();drop(db);let db=open(&path).unwrap();let got=read(&db).unwrap();assert_eq!(got.reviewed["x"]["appliedDate"],"2020-01-01");assert_eq!(got.reviewed["x"]["history"].as_array().unwrap().len(),2);assert_eq!(got.reviewed["x"]["note"],"hello");}
    #[test] fn undo_never_removes_status_history(){
        let dir=tempfile::tempdir().unwrap();let mut db=open(&dir.path().join("db.sqlite")).unwrap();
        replace_review(&mut db,"job",Some(&json!({"status":"applied","appliedDate":"2022-04-10","note":"first","history":[{"to":"applied"}]}))).unwrap();
        replace_review(&mut db,"job",Some(&json!({"status":"interview","appliedDate":"2026-05-01","note":"followup","history":[{"to":"applied"},{"to":"interview"}]}))).unwrap();
        replace_review(&mut db,"job",None).unwrap();
        let state=read(&db).unwrap();
        assert_eq!(state.reviewed["job"]["status"],"new");
        assert_eq!(state.reviewed["job"]["appliedDate"],"2022-04-10");
        assert_eq!(state.reviewed["job"]["history"].as_array().unwrap().len(),3);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM status_history WHERE job_id='job'",[],|r|r.get::<_,i64>(0)).unwrap(),3);
    }
    #[test] fn restore_snapshot_preserves_backup(){
        let dir=tempfile::tempdir().unwrap();let mut db=open(&dir.path().join("live.sqlite")).unwrap();
        replace_review(&mut db,"a",Some(&json!({"status":"applied","appliedDate":"2020-01-01"}))).unwrap();
        let snapshot=dir.path().join("backup.sqlite");backup(&db,&snapshot).unwrap();
        replace_review(&mut db,"a",Some(&json!({"status":"interview"}))).unwrap();
        restore(&mut db,&snapshot).unwrap();
        assert_eq!(read(&db).unwrap().reviewed["a"]["status"],"applied");
        assert!(snapshot.exists());
    }
    #[test] fn legacy_source_is_not_modified(){
        let dir=tempfile::tempdir().unwrap();let original=dir.path().join("legacy");
        std::fs::create_dir_all(original.join("data/2026/10")).unwrap();
        let file=original.join("data/2026/10/jobs_2026-10-25.json");
        let bytes=br#"{"jobs":[{"id":"one","title":"Sales"}]}"#;
        std::fs::write(&file,bytes).unwrap();
        let appdata=dir.path().join("appdata");
        let mut db=open(&appdata.join("jobup.sqlite3")).unwrap();
        let first=import_legacy(&mut db,&original,&appdata.join("backups")).unwrap();
        assert_eq!(first.imported,1);
        let second=import_legacy(&mut db,&original,&appdata.join("backups")).unwrap();
        assert_eq!(second.imported,0);
        assert_eq!(std::fs::read(&file).unwrap(),bytes);
        assert!(!original.join("data/backups").exists());
    }
    #[test] fn conflicts_and_backup(){let dir=tempfile::tempdir().unwrap();let mut db=open(&dir.path().join("db.sqlite")).unwrap();import_reviewed(&mut db,&json!({"x":{"status":"new"}}),"first").unwrap();assert_eq!(import_reviewed(&mut db,&json!({"x":{"status":"applied"}}),"second").unwrap().conflicts,1);assert_eq!(read(&db).unwrap().reviewed["x"]["status"],"new");let path=dir.path().join("backup.sqlite");backup(&db,&path).unwrap();assert!(path.exists());}
}
