//! Phase 3 dry run: no database access, no mutation, no automatic deduplication.
use crate::{detail_preview, eligibility_preview, fetch_preview, job_identity};
use serde::Serialize;
use serde_json::{json, Value};

const MAX_SAMPLE: usize = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct RefreshProgress {
    pub phase: &'static str,
    pub completed: usize,
    pub total: Option<usize>,
    pub message: String,
}
fn notify(callback: &mut dyn FnMut(RefreshProgress), phase: &'static str,
          completed:usize, total:Option<usize>, message:String) {
    callback(RefreshProgress {phase,completed,total,message});
}


#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct PreviewItem {
    pub source_id: String,
    pub title: String,
    pub company: String,
    pub place: String,
    pub detail_status: String,
    pub classification: String,
    pub legacy_reason: String,
    pub evidence: Vec<eligibility_preview::Evidence>,
    pub notes: Vec<String>,
    pub detail_warning: Option<String>,
    pub potential_duplicate_of: Vec<String>,
    #[serde(skip_serializing)]
    pub source_job: job_identity::SourceJob,
    #[serde(skip_serializing)]
    pub raw_job: Value,

}

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct PipelinePreview {
    pub fetched: usize,
    pub pages_fetched: u32,
    pub fetch_complete: bool,
    pub inspected: usize,
    pub eligible: usize,
    pub excluded: usize,
    pub unverified: usize,
    pub detail_found: usize,
    pub details_requested: usize,
    pub descriptions_reused: usize,
    pub detail_failures: usize,
    pub possible_duplicates: usize,
    pub database_writes: usize,
    pub warnings: Vec<String>,
    pub items: Vec<PreviewItem>,
}

fn name(job:&Value)->String {
    job.get("company").and_then(|x|x.get("name")).and_then(Value::as_str).unwrap_or_default().to_owned()
}
fn str_field(job:&Value,key:&str)->String {job.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()}
fn as_source(job:&Value)->job_identity::SourceJob {
    let id=str_field(job,"id");
    job_identity::SourceJob {
        source:"jobup".into(),source_id:id.clone(),title:str_field(job,"title"),company:name(job),
        place:str_field(job,"place"),url:format!("https://www.jobup.ch/fr/emplois/detail/{id}/"),
        grades:job.get("employmentGrades").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).filter_map(|x|u32::try_from(x).ok()).collect(),
        description:None,employer_application_url:None,published_at:job.get("publicationDate").and_then(Value::as_str).map(str::to_owned),
    }
}
fn classify(job:&Value,description:Option<&str>)->eligibility_preview::EligibilityPreview {
    let adapted=json!({"title":job.get("title"),"company":name(job),"place":job.get("place"),"grades":job.get("employmentGrades")});
    eligibility_preview::evaluate(&adapted,description)
}

// Keep a pure transformation available for deterministic unit tests.
fn make_item(job:&Value,description:Option<&str>,warning:Option<String>,duplicates:Vec<String>)->PreviewItem {
    let decision=classify(job,description);
    PreviewItem{source_id:str_field(job,"id"),title:str_field(job,"title"),company:name(job),
        place:str_field(job,"place"),detail_status:if description.is_some(){"description_found"}else{"unavailable"}.into(),
        classification:decision.classification,legacy_reason:decision.legacy_reason,evidence:decision.evidence,
        notes:decision.notes,detail_warning:warning,potential_duplicate_of:duplicates,
        source_job: { let mut source=as_source(job);source.description=description.map(str::to_owned);source },
        raw_job: job.clone()}
}

pub fn run(days_back:u32,max_details:usize)->Result<PipelinePreview,String> {
    if max_details==0 || max_details>MAX_SAMPLE {return Err(format!("maxDetails must be 1..={MAX_SAMPLE}"));}
    run_impl(days_back,max_details,false,&mut |_| {},None,&||false)
}

/// Full-fetch mode: stage every search result; only a bounded number of detail requests.
pub fn run_complete(days_back:u32,detail_budget:usize)->Result<PipelinePreview,String> {
    if detail_budget>20 {return Err("detailBudget must be 0..=20".into());}
    run_complete_with_progress(days_back,detail_budget,&mut |_| {})
}

pub fn run_complete_with_progress(days_back:u32, detail_budget:usize,
        progress:&mut dyn FnMut(RefreshProgress))->Result<PipelinePreview,String> {
    if detail_budget>20 {return Err("detailBudget must be 0..=20".into());}
    run_impl(days_back,detail_budget,true,progress,None,&||false)
}

pub fn run_complete_controlled(days_back:u32, detail_budget:usize,
    progress:&mut dyn FnMut(RefreshProgress),
    cache:&std::collections::HashMap<String,(job_identity::SourceJob,String)>,
    cancelled:&dyn Fn()->bool)->Result<PipelinePreview,String>{
    if detail_budget>20{return Err("detailBudget must be 0..=20".into());}
    run_impl(days_back,detail_budget,true,progress,Some(cache),cancelled)
}
fn cache_match(saved:&job_identity::SourceJob,current:&job_identity::SourceJob)->bool{
    saved.source==current.source && saved.source_id==current.source_id
    && saved.title==current.title && saved.company==current.company && saved.place==current.place
    && saved.grades==current.grades && saved.published_at==current.published_at
}
// Cached descriptions should not consume a network-request slot. Otherwise each
// refresh repeatedly prioritizes the same already-covered listings and never
// expands description coverage to other plausible candidates.
fn detail_priority_ids(
    jobs:&[Value], budget:usize,
    cache:Option<&std::collections::HashMap<String,(job_identity::SourceJob,String)>>,
)->std::collections::HashSet<String> {
    let mut ranked:Vec<(u8,usize,String)>=jobs.iter().enumerate()
        .filter_map(|(i,job)| {
            if classify(job,None).classification=="excluded" { return None; }
            let source=as_source(job);
            if source.source_id.is_empty(){return None;}
            let already_cached=cache.and_then(|all|all.get(&source.source_id))
                .is_some_and(|(saved,description)| !description.is_empty() && cache_match(saved,&source));
            if already_cached {return None;}
            let role=source.title.to_lowercase();
            let likely=["vente","vendeur","vendeuse","caissier","magasinier","logistique","stock","administratif","administrative","back-office","service client","sav","commandes","reception marchandises"]
                .iter().any(|word|role.contains(word));
            Some((if likely {0}else{1},i,source.source_id))
        }).collect();
    ranked.sort_by_key(|(priority,index,_)|(*priority,*index));
    ranked.into_iter().take(budget).map(|(_,_,id)|id).collect()
}

fn run_impl(days_back:u32, detail_budget:usize, full_batch:bool,
            progress:&mut dyn FnMut(RefreshProgress),
            cache:Option<&std::collections::HashMap<String,(job_identity::SourceJob,String)>>,
            cancelled:&dyn Fn()->bool)->Result<PipelinePreview,String> {
    notify(progress,"fetch",0,None,"Fetching Jobup search pages".into());
    let fetched=fetch_preview::fetch_recent_cancellable(days_back,&mut |done,total| {
        notify(progress,"fetch",done as usize,Some(total as usize),format!("Fetched {done} of {total} search pages"));
    },cancelled)?;
    if cancelled(){return Err("Refresh cancelled; nothing saved".into());}
    let mut result=PipelinePreview{fetched:fetched.jobs.len(),pages_fetched:fetched.pages_fetched,
        fetch_complete:fetched.complete,inspected:0,eligible:0,excluded:0,unverified:0,
        detail_found:0,details_requested:0,descriptions_reused:0,detail_failures:0,possible_duplicates:0,database_writes:0,warnings:fetched.warnings,items:Vec::new()};
    let mut previous:Vec<job_identity::SourceJob>=Vec::new();
    let mut details_attempted=0usize;
    // Reserve scarce detail requests for plausible work before processing the
    // full batch in its original order. Never discard any source listing.
    let detail_priority:std::collections::HashSet<String>=if full_batch {
        detail_priority_ids(&fetched.jobs,detail_budget,cache)
    }else{std::collections::HashSet::new()};
    let count=if full_batch {fetched.jobs.len()} else {detail_budget};
    let selected_details=if full_batch {detail_priority.len()}else{detail_budget};
    notify(progress,"details",0,Some(selected_details),"Checking selected job descriptions".into());
    notify(progress,"classify",0,Some(count),"Classifying advertisements".into());
    for job in fetched.jobs.iter().take(count) {
        if cancelled(){return Err("Refresh cancelled; nothing saved".into());}
        let current=as_source(job);
        let duplicates:Vec<String>=previous.iter().filter(|prev| {
            matches!(job_identity::compare(prev,&current).relation.as_str(),"possible_duplicate"|"same_opportunity"|"same_listing")
        }).map(|prev|prev.source_id.clone()).collect();
        if !duplicates.is_empty(){result.possible_duplicates+=1;}
        let legacy=classify(job,None);
        let cached=cache.and_then(|all|all.get(&current.source_id))
            .filter(|(saved,description)|!description.is_empty() && cache_match(saved,&current))
            .map(|(_,desc)|desc.clone());
        let mut requested_network_detail=false;
        let (description,warning)=if !legacy.legacy_keep {(None,None)}
        else if let Some(desc)=cached {result.descriptions_reused+=1;(Some(desc),Some("Reused recent stored description".into()))}
        else if current.source_id.is_empty(){(None,Some("Missing listing ID".into()))}
        else if (full_batch && !detail_priority.contains(&current.source_id)) || details_attempted>=detail_budget {(None,Some("Description not requested: detail budget or priority".into()))}
        else {details_attempted+=1;
            requested_network_detail=true;
            let detail_result=match detail_preview::fetch_detail(current.source_id.clone()) {
                Ok(d)=>(d.description_html,d.warning),Err(e)=>(None,Some(e))
            };
            notify(progress,"details",details_attempted,Some(selected_details),
                   format!("Checked {details_attempted} of {selected_details} descriptions"));
            detail_result
        };
        if cancelled(){return Err("Refresh cancelled; nothing saved".into());}
        if requested_network_detail {
            result.details_requested+=1;
            if description.is_none(){result.detail_failures+=1;}
        }
        if description.is_some(){result.detail_found+=1;}
        let item=make_item(job,description.as_deref(),warning,duplicates);
        match item.classification.as_str(){"eligible"=>result.eligible+=1,"excluded"=>result.excluded+=1,_=>result.unverified+=1}
        result.items.push(item);
        result.inspected+=1;
        if result.inspected%5==0 || result.inspected==count {
            notify(progress,"classify",result.inspected,Some(count),
                   format!("Classified {} of {count} advertisements",result.inspected));
        }
        previous.push(current);
        // Conservative throttle; initiated only by an explicit preview command.
        if requested_network_detail {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    if cancelled(){return Err("Refresh cancelled; nothing saved".into());}
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample()->Value {json!({"id":"09c2d49a-207f-4eda-bc9e-2ddc6346dae9","title":"Vendeur","company":{"name":"Magasin SA"},"place":"Sion","employmentGrades":[80,100]})}
    #[test] fn maps_jobup_listing_fields(){let s=as_source(&sample());assert_eq!(s.company,"Magasin SA");assert_eq!(s.grades,vec![80,100]);assert_eq!(s.source,"jobup");}
    #[test] fn description_absent_does_not_become_eligible(){let item=make_item(&sample(),None,None,vec![]);assert_eq!(item.classification,"unverified");}
    #[test] fn preserves_rejection_without_description(){let mut job=sample();job["title"]=json!("Cuisinier");let item=make_item(&job,None,None,vec![]);assert_eq!(item.classification,"excluded");}
    #[test] fn preview_is_explicitly_read_only(){let r=PipelinePreview{fetched:0,pages_fetched:0,fetch_complete:false,inspected:0,eligible:0,excluded:0,unverified:0,detail_found:0,details_requested:0,descriptions_reused:0,detail_failures:0,possible_duplicates:0,database_writes:0,warnings:vec![],items:vec![]};assert_eq!(r.database_writes,0);}
}

#[cfg(test)]
mod progress_tests {
    use super::*;
    #[test]
    fn refresh_event_uses_typed_phase_and_counts() {
        let mut events=Vec::new();
        notify(&mut |x|events.push(x),"fetch",2,Some(6),"Fetched 2 pages".to_string());
        let value=serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["phase"],"fetch");
        assert_eq!(value["completed"],2);
        assert_eq!(value["total"],6);
    }
    #[test]
    fn refresh_event_allows_unknown_page_total() {
        let value=serde_json::to_value(RefreshProgress {
            phase:"fetch",completed:0,total:None,message:"Connecting".into()
        }).unwrap();
        assert!(value["total"].is_null());
    }
}

#[cfg(test)]
mod step16_tests {
    use super::*;
    fn job(id:&str,title:&str)->job_identity::SourceJob {
        job_identity::SourceJob{source:"jobup".into(),source_id:id.into(),title:title.into(),company:"Shop".into(),place:"Lausanne".into(),url:"https://example.test/1".into(),grades:vec![80],description:None,employer_application_url:None,published_at:Some("2026-10-25".into())}
    }
    #[test] fn cache_requires_same_identity_and_metadata(){
        let a=job("123","Vendeur");
        assert!(cache_match(&a,&a));
        assert!(!cache_match(&a,&job("456","Vendeur")));
        assert!(!cache_match(&a,&job("123","Technicien")));
    }
    #[test] fn cached_description_does_not_depend_on_listing_url(){
        let a=job("123","Vendeur");
        let mut b=a.clone();b.url="https://www.jobup.ch/fr/emplois/detail/123/".into();
        assert!(cache_match(&a,&b));
    }
}

#[cfg(test)]
mod step17_tests {
    use super::*;
    use std::collections::HashMap;
    fn listing(id:&str,title:&str)->Value {
        json!({"id":id,"title":title,"company":{"name":"Magasin SA"},
            "place":"Lausanne","employmentGrades":[80],"publicationDate":"2026-10-25"})
    }
    #[test]
    fn cached_candidates_do_not_consume_new_detail_budget() {
        let jobs=vec![listing("1","Vendeur"),listing("2","Vendeur"),listing("3","Magasinier")];
        let mut cache=HashMap::new();
        cache.insert("1".into(),(as_source(&jobs[0]),"<p>Previous requirements</p>".into()));
        let chosen=detail_priority_ids(&jobs,1,Some(&cache));
        assert_eq!(chosen.len(),1);
        assert!(chosen.contains("2"));
        assert!(!chosen.contains("1"));
    }
    #[test]
    fn changed_metadata_invalidates_cache_and_is_prioritized() {
        let jobs=vec![listing("1","Vendeur"),listing("2","Magasinier")];
        let mut changed=as_source(&jobs[0]);changed.title="Old title".into();
        let mut cache=HashMap::new();cache.insert("1".into(),(changed,"Old description".into()));
        assert!(detail_priority_ids(&jobs,1,Some(&cache)).contains("1"));
    }
    #[test]
    fn empty_cached_descriptions_are_not_reused() {
        let jobs=vec![listing("1","Vendeur")];
        let mut cache=HashMap::new();cache.insert("1".into(),(as_source(&jobs[0]),String::new()));
        assert!(detail_priority_ids(&jobs,1,Some(&cache)).contains("1"));
    }
    #[test]
    fn excluded_jobs_cannot_use_detail_request_budget() {
        let jobs=vec![listing("1","Cuisinier"),listing("2","Vendeur")];
        let chosen=detail_priority_ids(&jobs,1,None);
        assert!(!chosen.contains("1"));
        assert!(chosen.contains("2"));
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;
    #[test]
    fn counters_start_at_zero_and_serialize_camel_case() {
        let p=PipelinePreview{fetched:0,pages_fetched:0,fetch_complete:true,inspected:0,eligible:0,excluded:0,unverified:0,detail_found:0,details_requested:0,descriptions_reused:0,detail_failures:0,possible_duplicates:0,database_writes:0,warnings:vec![],items:vec![]};
        let v=serde_json::to_value(&p).unwrap();
        assert_eq!(v["detailsRequested"],0);
        assert_eq!(v["descriptionsReused"],0);
        assert_eq!(v["detailFailures"],0);
    }
}
