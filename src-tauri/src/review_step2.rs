//! Native review triage for staged vacancies. Independent of application status.
//! Decisions are reversible, persist in SQLite, and never merge source records.
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, TransactionBehavior};
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn migrate(db: &mut Connection) -> Result<(), String> {
    db.execute_batch(r#"
      CREATE TABLE IF NOT EXISTS review_triage (
        source TEXT NOT NULL, source_id TEXT NOT NULL,
        decision TEXT NOT NULL CHECK(decision IN ('later','skipped','approved')),
        note TEXT NOT NULL DEFAULT '',
        updated_at TEXT NOT NULL DEFAULT (datetime('now')),
        PRIMARY KEY(source,source_id),
        FOREIGN KEY(source,source_id) REFERENCES source_listings(source,source_id)
      );
      CREATE TABLE IF NOT EXISTS review_triage_events (
        event_id INTEGER PRIMARY KEY AUTOINCREMENT,
        source TEXT NOT NULL, source_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('decide','note','undo')),
        before_decision TEXT, after_decision TEXT,
        before_note TEXT NOT NULL, after_note TEXT NOT NULL,
        happened_at TEXT NOT NULL DEFAULT (datetime('now'))
      );
      CREATE INDEX IF NOT EXISTS idx_triage_events_identity
        ON review_triage_events(source,source_id,event_id);
      CREATE INDEX IF NOT EXISTS idx_triage_current_decision
        ON review_triage(decision,updated_at);
      INSERT OR IGNORE INTO schema_migrations(version) VALUES (40);
    "#).map_err(|e| e.to_string())
}

#[derive(Debug,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ReviewFilter {
    #[serde(default)] pub days_back:Option<u32>,
    #[serde(default)] pub include_unknown_dates:bool,
    pub source: Option<String>,
    pub classification: Option<String>,
    pub scope: Option<String>,
    pub decision: Option<String>,
    pub query: Option<String>,
    pub location: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ReviewItem {
    pub source:String, pub source_id:String, pub title:String, pub company:String, pub place:String,
    pub listing_url:String, pub description:String, pub published_at:Option<String>,
    pub retrieved_at:String, pub grades:Value, pub evidence:Value,
    pub classification:String, pub legacy_reason:String, pub detail_status:String,
    pub decision:String, pub note:String, pub in_review:bool, pub duplicate_count:i64,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ReviewOverview {
    pub discovered:i64, pub eligible:i64, pub unverified:i64,
    pub excluded:i64, pub priority:i64,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ReviewPage {pub items:Vec<ReviewItem>, pub total:i64, pub limit:usize, pub offset:usize, pub overview:ReviewOverview}

// Priority is a queue preference, NOT an eligibility verdict. Only title clues
// are used for unverified positions. Hard exclusions still override this.
// Keep the vocabulary intentionally conservative and reviewable in code.
const TARGET_ROLE_SQL:&str=r#"(
    lower(s.title) LIKE '%vendeur%' OR lower(s.title) LIKE '%vendeuse%' OR
    lower(s.title) LIKE '%vente%' OR lower(s.title) LIKE '%sales assistant%' OR
    lower(s.title) LIKE '%retail%' OR lower(s.title) LIKE '%caissier%' OR
    lower(s.title) LIKE '%logistiq%' OR lower(s.title) LIKE '%magasinier%' OR
    lower(s.title) LIKE '%gestionnaire de stock%' OR lower(s.title) LIKE '%gestion des stocks%' OR
    lower(s.title) LIKE '%préparateur de commande%' OR lower(s.title) LIKE '%preparateur de commande%' OR
    lower(s.title) LIKE '%préparatrice de commande%' OR lower(s.title) LIKE '%collaborateur de vente%' OR
    lower(s.title) LIKE '%employé de commerce%' OR lower(s.title) LIKE '%employe de commerce%' OR
    lower(s.title) LIKE '%assistant administratif%' OR lower(s.title) LIKE '%assistante administrative%' OR
    lower(s.title) LIKE '%administration commerciale%' OR lower(s.title) LIKE '%employé administratif%' OR
    lower(s.title) LIKE '%employée administrative%' OR lower(s.title) LIKE '%gestion des commandes%' OR
    lower(s.title) LIKE '%service client%' OR lower(s.title) LIKE '%service clientèle%' OR
    lower(s.title) LIKE '%service clientele%' OR lower(s.title) LIKE '%customer service%' OR
    lower(s.title) LIKE '%conseiller clientèle%' OR lower(s.title) LIKE '%conseillère clientèle%' OR
    lower(s.title) LIKE '%conseiller clientele%' OR lower(s.title) LIKE '%support client%' OR
    lower(s.title) LIKE '%gestionnaire clientèle%' OR lower(s.title) LIKE '%collaborateur administratif%'
)"#;


fn validate_identity(source:&str,id:&str)->Result<(),String>{
    if source.is_empty()||id.is_empty()||source.len()>80||id.len()>200{return Err("Invalid source identity".into())}
    Ok(())
}
fn validate_decision(decision:&str)->Result<(),String>{
    if !["later","skipped"].contains(&decision){return Err("Only Later or Skip can be set by triage; approval requires the native promotion command".into())}
    Ok(())
}
fn ensure_staged(db:&Connection,source:&str,id:&str)->Result<(),String>{
    validate_identity(source,id)?;
    let n:i64=db.query_row("SELECT COUNT(*) FROM source_listings WHERE source=?1 AND source_id=?2",params![source,id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if n!=1{return Err("Staged source record not found".into())}
    Ok(())
}
fn current(db:&Connection,source:&str,id:&str)->Result<(Option<String>,String),String>{
    db.query_row("SELECT decision,note FROM review_triage WHERE source=?1 AND source_id=?2",params![source,id],|r|Ok((Some(r.get(0)?),r.get(1)?)))
      .optional().map_err(|e|e.to_string()).map(|o|o.unwrap_or((None,String::new())))
}
fn set_current(db:&Connection,source:&str,id:&str,decision:Option<&str>,note:&str)->Result<(),String>{
    match decision {
        Some(d)=>{db.execute("INSERT INTO review_triage(source,source_id,decision,note) VALUES (?1,?2,?3,?4) ON CONFLICT(source,source_id) DO UPDATE SET decision=excluded.decision,note=excluded.note,updated_at=datetime('now')",params![source,id,d,note]).map_err(|e|e.to_string())?;}
        None=>{db.execute("DELETE FROM review_triage WHERE source=?1 AND source_id=?2",params![source,id]).map_err(|e|e.to_string())?;}
    }
    Ok(())
}
fn log_event(db:&Connection,source:&str,id:&str,kind:&str,before:&(Option<String>,String),after:&(Option<String>,String))->Result<(),String>{
    db.execute("INSERT INTO review_triage_events(source,source_id,kind,before_decision,after_decision,before_note,after_note) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![source,id,kind,before.0,after.0,before.1,after.1]).map_err(|e|e.to_string())?;
    Ok(())
}

/// Approved is only recorded after native promotion succeeds. Does not touch reviews/status_history.
pub fn decide(db:&mut Connection,source:&str,id:&str,decision:&str, note:Option<&str>, promoted:bool)->Result<(),String>{
    if decision=="approved" {if !promoted {return Err("Approval requires successful native promotion".into())}}
    else {validate_decision(decision)?}
    if let Some(n)=note{if n.len()>5000{return Err("Review note exceeds 5000 bytes".into())}}
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    ensure_staged(&tx,source,id)?;
    if decision=="approved" {
        let job_id=if source=="jobup"{id.to_string()}else{format!("{source}:{id}")};
        let exists:i64=tx.query_row("SELECT COUNT(*) FROM jobs WHERE id=?1",[job_id],|r|r.get(0)).map_err(|e|e.to_string())?;
        if exists!=1{return Err("Cannot mark approved before native job promotion".into())}
    }
    let before=current(&tx,source,id)?;
    if before.0.as_deref()==Some("approved") && decision!="approved" {return Err("Already approved; manage this job from Applications rather than demoting it in Review".into())}
    let after=(Some(decision.to_string()),note.unwrap_or(&before.1).to_string());
    if before!=after{set_current(&tx,source,id,after.0.as_deref(),&after.1)?;log_event(&tx,source,id,"decide",&before,&after)?;}
    tx.commit().map_err(|e|e.to_string())
}

pub fn save_note(db:&mut Connection,source:&str,id:&str,note:&str)->Result<(),String>{
    if note.len()>5000{return Err("Review note exceeds 5000 bytes".into())}
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    ensure_staged(&tx,source,id)?;
    let before=current(&tx,source,id)?;
    // Pending notes are retained as a 'later' decision to prevent erasure on reload.
    let after=(Some(before.0.clone().unwrap_or_else(||"later".into())),note.to_string());
    if before!=after{set_current(&tx,source,id,after.0.as_deref(),&after.1)?;log_event(&tx,source,id,"note",&before,&after)?;}
    tx.commit().map_err(|e|e.to_string())
}

pub fn undo(db:&mut Connection,source:&str,id:&str)->Result<String,String>{
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    ensure_staged(&tx,source,id)?;
    let last:Option<(String,Option<String>,String)>=tx.query_row(
        "SELECT kind,before_decision,before_note FROM review_triage_events WHERE source=?1 AND source_id=?2 ORDER BY event_id DESC LIMIT 1",params![source,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))
    ).optional().map_err(|e|e.to_string())?;
    let Some((kind,previous_decision,previous_note))=last else{return Err("No earlier triage decision to undo".into())};
    if kind=="undo" {return Err("Last action already undone; make a new decision first".into())}
    let before=current(&tx,source,id)?;
    if before.0.as_deref()==Some("approved") {return Err("Approval cannot be undone automatically because the job is now tracked separately; use Applications".into())}
    let after=(previous_decision,previous_note);
    set_current(&tx,source,id,after.0.as_deref(),&after.1)?;
    log_event(&tx,source,id,"undo",&before,&after)?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(after.0.unwrap_or_else(||"pending".into()))
}

#[derive(Debug,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ReviewIdentity{pub source:String,pub source_id:String}

/// Batch actions never approve or change an application. Fully transactional.
pub fn bulk_decide(db:&mut Connection,items:&[ReviewIdentity],decision:&str)->Result<usize,String>{
    validate_decision(decision)?;
    if items.is_empty()||items.len()>100{return Err("Select 1 to 100 vacancies".into())}
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let mut seen=std::collections::HashSet::new(); let mut changed=0;
    for item in items {
        ensure_staged(&tx,&item.source,&item.source_id)?;
        if !seen.insert((&item.source,&item.source_id)){continue}
        let before=current(&tx,&item.source,&item.source_id)?;
        if before.0.as_deref()==Some("approved") {return Err("Batch action refused: one vacancy was already approved".into())}
        let after=(Some(decision.to_string()),before.1.clone());
        if before!=after {
            set_current(&tx,&item.source,&item.source_id,after.0.as_deref(),&after.1)?;
            log_event(&tx,&item.source,&item.source_id,"decide",&before,&after)?;
            changed+=1;
        }
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(changed)
}

pub fn inspect(db:&Connection,filter:ReviewFilter)->Result<ReviewPage,String>{
    if filter.days_back.is_some()&&(filter.date_from.is_some()||filter.date_to.is_some()){return Err("Choisissez une période relative ou des dates personnalisées, pas les deux".into());}
    if filter.days_back.is_some_and(|days|![1,3,7,14,30].contains(&days)){return Err("Invalid publication window".into());}
    let now=chrono::Local::now();
    let cutoff=filter.days_back.map(|days|(now.with_timezone(&chrono::Utc)-chrono::Duration::days(days as i64)).to_rfc3339());
    let cutoff_date=filter.days_back.map(|days|(now.date_naive()-chrono::Duration::days(days.saturating_sub(1) as i64)).to_string());
    let limit=filter.limit.unwrap_or(50);
    let offset=filter.offset.unwrap_or(0);
    if !(1..=100).contains(&limit)||offset>1_000_000{return Err("Invalid pagination".into())}
    for s in [&filter.source,&filter.classification,&filter.decision,&filter.query,&filter.location] {
        if s.as_ref().is_some_and(|v|v.len()>250){return Err("Filter too long".into())}
    }
    if filter.classification.as_deref().is_some_and(|c|!["eligible","excluded","unverified"].contains(&c)){return Err("Invalid eligibility filter".into())}
    if filter.scope.as_deref().is_some_and(|v|!["priority","eligible","targeted_unverified","unverified","excluded","all"].contains(&v)){return Err("Invalid queue view".into())}
    if filter.decision.as_deref().is_some_and(|d|!["pending","later","skipped","approved"].contains(&d)){return Err("Invalid decision filter".into())}
    for date in [&filter.date_from,&filter.date_to] {
        if date.as_ref().is_some_and(|d|d.len()!=10 || !d.bytes().enumerate().all(|(i,c)|if i==4||i==7{c==b'-'}else{c.is_ascii_digit()})){return Err("Invalid date filter".into())}
    }
    let order=match filter.sort.as_deref(){
        Some("title")=>"s.title COLLATE NOCASE ASC,s.source_id ASC",
        Some("company")=>"s.company COLLATE NOCASE ASC,s.source_id ASC",
        Some("published")=>"s.published_at DESC,s.retrieved_at DESC,s.source_id ASC",
        _=>"s.retrieved_at DESC,s.source,s.source_id",
    };
    let base_sql=" FROM source_listings s LEFT JOIN source_eligibility e ON s.source=e.source AND s.source_id=e.source_id LEFT JOIN review_triage t ON s.source=t.source AND s.source_id=t.source_id WHERE (s.source!='efinancialcareers' OR s.listing_url GLOB '*.id[0-9]*') AND (s.source!='swissdevjobs' OR s.listing_url NOT LIKE '%/all%') AND (?1 IS NULL OR s.source=?1) AND (?2 IS NULL OR e.classification=?2) AND (?3 IS NULL OR COALESCE(t.decision,'pending')=?3) AND (?4 IS NULL OR (lower(s.title) LIKE ?4 OR lower(s.company) LIKE ?4 OR lower(COALESCE(s.description,'')) LIKE ?4)) AND (?5 IS NULL OR lower(s.place) LIKE ?5) AND (((?6 IS NULL OR substr(s.published_at,1,10)>=?6) AND (?7 IS NULL OR substr(s.published_at,1,10)<=?7) AND (?8 IS NULL OR (CASE WHEN length(s.published_at)>10 THEN datetime(s.published_at)>=datetime(?8) AND datetime(s.published_at)<=datetime('now') ELSE substr(s.published_at,1,10)>=?9 AND substr(s.published_at,1,10)<=date('now','localtime') END))) OR (?10=1 AND (s.published_at IS NULL OR datetime(s.published_at) IS NULL)))";
    // SQL fragments come only from fixed server-side variants, never arbitrary client text.
    let queue_view=match filter.scope.as_deref().unwrap_or("all") {
        "priority"=>format!(" AND (e.classification='eligible' OR (COALESCE(e.classification,'unverified')='unverified' AND {TARGET_ROLE_SQL}))"),
        "eligible"=>" AND e.classification='eligible'".into(),
        "targeted_unverified"=>format!(" AND COALESCE(e.classification,'unverified')='unverified' AND {TARGET_ROLE_SQL}"),
        "unverified"=>" AND COALESCE(e.classification,'unverified')='unverified'".into(),
        "excluded"=>" AND e.classification='excluded'".into(),
        _=>String::new(),
    };
    let where_sql=format!("{base_sql}{queue_view}");
    let pattern=|value:Option<String>|value.filter(|x|!x.trim().is_empty()).map(|v|format!("%{}%",v.trim().to_lowercase().replace('\\',"\\\\").replace('%',"\\%").replace('_',"\\_")));
    let text=pattern(filter.query);
    let place=pattern(filter.location);
    let clause=where_sql.replace("LIKE ?4","LIKE ?4 ESCAPE '\\'").replace("LIKE ?5","LIKE ?5 ESCAPE '\\'");
    let values:Vec<SqlValue>=vec![filter.source.map_or(SqlValue::Null,SqlValue::Text),filter.classification.map_or(SqlValue::Null,SqlValue::Text),filter.decision.map_or(SqlValue::Null,SqlValue::Text),text.map_or(SqlValue::Null,SqlValue::Text),place.map_or(SqlValue::Null,SqlValue::Text),filter.date_from.map_or(SqlValue::Null,SqlValue::Text),filter.date_to.map_or(SqlValue::Null,SqlValue::Text),cutoff.map_or(SqlValue::Null,SqlValue::Text),cutoff_date.map_or(SqlValue::Null,SqlValue::Text),SqlValue::Integer(if filter.include_unknown_dates{1}else{0})];
    let total:i64=db.query_row(&format!("SELECT COUNT(*) {clause}"),params_from_iter(values.iter()),|r|r.get(0)).map_err(|e|e.to_string())?;
    let ranked_order=if filter.scope.as_deref()==Some("priority") {
        format!("CASE WHEN e.classification='eligible' THEN 0 ELSE 1 END,{order}")
    }else{order.to_string()};
    let sql=format!(r#"SELECT s.source,s.source_id,s.title,s.company,s.place,s.listing_url,COALESCE(s.description,''),s.published_at,s.retrieved_at,s.grades_json,COALESCE(e.evidence_json,'[]'),COALESCE(e.classification,'unverified'),COALESCE(e.legacy_reason,''),COALESCE(e.detail_status,'unavailable'),COALESCE(t.decision,'pending'),COALESCE(t.note,''),EXISTS(SELECT 1 FROM jobs j WHERE j.id=CASE WHEN s.source='jobup' THEN s.source_id ELSE s.source||':'||s.source_id END), (SELECT COUNT(*) FROM source_listings x WHERE x.source!=s.source AND lower(x.title)=lower(s.title) AND lower(x.company)=lower(s.company) AND lower(x.place)=lower(s.place)) {clause} ORDER BY {ranked_order} LIMIT ?11 OFFSET ?12"#);
    let mut stmt=db.prepare(&sql).map_err(|e|e.to_string())?;
    let mut paged=values; paged.push(SqlValue::Integer(limit as i64));paged.push(SqlValue::Integer(offset as i64));
    let rows=stmt.query_map(params_from_iter(paged.iter()),|r|{
        let grades:String=r.get(9)?;let evidence:String=r.get(10)?;
        Ok(ReviewItem{source:r.get(0)?,source_id:r.get(1)?,title:r.get(2)?,company:r.get(3)?,place:r.get(4)?,listing_url:r.get(5)?,description:r.get(6)?,published_at:r.get(7)?,retrieved_at:r.get(8)?,grades:serde_json::from_str(&grades).unwrap_or(Value::Null),evidence:serde_json::from_str(&evidence).unwrap_or(Value::Null),classification:r.get(11)?,legacy_reason:r.get(12)?,detail_status:r.get(13)?,decision:r.get(14)?,note:r.get(15)?,in_review:r.get::<_,i64>(16)?!=0,duplicate_count:r.get(17)?})
    }).map_err(|e|e.to_string())?;
    let items=rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    // This is an honest staged-database overview (all sources/all decisions),
    // not the count of vacancies requiring a manual decision in the active view.
    let overview_sql=format!("SELECT COUNT(*), SUM(CASE WHEN e.classification='eligible' THEN 1 ELSE 0 END), SUM(CASE WHEN COALESCE(e.classification,'unverified')='unverified' THEN 1 ELSE 0 END), SUM(CASE WHEN e.classification='excluded' THEN 1 ELSE 0 END), SUM(CASE WHEN e.classification='eligible' OR (COALESCE(e.classification,'unverified')='unverified' AND {TARGET_ROLE_SQL}) THEN 1 ELSE 0 END) FROM source_listings s LEFT JOIN source_eligibility e ON s.source=e.source AND s.source_id=e.source_id WHERE (s.source!='efinancialcareers' OR s.listing_url GLOB '*.id[0-9]*') AND (s.source!='swissdevjobs' OR s.listing_url NOT LIKE '%/all%')");
    let overview=db.query_row(&overview_sql,[],|r|Ok(ReviewOverview{
        discovered:r.get(0)?,eligible:r.get::<_,Option<i64>>(1)?.unwrap_or(0),
        unverified:r.get::<_,Option<i64>>(2)?.unwrap_or(0),
        excluded:r.get::<_,Option<i64>>(3)?.unwrap_or(0),
        priority:r.get::<_,Option<i64>>(4)?.unwrap_or(0)
    })).map_err(|e|e.to_string())?;
    Ok(ReviewPage{items,total,limit,offset,overview})
}

#[cfg(test)]
mod tests{
    use super::*;
    fn db()->Connection{
        let mut c=Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY);CREATE TABLE reviews(job_id TEXT PRIMARY KEY);CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
        crate::source_store::migrate(&mut c).unwrap(); migrate(&mut c).unwrap();
        c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,description,published_at,original_json) VALUES ('jobup','id1','Vendeur','Boutique','Sion','https://example.com/1','[80]','Accueil clientèle','2026-10-25','{}')",[]).unwrap();c
    }
    fn f()->ReviewFilter{ReviewFilter{days_back:None,include_unknown_dates:false,source:None,classification:None,scope:None,decision:None,query:None,location:None,date_from:None,date_to:None,sort:None,limit:None,offset:None}}
    #[test]fn migrating_twice_is_safe(){let mut c=db();migrate(&mut c).unwrap();assert_eq!(inspect(&c,f()).unwrap().total,1);}
    #[test]fn skip_later_undo_persists(){let mut c=db();decide(&mut c,"jobup","id1","skipped",None,false).unwrap();decide(&mut c,"jobup","id1","later",None,false).unwrap();assert_eq!(inspect(&c,f()).unwrap().items[0].decision,"later");undo(&mut c,"jobup","id1").unwrap();assert_eq!(inspect(&c,f()).unwrap().items[0].decision,"skipped");assert!(undo(&mut c,"jobup","id1").is_err());}
    #[test]fn approved_undo_is_protected(){let mut c=db();c.execute("INSERT INTO jobs(id) VALUES ('id1')",[]).unwrap();decide(&mut c,"jobup","id1","approved",None,true).unwrap();assert!(undo(&mut c,"jobup","id1").is_err());assert!(decide(&mut c,"jobup","id1","skipped",None,false).is_err());}
    #[test]fn invalid_approval_and_missing_record_fail(){let mut c=db();assert!(decide(&mut c,"jobup","id1","approved",None,false).is_err());assert!(decide(&mut c,"jobup","missing","skipped",None,false).is_err());}
    #[test]fn batch_is_atomic(){let mut c=db();let ids=[ReviewIdentity{source:"jobup".into(),source_id:"id1".into()},ReviewIdentity{source:"jobup".into(),source_id:"absent".into()}];assert!(bulk_decide(&mut c,&ids,"skipped").is_err());assert_eq!(inspect(&c,f()).unwrap().items[0].decision,"pending");}
    #[test]fn notes_and_filters(){let mut c=db();save_note(&mut c,"jobup","id1","À appeler").unwrap();let mut q=f();q.query=Some("clientèle".into());assert_eq!(inspect(&c,q).unwrap().total,1);assert_eq!(inspect(&c,f()).unwrap().items[0].note,"À appeler");}
    #[test]fn duplicate_warning_does_not_merge(){let mut c=db();c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,original_json) VALUES ('jobs_ch','id2','Vendeur','Boutique','Sion','https://example.com/2','[]','{}')",[]).unwrap();assert_eq!(inspect(&c,f()).unwrap().items[0].duplicate_count,1);}
    #[test]fn default_priority_hides_irrelevant_and_excluded_but_retains_them_in_all(){
        let mut c=db();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('jobup','id1','unverified','unavailable','pending','[]','[]')",[]).unwrap();
        c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,original_json) VALUES ('jobs_ch','med1','Auxiliaire de Santé / Aide-Soignant','Clinique','Mont-sur-Rolle','https://example.com/med','[]','{}')",[]).unwrap();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('jobs_ch','med1','unverified','unavailable','pending','[]','[]')",[]).unwrap();
        c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,original_json) VALUES ('jobs_ch','med2','Médecin urgentiste','Clinique','Lausanne','https://example.com/med2','[]','{}')",[]).unwrap();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('jobs_ch','med2','excluded','description_found','Diplôme de médecine obligatoire','[]','[]')",[]).unwrap();
        let mut query=f();query.scope=Some("priority".into());let result=inspect(&c,query).unwrap();
        assert_eq!(result.total,1);assert_eq!(result.items[0].title,"Vendeur");
        assert_eq!(result.overview.discovered,3);assert_eq!(result.overview.priority,1);
        let mut audit=f();audit.scope=Some("all".into());assert_eq!(inspect(&c,audit).unwrap().total,3);
        let mut unverified=f();unverified.scope=Some("unverified".into());assert_eq!(inspect(&c,unverified).unwrap().total,2);
    }
    #[test]fn priority_counts_eligible_even_if_title_is_not_in_target_family(){
        let mut c=db();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('jobup','id1','eligible','description_found','ok','[]','[]')",[]).unwrap();
        let mut query=f();query.scope=Some("priority".into());
        assert_eq!(inspect(&c,query).unwrap().total,1);
    }
    #[test]fn rejects_invalid_scope(){let c=db();let mut query=f();query.scope=Some("' OR 1=1 --".into());assert!(inspect(&c,query).is_err());}

    #[test]fn one_day_review_never_uses_fresh_collection_for_old_publication(){
        let c=db();c.execute("UPDATE source_listings SET published_at='2026-09-01',retrieved_at=datetime('now')",[]).unwrap();
        let mut filter=f();filter.days_back=Some(1);filter.include_unknown_dates=true;
        assert_eq!(inspect(&c,filter).unwrap().total,0);
        c.execute("UPDATE source_listings SET published_at=date('now','localtime')",[]).unwrap();
        let mut filter=f();filter.days_back=Some(1);assert_eq!(inspect(&c,filter).unwrap().total,1);
        c.execute("UPDATE source_listings SET published_at=NULL",[]).unwrap();
        let mut filter=f();filter.days_back=Some(1);assert_eq!(inspect(&c,filter).unwrap().total,0);
        let mut filter=f();filter.days_back=Some(1);filter.include_unknown_dates=true;assert_eq!(inspect(&c,filter).unwrap().total,1);
    }

}
