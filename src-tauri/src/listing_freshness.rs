use rusqlite::{Connection,OptionalExtension,params};
use serde::Serialize;

pub fn migrate(db:&Connection)->Result<(),String>{
    db.execute_batch("CREATE TABLE IF NOT EXISTS listing_observations(
        source TEXT NOT NULL,source_id TEXT NOT NULL,first_seen_at TEXT,last_seen_at TEXT,
        last_verified_at TEXT,availability TEXT NOT NULL DEFAULT 'unknown',
        PRIMARY KEY(source,source_id),FOREIGN KEY(source,source_id) REFERENCES source_listings(source,source_id) ON DELETE CASCADE);
        INSERT OR IGNORE INTO listing_observations(source,source_id,last_seen_at) SELECT source,source_id,retrieved_at FROM source_listings;
        CREATE TRIGGER IF NOT EXISTS listing_seen_insert AFTER INSERT ON source_listings BEGIN
          INSERT OR IGNORE INTO listing_observations(source,source_id,first_seen_at,last_seen_at) VALUES(NEW.source,NEW.source_id,datetime('now'),NEW.retrieved_at);
        END;
        CREATE TRIGGER IF NOT EXISTS listing_seen_update AFTER UPDATE OF retrieved_at ON source_listings BEGIN
          UPDATE listing_observations SET last_seen_at=NEW.retrieved_at WHERE source=NEW.source AND source_id=NEW.source_id;
        END;
        CREATE INDEX IF NOT EXISTS idx_listing_observations_availability ON listing_observations(availability,last_seen_at);
        INSERT OR IGNORE INTO schema_migrations(version) VALUES(60);").map_err(|e|e.to_string())
}
#[derive(Debug,Default,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Observation{pub first_seen_at:Option<String>,pub last_seen_at:Option<String>,pub last_verified_at:Option<String>,pub availability:String}
pub fn get(db:&Connection,source:&str,id:&str)->Result<Observation,String>{
    db.query_row("SELECT first_seen_at,last_seen_at,last_verified_at,availability FROM listing_observations WHERE source=?1 AND source_id=?2",params![source,id],|r|Ok(Observation{first_seen_at:r.get(0)?,last_seen_at:r.get(1)?,last_verified_at:r.get(2)?,availability:r.get(3)?})).optional().map_err(|e|e.to_string()).map(|o|o.unwrap_or_default())
}

#[tauri::command]
pub async fn verify_vacancy_link(state:tauri::State<'_,crate::NativeDb>,source:String,source_id:String)->Result<String,String>{
    let url=crate::with_db(state.clone(),|db|db.query_row("SELECT listing_url FROM source_listings WHERE source=?1 AND source_id=?2",params![source,source_id],|r|r.get::<_,String>(0)).map_err(|e|e.to_string()))?;
    let checked=crate::source_store::checked_listing_url(&url,&source)?;
    let source_clone=source.clone();
    let result=tauri::async_runtime::spawn_blocking(move||{
        let client=reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20)).redirect(reqwest::redirect::Policy::limited(2)).build().map_err(|e|e.to_string())?;
        let availability=match client.head(&checked).send(){
            Ok(response)=>{
                crate::source_store::checked_listing_url(response.url().as_str(),&source_clone)?;
                match response.status().as_u16(){200..=299=>"reachable",404|410=>"expired",401|403|429=>"blocked",_=>"unknown"}
            },Err(_)=>"unreachable"
        };
        Ok::<_,String>(availability.to_string())
    }).await.map_err(|e|e.to_string())??;
    crate::with_db(state,|db|{
        db.execute("UPDATE listing_observations SET availability=?3,last_verified_at=CASE WHEN ?3='reachable' THEN datetime('now') ELSE last_verified_at END WHERE source=?1 AND source_id=?2",params![source,source_id,result]).map_err(|e|e.to_string())?;Ok(())
    })?;
    Ok(result)
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn additive_migration_preserves_unknown_first_seen(){
        let db=Connection::open_in_memory().unwrap();db.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);CREATE TABLE source_listings(source TEXT,source_id TEXT,retrieved_at TEXT,PRIMARY KEY(source,source_id));INSERT INTO source_listings VALUES('jobup','old','2026-01-01');").unwrap();migrate(&db).unwrap();migrate(&db).unwrap();assert!(get(&db,"jobup","old").unwrap().first_seen_at.is_none());db.execute("INSERT INTO source_listings VALUES('jobup','new',datetime('now'))",[]).unwrap();let first=get(&db,"jobup","new").unwrap().first_seen_at;assert!(first.is_some());db.execute("UPDATE source_listings SET retrieved_at='2026-11-01' WHERE source_id='new'",[]).unwrap();let seen=get(&db,"jobup","new").unwrap();assert_eq!(seen.first_seen_at,first);assert_eq!(seen.last_seen_at.as_deref(),Some("2026-11-01"));
    }
}
