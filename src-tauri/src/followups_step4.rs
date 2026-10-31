//! Step 4: Manual follow-up action queue and calendar. No email is sent.
//! Reviews/status_history (Step 3) remain the authority for application status.
use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Settings { pub delay_days: u32, pub delay_mode: String }

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct FollowupRow {
    pub job_id: String,
    pub title: String,
    pub company: String,
    pub place: String,
    pub listing_url: String,
    pub status: String,
    pub applied_date: String,
    pub source_due_date: String,
    pub due_date: String,
    pub state: String,
    pub interview_at: String,
    pub contact: String,
    pub revision: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all="camelCase")]
pub struct Summary {
    pub due_today: usize,
    pub overdue: usize,
    pub upcoming: usize,
    pub completed: usize,
    pub dismissed: usize,
    pub paused_after_reply: usize,
    pub interviews: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct FollowupPage { pub items: Vec<FollowupRow>, pub summary: Summary, pub settings: Settings, pub truncated: bool }

#[derive(Debug, Deserialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct Action { pub job_id: String, pub expected_revision: i64, pub action: String, pub due_date: Option<String> }

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct Event { pub action: String, pub previous: Value, pub current: Value, pub recorded_at: String }

fn valid_day(s:&str)->Result<NaiveDate,String> {
    if s.len()!=10 {return Err("Expected date YYYY-MM-DD".into())}
    NaiveDate::parse_from_str(s,"%Y-%m-%d").map_err(|_|"Invalid calendar date".into())
}
fn business_plus(mut day: NaiveDate, mut days: u32) -> NaiveDate {
    while days>0 {
        day += Duration::days(1);
        if day.weekday().number_from_monday()<=5 {days-=1;}
    }
    day
}
fn derived_due(applied:&str, settings:&Settings)->Result<String,String>{
    if applied.is_empty(){return Ok(String::new())}
    let date=valid_day(applied)?;
    let next=if settings.delay_mode=="calendar" {date+Duration::days(i64::from(settings.delay_days))}
       else {business_plus(date,settings.delay_days)};
    Ok(next.format("%Y-%m-%d").to_string())
}

pub fn migrate(db:&Connection)->Result<(),String>{
    db.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS followup_settings (
          key TEXT PRIMARY KEY, value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS followup_tasks (
          job_id TEXT PRIMARY KEY,
          source_due_date TEXT NOT NULL,
          due_date TEXT NOT NULL,
          state TEXT NOT NULL CHECK(state IN ('open','completed','dismissed')),
          revision INTEGER NOT NULL DEFAULT 1,
          updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS followup_events (
          event_id INTEGER PRIMARY KEY AUTOINCREMENT,
          job_id TEXT NOT NULL, action TEXT NOT NULL,
          previous_json TEXT NOT NULL, current_json TEXT NOT NULL,
          recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS followup_events_job ON followup_events(job_id,event_id);
    "#).map_err(|e|e.to_string())
}

pub fn settings(db:&Connection)->Result<Settings,String>{
    let get=|key:&str|->Result<Option<String>,String>{db.query_row("SELECT value FROM followup_settings WHERE key=?1",[key],|r|r.get::<_,String>(0)).optional().map_err(|e|e.to_string())};
    let days=get("delay_days")?.and_then(|v|v.parse::<u32>().ok()).filter(|n|(1..=30).contains(n)).unwrap_or(5);
    let mode=match get("delay_mode")?.as_deref(){Some("calendar")=>"calendar",_=>"business"};
    Ok(Settings{delay_days:days,delay_mode:mode.to_string()})
}

pub fn save_settings(db:&mut Connection, s:Settings)->Result<Settings,String>{
    if !(1..=30).contains(&s.delay_days) || !["business","calendar"].contains(&s.delay_mode.as_str()) {return Err("Invalid follow-up delay (1–30 days, business or calendar)".into())}
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    for (k,v) in [("delay_days",s.delay_days.to_string()),("delay_mode",s.delay_mode.clone())] {
        tx.execute("INSERT INTO followup_settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![k,v]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(s)
}

#[derive(Debug)]
struct Stored { source_due:String, due:String, state:String, revision:i64 }
fn stored(db:&Connection,id:&str)->Result<Option<Stored>,String>{
    db.query_row("SELECT source_due_date,due_date,state,revision FROM followup_tasks WHERE job_id=?1",[id],|r|Ok(Stored{source_due:r.get(0)?,due:r.get(1)?,state:r.get(2)?,revision:r.get(3)?})).optional().map_err(|e|e.to_string())
}
fn effective_base(date:&str, applied:&str, settings:&Settings)->Result<String,String>{
    if !date.is_empty() {valid_day(date)?;return Ok(date.to_string())}
    derived_due(applied,settings)
}
fn row_from(db:&Connection,app:&crate::applications_step3::ApplicationRow,s:&Settings)->Result<FollowupRow,String>{
    let base=effective_base(&app.follow_up_date,&app.applied_date,s)?;
    let plan=stored(db,&app.job_id)?.filter(|v|v.source_due==base);
    let (due,state,revision)=if let Some(p)=plan {(p.due,p.state,p.revision)}else{(base.clone(),"open".into(),0)};
    // A reply, interview, offer or final decision suppresses pending reminders.
    let state=if app.status!="applied" {"paused".to_string()} else if due.is_empty(){"unscheduled".to_string()} else {state};
    Ok(FollowupRow{job_id:app.job_id.clone(),title:app.title.clone(),company:app.company.clone(),place:app.place.clone(),listing_url:app.listing_url.clone(),status:app.status.clone(),applied_date:app.applied_date.clone(),source_due_date:base,due_date:due,state,interview_at:app.interview_at.clone(),contact:app.contact.clone(),revision})
}

pub fn list(db:&Connection)->Result<FollowupPage,String>{
    let applications=crate::applications_step3::list(db)?;
    let settings=settings(db)?;
    let mut items=Vec::new();let mut summary=Summary::default();
    let today=Local::now().date_naive().format("%Y-%m-%d").to_string();
    for app in &applications.items {
        if !["applied","screening","interview","offer","rejected","withdrawn"].contains(&app.status.as_str()){continue}
        let row=row_from(db,app,&settings)?;
        if !row.interview_at.is_empty(){summary.interviews+=1;}
        match row.state.as_str(){
            "completed"=>summary.completed+=1,
            "dismissed"=>summary.dismissed+=1,
            "paused"=>summary.paused_after_reply+=1,
            "open" if row.due_date<today=>summary.overdue+=1,
            "open" if row.due_date==today=>summary.due_today+=1,
            "open" if !row.due_date.is_empty()=>summary.upcoming+=1,
            _=>(),
        }
        items.push(row);
    }
    items.sort_by(|a,b|{
        let category=|v:&FollowupRow|match v.state.as_str(){"open"=>0,"unscheduled"=>1,"completed"=>2,"dismissed"=>3,_=>4};
        category(a).cmp(&category(b)).then_with(||a.due_date.cmp(&b.due_date)).then_with(||a.title.cmp(&b.title))
    });
    Ok(FollowupPage{items,summary,settings,truncated:applications.truncated})
}

pub fn apply_action(db:&mut Connection, action:Action)->Result<(),String>{
    if action.job_id.is_empty()||action.job_id.len()>250||action.expected_revision<0{return Err("Invalid job or revision".into())}
    if !["complete","dismiss","postpone","reopen"].contains(&action.action.as_str()){return Err("Unknown follow-up action".into())}
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let apps=crate::applications_step3::list(&tx)?;
    let app=apps.items.iter().find(|v|v.job_id==action.job_id).ok_or("Application not found")?;
    if app.status!="applied"{return Err("Employer has replied or application is no longer awaiting a reply; reminder suppressed".into())}
    let opts=settings(&tx)?;
    let base=effective_base(&app.follow_up_date,&app.applied_date,&opts)?;
    if base.is_empty(){return Err("First record an application submission date or follow-up date".into())}
    // A new source due date (edited in Applications or derived from revised settings)
    // starts a fresh action cycle. Old events remain in the append-only log.
    let prior=stored(&tx,&action.job_id)?.filter(|v|v.source_due==base);
    let rev=prior.as_ref().map(|v|v.revision).unwrap_or(0);
    if rev!=action.expected_revision{return Err("Follow-up changed elsewhere. Reload before saving".into())}
    let old_state=prior.as_ref().map(|v|v.state.as_str()).unwrap_or("open");
    let old_due=prior.as_ref().map(|v|v.due.as_str()).unwrap_or(&base);
    if (action.action=="complete"||action.action=="dismiss") && old_state!="open"{return Err("Reminder is already resolved. Reopen it first".into())}
    let (state,due)=match action.action.as_str(){
        "complete"=>("completed",old_due.to_string()),
        "dismiss"=>("dismissed",old_due.to_string()),
        "postpone"|"reopen"=>{
            let proposed=action.due_date.as_deref().filter(|v|!v.is_empty()).unwrap_or(old_due);
            let _=valid_day(proposed)?;
            if action.action=="postpone" && proposed<=old_due {return Err("Postponed date must be later than the current due date".into())}
            ("open",proposed.to_string())
        },
        _=>unreachable!(),
    };
    tx.execute("INSERT INTO followup_tasks(job_id,source_due_date,due_date,state,revision) VALUES (?1,?2,?3,?4,1) ON CONFLICT(job_id) DO UPDATE SET source_due_date=excluded.source_due_date,due_date=excluded.due_date,state=excluded.state,revision=followup_tasks.revision+1,updated_at=datetime('now')",params![action.job_id,base,due,state]).map_err(|e|e.to_string())?;
    let before=json!({"state":old_state,"dueDate":old_due});
    let after=json!({"state":state,"dueDate":due});
    tx.execute("INSERT INTO followup_events(job_id,action,previous_json,current_json) VALUES (?1,?2,?3,?4)",params![action.job_id,action.action,before.to_string(),after.to_string()]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())
}

pub fn events(db:&Connection,id:&str)->Result<Vec<Event>,String>{
    if id.is_empty()||id.len()>250{return Err("Invalid job ID".into())}
    let mut stmt=db.prepare("SELECT action,previous_json,current_json,recorded_at FROM followup_events WHERE job_id=?1 ORDER BY event_id DESC LIMIT 100").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).map_err(|e|e.to_string())?;
    rows.map(|r|{let (action,old,new,recorded_at)=r.map_err(|e|e.to_string())?;
        Ok(Event{action,previous:serde_json::from_str(&old).map_err(|e|e.to_string())?,current:serde_json::from_str(&new).map_err(|e|e.to_string())?,recorded_at})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup()->Connection {
        let db=Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE jobs(id TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '',company TEXT NOT NULL DEFAULT '',place TEXT NOT NULL DEFAULT '',link TEXT NOT NULL DEFAULT '',source TEXT NOT NULL DEFAULT '',retrieved_at TEXT NOT NULL DEFAULT ''); CREATE TABLE reviews(job_id TEXT PRIMARY KEY,status TEXT NOT NULL DEFAULT 'new',applied_date TEXT,follow_up_date TEXT,note TEXT NOT NULL DEFAULT '',reviewed_json TEXT NOT NULL); CREATE TABLE status_history(job_id TEXT NOT NULL, ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
        crate::applications_step3::migrate(&db).unwrap();migrate(&db).unwrap();
        db.execute("INSERT INTO jobs(id,title,company) VALUES ('j1','Vendeur','Coop')",[]).unwrap();
        db.execute("INSERT INTO reviews(job_id,status,reviewed_json) VALUES ('j1','applied',?1)",[r#"{"status":"applied","appliedDate":"2026-10-26","followUpDate":"2026-11-02"}"#]).unwrap();db
    }
    #[test]fn skips_weekends(){let s=Settings{delay_days:5,delay_mode:"business".into()};assert_eq!(derived_due("2026-10-30",&s).unwrap(),"2026-11-06");}
    #[test]fn respects_calendar_days(){let s=Settings{delay_days:7,delay_mode:"calendar".into()};assert_eq!(derived_due("2026-10-30",&s).unwrap(),"2026-11-06");}
    #[test]fn migration_repeated(){let db=setup();migrate(&db).unwrap();}
    #[test]fn lists_applied_date_without_fabrication(){let db=setup();let page=list(&db).unwrap();assert_eq!(page.items.len(),1);assert_eq!(page.items[0].due_date,"2026-11-02");}
    #[test]fn complete_does_not_change_review_status(){let mut db=setup();apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:0,action:"complete".into(),due_date:None}).unwrap();assert_eq!(list(&db).unwrap().items[0].state,"completed");let status:String=db.query_row("SELECT status FROM reviews WHERE job_id='j1'",[],|r|r.get(0)).unwrap();assert_eq!(status,"applied");}
    #[test]fn postponement_is_reversible_and_logged(){let mut db=setup();apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:0,action:"postpone".into(),due_date:Some("2026-11-10".into())}).unwrap();let page=list(&db).unwrap();assert_eq!(page.items[0].due_date,"2026-11-10");assert_eq!(page.items[0].revision,1);apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:1,action:"complete".into(),due_date:None}).unwrap();apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:2,action:"reopen".into(),due_date:Some("2026-11-11".into())}).unwrap();assert_eq!(list(&db).unwrap().items[0].state,"open");let count:i64=db.query_row("SELECT COUNT(*) FROM followup_events",[],|r|r.get(0)).unwrap();assert_eq!(count,3);}
    #[test]fn reply_suppresses_reminder(){let db=setup();db.execute("UPDATE reviews SET status='screening',reviewed_json=?1 WHERE job_id='j1'",[r#"{"status":"screening","appliedDate":"2026-10-26","followUpDate":"2026-11-02"}"#]).unwrap();let page=list(&db).unwrap();assert_eq!(page.items[0].state,"paused");assert_eq!(page.summary.due_today+page.summary.overdue+page.summary.upcoming,0);}
    #[test]fn changed_application_due_starts_new_cycle(){let mut db=setup();apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:0,action:"complete".into(),due_date:None}).unwrap();db.execute("UPDATE reviews SET reviewed_json=?1 WHERE job_id='j1'",[r#"{"status":"applied","appliedDate":"2026-10-26","followUpDate":"2026-11-12"}"#]).unwrap();let row=list(&db).unwrap().items.remove(0);assert_eq!(row.state,"open");assert_eq!(row.revision,0);apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:0,action:"postpone".into(),due_date:Some("2026-11-13".into())}).unwrap();assert_eq!(list(&db).unwrap().items[0].due_date,"2026-11-13");}
    #[test]fn rejects_stale_revision(){let mut db=setup();apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:0,action:"dismiss".into(),due_date:None}).unwrap();assert!(apply_action(&mut db,Action{job_id:"j1".into(),expected_revision:0,action:"complete".into(),due_date:None}).is_err());}
}
