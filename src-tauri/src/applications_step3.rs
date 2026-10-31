//! Step 3/7 — application manager. Legacy `reviews` + `status_history` remain
//! the status source of truth. This module only adds supplemental metadata and
//! its own append-only audit events. Nothing is submitted to employers.
use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

const MAX_ITEMS: usize = 5000;
const STAGES: &[&str] = &["new", "applied", "screening", "interview", "offer", "rejected", "withdrawn"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRow {
    pub job_id: String,
    pub title: String,
    pub company: String,
    pub place: String,
    pub listing_url: String,
    pub source: String,
    pub status: String,
    pub applied_date: String,
    pub follow_up_date: String,
    pub note: String,
    pub cv_profile: String,
    pub cv_file: String,
    pub letter_file: String,
    pub application_method: String,
    pub contact: String,
    pub interview_at: String,
    pub revision: i64,
    pub history: Vec<Value>,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub tracked: usize, pub to_apply: usize, pub applied: usize,
    pub interviews: usize, pub offers: usize, pub closed: usize,
    pub follow_ups_due: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationPage { pub items: Vec<ApplicationRow>, pub summary: Summary, pub truncated: bool }

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationUpdate {
    pub job_id: String,
    pub revision: i64,
    pub status: String,
    pub note: String,
    pub applied_date: String,
    pub follow_up_date: String,
    pub cv_profile: String,
    pub cv_file: String,
    pub letter_file: String,
    pub application_method: String,
    pub contact: String,
    pub interview_at: String,
}

pub fn migrate(db: &Connection) -> Result<(), String> {
    db.execute_batch(r#"
    CREATE TABLE IF NOT EXISTS application_details (
      job_id TEXT PRIMARY KEY,
      cv_profile TEXT NOT NULL DEFAULT '',
      cv_file TEXT NOT NULL DEFAULT '',
      letter_file TEXT NOT NULL DEFAULT '',
      application_method TEXT NOT NULL DEFAULT '',
      contact TEXT NOT NULL DEFAULT '',
      interview_at TEXT NOT NULL DEFAULT '',
      revision INTEGER NOT NULL DEFAULT 0,
      updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE IF NOT EXISTS application_events (
      event_id INTEGER PRIMARY KEY AUTOINCREMENT,
      job_id TEXT NOT NULL,
      kind TEXT NOT NULL,
      previous_json TEXT NOT NULL,
      current_json TEXT NOT NULL,
      recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE INDEX IF NOT EXISTS application_events_by_job ON application_events(job_id,event_id);
    "#).map_err(|e|e.to_string())
}

fn date_or_empty(s:&str)->bool { s.is_empty() || (s.len()==10 && NaiveDate::parse_from_str(s,"%Y-%m-%d").is_ok()) }
fn text_ok(value:&str, max:usize)->bool {value.len()<=max && !value.contains('\0')}
fn add_weekdays(mut day:NaiveDate, mut left:usize)->NaiveDate{
    while left>0 { day += Duration::days(1); if day.weekday().number_from_monday()<=5 { left-=1; } }
    day
}
fn next_five_weekdays()->String{add_weekdays(Local::now().date_naive(),5).format("%Y-%m-%d").to_string()}
use chrono::Datelike;

fn validate(update:&ApplicationUpdate)->Result<(),String>{
    if update.job_id.is_empty()||update.job_id.len()>250 {return Err("Invalid application identity".into())}
    if !STAGES.contains(&update.status.as_str()) {return Err("Invalid application stage".into())}
    if update.revision<0 {return Err("Invalid revision".into())}
    for (field,s,max) in [
      ("note",update.note.as_str(),10_000),("cv profile",&update.cv_profile,120),
      ("CV reference",&update.cv_file,1024),("letter reference",&update.letter_file,1024),
      ("application method",&update.application_method,200),("contact",&update.contact,300),
    ] {if !text_ok(s,max){return Err(format!("Invalid or oversized {field}"))}}
    if !date_or_empty(&update.applied_date){return Err("Submission date must be a valid YYYY-MM-DD date".into())}
    if !date_or_empty(&update.follow_up_date){return Err("Follow-up date must be a valid YYYY-MM-DD date".into())}
    if !update.interview_at.is_empty() && NaiveDateTime::parse_from_str(&format!("{}:00",update.interview_at),"%Y-%m-%dT%H:%M:%S").is_err() {return Err("Interview date/time must be YYYY-MM-DDTHH:MM".into())}
    Ok(())
}

fn history_of(db:&Connection,id:&str,raw:&Value)->Result<Vec<Value>,String>{
    let mut stmt=db.prepare("SELECT entry_json FROM status_history WHERE job_id=?1 ORDER BY ordinal") .map_err(|e|e.to_string())?;
    let stored=stmt.query_map([id],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?
       .map(|r|r.map_err(|e|e.to_string()).and_then(|s|serde_json::from_str::<Value>(&s).map_err(|e|e.to_string())))
       .collect::<Result<Vec<_>,_>>()?;
    if !stored.is_empty(){return Ok(stored)}
    Ok(raw.get("history").and_then(Value::as_array).cloned().unwrap_or_default())
}

pub fn save(db:&mut Connection, p:ApplicationUpdate)->Result<(),String>{
    validate(&p)?;
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    // A promoted job exists in jobs. Also accept imported legacy reviews without a job row.
    let job_present:i64=tx.query_row("SELECT COUNT(*) FROM jobs WHERE id=?1",[&p.job_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    let old_json:Option<String>=tx.query_row("SELECT reviewed_json FROM reviews WHERE job_id=?1",[&p.job_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    if job_present==0&&old_json.is_none(){return Err("Application does not exist in Jobup".into())}
    let old_meta:Option<(String,String,String,String,String,String,i64)>=tx.query_row(
       "SELECT cv_profile,cv_file,letter_file,application_method,contact,interview_at,revision FROM application_details WHERE job_id=?1",
       [&p.job_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))
       .optional().map_err(|e|e.to_string())?;
    let rev=old_meta.as_ref().map(|v|v.6).unwrap_or(0);
    if rev != p.revision {return Err("This application was updated elsewhere. Reload before saving.".into())}
    let mut previous=old_json.as_deref().map(serde_json::from_str::<Value>).transpose().map_err(|e|e.to_string())?.unwrap_or_else(||json!({"status":"new","history":[]}));
    if !previous.is_object(){return Err("Stored application review is invalid".into())}
    let old_status=previous.get("status").and_then(Value::as_str).unwrap_or("new").to_owned();
    if old_status=="new" && !["new","applied","withdrawn"].contains(&p.status.as_str()) {
        return Err("Mark the job Applied before recording an employer response or interview".into())
    }
    let today=Local::now().date_naive().format("%Y-%m-%d").to_string();
    let old_applied=previous.get("appliedDate").and_then(Value::as_str).unwrap_or("");
    if !old_applied.is_empty() && !p.applied_date.is_empty() && p.applied_date!=old_applied {
        return Err("Submission date is already recorded; it cannot be silently rewritten".into())
    }
    let mut applied_date=if old_applied.is_empty(){p.applied_date.clone()}else{old_applied.to_string()};
    if p.status=="new" && !applied_date.is_empty(){return Err("Cannot record a submission date while the application is To Apply".into())}
    if p.status=="applied" && old_status!="applied" && applied_date.is_empty(){applied_date=today.clone();}
    let mut follow=p.follow_up_date.clone();
    if p.status=="applied" && old_status!="applied" && follow.is_empty(){
        let base=NaiveDate::parse_from_str(&applied_date,"%Y-%m-%d").map_err(|e|e.to_string())?;
        follow=add_weekdays(base,5).format("%Y-%m-%d").to_string();
    }
    let mut history=history_of(&tx,&p.job_id,&previous)?;
    let old_count:i64=tx.query_row("SELECT COUNT(*) FROM status_history WHERE job_id=?1",[&p.job_id],|r|r.get(0)).map_err(|e|e.to_string())?;
    // Imported historical events may be present in reviewed_json but not the separate table.
    for (i,event) in history.iter().enumerate().skip(old_count as usize) {
        tx.execute("INSERT INTO status_history(job_id,ordinal,entry_json) VALUES (?1,?2,?3)",params![p.job_id,i as i64,event.to_string()]).map_err(|e|e.to_string())?;
    }
    if old_status!=p.status {
       let event=json!({"from":old_status,"to":p.status,"date":today,"kind":"applications-step3"});
       tx.execute("INSERT INTO status_history(job_id,ordinal,entry_json) VALUES (?1,?2,?3)",params![p.job_id,history.len() as i64,event.to_string()]).map_err(|e|e.to_string())?;
       history.push(event);
    }
    let old_snapshot=json!({"status":old_status,"note":previous.get("note"),"followUpDate":previous.get("followUpDate"),"metadata":old_meta});
    previous["status"]=json!(p.status);
    previous["date"]=json!(today);
    previous["note"]=json!(p.note);
    previous["appliedDate"]=json!(applied_date);
    previous["followUpDate"]=json!(follow);
    previous["history"]=json!(history);
    tx.execute("INSERT INTO reviews(job_id,status,applied_date,follow_up_date,note,reviewed_json) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(job_id) DO UPDATE SET status=excluded.status,applied_date=excluded.applied_date,follow_up_date=excluded.follow_up_date,note=excluded.note,reviewed_json=excluded.reviewed_json",
      params![p.job_id,p.status,if applied_date.is_empty(){None}else{Some(applied_date.as_str())},if follow.is_empty(){None}else{Some(follow.as_str())},p.note,previous.to_string()]).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO application_details(job_id,cv_profile,cv_file,letter_file,application_method,contact,interview_at,revision) VALUES (?1,?2,?3,?4,?5,?6,?7,1) ON CONFLICT(job_id) DO UPDATE SET cv_profile=excluded.cv_profile,cv_file=excluded.cv_file,letter_file=excluded.letter_file,application_method=excluded.application_method,contact=excluded.contact,interview_at=excluded.interview_at,revision=application_details.revision+1,updated_at=datetime('now')",
      params![p.job_id,p.cv_profile,p.cv_file,p.letter_file,p.application_method,p.contact,p.interview_at]).map_err(|e|e.to_string())?;
    let new_snapshot=json!({"status":p.status,"note":p.note,"followUpDate":follow,"cvProfile":p.cv_profile,"cvFile":p.cv_file,"letterFile":p.letter_file,"applicationMethod":p.application_method,"contact":p.contact,"interviewAt":p.interview_at});
    tx.execute("INSERT INTO application_events(job_id,kind,previous_json,current_json) VALUES (?1,'update',?2,?3)",params![p.job_id,old_snapshot.to_string(),new_snapshot.to_string()]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())
}

pub fn list(db:&Connection)->Result<ApplicationPage,String>{
    let mut stmt=db.prepare(r#"SELECT j.id,j.title,j.company,j.place,j.link,j.source,
       COALESCE(r.reviewed_json,'{}'),
       COALESCE(m.cv_profile,''),COALESCE(m.cv_file,''),COALESCE(m.letter_file,''),
       COALESCE(m.application_method,''),COALESCE(m.contact,''),COALESCE(m.interview_at,''),COALESCE(m.revision,0)
       FROM jobs j LEFT JOIN reviews r ON r.job_id=j.id
       LEFT JOIN application_details m ON m.job_id=j.id
       ORDER BY j.retrieved_at DESC,j.id LIMIT ?1"#).map_err(|e|e.to_string())?;
    let rows=stmt.query_map([MAX_ITEMS as i64],|r|{
       Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,
       r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,String>(9)?,r.get::<_,String>(10)?,r.get::<_,String>(11)?,r.get::<_,String>(12)?,r.get::<_,i64>(13)?))
    }).map_err(|e|e.to_string())?;
    let mut items=Vec::new();let mut seen=HashSet::new();
    for row in rows {
       let (id,title,company,place,url,source,raw,cv_profile,cv_file,letter_file,method,contact,interview_at,revision)=row.map_err(|e|e.to_string())?;
       seen.insert(id.clone());let v:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
       let status=v.get("status").and_then(Value::as_str).unwrap_or("new").to_string();
       let history=history_of(db,&id,&v)?;
       items.push(ApplicationRow{job_id:id,title,company,place,listing_url:url,source,status,
         applied_date:v.get("appliedDate").and_then(Value::as_str).unwrap_or("").into(),
         follow_up_date:v.get("followUpDate").and_then(Value::as_str).unwrap_or("").into(),
         note:v.get("note").and_then(Value::as_str).unwrap_or("").into(),cv_profile,cv_file,letter_file,application_method:method,contact,interview_at,revision,
         history});
    }
    drop(stmt);
    // Preserve legacy reviewed.json imports even when their job JSON is absent.
    let mut orphan=db.prepare("SELECT r.job_id,r.reviewed_json FROM reviews r LEFT JOIN jobs j ON j.id=r.job_id WHERE j.id IS NULL ORDER BY r.job_id LIMIT ?1").map_err(|e|e.to_string())?;
    let others=orphan.query_map([MAX_ITEMS as i64],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
    for row in others {let (id,raw)=row.map_err(|e|e.to_string())?;if !seen.insert(id.clone()){continue};let v:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
      let fallback=Value::Null;let meta=v.get("_meta").unwrap_or(&fallback);
      let extras:Option<(String,String,String,String,String,String,i64)>=db.query_row("SELECT cv_profile,cv_file,letter_file,application_method,contact,interview_at,revision FROM application_details WHERE job_id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(|e|e.to_string())?;
      let extras=extras.unwrap_or_default();
      items.push(ApplicationRow{job_id:id.clone(),title:meta.get("title").and_then(Value::as_str).unwrap_or(&id).into(),company:meta.get("company").and_then(Value::as_str).unwrap_or("").into(),place:meta.get("place").and_then(Value::as_str).unwrap_or("").into(),listing_url:meta.get("link").and_then(Value::as_str).unwrap_or("").into(),source:"legacy".into(),status:v.get("status").and_then(Value::as_str).unwrap_or("new").into(),applied_date:v.get("appliedDate").and_then(Value::as_str).unwrap_or("").into(),follow_up_date:v.get("followUpDate").and_then(Value::as_str).unwrap_or("").into(),note:v.get("note").and_then(Value::as_str).unwrap_or("").into(),cv_profile:extras.0,cv_file:extras.1,letter_file:extras.2,application_method:extras.3,contact:extras.4,interview_at:extras.5,revision:extras.6,history:history_of(db,&id,&v)?});
    }
    let today=Local::now().date_naive().format("%Y-%m-%d").to_string();
    let mut summary=Summary::default();
    for item in &items {
       summary.tracked+=1;
       match item.status.as_str(){"new"=>summary.to_apply+=1,"applied"|"screening"=>summary.applied+=1,"interview"=>summary.interviews+=1,"offer"=>summary.offers+=1,"rejected"|"withdrawn"=>summary.closed+=1,_=>()};
       if !item.follow_up_date.is_empty() && item.follow_up_date<=today && ["applied","screening"].contains(&item.status.as_str()){summary.follow_ups_due+=1;}
    }
    let truncated=items.len()>=MAX_ITEMS;
    Ok(ApplicationPage{items,summary,truncated})
}

#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ActivityEvent { pub kind:String, pub previous:Value, pub current:Value, pub recorded_at:String }

pub fn activity(db:&Connection,id:&str)->Result<Vec<ActivityEvent>,String>{
    if id.is_empty()||id.len()>250 {return Err("Invalid job identity".into())}
    let mut stmt=db.prepare("SELECT kind,previous_json,current_json,recorded_at FROM application_events WHERE job_id=?1 ORDER BY event_id DESC LIMIT 100").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).map_err(|e|e.to_string())?;
    rows.map(|r|{let (kind,old,new,recorded_at)=r.map_err(|e|e.to_string())?;
      Ok(ActivityEvent{kind,previous:serde_json::from_str(&old).map_err(|e|e.to_string())?,current:serde_json::from_str(&new).map_err(|e|e.to_string())?,recorded_at})
    }).collect()
}

#[cfg(test)]
mod tests {
 use super::*;
 fn seeded()->Connection{let mut db=Connection::open_in_memory().unwrap();db.execute_batch("CREATE TABLE jobs(id TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '',company TEXT NOT NULL DEFAULT '',place TEXT NOT NULL DEFAULT '',link TEXT NOT NULL DEFAULT '',source TEXT NOT NULL DEFAULT '',retrieved_at TEXT NOT NULL DEFAULT ''); CREATE TABLE reviews(job_id TEXT PRIMARY KEY,status TEXT NOT NULL DEFAULT 'new',applied_date TEXT,follow_up_date TEXT,note TEXT NOT NULL DEFAULT '',reviewed_json TEXT NOT NULL); CREATE TABLE status_history(job_id TEXT NOT NULL, ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();migrate(&db).unwrap();db.execute("INSERT INTO jobs(id,title) VALUES ('job1','Vendeur')",[]).unwrap();db}
 fn update(status:&str)->ApplicationUpdate{ApplicationUpdate{job_id:"job1".into(),revision:0,status:status.into(),note:"note".into(),applied_date:String::new(),follow_up_date:String::new(),cv_profile:"Vente".into(),cv_file:"C:\\Documents\\cv.pdf".into(),letter_file:String::new(),application_method:"Site employeur".into(),contact:String::new(),interview_at:String::new()}}
 #[test] fn five_business_days_skip_weekend(){assert_eq!(add_weekdays(NaiveDate::from_ymd_opt(2026,10,30).unwrap(),5),NaiveDate::from_ymd_opt(2026,11,6).unwrap());}
 #[test] fn idempotent_schema(){let db=seeded();migrate(&db).unwrap();migrate(&db).unwrap();}
 #[test] fn new_to_applied_preserves_atomic_history(){let mut db=seeded();save(&mut db,update("applied")).unwrap();let page=list(&db).unwrap();assert_eq!(page.items[0].status,"applied");assert_eq!(page.items[0].cv_profile,"Vente");assert!(!page.items[0].follow_up_date.is_empty());assert_eq!(page.items[0].history.len(),1);assert_eq!(db.query_row("SELECT COUNT(*) FROM status_history",[],|r|r.get::<_,i64>(0)).unwrap(),1);}
 #[test] fn invalid_transition_does_not_write(){let mut db=seeded();assert!(save(&mut db,update("interview")).is_err());assert_eq!(db.query_row("SELECT COUNT(*) FROM reviews",[],|r|r.get::<_,i64>(0)).unwrap(),0);}
 #[test] fn stale_revision_rejected(){let mut db=seeded();save(&mut db,update("new")).unwrap();assert!(save(&mut db,update("applied")).is_err());}
 #[test] fn cannot_create_unknown_application(){let mut db=seeded();let mut p=update("new");p.job_id="missing".into();assert!(save(&mut db,p).is_err());}
 #[test] fn backdated_submission_drives_follow_up(){let mut db=seeded();let mut p=update("applied");p.applied_date="2026-10-30".into();save(&mut db,p).unwrap();let page=list(&db).unwrap();let r=&page.items[0];assert_eq!(r.applied_date,"2026-10-30");assert_eq!(r.follow_up_date,"2026-11-06");}
 #[test] fn invalid_date_does_not_write(){let mut db=seeded();let mut p=update("new");p.follow_up_date="2026-02-30".into();assert!(save(&mut db,p).is_err());}
 #[test] fn legacy_review_without_job_is_kept(){let mut db=seeded();db.execute("INSERT INTO reviews(job_id,reviewed_json) VALUES ('orphan',?1)",[json!({"status":"applied","_meta":{"title":"Archive"}}).to_string()]).unwrap();assert!(list(&db).unwrap().items.iter().any(|x|x.job_id=="orphan"&&x.title=="Archive"));}
}
