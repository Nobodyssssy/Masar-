//! Additive storage for provider advertisements; not yet wired to live fetching.
//! Reviews and legacy job IDs retain their existing ownership.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use crate::job_identity::{SourceJob, compare};
use crate::pipeline_preview::PipelinePreview;
use serde::Serialize;
use serde_json::Value;

pub fn migrate(db: &mut Connection) -> Result<(), String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(r#"
       CREATE TABLE IF NOT EXISTS source_listings (
          source TEXT NOT NULL, source_id TEXT NOT NULL,
          title TEXT NOT NULL, company TEXT NOT NULL, place TEXT NOT NULL,
          listing_url TEXT NOT NULL, employer_application_url TEXT,
          grades_json TEXT NOT NULL, description TEXT, published_at TEXT,
          original_json TEXT NOT NULL, retrieved_at TEXT NOT NULL DEFAULT (datetime('now')),
          PRIMARY KEY(source,source_id)
       );
       CREATE TABLE IF NOT EXISTS opportunity_links (
          source TEXT NOT NULL, source_id TEXT NOT NULL,
          opportunity_key TEXT NOT NULL,
          link_reason TEXT NOT NULL,
          PRIMARY KEY(source,source_id),
          FOREIGN KEY(source,source_id) REFERENCES source_listings(source,source_id)
       );
       CREATE INDEX IF NOT EXISTS idx_source_listings_company_place ON source_listings(company,place);
       CREATE INDEX IF NOT EXISTS idx_opportunity_links_key ON opportunity_links(opportunity_key);
       INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);
    "#).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    let tx = db.transaction().map_err(|e|e.to_string())?;
    tx.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS source_listing_revisions (
            source TEXT NOT NULL, source_id TEXT NOT NULL, revision_hash TEXT NOT NULL,
            snapshot_json TEXT NOT NULL, recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(source,source_id,revision_hash)
        );
        CREATE TABLE IF NOT EXISTS source_eligibility (
            source TEXT NOT NULL, source_id TEXT NOT NULL,
            classification TEXT NOT NULL CHECK(classification IN ('eligible','excluded','unverified')),
            detail_status TEXT NOT NULL, legacy_reason TEXT NOT NULL,
            evidence_json TEXT NOT NULL, notes_json TEXT NOT NULL,
            evaluated_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY(source,source_id),
            FOREIGN KEY(source,source_id) REFERENCES source_listings(source,source_id)
        );
        INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
    "#).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    crate::listing_freshness::migrate(db)
}

/// Explicit import for pre-validated provider records, not invoked from the UI.
/// Never alters `jobs`, `reviews`, or `status_history`.
pub fn store_listing(db: &mut Connection, job: &SourceJob) -> Result<bool, String> {
    if job.source.trim().is_empty() || job.source_id.trim().is_empty() {
        return Err("Nonempty source and source ID required".into());
    }
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let old:Option<String>=tx.query_row("SELECT original_json FROM source_listings WHERE source=? AND source_id=?", params![job.source,job.source_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    let raw=serde_json::to_string(job).map_err(|e|e.to_string())?;
    if old.as_deref()==Some(raw.as_str()) {tx.commit().map_err(|e|e.to_string())?;return Ok(false)}
    if old.is_some() {
      // Preserve the earlier snapshot until versioned revisions are implemented.
      return Err("Listing identity already exists with different content; revision is not discarded".into());
    }
    tx.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,employer_application_url,grades_json,description,published_at,original_json) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
      params![job.source,job.source_id,job.title,job.company,job.place,job.url,job.employer_application_url,serde_json::to_string(&job.grades).map_err(|e|e.to_string())?,job.description,job.published_at,raw]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(true)
}

pub fn link_confirmed(db:&mut Connection, a:&SourceJob,b:&SourceJob, opportunity_key:&str)->Result<(),String>{
    if opportunity_key.trim().is_empty(){return Err("Missing opportunity key".into())}
    let relation=compare(a,b);
    if !matches!(relation.relation.as_str(), "same_listing"|"same_opportunity") {return Err(format!("Cannot auto-link: {}",relation.relation))}
    let tx=db.transaction().map_err(|e|e.to_string())?;
    for j in [a,b] {
       let exists:i64=tx.query_row("SELECT COUNT(*) FROM source_listings WHERE source=? AND source_id=?",params![j.source,j.source_id],|r|r.get(0)).map_err(|e|e.to_string())?;
       if exists!=1{return Err("Both source records must exist before linking".into())}
       let other:Option<String>=tx.query_row("SELECT opportunity_key FROM opportunity_links WHERE source=? AND source_id=?",params![j.source,j.source_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
       if other.as_deref().is_some_and(|key|key!=opportunity_key){return Err("Source already belongs to another opportunity".into())}
       tx.execute("INSERT OR IGNORE INTO opportunity_links(source,source_id,opportunity_key,link_reason) VALUES (?,?,?,?)",params![j.source,j.source_id,opportunity_key,relation.reason]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e|e.to_string())
}


#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct BatchReport {
    pub inspected: usize,
    pub inserted: usize,
    pub unchanged: usize,
    pub revised: usize,
    pub eligible: usize,
    pub excluded: usize,
    pub unverified: usize,
    pub reviewed_records_touched: usize,
    pub automatic_links_created: usize,
    pub details_requested: usize,
    pub descriptions_reused: usize,
    pub detail_failures: usize,
}

/// Cached details are loaded read-only and trusted only for matching source metadata.
/// The cache is an optimization, not a replacement for required detail verification.
pub fn cached_descriptions(db:&Connection)->Result<std::collections::HashMap<String,(crate::job_identity::SourceJob,String)>,String>{
    // Revalidate after seven days, even if Jobup search metadata is unchanged.
    // Using SQLite datetime('now') handles the app's local database timestamps
    // without relying on the operating system locale.
    let mut stmt=db.prepare("SELECT source_id,original_json,description FROM source_listings WHERE source='jobup' AND description IS NOT NULL AND length(description)>0 AND datetime(retrieved_at) >= datetime('now','-7 days')")
       .map_err(|e|e.to_string())?;
    let rows=stmt.query_map([],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))
       .map_err(|e|e.to_string())?;
    let mut result=std::collections::HashMap::new();
    for row in rows {
       let (id,raw,description)=row.map_err(|e|e.to_string())?;
       if let Ok(job)=serde_json::from_str::<crate::job_identity::SourceJob>(&raw){
           result.insert(id,(job,description));
       }
    }
    Ok(result)
}

/// A single explicit commit of a successfully fetched preview batch. No writes to
/// the legacy `jobs`, `reviews`, `seen` or `status_history` tables.
/// A partial search is never accepted, and every item is committed atomically.
pub fn ingest_preview(db: &mut Connection, preview: &PipelinePreview) -> Result<BatchReport,String> {
    if !preview.fetch_complete || !preview.warnings.is_empty() {
        return Err("Refusing to store a partial or warning-bearing fetch".into());
    }
    if preview.items.is_empty() || preview.items.len()>5000 || preview.inspected!=preview.items.len() {
        return Err("Invalid preview batch size".into());
    }
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let mut out=BatchReport{inspected:preview.items.len(),inserted:0,unchanged:0,revised:0,
        eligible:0,excluded:0,unverified:0,reviewed_records_touched:0,automatic_links_created:0,
        details_requested:preview.details_requested,descriptions_reused:preview.descriptions_reused,detail_failures:preview.detail_failures};
    for item in &preview.items {
        let job=&item.source_job;
        if job.source!="jobup" || job.source_id.trim().is_empty() || job.source_id!=item.source_id {
            return Err("Invalid provider identity in preview".into());
        }
        if !["eligible","excluded","unverified"].contains(&item.classification.as_str()) {
            return Err("Invalid eligibility classification".into());
        }
        let raw=serde_json::to_string(&item.raw_job).map_err(|e|e.to_string())?;
        let job_json=serde_json::to_string(job).map_err(|e|e.to_string())?;
        let hash=blake3::hash(job_json.as_bytes()).to_hex().to_string();
        let existing:Option<String>=tx.query_row(
            "SELECT original_json FROM source_listings WHERE source=? AND source_id=?",
            params![job.source,job.source_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let prior:Option<Value>=existing.as_ref().map(|text|serde_json::from_str(text))
            .transpose().map_err(|e|format!("Existing listing JSON invalid: {e}"))?;
        let current:Value=serde_json::from_str(&job_json).map_err(|e|e.to_string())?;
        if prior.as_ref()==Some(&current) {
            out.unchanged+=1;
            // Successful network rechecks extend cache freshness even when the
            // description bytes and listing metadata did not change.
            if item.detail_status=="description_found" && !item.detail_warning.as_deref().unwrap_or("").starts_with("Reused recent stored description") {
                tx.execute("UPDATE source_listings SET retrieved_at=datetime('now') WHERE source=?1 AND source_id=?2",
                    params![job.source,job.source_id]).map_err(|e|e.to_string())?;
            }
        }
        else {
            if existing.is_some() {out.revised+=1;}
            else {out.inserted+=1;}
            tx.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,employer_application_url,grades_json,description,published_at,original_json) VALUES (?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(source,source_id) DO UPDATE SET title=excluded.title,company=excluded.company,place=excluded.place,listing_url=excluded.listing_url,employer_application_url=excluded.employer_application_url,grades_json=excluded.grades_json,description=COALESCE(excluded.description,source_listings.description),published_at=excluded.published_at,original_json=excluded.original_json,retrieved_at=datetime('now')",
                params![job.source,job.source_id,job.title,job.company,job.place,job.url,
                    job.employer_application_url,serde_json::to_string(&job.grades).map_err(|e|e.to_string())?,
                    job.description,job.published_at,job_json]).map_err(|e|e.to_string())?;
        }
        // Keep original and refreshed snapshots; repeat runs do not duplicate a revision.
        tx.execute("INSERT OR IGNORE INTO source_listing_revisions(source,source_id,revision_hash,snapshot_json) VALUES (?,?,?,?)",
            params![job.source,job.source_id,hash,raw]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES (?,?,?,?,?,?,?) ON CONFLICT(source,source_id) DO UPDATE SET classification=excluded.classification,detail_status=excluded.detail_status,legacy_reason=excluded.legacy_reason,evidence_json=excluded.evidence_json,notes_json=excluded.notes_json,evaluated_at=datetime('now')",
            params![job.source,job.source_id,item.classification,item.detail_status,item.legacy_reason,
                serde_json::to_string(&item.evidence).map_err(|e|e.to_string())?,
                serde_json::to_string(&item.notes).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        match item.classification.as_str(){"eligible"=>out.eligible+=1,"excluded"=>out.excluded+=1,_=>out.unverified+=1}
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(out)
}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct StagedListing {
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub company: String,
    pub place: String,
    pub listing_url: String,
    pub classification: Option<String>,
    pub detail_status: Option<String>,
    pub legacy_reason: Option<String>,
    pub evidence: Value,
    pub retrieved_at: String,
}

/// Only reports staged provider records; never promotes them into `jobs` or `reviews`.
pub fn inspect_staged(db:&Connection,limit:usize,offset:usize,classification:Option<&str>)->Result<Vec<StagedListing>,String>{
    if !(1..=500).contains(&limit) || offset>1_000_000 {return Err("Invalid staging pagination".into())}
    if classification.is_some_and(|c| !["eligible","excluded","unverified"].contains(&c)) {return Err("Invalid classification".into())}
    let mut stmt=db.prepare("SELECT s.source,s.source_id,s.title,s.company,s.place,s.listing_url,e.classification,e.detail_status,e.legacy_reason,COALESCE(e.evidence_json,'[]'),s.retrieved_at FROM source_listings s LEFT JOIN source_eligibility e ON s.source=e.source AND s.source_id=e.source_id WHERE (?1 IS NULL OR e.classification=?1) ORDER BY s.retrieved_at DESC,s.source,s.source_id LIMIT ?2 OFFSET ?3").map_err(|e|e.to_string())?;
    let rows=stmt.query_map(params![classification,limit as i64,offset as i64],|r|{
        let evidence_text:String=r.get(9)?;
        Ok(StagedListing{source:r.get(0)?,source_id:r.get(1)?,title:r.get(2)?,company:r.get(3)?,place:r.get(4)?,listing_url:r.get(5)?,classification:r.get(6)?,detail_status:r.get(7)?,legacy_reason:r.get(8)?,evidence:serde_json::from_str(&evidence_text).unwrap_or(Value::Null),retrieved_at:r.get(10)?})
    }).map_err(|e|e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}

/// Review candidates from staged ads. Never inserts into `jobs` or `reviews`.
/// Only a full description together with an eligible classification can appear.
#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct CandidateQueueItem {
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub company: String,
    pub place: String,
    pub listing_url: String,
    pub grades: Value,
    pub evidence: Value,
    pub notes: Value,
    pub retrieved_at: String,
    pub classification: String,
    pub description_available: bool,
    pub already_in_review_queue: bool,
}

pub fn candidate_queue(db:&Connection, limit:usize, offset:usize, include_unverified:bool) -> Result<Vec<CandidateQueueItem>,String> {
    if !(1..=100).contains(&limit) || offset > 1_000_000 { return Err("Invalid queue pagination".into()); }
    let mut stmt=db.prepare(r#"
        SELECT s.source,s.source_id,s.title,s.company,s.place,s.listing_url,
               s.grades_json,e.evidence_json,e.notes_json,s.retrieved_at,e.classification,
               EXISTS(SELECT 1 FROM jobs j WHERE j.id=(CASE WHEN s.source='jobup' THEN s.source_id ELSE s.source||':'||s.source_id END))
        FROM source_listings s
        JOIN source_eligibility e ON e.source=s.source AND e.source_id=s.source_id
        WHERE (e.classification='eligible' OR (?3=1 AND e.classification='unverified'))
          AND e.detail_status='description_found'
          AND s.description IS NOT NULL AND length(trim(s.description))>0
        ORDER BY CASE e.classification WHEN 'eligible' THEN 0 ELSE 1 END,
                 s.retrieved_at DESC, s.source, s.source_id
        LIMIT ?1 OFFSET ?2
    "#).map_err(|e|e.to_string())?;
    let rows=stmt.query_map(params![limit as i64,offset as i64, if include_unverified {1} else {0}],|r|{
        let grades:String=r.get(6)?;
        let evidence:String=r.get(7)?;
        let notes:String=r.get(8)?;
        Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
            r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,
            grades,evidence,notes,r.get::<_,String>(9)?,r.get::<_,String>(10)?,r.get::<_,i64>(11)?!=0))
    }).map_err(|e|e.to_string())?;
    let mut output=Vec::new();
    for row in rows {
        let (source,source_id,title,company,place,listing_url,grades,evidence,notes,retrieved_at,classification,already)=row.map_err(|e|e.to_string())?;
        output.push(CandidateQueueItem{source,source_id,title,company,place,listing_url,
            grades:serde_json::from_str(&grades).map_err(|e|e.to_string())?,
            evidence:serde_json::from_str(&evidence).map_err(|e|e.to_string())?,
            notes:serde_json::from_str(&notes).map_err(|e|e.to_string())?,
            retrieved_at,classification,description_available:true,already_in_review_queue:already});
    }
    Ok(output)
}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct PromotionReport { pub job_id:String, pub inserted:bool, pub review_records_touched:usize, pub classification:String }

// Deliberately never inserts a review: approval only exposes a job in the existing
// review list. Existing application history and status are strictly untouched.
pub fn promote_candidate(db:&mut Connection,source:&str,source_id:&str,allow_unverified:bool)->Result<PromotionReport,String>{
    promote_selected(db,source,source_id,allow_unverified,false)
}
pub fn promote_selected(db:&mut Connection,source:&str,source_id:&str,allow_unverified:bool,allow_excluded:bool)->Result<PromotionReport,String>{
    if source.is_empty() || source_id.is_empty() || source.len()>80 || source_id.len()>200 {
        return Err("Invalid source identity".into());
    }
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let record:Option<(String,String,String,String,String,String,String,String,String)>=tx.query_row(r#"
        SELECT s.title,s.company,s.place,s.listing_url,s.grades_json,s.description,
               e.evidence_json,e.notes_json,e.classification
        FROM source_listings s JOIN source_eligibility e
          ON e.source=s.source AND e.source_id=s.source_id
        WHERE s.source=?1 AND s.source_id=?2
          AND (e.classification='eligible' OR (?3=1 AND e.classification='unverified') OR (?4=1 AND e.classification='excluded'))
          AND e.detail_status='description_found'
          AND s.description IS NOT NULL AND length(trim(s.description))>0
    "#,params![source,source_id,if allow_unverified {1} else {0},if allow_excluded {1} else {0}],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?)))
        .optional().map_err(|e|e.to_string())?;
    let (title,company,place,url,grades_raw,description,evidence_raw,notes_raw,classification)=record.ok_or("Candidate cannot be promoted without an eligible result or explicit unverified approval and a full description")?;
    // The original Jobup ID is backward compatible. Other sources have
    // namespaced identifiers to prevent unrelated providers overwriting jobs.
    let id=if source=="jobup" {source_id.to_string()}else{format!("{source}:{source_id}")};
    let existing:Option<(String,String)>=tx.query_row("SELECT source,link FROM jobs WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    if let Some((existing_source,existing_url))=existing {
        if !existing_source.is_empty() && existing_source!=source {return Err("Job ID collision across providers; manual reconciliation required".into());}
        if !existing_url.is_empty() && existing_url!=url {return Err("Existing job with different source URL; manual reconciliation required".into());}
        tx.commit().map_err(|e|e.to_string())?;
        return Ok(PromotionReport{job_id:id,inserted:false,review_records_touched:0,classification});
    }
    let grades:Value=serde_json::from_str(&grades_raw).map_err(|e|e.to_string())?;
    let evidence:Value=serde_json::from_str(&evidence_raw).map_err(|e|e.to_string())?;
    let notes:Value=serde_json::from_str(&notes_raw).map_err(|e|e.to_string())?;
    let raw=serde_json::json!({"id":id,"title":title,"company":company,"place":place,
        "link":url,"grades":grades,"description":description,"source":source,
        "_eligibility":{"classification":classification,"evidence":evidence,"notes":notes,"sourceId":source_id}});
    let inserted=tx.execute(r#"INSERT OR IGNORE INTO jobs
        (id,title,company,place,link,grades_json,source,retrieved_at,description,eligibility,raw_json)
        VALUES (?1,?2,?3,?4,?5,?6,?7,datetime('now'),?8,?9,?10)"#,
        params![id,title,company,place,url,grades_raw,source,description,classification,raw.to_string()])
        .map_err(|e|e.to_string())? == 1;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(PromotionReport{job_id:id,inserted,review_records_touched:0,classification})
}

#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct StagingCounts { pub total: i64, pub eligible:i64, pub excluded:i64, pub unverified:i64, pub unclassified:i64, pub with_description:i64 }

pub fn staging_counts(db:&Connection)->Result<StagingCounts,String>{
    db.query_row("SELECT COUNT(*), COALESCE(SUM(e.classification='eligible'),0), COALESCE(SUM(e.classification='excluded'),0), COALESCE(SUM(e.classification='unverified'),0), COALESCE(SUM(e.classification IS NULL),0), COALESCE(SUM(s.description IS NOT NULL AND LENGTH(s.description)>0),0) FROM source_listings s LEFT JOIN source_eligibility e ON s.source=e.source AND s.source_id=e.source_id",[],|r|Ok(StagingCounts{total:r.get(0)?,eligible:r.get(1)?,excluded:r.get(2)?,unverified:r.get(3)?,unclassified:r.get(4)?,with_description:r.get(5)?})).map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests {
 use super::*;
 fn j(source:&str,id:&str,app:Option<&str>)->SourceJob{SourceJob{source:source.into(),source_id:id.into(),title:"Vendeur".into(),company:"Boutique".into(),place:"Sion".into(),url:format!("https://{source}.example/{id}"),grades:vec![80],description:None,employer_application_url:app.map(str::to_string),published_at:None}}
 fn db()->Connection{let mut c=Connection::open_in_memory().unwrap();c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL, PRIMARY KEY(job_id,ordinal));").unwrap();migrate(&mut c).unwrap();c}
 #[test] fn count_report_is_read_only(){let mut c=db();store_listing(&mut c,&j("jobup","1",None)).unwrap();let v=staging_counts(&c).unwrap();assert_eq!(v.total,1);assert_eq!(v.unclassified,1);assert_eq!(v.with_description,0);}
 #[test] fn migration_twice(){let mut c=db();migrate(&mut c).unwrap();assert_eq!(c.query_row("SELECT COUNT(*) FROM schema_migrations WHERE version=2",[],|r|r.get::<_,i64>(0)).unwrap(),1);}
 #[test] fn idempotence_and_existing_records(){let mut c=db();c.execute("INSERT INTO jobs(id) VALUES ('legacy')",[]).unwrap();let a=j("jobup","1",None);assert!(store_listing(&mut c,&a).unwrap());assert!(!store_listing(&mut c,&a).unwrap());assert_eq!(c.query_row("SELECT COUNT(*) FROM jobs",[],|r|r.get::<_,i64>(0)).unwrap(),1);}
 #[test] fn same_id_different_source_stays_separate(){let mut c=db();store_listing(&mut c,&j("jobup","1",None)).unwrap();store_listing(&mut c,&j("jobs","1",None)).unwrap();assert_eq!(c.query_row("SELECT COUNT(*) FROM source_listings",[],|r|r.get::<_,i64>(0)).unwrap(),2);}
 #[test] fn changed_listing_is_not_silently_overwritten(){let mut c=db();let a=j("jobup","1",None);store_listing(&mut c,&a).unwrap();let mut changed=a.clone();changed.title="Caissier".into();assert!(store_listing(&mut c,&changed).is_err());}
 #[test] fn possible_duplicate_cannot_link(){let mut c=db();let a=j("jobup","1",None);let b=j("jobs","2",None);store_listing(&mut c,&a).unwrap();store_listing(&mut c,&b).unwrap();assert!(link_confirmed(&mut c,&a,&b,"shared").is_err());}
 #[test] fn verified_application_url_can_link(){let mut c=db();let a=j("jobup","1",Some("https://employer.ch/r/42?utm_source=jobup"));let b=j("jobs","2",Some("https://employer.ch/r/42?utm_source=jobs"));store_listing(&mut c,&a).unwrap();store_listing(&mut c,&b).unwrap();link_confirmed(&mut c,&a,&b,"opportunity-42").unwrap();assert_eq!(c.query_row("SELECT COUNT(*) FROM opportunity_links",[],|r|r.get::<_,i64>(0)).unwrap(),2);}
}

#[cfg(test)]
mod batch_tests {
  use super::*;
  use crate::pipeline_preview::{PreviewItem,PipelinePreview};
  fn setup()->Connection {let mut c=Connection::open_in_memory().unwrap();c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();migrate(&mut c).unwrap();c}
  fn fixture()->PipelinePreview {
    let j=SourceJob{source:"jobup".into(),source_id:"id-1".into(),title:"Vendeur".into(),company:"Shop".into(),place:"Sion".into(),url:"https://jobup.ch/1".into(),grades:vec![80],description:None,employer_application_url:None,published_at:None};
    let item=PreviewItem{source_id:"id-1".into(),title:j.title.clone(),company:j.company.clone(),place:j.place.clone(),detail_status:"unavailable".into(),classification:"unverified".into(),legacy_reason:"kept".into(),evidence:vec![],notes:vec![],detail_warning:None,potential_duplicate_of:vec![],source_job:j,raw_job:serde_json::json!({"id":"id-1"})};
    PipelinePreview{fetched:1,pages_fetched:1,fetch_complete:true,inspected:1,eligible:0,excluded:0,unverified:1,detail_found:0,details_requested:0,descriptions_reused:0,detail_failures:0,possible_duplicates:0,database_writes:0,warnings:vec![],items:vec![item]}
  }
  #[test] fn repeated_ingestion_is_idempotent() {let mut db=setup();let p=fixture();assert_eq!(ingest_preview(&mut db,&p).unwrap().inserted,1);assert_eq!(ingest_preview(&mut db,&p).unwrap().unchanged,1);assert_eq!(db.query_row("SELECT count(*) FROM source_listing_revisions",[],|r|r.get::<_,i64>(0)).unwrap(),1);}
  #[test] fn changed_listing_preserves_both_revisions(){let mut db=setup();let mut p=fixture();ingest_preview(&mut db,&p).unwrap();p.items[0].source_job.title="Conseiller vente".into();assert_eq!(ingest_preview(&mut db,&p).unwrap().revised,1);assert_eq!(db.query_row("SELECT count(*) FROM source_listing_revisions",[],|r|r.get::<_,i64>(0)).unwrap(),2);}
  #[test] fn warnings_forbid_any_write(){let mut db=setup();let mut p=fixture();p.warnings.push("partial".into());assert!(ingest_preview(&mut db,&p).is_err());assert_eq!(db.query_row("SELECT count(*) FROM source_listings",[],|r|r.get::<_,i64>(0)).unwrap(),0);}
  #[test] fn staged_inspection_is_filtered_and_read_only(){
    let mut db=setup();ingest_preview(&mut db,&fixture()).unwrap();
    let rows=inspect_staged(&db,10,0,Some("unverified")).unwrap();
    assert_eq!(rows.len(),1);assert_eq!(rows[0].source_id,"id-1");
    assert!(inspect_staged(&db,10,0,Some("unknown")).is_err());
    assert_eq!(inspect_staged(&db,10,0,Some("eligible")).unwrap().len(),0);
  }
  #[test] fn reviews_and_seen_are_never_touched(){let mut db=setup();db.execute("INSERT INTO reviews(job_id) VALUES ('old')",[]).unwrap();ingest_preview(&mut db,&fixture()).unwrap();assert_eq!(db.query_row("SELECT count(*) FROM reviews",[],|r|r.get::<_,i64>(0)).unwrap(),1);}
}

#[cfg(test)]
mod candidate_tests {
    use super::*;
    fn db()->Connection {
        let temp=Connection::open_in_memory().unwrap();
        let mut c=temp;
        c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
        migrate(&mut c).unwrap();c
    }
    fn stage(c:&Connection,id:&str,classification:&str,detail:&str,description:Option<&str>) {
        c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,description,original_json) VALUES('jobup',?1,'Vendeur','Shop','Lausanne','https://example.ch/job', '[80]',?2,'{}')",params![id,description]).unwrap();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES('jobup',?1,?2,?3,'kept','[]','[]')",params![id,classification,detail]).unwrap();
    }
    #[test] fn only_full_description_eligible_can_be_candidates() {
        let c=db();
        stage(&c,"good","eligible","description_found",Some("Full requirements"));
        stage(&c,"missing","eligible","unavailable",None);
        stage(&c,"excluded","excluded","description_found",Some("No"));
        stage(&c,"unverified","unverified","description_found",Some("Maybe"));
        let results=candidate_queue(&c,50,0,false).unwrap();
        assert_eq!(results.len(),1);assert_eq!(results[0].source_id,"good");
    }
    #[test] fn candidate_read_is_non_mutating_and_tracks_existing_jobs() {
        let c=db();stage(&c,"good","eligible","description_found",Some("Description"));
        c.execute("INSERT INTO jobs(id) VALUES ('good')",[]).unwrap();
        assert!(candidate_queue(&c,1,0,false).unwrap()[0].already_in_review_queue);
        let count:i64=c.query_row("SELECT COUNT(*) FROM reviews",[],|r|r.get(0)).unwrap();
        assert_eq!(count,0);
        assert!(candidate_queue(&c,0,0,false).is_err());
    }
}

#[cfg(test)]
mod optional_candidate_tests {
    use super::*;
    fn db()->Connection {
        let mut c=Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '',company TEXT NOT NULL DEFAULT '',place TEXT NOT NULL DEFAULT '',link TEXT NOT NULL DEFAULT '',grades_json TEXT NOT NULL DEFAULT '[]',source TEXT NOT NULL DEFAULT '',retrieved_at TEXT NOT NULL DEFAULT '',description TEXT,eligibility TEXT,raw_json TEXT NOT NULL DEFAULT '{}'); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
        migrate(&mut c).unwrap(); c
    }
    #[test]
    fn unverified_with_description_is_opt_in_only() {
        let c=db();
        c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,description,original_json) VALUES ('okjob','123','Vendeur','Shop','Sion','https://www.okjob.ch/x','[]','Full details','{}')",[]).unwrap();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('okjob','123','unverified','description_found','manual check','[]','[]')",[]).unwrap();
        assert!(candidate_queue(&c,50,0,false).unwrap().is_empty());
        let rows=candidate_queue(&c,50,0,true).unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].classification,"unverified");
    }
    #[test]
    fn unverified_requires_explicit_flag_for_promotion() {
        let mut c=db();
        c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,description,original_json) VALUES ('okjob','123','Vendeur','Shop','Sion','https://www.okjob.ch/x','[]','Full details','{}')",[]).unwrap();
        c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('okjob','123','unverified','description_found','manual check','[]','[]')",[]).unwrap();
        assert!(promote_candidate(&mut c,"okjob","123",false).is_err());
        let outcome=promote_candidate(&mut c,"okjob","123",true).unwrap();
        assert!(outcome.inserted);
        assert_eq!(outcome.classification,"unverified");
    }
}

#[cfg(test)]
mod promotion_tests {
 use super::*;
 fn setup()->Connection {
  let mut c=Connection::open_in_memory().unwrap();
  c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY,title TEXT NOT NULL DEFAULT '',company TEXT NOT NULL DEFAULT '',place TEXT NOT NULL DEFAULT '',link TEXT NOT NULL DEFAULT '',grades_json TEXT NOT NULL DEFAULT '[]',source TEXT NOT NULL DEFAULT '',retrieved_at TEXT NOT NULL DEFAULT '',description TEXT,eligibility TEXT,raw_json TEXT NOT NULL DEFAULT '{}'); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
  migrate(&mut c).unwrap();c
 }
 fn stage(c:&Connection,classification:&str){
  c.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,description,original_json) VALUES ('jobup','uuid-1','Vendeur','Boutique','Lausanne','https://www.jobup.ch/fr/emplois/detail/uuid-1/','[80]','Vente en magasin','{}')",[]).unwrap();
  c.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES ('jobup','uuid-1',?1,'description_found','kept','[]','[]')",[classification]).unwrap();
 }
 #[test] fn approved_candidate_is_idempotent_and_preserves_reviews(){
  let mut db=setup();stage(&db,"eligible");db.execute("INSERT INTO reviews(job_id) VALUES ('historical')",[]).unwrap();
  let r=promote_candidate(&mut db,"jobup","uuid-1",false).unwrap();assert!(r.inserted);assert_eq!(r.review_records_touched,0);
  assert!(!promote_candidate(&mut db,"jobup","uuid-1",false).unwrap().inserted);
  assert_eq!(db.query_row("SELECT count(*) FROM reviews",[],|r|r.get::<_,i64>(0)).unwrap(),1);
  let raw:String=db.query_row("SELECT raw_json FROM jobs WHERE id='uuid-1'",[],|r|r.get(0)).unwrap();
  assert!(raw.contains("_eligibility"));assert!(raw.contains("jobup.ch"));
 }
 #[test] fn unverified_cannot_be_promoted(){let mut db=setup();stage(&db,"unverified");assert!(promote_candidate(&mut db,"jobup","uuid-1",false).is_err());}
 #[test] fn url_collision_does_not_overwrite_existing_job(){let mut db=setup();stage(&db,"eligible");db.execute("INSERT INTO jobs(id,source,link) VALUES ('uuid-1','jobup','https://different.example/')",[]).unwrap();assert!(promote_candidate(&mut db,"jobup","uuid-1",false).is_err());}
}

#[cfg(test)]
mod cache_expiry_tests {
    use super::*;
    #[test]
    fn recent_cached_description_is_reused_but_expired_is_not() {
        let mut db=Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);").unwrap();
        migrate(&mut db).unwrap();
        let job=SourceJob{source:"jobup".into(), source_id:"abc".into(), title:"Vendeur".into(),company:"Magasin SA".into(),place:"Sion".into(),url:"https://jobup.ch/abc".into(),grades:vec![80],description:Some("Full details".into()),employer_application_url:None,published_at:None};
        let raw=serde_json::to_string(&job).unwrap();
        db.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,grades_json,description,original_json) VALUES ('jobup','abc','Vendeur','Magasin SA','Sion','https://jobup.ch/abc','[80]','Full details',?1)",[raw]).unwrap();
        assert!(cached_descriptions(&db).unwrap().contains_key("abc"));
        db.execute("UPDATE source_listings SET retrieved_at=datetime('now','-8 days')",[]).unwrap();
        assert!(!cached_descriptions(&db).unwrap().contains_key("abc"));
    }
}

/// User-provided exports only. This never fetches external sites and never auto-approves.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all="camelCase")]
pub struct UserListing {
    pub title: String,
    #[serde(default)] pub company: String,
    #[serde(default)] pub place: String,
    pub url: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub source_id: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct UserImportReport {
    pub inspected: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged: usize,
    pub duplicates_in_batch: usize,
    pub unverified: usize,
    pub reviewed_records_touched: usize,
}
fn allowed_host(source: &str, host: &str) -> bool {
    let hosts: &[&str] = match source {
        "jobup" => &["jobup.ch"],
        "jobs_ch" => &["jobs.ch"],
        "indeed_ch" => &["indeed.com", "indeed.ch"],
        "emploi_en_suisse" => &["emploi-en-suisse.ch"],
        "jobscout24" => &["jobscout24.ch"],
        "okjob" => &["okjob.ch"],
        "100000jobs" => &["100000jobs.ch"],
        "job_room" => &["job-room.ch", "job-room.swiss"],
        "zuri_jobs" => &["zueri.jobs"],
        "michael_page" => &["michaelpage.ch"],
        "linkedin_jobs" => &["linkedin.com"],
        "swissdevjobs" => &["swissdevjobs.ch"],
        "efinancialcareers" => &["efinancialcareers.ch"],
        _ => return false,
    };
    hosts.iter().any(|allowed| host==*allowed || host.ends_with(&format!(".{allowed}")))
}
pub fn checked_listing_url(raw:&str,source:&str)->Result<String,String>{canonical_user_url(raw,source)}
fn canonical_user_url(raw: &str, source: &str) -> Result<String,String> {
    // `url` is already an existing dependency used by Tauri; use a proper URL parser.
    let url=reqwest::Url::parse(raw.trim()).map_err(|_|"Invalid listing URL".to_string())?;
    if url.scheme()!="https" {return Err("Only HTTPS listing URLs are accepted".into());}
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {return Err("URL credentials and explicit ports are not allowed".into());}
    let host=url.host_str().unwrap_or("").to_ascii_lowercase();
    if !allowed_host(source,&host){return Err(format!("URL host {host} does not match selected source {source}"));}
    if matches!(source,"swissdevjobs"|"efinancialcareers")&&!crate::swiss_hunter_collectors::valid_browser_job_url(source,url.as_str()){return Err("A vacancy URL is required; category and country navigation pages cannot be imported".into());}
    let mut normalized=url;
    normalized.set_fragment(None);
    // Retain query: on Indeed and some boards, a query parameter IS the job identity.
    Ok(crate::job_identity::canonical_url(normalized.as_str()))
}
/// Atomic provider-local user import. No automatic cross-provider links are made.
/// Distinct provider + canonical URLs are always separate, even if titles match.
pub fn import_user_listings(db: &mut Connection, source: &str, items: &[UserListing]) -> Result<UserImportReport,String> {
    if items.is_empty() || items.len()>2000 {return Err("Import requires 1–2000 listings".into());}
    if !["jobup","jobs_ch","indeed_ch","emploi_en_suisse","jobscout24","okjob","100000jobs","job_room","zuri_jobs","michael_page","linkedin_jobs","swissdevjobs","efinancialcareers"].contains(&source) {
        return Err("Unknown Swiss source".into());
    }
    // Validate every entry before starting the transaction: one malformed row rejects the batch.
    let mut validated=Vec::new();
    let mut within=std::collections::HashSet::new();
    let mut duplicates=0;
    for (ix,item) in items.iter().enumerate() {
        if item.title.trim().is_empty() || item.title.len()>300 || item.company.len()>300 || item.place.len()>250 || item.description.len()>40_000 {
            return Err(format!("Row {} has invalid or overlong fields",ix+1));
        }
        let canonical=canonical_user_url(&item.url,source).map_err(|e|format!("Row {}: {e}",ix+1))?;
        // Stable deterministic source ID, independent of title. Query parameters are kept because they can identify jobs.
        let identity=blake3::hash(canonical.as_bytes()).to_hex().to_string();
        if !within.insert(identity.clone()){duplicates+=1;continue;}
        validated.push((item,canonical,format!("user-{identity}")));
    }
    let mut canonical_ids=std::collections::HashMap::new();
    {
        let mut stmt=db.prepare("SELECT source_id,listing_url FROM source_listings WHERE source=?1 ORDER BY retrieved_at,source_id").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([source],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
        for row in rows {let (id,url)=row.map_err(|e|e.to_string())?;canonical_ids.entry(crate::job_identity::canonical_url(&url)).or_insert(id);}
    }
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let mut report=UserImportReport{inspected:items.len(),inserted:0,revised:0,unchanged:0,duplicates_in_batch:duplicates,unverified:0,reviewed_records_touched:0};
    for (item,canonical,id) in validated {
        let existing_id:Option<String>=tx.query_row("SELECT source_id FROM source_listings WHERE source=?1 AND (listing_url=?2 OR listing_url=?3) LIMIT 1",params![source,canonical,format!("{}/",canonical.trim_end_matches('/'))],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let id=existing_id.or_else(||canonical_ids.get(&canonical).cloned()).unwrap_or(id);
        let cached:Option<String>=tx.query_row("SELECT description FROM source_listings WHERE source=?1 AND source_id=?2",params![source,id],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
        let record=crate::job_identity::SourceJob{source:source.into(),source_id:id.clone(),title:item.title.trim().into(),company:item.company.trim().into(),place:item.place.trim().into(),url:canonical.clone(),grades:vec![],description:if item.description.trim().is_empty(){cached.filter(|s|!s.trim().is_empty())}else{Some(item.description.trim().into())},employer_application_url:None,published_at:None};
        // Evaluate only the information supplied by the user. No URL is fetched.
        // The shared classifier is intentionally conservative about absent details.
        // Missing employment grade is unknown, not an automatic below-minimum exclusion.
        // The sentinel only allows other definitive exclusions to be assessed;
        // it must never be accepted as proof of a qualifying workload.
        let has_grade_info=!record.grades.is_empty();
        let grades=if has_grade_info {serde_json::json!(record.grades)} else {serde_json::json!([100])};
        let adapted=serde_json::json!({"title":record.title,"company":record.company,"place":record.place,"grades":grades});
        let decision=crate::eligibility_preview::evaluate(&adapted,record.description.as_deref());
        let mut classification=decision.classification;
        let mut reason=decision.legacy_reason;
        if record.description.is_none() {
            classification="unverified".into();
            reason="Imported without full description; requirements unverified".into();
        } else if !has_grade_info && classification=="eligible" {
            classification="unverified".into();
            reason="Employment percentage not provided; eligibility unverified".into();
        }
        let evidence=serde_json::to_string(&decision.evidence).map_err(|e|e.to_string())?;
        let notes=serde_json::to_string(&decision.notes).map_err(|e|e.to_string())?;
        let detail_status=if record.description.is_some(){"description_found"}else{"user_import"};
        let raw=serde_json::to_string(&record).map_err(|e|e.to_string())?;
        let old:Option<String>=tx.query_row("SELECT original_json FROM source_listings WHERE source=?1 AND source_id=?2",params![source,id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        if old.as_deref()==Some(raw.as_str()) {tx.execute("UPDATE source_listings SET retrieved_at=datetime('now') WHERE source=?1 AND source_id=?2",params![source,id]).map_err(|e|e.to_string())?;report.unchanged+=1;continue;}
        if old.is_some(){report.revised+=1;}else{report.inserted+=1;}
        tx.execute("INSERT INTO source_listings(source,source_id,title,company,place,listing_url,employer_application_url,grades_json,description,published_at,original_json) VALUES (?1,?2,?3,?4,?5,?6,NULL,'[]',?7,NULL,?8) ON CONFLICT(source,source_id) DO UPDATE SET title=excluded.title,company=excluded.company,place=excluded.place,listing_url=excluded.listing_url,description=excluded.description,original_json=excluded.original_json,retrieved_at=datetime('now')",
            params![source,id,record.title,record.company,record.place,canonical,record.description,raw]).map_err(|e|e.to_string())?;
        let hash=blake3::hash(raw.as_bytes()).to_hex().to_string();
        tx.execute("INSERT OR IGNORE INTO source_listing_revisions(source,source_id,revision_hash,snapshot_json) VALUES (?1,?2,?3,?4)",params![source,id,hash,raw]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO source_eligibility(source,source_id,classification,detail_status,legacy_reason,evidence_json,notes_json) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(source,source_id) DO UPDATE SET classification=excluded.classification,detail_status=excluded.detail_status,legacy_reason=excluded.legacy_reason,evidence_json=excluded.evidence_json,notes_json=excluded.notes_json,evaluated_at=datetime('now')",params![source,id,classification,detail_status,reason,evidence,notes]).map_err(|e|e.to_string())?;
        if classification=="unverified" {report.unverified+=1;}
    }
    tx.commit().map_err(|e|e.to_string())?;
    Ok(report)
}

#[cfg(test)]
mod user_import_tests {
    #[test]
    fn import_only_url_is_unverified_and_does_not_touch_reviews(){
        let mut db=Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
        migrate(&mut db).unwrap();
        let listing=UserListing{title:"Vendeur 80%".into(),company:"Boutique SA".into(),place:"Lausanne".into(),url:"https://www.jobs.ch/test/listing".into(),description:String::new(),source_id:String::new()};
        assert_eq!(import_user_listings(&mut db,"jobs_ch",&[listing]).unwrap().unverified,1);
        let class:String=db.query_row("SELECT classification FROM source_eligibility",[],|r|r.get(0)).unwrap();
        assert_eq!(class,"unverified");
        assert_eq!(db.query_row("SELECT COUNT(*) FROM reviews",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
    #[test]
    fn reimport_does_not_duplicate_records(){
        let mut db=Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
        migrate(&mut db).unwrap();
        let listing=UserListing{title:"Gestionnaire administratif".into(),company:"Entreprise SA".into(),place:"Sion".into(),url:"https://www.jobs.ch/test/item".into(),description:"Description test".into(),source_id:String::new()};
        assert_eq!(import_user_listings(&mut db,"jobs_ch",std::slice::from_ref(&listing)).unwrap().inserted,1);
        assert_eq!(import_user_listings(&mut db,"jobs_ch",&[listing]).unwrap().unchanged,1);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM source_listings",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }

    use super::*;
    #[test]
    fn swiss_hosts_are_source_scoped() {
        assert!(canonical_user_url("https://www.jobs.ch/fr/offres-emplois/x?utm_source=test", "jobs_ch").is_ok());
        assert!(canonical_user_url("https://job-room.ch/home/job-seeker", "job_room").is_ok());
        assert!(canonical_user_url("https://jobs.ch.evil.example/x", "jobs_ch").is_err());
        assert!(canonical_user_url("https://www.jobs.ch/x", "jobup").is_err());
        assert!(canonical_user_url("http://www.jobs.ch/x", "jobs_ch").is_err());
        assert!(canonical_user_url("https://user:password@www.jobs.ch/x", "jobs_ch").is_err());
    }
    #[test]
    fn fragments_are_not_identity_but_query_can_be() {
        let a=canonical_user_url("https://ch.indeed.com/viewjob?jk=first#fragment","indeed_ch").unwrap();
        let b=canonical_user_url("https://ch.indeed.com/viewjob?jk=first","indeed_ch").unwrap();
        let c=canonical_user_url("https://ch.indeed.com/viewjob?jk=second","indeed_ch").unwrap();
        assert_eq!(a,b);
        assert_ne!(a,c);
    }

#[test]
fn grade_less_imports_become_unverified_not_excluded() {
    let mut db=Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
    migrate(&mut db).unwrap();
    let listing=UserListing{
        title:"Vendeur 80%".into(),
        company:"Boutique SA".into(),
        place:"Lausanne".into(),
        url:"https://www.okjob.ch/offres-demplois/lausanne/?ref=12345".into(),
        description:"<p>Notre société recrute pour renforcer son équipe en magasin.</p><p>Accueil client, conseil client, encaissement et mise en rayon. Gestion des stocks et réception des marchandises.</p>".into(),
        source_id:String::new(),
    };
    let report=import_user_listings(&mut db,"okjob",&[listing]).unwrap();
    assert_eq!(report.inserted,1);
    let class:String=db.query_row("SELECT classification FROM source_eligibility WHERE source='okjob'",[],|r|r.get(0)).unwrap();
    assert_eq!(class,"unverified","grade-less import must not become excluded just because employment % is unknown");
}

#[test]
fn grade_less_import_with_cfc_requirement_still_excluded() {
    let mut db=Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY); CREATE TABLE jobs(id TEXT PRIMARY KEY); CREATE TABLE reviews(job_id TEXT PRIMARY KEY); CREATE TABLE status_history(job_id TEXT NOT NULL,ordinal INTEGER NOT NULL,entry_json TEXT NOT NULL,PRIMARY KEY(job_id,ordinal));").unwrap();
    migrate(&mut db).unwrap();
    let listing=UserListing{
        title:"Vendeur 80%".into(),
        company:"Boutique SA".into(),
        place:"Lausanne".into(),
        url:"https://www.okjob.ch/offres-demplois/lausanne/?ref=12346".into(),
        description:"<p>Nous recherchons un vendeur pour notre magasin de Lausanne.</p><p>CFC obligatoire pour ce poste. Accueil client et vente en magasin. Gestion des stocks et encaissement quotidien.</p>".into(),
        source_id:String::new(),
    };
    let _=import_user_listings(&mut db,"okjob",&[listing]).unwrap();
    let class:String=db.query_row("SELECT classification FROM source_eligibility WHERE source='okjob'",[],|r|r.get(0)).unwrap();
    assert_eq!(class,"excluded","definitive description-based exclusions still apply");
}
}
