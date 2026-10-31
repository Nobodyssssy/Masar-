//! Tauri command: read-only network fetch followed by existing SQLite staging.
use serde::Serialize;
use rusqlite::params;
use std::sync::atomic::Ordering;
use crate::{NativeDb,RefreshControl,source_store,swiss_hunter_collectors};

#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct HunterRefreshReport {
    pub source:String,pub fetched:usize,pub pages_fetched:usize,
    pub new_count:usize,pub revised:usize,pub unchanged:usize,pub duplicates:usize,
    pub descriptions_retrieved:usize,pub eligible:usize,pub excluded:usize,pub unverified:usize,
    pub previously_known:usize,pub warnings:Vec<String>,
}
#[tauri::command]
pub async fn refresh_swiss_hunter_source(
    state:tauri::State<'_,NativeDb>, control:tauri::State<'_,RefreshControl>,
    source:String,keywords:Option<String>,location:Option<String>,days_back:Option<u32>
)->Result<HunterRefreshReport,String>{
    if ["indeed_ch","emploi_en_suisse","100000jobs","job_room"].contains(&source.as_str()){return Err(format!("{source}: official website only; no verified collection method"));}
    if !["jobup","jobs_ch","indeed_ch","jobscout24","job_room","zuri_jobs","michael_page","linkedin_jobs","swissdevjobs","efinancialcareers"].contains(&source.as_str()) {
        return Err("Unsupported Swiss Hunter collector".into());
    }
    if control.running.swap(true,Ordering::SeqCst){return Err("Collection already running".into());}
    let _running=crate::CollectionGuard(&control.running);
    let keyword=keywords.unwrap_or_default();let city=location.unwrap_or_default();let days=days_back.unwrap_or(3);
    // Each provider is invoked sequentially by Search; clear any earlier cancellation request.
    control.cancelled.store(false,Ordering::SeqCst);
    let flag=control.cancelled.clone();
    let relevance_keywords=keyword.clone();
    let name=source.clone();
    let mut discovered=tauri::async_runtime::spawn_blocking(move || swiss_hunter_collectors::collect(&name,&keyword,&city,days,&||flag.load(Ordering::SeqCst)))
        .await.map_err(|e|format!("{source} collector task error: {e}"))??;
    if control.cancelled.load(Ordering::SeqCst){return Err("Refresh cancelled before database staging".into());}
    let before=discovered.jobs.len();
    if !relevance_keywords.trim().is_empty(){discovered.jobs.retain(|job|crate::search_step1::matches_text(&format!("{} {}",job.listing.title,job.listing.description),&relevance_keywords));}
    if discovered.jobs.len()<before{discovered.warnings.push(format!("{} unrelated listings rejected by deterministic keyword validation",before-discovered.jobs.len()));}
    if discovered.jobs.len()>=80{discovered.warnings.push("Result cap reached; collection may be partial".into());}

    let descriptions=discovered.jobs.iter().filter(|j|!j.listing.description.is_empty()).count();
    let pages=discovered.pages;let fetched=discovered.inspected;
    let mut previously_known=0usize;
    let result=crate::with_db(state, |db| {
        // Guard: do not introduce a second per-source record just because the new
        // importer uses a hash ID while an existing collector uses a provider UUID.
        let mut new_listings=Vec::new();
        let mut newly_imported_urls=std::collections::HashSet::<String>::new();
        for job in &discovered.jobs {
            let raw=job.listing.url.trim_end_matches('/');
            let exists:bool=db.query_row(
                "SELECT EXISTS(SELECT 1 FROM source_listings WHERE source=?1 AND (listing_url=?2 OR listing_url=?3 OR (?4<>'' AND source_id=?4)))",
                params![source,raw,job.listing.url,job.listing.source_id],|r|r.get::<_,i64>(0)
            ).map_err(|e|e.to_string())? !=0;
            if exists {previously_known+=1;}else{newly_imported_urls.insert(job.listing.url.clone());}
            {
                new_listings.push(source_store::UserListing {title:job.listing.title.clone(),company:job.listing.company.clone(),place:job.listing.place.clone(),url:job.listing.url.clone(),description:job.listing.description.clone(),source_id:job.listing.source_id.clone()});
            }
        }
        let report=if new_listings.is_empty(){None}else{Some(source_store::import_user_listings(db,&source,&new_listings)?)};
        // Backfill metadata for every matching canonical URL, regardless of whether
        // existing rows came from the old collector. Unknown dates stay NULL.
        for job in &discovered.jobs {
            let grades=serde_json::to_string(&job.grades).map_err(|e|e.to_string())?;
            let url=job.listing.url.trim_end_matches('/');
            db.execute("UPDATE source_listings SET
                published_at=COALESCE(published_at, ?3),
                grades_json=CASE WHEN grades_json='[]' AND ?4<>'[]' THEN ?4 ELSE grades_json END,
                description=CASE WHEN (description IS NULL OR length(trim(description))=0) AND length(?5)>0 THEN ?5 ELSE description END
                WHERE source=?1 AND (listing_url=?2 OR listing_url=?6 OR (?7<>'' AND source_id=?7))",
                params![source,url,job.published_at,grades,job.listing.description,job.listing.url,job.listing.source_id]
            ).map_err(|e|e.to_string())?;
            // The shared import API assumes unknown workload. Now that this
            // collector has extracted an actual percentage, re-evaluate only
            // NEW records (never overwrite a previous manual review).
            if newly_imported_urls.contains(&job.listing.url) && !job.grades.is_empty()
                && !job.listing.description.is_empty() {
                let metadata=serde_json::json!({"title":job.listing.title,
                    "company":job.listing.company,"place":job.listing.place,
                    "grades":job.grades});
                let decision=crate::eligibility_preview::evaluate(&metadata,Some(&job.listing.description));
                if decision.classification=="eligible" || decision.classification=="excluded" {
                    let evidence=serde_json::to_string(&decision.evidence).map_err(|e|e.to_string())?;
                    let notes=serde_json::to_string(&decision.notes).map_err(|e|e.to_string())?;
                    db.execute("UPDATE source_eligibility SET classification=?1, legacy_reason=?2, evidence_json=?3, notes_json=?4, evaluated_at=datetime('now')
                        WHERE source=?5 AND source_id IN
                            (SELECT source_id FROM source_listings WHERE source=?5 AND (listing_url=?6 OR listing_url=?7) AND source_id LIKE 'user-%')",
                        params![decision.classification,decision.legacy_reason,evidence,notes,source,url,job.listing.url]
                    ).map_err(|e|e.to_string())?;
                }
            }
        }
        let (eligible,excluded,unverified)=crate::count_by_classification(db,&source)?;
        Ok((report,eligible,excluded,unverified))
    })?;
    let (report,eligible,excluded,unverified)=result;
    Ok(HunterRefreshReport{source,fetched,pages_fetched:pages,
        new_count:report.as_ref().map_or(0,|r|r.inserted),revised:report.as_ref().map_or(0,|r|r.revised),
        unchanged:report.as_ref().map_or(0,|r|r.unchanged),
        duplicates:report.as_ref().map_or(0,|r|r.duplicates_in_batch),
        descriptions_retrieved:descriptions,eligible,excluded,unverified,previously_known,warnings:discovered.warnings})
}

/// The existing cancel command only reports success while Jobup is active;
/// this command cooperatively cancels the other collectors too.
#[tauri::command]
pub fn cancel_swiss_hunter_refresh(control:tauri::State<'_,RefreshControl>)->bool {
    control.cancelled.store(true,Ordering::SeqCst);true
}

// Only hardcoded HTTPS links to the official websites can be opened.
fn official_job_url(source: &str) -> Option<&'static str> {
    match source {
        "jobup" => Some("https://www.jobup.ch/fr/"),
        "jobs_ch" => Some("https://www.jobs.ch/fr/"),
        "indeed_ch" => Some("https://ch.indeed.com/"),
        "emploi_en_suisse" => Some("https://emploi-en-suisse.ch/"),
        "jobscout24" => Some("https://www.jobscout24.ch/fr/"),
        "okjob" => Some("https://www.okjob.ch/offres-demplois/"),
        "100000jobs" => Some("https://www.100000jobs.ch/"),
        "job_room" => Some("https://www.job-room.ch/index.html"),
        "zuri_jobs" => Some("https://www.zueri.jobs/jobs"),
        "michael_page" => Some("https://www.michaelpage.ch/jobs"),
        "linkedin_jobs" => Some("https://www.linkedin.com/jobs/"),
        "swissdevjobs" => Some("https://swissdevjobs.ch/jobs"),
        "efinancialcareers" => Some("https://www.efinancialcareers.ch/jobs"),
        _ => None,
    }
}
#[tauri::command]
pub fn open_official_job_site(source: String) -> Result<(),String> {
    let url = official_job_url(&source).ok_or("Unknown official job source")?;
    #[cfg(target_os="windows")]
    {
        std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn().map_err(|e|format!("Cannot open browser: {e}"))?;
        Ok(())
    }
    #[cfg(not(target_os="windows"))]
    { let _ = url; Err("Opening official sites is supported on Windows".into()) }
}
#[cfg(test)] mod official_source_tests {
    use super::*;
    #[test] fn all_thirteen_have_official_links() {
        for source in ["jobup","jobs_ch","indeed_ch","emploi_en_suisse","jobscout24","okjob","100000jobs","job_room","zuri_jobs","michael_page","linkedin_jobs","swissdevjobs","efinancialcareers"] {
            assert!(official_job_url(source).unwrap().starts_with("https://"));
        }
        assert_eq!(official_job_url("invalid"),None);
    }
}
