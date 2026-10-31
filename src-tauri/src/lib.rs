mod database;
mod filter_parity;
mod fetch_preview;
mod detail_preview;
mod eligibility_preview;
mod job_identity;
mod source_store;
mod swiss_hunter_collectors;
mod swiss_hunter_command;
mod applications_step3;
mod followups_step4;
mod sources_step6;
mod reset_step8;
mod cleanup;
mod browser_runtime;
mod review_step2;
mod search_step1;
mod swiss_filters;
mod geography;
mod job_facts;
mod eligibility_rules;
mod listing_freshness;
mod publication_window;
mod pipeline_preview;

mod okjob;
mod jobchannel;
mod jobs_ch;
use tauri::{Manager, Emitter};
use std::sync::{Mutex, Arc, atomic::{AtomicBool,Ordering}};

struct NativeDb { path: PathBuf, lock: Mutex<()> }
struct RefreshControl { running:AtomicBool, cancelled:Arc<AtomicBool> }
struct CollectionGuard<'a>(&'a AtomicBool);
impl Drop for CollectionGuard<'_>{fn drop(&mut self){self.0.store(false,Ordering::SeqCst);}}


fn with_db<T>(state: tauri::State<NativeDb>, f: impl FnOnce(&mut rusqlite::Connection)->Result<T,String>) -> Result<T,String> {
 let _guard=state.lock.lock().map_err(|e|e.to_string())?;
 let mut db=database::open(&state.path)?;
 f(&mut db)
}

#[tauri::command]
fn reset_all_jobup_data(state:tauri::State<'_,NativeDb>, confirmation:String)->Result<reset_step8::ResetReport,String>{
    if confirmation!="EFFACER" {return Err("Explicit EFFACER confirmation required".into());}
    let backups=state.path.parent().ok_or("Missing database directory")?.join("backups");
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
    let path=backups.join(format!("before-reset-{stamp}.sqlite3"));
    with_db(state,|db|reset_step8::execute(db,&path))
}
#[tauri::command]
fn cleanup_scraped_cache(state:tauri::State<'_,NativeDb>,days:u32)->Result<cleanup::CleanupReport,String>{
    let dir=state.path.parent().ok_or("Missing database directory")?.join("backups");
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
    with_db(state,|db|cleanup::execute(db,&dir.join(format!("before-cleanup-{stamp}.sqlite3")),days))
}
#[tauri::command]
fn sources_step6_inspect(state:tauri::State<'_,NativeDb>)->Result<Vec<sources_step6::SourceMetrics>,String>{
    with_db(state,|db|sources_step6::inspect(db))
}
#[tauri::command]
fn followups_step4_list(state:tauri::State<'_,NativeDb>)->Result<followups_step4::FollowupPage,String>{
    with_db(state,|db|{applications_step3::migrate(db)?;followups_step4::migrate(db)?;followups_step4::list(db)})
}
#[tauri::command]
fn followups_step4_action(state:tauri::State<'_,NativeDb>,update:followups_step4::Action)->Result<(),String>{
    with_db(state,|db|{applications_step3::migrate(db)?;followups_step4::migrate(db)?;followups_step4::apply_action(db,update)})
}
#[tauri::command]
fn followups_step4_settings(state:tauri::State<'_,NativeDb>,settings:followups_step4::Settings)->Result<followups_step4::Settings,String>{
    with_db(state,|db|{followups_step4::migrate(db)?;followups_step4::save_settings(db,settings)})
}
#[tauri::command]
fn followups_step4_events(state:tauri::State<'_,NativeDb>,job_id:String)->Result<Vec<followups_step4::Event>,String>{
    with_db(state,|db|{followups_step4::migrate(db)?;followups_step4::events(db,&job_id)})
}
#[tauri::command]
fn applications_step3_list(state:tauri::State<'_,NativeDb>)->Result<applications_step3::ApplicationPage,String>{
    with_db(state,|db|{applications_step3::migrate(db)?;applications_step3::list(db)})
}
#[tauri::command]
fn applications_step3_save(state:tauri::State<'_,NativeDb>,update:applications_step3::ApplicationUpdate)->Result<(),String>{
    with_db(state,|db|{applications_step3::migrate(db)?;applications_step3::save(db,update)})
}
#[tauri::command]
fn applications_step3_activity(state:tauri::State<'_,NativeDb>,job_id:String)->Result<Vec<applications_step3::ActivityEvent>,String>{
    with_db(state,|db|{applications_step3::migrate(db)?;applications_step3::activity(db,&job_id)})
}
#[tauri::command]
fn review_step2_inspect(state:tauri::State<'_,NativeDb>,filter:review_step2::ReviewFilter)->Result<review_step2::ReviewPage,String>{
    with_db(state,|db|{review_step2::migrate(db)?;review_step2::inspect(db,filter)})
}
#[tauri::command]
fn review_step2_decide(state:tauri::State<'_,NativeDb>,source:String,source_id:String,decision:String)->Result<(),String>{
    with_db(state,|db|{review_step2::migrate(db)?;review_step2::decide(db,&source,&source_id,&decision,None,false)})
}
#[tauri::command]
fn review_step2_note(state:tauri::State<'_,NativeDb>,source:String,source_id:String,note:String)->Result<(),String>{
    with_db(state,|db|{review_step2::migrate(db)?;review_step2::save_note(db,&source,&source_id,&note)})
}
#[tauri::command]
fn review_step2_undo(state:tauri::State<'_,NativeDb>,source:String,source_id:String)->Result<String,String>{
    with_db(state,|db|{review_step2::migrate(db)?;review_step2::undo(db,&source,&source_id)})
}
#[tauri::command]
fn review_step2_bulk(state:tauri::State<'_,NativeDb>,items:Vec<review_step2::ReviewIdentity>,decision:String)->Result<usize,String>{
    with_db(state,|db|{review_step2::migrate(db)?;review_step2::bulk_decide(db,&items,&decision)})
}
#[tauri::command]
fn review_step2_approve(state:tauri::State<'_,NativeDb>,source:String,source_id:String,allow_unverified:Option<bool>,allow_excluded:Option<bool>)->Result<source_store::PromotionReport,String>{
    with_db(state,|db|{
        review_step2::migrate(db)?;
        let report=source_store::promote_selected(db,&source,&source_id,allow_unverified.unwrap_or(false),allow_excluded.unwrap_or(false))?;
        review_step2::decide(db,&source,&source_id,"approved",None,true)?;
        Ok(report)
    })
}
#[tauri::command]
fn inspect_search_results(state:tauri::State<'_,NativeDb>,filters:search_step1::SearchFilters)->Result<search_step1::SearchResponse,String>{
    with_db(state,|db|search_step1::inspect(db,filters))
}
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| "Invalid URL".to_string())?;
    if parsed.scheme() != "https" { return Err("Only HTTPS URLs can be opened externally".into()); }
    let host = parsed.host_str().ok_or("URL has no host")?;
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.parse::<std::net::IpAddr>().is_ok() {
        return Err("Local/private URLs cannot be opened externally".into());
    }
    webbrowser::open(parsed.as_str()).map_err(|e| format!("Failed to open browser: {e}"))
}

#[tauri::command]
fn preview_legacy_filter(jobs: Vec<serde_json::Value>) -> Result<Vec<filter_parity::FilterResult>, String> {
 if jobs.len()>5000 {return Err("Maximum 5000 jobs per parity preview".into())}
 Ok(jobs.iter().map(filter_parity::evaluate).collect())
}

#[tauri::command]
async fn preview_jobup_fetch(days_back: Option<u32>) -> Result<fetch_preview::FetchPreview, String> {
    let days = days_back.unwrap_or(1);
    tauri::async_runtime::spawn_blocking(move || fetch_preview::fetch_recent_preview(days))
        .await.map_err(|e| format!("Fetch task failed: {e}"))?
}

#[tauri::command]
async fn preview_jobup_detail(id: String) -> Result<detail_preview::DetailPreview, String> {
    tauri::async_runtime::spawn_blocking(move || detail_preview::fetch_detail(id))
        .await.map_err(|e| format!("Detail task failed: {e}"))?
}

#[tauri::command]
fn preview_job_eligibility(job: serde_json::Value, description_html: Option<String>) -> Result<eligibility_preview::EligibilityPreview, String> {
    if description_html.as_ref().is_some_and(|s| s.len()>150_000) {return Err("Description too large".into());}
    Ok(eligibility_preview::evaluate(&job,description_html.as_deref()))
}


#[tauri::command]
async fn preview_jobup_pipeline(days_back: Option<u32>, max_details: Option<usize>) -> Result<pipeline_preview::PipelinePreview, String> {
    let days=days_back.unwrap_or(1);
    let limit=max_details.unwrap_or(5);
    tauri::async_runtime::spawn_blocking(move || pipeline_preview::run(days,limit))
       .await.map_err(|e|format!("Pipeline preview task failed: {e}"))?
}

#[tauri::command]
async fn commit_jobup_batch(state:tauri::State<'_,NativeDb>, days_back:Option<u32>, max_details:Option<usize>) -> Result<source_store::BatchReport,String> {
    let days=days_back.unwrap_or(1);
    let limit=max_details.unwrap_or(5);
    // Never hold the database mutex while fetching from the network.
    let preview=tauri::async_runtime::spawn_blocking(move || pipeline_preview::run(days,limit))
        .await.map_err(|e|format!("Pipeline task failed: {e}"))??;
    with_db(state,|db|source_store::ingest_preview(db,&preview))
}

#[tauri::command]
async fn commit_jobup_complete(app:tauri::AppHandle,state:tauri::State<'_,NativeDb>,control:tauri::State<'_,RefreshControl>,days_back:Option<u32>,detail_budget:Option<usize>)->Result<source_store::BatchReport,String>{
    if control.running.swap(true,Ordering::SeqCst){return Err("Refresh already running".into());}
    control.cancelled.store(false,Ordering::SeqCst);
    let result=commit_jobup_complete_inner(app,state,control.cancelled.clone(),days_back,detail_budget).await;
    control.running.store(false,Ordering::SeqCst);
    result
}

#[tauri::command]
fn cancel_jobup_refresh(control:tauri::State<'_,RefreshControl>)->bool{
    // The one-button refresh continues into other collectors after Jobup completes.
    // Keep cancellation available throughout that frontend-orchestrated sequence.
    control.cancelled.store(true,Ordering::SeqCst);
    true
}

async fn commit_jobup_complete_inner(app:tauri::AppHandle,state:tauri::State<'_,NativeDb>,cancelled:Arc<AtomicBool>,days_back:Option<u32>,detail_budget:Option<usize>)->Result<source_store::BatchReport,String>{
    let days=days_back.unwrap_or(1);
    let budget=detail_budget.unwrap_or(10);
    // Read-only cache is loaded before network activity, without holding the DB lock.
    let cache=source_store::cached_descriptions(&database::open(&state.path)?)?;
    let events=app.clone();
    let cancel_flag=cancelled.clone();
    let preview=tauri::async_runtime::spawn_blocking(move || {
        pipeline_preview::run_complete_controlled(days,budget,&mut |step| {
            let _=events.emit("jobup-refresh-progress",step);
        },&cache,&||cancel_flag.load(Ordering::SeqCst))
    }).await.map_err(|e|format!("Full-batch fetch failed: {e}"))??;
    if cancelled.load(Ordering::SeqCst){return Err("Refresh cancelled; nothing saved".into());}
    let _=app.emit("jobup-refresh-progress", pipeline_preview::RefreshProgress {
        phase:"save",completed:0,total:Some(1),message:"Saving advertisements to SQLite".into()
    });
    // Once committed, cancellation can't undo the transaction; report this explicitly.
    if cancelled.load(Ordering::SeqCst){return Err("Refresh cancelled; nothing saved".into());}
    let report=with_db(state,|db|source_store::ingest_preview(db,&preview))?;
    let _=app.emit("jobup-refresh-progress", pipeline_preview::RefreshProgress {
        phase:"save",completed:1,total:Some(1),message:"SQLite transaction committed".into()
    });
    Ok(report)
}


#[derive(Debug,serde::Serialize)]
#[serde(rename_all="camelCase")]
struct JobsChRefreshReport {
    source:String, fetched:usize, pages_fetched:u32,
    new_count:usize,revised:usize,unchanged:usize,duplicates:usize,
    descriptions_retrieved:usize,descriptions_cached:usize,eligible:usize,excluded:usize,unverified:usize,
    warnings:Vec<String>,
}
#[tauri::command]
async fn refresh_jobs_ch(state:tauri::State<'_,NativeDb>,control:tauri::State<'_,RefreshControl>)->Result<JobsChRefreshReport,String>{
    if control.running.swap(true,Ordering::SeqCst){return Err("Collection already running".into());}
    let _running=CollectionGuard(&control.running);
    control.cancelled.store(false,Ordering::SeqCst);

    let cache:std::collections::HashMap<String,String>=with_db(state.clone(),|db|{
        let mut stmt=db.prepare("SELECT source_id,description FROM source_listings WHERE source='jobs_ch' AND description IS NOT NULL AND length(description)>0 AND datetime(retrieved_at)>=datetime('now','-7 days')").map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
        let mut map=std::collections::HashMap::new();
        for row in rows {let (id,desc)=row.map_err(|e|e.to_string())?;map.insert(id,desc);}
        Ok(map)
    })?;
    let cancelled=control.cancelled.clone();
    let discovery=tauri::async_runtime::spawn_blocking(move||jobs_ch::fetch_with_cache_cancel(&cache,&||cancelled.load(Ordering::SeqCst))).await
        .map_err(|e|format!("Jobs.ch task: {e}"))??;
    if control.cancelled.load(Ordering::SeqCst) { return Err("Refresh cancelled before saving Jobs.ch".into()); }
    let (new_count,revised,unchanged,duplicates)=if discovery.listings.is_empty(){(0,0,0,0)}else{
        let report=with_db(state.clone(),|db|source_store::import_user_listings(db,"jobs_ch",&discovery.listings))?;
        (report.inserted,report.revised,report.unchanged,report.duplicates_in_batch)
    };
    let (eligible,excluded,unverified)=with_db(state,|db|count_by_classification(db,"jobs_ch"))?;
    Ok(JobsChRefreshReport{source:"jobs_ch".into(),fetched:discovery.inspected,pages_fetched:discovery.pages_fetched,
        new_count,revised,unchanged,duplicates,descriptions_retrieved:discovery.detail_success,descriptions_cached:discovery.detail_cached,eligible,excluded,unverified,warnings:discovery.warnings})
}

#[derive(Debug,serde::Serialize)]
#[serde(rename_all="camelCase")]
struct OkJobRefreshReport {
    fetched:usize,
    pages_fetched:usize,
    new_count:usize,
    revised:usize,
    unchanged:usize,
    duplicates:usize,
    descriptions_retrieved:usize,
    descriptions_cached:usize,
    eligible:usize,
    excluded:usize,
    unverified:usize,
    warnings:Vec<String>,
}

fn count_by_classification(db: &rusqlite::Connection, source: &str) -> Result<(usize,usize,usize),String> {
    let mut stmt = db.prepare(
        "SELECT e.classification, COUNT(*) FROM source_listings s \
         JOIN source_eligibility e ON s.source=e.source AND s.source_id=e.source_id \
         WHERE s.source=?1 GROUP BY e.classification"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([source], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let (mut e_, mut x_, mut u_) = (0usize, 0usize, 0usize);
    for row in rows {
        let (c, n) = row.map_err(|e| e.to_string())?;
        match c.as_str() {
            "eligible" => e_ = n as usize,
            "excluded" => x_ = n as usize,
            "unverified" => u_ = n as usize,
            _ => {}
        }
    }
    Ok((e_, x_, u_))
}

#[tauri::command]
async fn refresh_okjob(state:tauri::State<'_,NativeDb>,control:tauri::State<'_,RefreshControl>,keywords:Option<String>,location:Option<String>)->Result<OkJobRefreshReport,String>{
    if control.running.swap(true,Ordering::SeqCst){return Err("Collection already running".into());}
    let _running=CollectionGuard(&control.running);
    control.cancelled.store(false,Ordering::SeqCst);

    // Description cache is read before network I/O. Only nonempty descriptions
    // from recently stored OK Job listings are eligible for reuse.
    let cache:std::collections::HashMap<String,String>=with_db(state.clone(),|db|{
        let mut stmt=db.prepare("SELECT listing_url,description FROM source_listings WHERE source='okjob' AND description IS NOT NULL AND length(trim(description))>0 AND datetime(retrieved_at)>=datetime('now','-7 days')")
            .map_err(|e|e.to_string())?;
        let rows=stmt.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))
            .map_err(|e|e.to_string())?;
        let mut results=std::collections::HashMap::new();
        for row in rows {
            let (url,description)=row.map_err(|e|e.to_string())?;
            let canonical=url.split('#').next().unwrap_or(&url).trim().trim_end_matches('/');
            results.insert(canonical.to_string(),description);
        }
        Ok(results)
    })?;
    let cancelled=control.cancelled.clone();
    let mut discovery=tauri::async_runtime::spawn_blocking(move||okjob::fetch_with_cache_cancel(&cache,&||cancelled.load(Ordering::SeqCst)))
        .await.map_err(|e|format!("OK Job task: {e}"))??;
    if control.cancelled.load(Ordering::SeqCst) { return Err("Refresh cancelled before saving OK Job".into()); }
    let keyword=keywords.unwrap_or_default();let city=location.unwrap_or_default();
    if keyword.len()>220||city.len()>140{return Err("OK Job search query too large".into());}
    let before=discovery.listings.len();
    discovery.listings.retain(|job|search_step1::matches_text(&format!("{} {}",job.title,job.description),&keyword)&&geography::postal_matches(&job.place,&city));
    discovery.warnings.push("Regional collector: Lausanne/Yverdon pages only; keyword/location validation is local, not provider-side".into());
    if discovery.listings.len()<before{discovery.warnings.push(format!("{} listings outside the requested keyword/location rejected",before-discovery.listings.len()));}
    if discovery.listings.is_empty() {
        return Ok(OkJobRefreshReport{
            fetched:0, pages_fetched:discovery.pages_fetched,
            new_count:0, revised:0, unchanged:0, duplicates:0,
            descriptions_retrieved:discovery.detail_success,
            descriptions_cached:discovery.detail_cached,
            eligible:0, excluded:0, unverified:0,
            warnings:discovery.warnings,
        });
    }
    let staged = with_db(state.clone(), |db| {
        source_store::import_user_listings(db, "okjob", &discovery.listings)
    })?;
    let (eligible, excluded, unverified) = with_db(state, |db| {
        count_by_classification(db, "okjob")
    })?;
    Ok(OkJobRefreshReport{
        fetched: discovery.inspected,
        pages_fetched: discovery.pages_fetched,
        new_count: staged.inserted,
        revised: staged.revised,
        unchanged: staged.unchanged,
        duplicates: staged.duplicates_in_batch,
        descriptions_retrieved: discovery.detail_success,
        descriptions_cached: discovery.detail_cached,
        eligible,
        excluded,
        unverified,
        warnings: discovery.warnings,
    })
}

#[derive(Debug,serde::Serialize)]
#[serde(rename_all="camelCase")]
struct JobchannelRefreshReport {
    source:String,
    fetched:usize,
    pages_fetched:usize,
    new_count:usize,
    revised:usize,
    unchanged:usize,
    duplicates:usize,
    eligible:usize,
    excluded:usize,
    unverified:usize,
    warnings:Vec<String>,
}

#[tauri::command]
async fn refresh_jobchannel(state:tauri::State<'_,NativeDb>,control:tauri::State<'_,RefreshControl>,source:String)->Result<JobchannelRefreshReport,String>{
    if ["indeed_ch","emploi_en_suisse","100000jobs","job_room"].contains(&source.as_str()){return Err(format!("{source}: official website only; no verified collection method"));}

    if control.running.swap(true,Ordering::SeqCst){return Err("Collection already running".into());}
    let _running=CollectionGuard(&control.running);
    control.cancelled.store(false,Ordering::SeqCst);

    if source!="emploi_en_suisse" && source!="100000jobs" {
        return Err("Unsupported source".into());
    }
    let cancelled=control.cancelled.clone();
    let discovery = tauri::async_runtime::spawn_blocking(move||jobchannel::fetch_cancel(&source,&||cancelled.load(Ordering::SeqCst)))
        .await.map_err(|e|format!("Jobchannel task: {e}"))??;
    if control.cancelled.load(Ordering::SeqCst) { return Err("Refresh cancelled before saving Jobchannel".into()); }
    let (new_count,revised,unchanged,duplicates) = if discovery.listings.is_empty() {
        (0usize, 0usize, 0usize, 0usize)
    } else {
        let saved = with_db(state.clone(), |db| {
            source_store::import_user_listings(db, &discovery.source, &discovery.listings)
        })?;
        (saved.inserted, saved.revised, saved.unchanged, saved.duplicates_in_batch)
    };
    let (eligible,excluded,unverified) = with_db(state, |db| {
        count_by_classification(db, &discovery.source)
    })?;
    Ok(JobchannelRefreshReport{
        source: discovery.source,
        fetched: discovery.listings.len(),
        pages_fetched: discovery.pages_fetched,
        new_count,
        revised,
        unchanged,
        duplicates,
        eligible,
        excluded,
        unverified,
        warnings: discovery.warnings,
    })
}





#[tauri::command]
fn import_swiss_listings(state:tauri::State<'_,NativeDb>,source:String,items:Vec<source_store::UserListing>)->Result<source_store::UserImportReport,String>{
    with_db(state,|db|source_store::import_user_listings(db,&source,&items))
}

#[tauri::command]
fn inspect_staged_listings(state:tauri::State<'_,NativeDb>,limit:Option<usize>,offset:Option<usize>,classification:Option<String>)->Result<Vec<source_store::StagedListing>,String>{
    with_db(state,|db|source_store::inspect_staged(db,limit.unwrap_or(100),offset.unwrap_or(0),classification.as_deref()))
}

#[tauri::command]
fn inspect_candidate_queue(state:tauri::State<'_,NativeDb>,limit:Option<usize>,offset:Option<usize>,include_unverified:Option<bool>)->Result<Vec<source_store::CandidateQueueItem>,String>{
    with_db(state,|db|source_store::candidate_queue(db,limit.unwrap_or(50),offset.unwrap_or(0),include_unverified.unwrap_or(false)))
}

#[tauri::command]
fn promote_candidate_to_review(state:tauri::State<'_,NativeDb>,source:String,source_id:String,allow_unverified:Option<bool>)->Result<source_store::PromotionReport,String>{
    with_db(state,|db|source_store::promote_candidate(db,&source,&source_id,allow_unverified.unwrap_or(false)))
}

#[tauri::command]
fn staged_listing_counts(state:tauri::State<'_,NativeDb>)->Result<source_store::StagingCounts,String>{
    with_db(state,|db|source_store::staging_counts(db))
}

#[tauri::command]
fn preview_job_duplicates(a: job_identity::SourceJob, b: job_identity::SourceJob) -> job_identity::DuplicateResult {
    job_identity::compare(&a,&b)
}

#[tauri::command]
fn db_load(state: tauri::State<NativeDb>)->Result<database::AppState,String>{with_db(state,|db| database::read(db))}
#[tauri::command]
fn db_import_reviewed(state:tauri::State<NativeDb>, reviewed:serde_json::Value, source:String)->Result<database::ImportReport,String>{with_db(state,|db|database::import_reviewed(db,&reviewed,&source))}
#[tauri::command]
fn db_put_review(state:tauri::State<NativeDb>, id:String, entry:Option<serde_json::Value>)->Result<(),String>{with_db(state,|db|database::replace_review(db,&id,entry.as_ref()))}
#[tauri::command]
fn db_add_jobs(state:tauri::State<NativeDb>, jobs:Vec<serde_json::Value>, source:String)->Result<usize,String>{with_db(state,|db|database::add_jobs(db,&jobs,&source))}
#[tauri::command]
fn db_settings(state:tauri::State<NativeDb>, settings:serde_json::Value, saved_at:String)->Result<(),String>{with_db(state,|db|database::write_settings(db,&settings,&saved_at))}
#[tauri::command]
fn db_import_legacy(state:tauri::State<NativeDb>, folder:String)->Result<database::ImportReport,String>{let archive=state.path.parent().ok_or("Missing app data directory")?.join("backups");with_db(state,|db|database::import_legacy(db,std::path::Path::new(&folder),&archive))}
#[tauri::command]
fn db_backup(state:tauri::State<NativeDb>, destination:String)->Result<(),String>{with_db(state,|db|database::backup(db,std::path::Path::new(&destination)))}

#[tauri::command]
fn db_backup_auto(state:tauri::State<NativeDb>)->Result<String,String>{
 let dir=state.path.parent().ok_or("Database path has no parent")?.join("backups");
 let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
 let path=dir.join(format!("jobup-{stamp}.sqlite3"));
 with_db(state,|db|database::backup(db,&path))?;
 Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn db_restore(state:tauri::State<NativeDb>,source:String)->Result<String,String>{
 let safe_source=std::path::Path::new(&source);
 let backup_dir=state.path.parent().ok_or("Missing database parent")?.join("backups");
 let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
 let emergency=backup_dir.join(format!("before-restore-{stamp}.sqlite3"));
 with_db(state,|db|{
    // Always preserve the currently functioning database before attempting restore.
    database::backup(db,&emergency)?;
    database::restore(db,safe_source)?;
    Ok(())
 })?;
 Ok(emergency.display().to_string())
}

// Phase 1: native desktop shell with a narrow, explicit JSON export command.
// Fetching, filtering, existing Python workflows and database are unchanged.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[tauri::command]
fn app_health() -> &'static str {
    "jobup-tauri-phase1-ok"
}

fn valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'_',
            13 | 16 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

fn export_directory() -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        // Cargo manifest sits under <project>/src-tauri.
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("Missing project directory")?
            .join("exports"))
    } else {
        // Portable builds: beside the Jobup executable. Installed folders such
        // as Program Files may be read-only; fail clearly rather than redirecting.
        Ok(std::env::current_exe()
            .map_err(|e| format!("Cannot locate application executable: {e}"))?
            .parent()
            .ok_or("Missing executable directory")?
            .join("exports"))
    }
}

#[tauri::command]
fn export_reviewed(json: String, timestamp: String) -> Result<String, String> {
    if !valid_timestamp(&timestamp) {
        return Err("Invalid export timestamp".into());
    }
    if json.len() > 20 * 1024 * 1024 {
        return Err("Export exceeds the 20 MB safety limit".into());
    }
    let parsed: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| format!("Invalid reviewed JSON: {e}"))?;
    if !parsed.is_object() {
        return Err("Reviewed JSON must be an object".into());
    }

    let directory = export_directory()?;
    fs::create_dir_all(&directory)
        .map_err(|e| format!("Unable to create {}: {e}", directory.display()))?;

    // create_new avoids overwriting backups when two exports occur in one second.
    for i in 0..1000 {
        let filename = if i == 0 {
            format!("reviewed_{timestamp}.json")
        } else {
            format!("reviewed_{timestamp}_{i:03}.json")
        };
        let path = directory.join(filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(json.as_bytes()).and_then(|_| file.sync_all()) {
                    let _ = fs::remove_file(&path);
                    return Err(format!("Unable to write export: {e}"));
                }
                return Ok(path.display().to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Unable to create export in {}: {e}", directory.display())),
        }
    }
    Err("Too many exports with this timestamp".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path=app.path().app_data_dir()?.join("jobup.sqlite3");
            database::open(&path).map_err(std::io::Error::other)?;
            app.manage(NativeDb {path,lock:Mutex::new(())});
            app.manage(RefreshControl {running:AtomicBool::new(false),cancelled:Arc::new(AtomicBool::new(false))});
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![swiss_hunter_command::cancel_swiss_hunter_refresh, swiss_hunter_command::refresh_swiss_hunter_source, swiss_hunter_command::open_official_job_site, reset_all_jobup_data, cleanup_scraped_cache, browser_runtime::setup_firefox_collector, listing_freshness::verify_vacancy_link, sources_step6_inspect, followups_step4_list, followups_step4_action, followups_step4_settings, followups_step4_events, applications_step3_list, applications_step3_save, applications_step3_activity, review_step2_inspect, review_step2_decide, review_step2_note, review_step2_undo, review_step2_bulk, review_step2_approve, inspect_search_results, refresh_okjob, refresh_jobchannel, refresh_jobs_ch, import_swiss_listings, open_external_url, cancel_jobup_refresh, promote_candidate_to_review, commit_jobup_complete, inspect_staged_listings, inspect_candidate_queue, staged_listing_counts, commit_jobup_batch, preview_jobup_pipeline, preview_job_duplicates, preview_job_eligibility, preview_jobup_detail, preview_jobup_fetch, preview_legacy_filter, app_health, export_reviewed, db_load, db_import_reviewed, db_put_review, db_add_jobs, db_settings, db_import_legacy, db_backup, db_backup_auto, db_restore])
        .run(tauri::generate_context!())
        .expect("error while running Jobup desktop shell");
}

#[cfg(test)]
mod tests {
    #[test]
    fn health_returns_phase1_marker() {
        assert_eq!(super::app_health(), "jobup-tauri-phase1-ok");
    }

    #[test]
    fn timestamp_validation() {
        assert!(super::valid_timestamp("2026-10-25_14-03-02"));
        assert!(!super::valid_timestamp("../../evil/file-name"));
        assert!(!super::valid_timestamp("2026-10-25_14:03:02"));
    }
}

#[cfg(test)]
mod private_acceptance_tests;
