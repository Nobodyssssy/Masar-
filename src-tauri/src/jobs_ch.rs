//! Bounded Jobs.ch collector with optional validated detail-page descriptions.
//! Access may be refused by the provider; this code does not bypass access controls.
use crate::source_store::UserListing;
use crate::okjob::extract_ld_json_description;
use serde::Serialize;
use std::{collections::{HashMap,HashSet}, time::Duration};

const API: &str = "https://job-search-api.jobs.ch/search/semantic";
const FILTER: &str = "employmentGradeMin=40&employmentTypeIds=1&employmentTypeIds=4&employmentTypeIds=5&regionIds=52&regionIds=57&rows=20";
const MAX_PAGES: u32 = 5;
const MAX_LISTINGS: usize = 100;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub source: String,
    pub pages_fetched: u32,
    pub inspected: usize,
    pub listings: Vec<UserListing>,
    pub warnings: Vec<String>,
    pub detail_success: usize,
    pub detail_cached: usize,
    pub detail_failed: usize,
}

fn build_url(page:u32)->String { format!("{API}?{FILTER}&page={page}") }
fn search_url(id:&str, title:&str, company:&str)->String {
    let query = format!("{} {}",title,company);
    let mut u=reqwest::Url::parse("https://www.jobs.ch/fr/emplois/").expect("constant URL");
    u.query_pairs_mut().append_pair("term",&query).append_pair("sourceListingId",id);
    return u.to_string();

}
fn detail_url(id:&str)->Option<String>{
    // Only construct a detail link when the source ID has UUID syntax.
    let bits:Vec<&str>=id.split('-').collect();
    if bits.len()!=5 || [8,4,4,4,12].iter().enumerate().any(|(i,&n)| bits[i].len()!=n || !bits[i].bytes().all(|b|b.is_ascii_hexdigit())) { return None; }
    Some(format!("https://www.jobs.ch/fr/offres-emplois/detail/{id}/"))
}
fn first_target_city(doc:&serde_json::Value)->Option<String> {
    let locations=doc.get("locations")?.as_array()?;
    for canton in ["VS","VD"] {
        for loc in locations {
            if loc.get("cantonCode").and_then(|v|v.as_str())==Some(canton) {
                let city=loc.get("city").and_then(|v|v.as_str()).unwrap_or("").trim();
                if !city.is_empty(){return Some(city.to_owned())}
            }
        }
    }
    None
}
fn parse_page(data:&serde_json::Value,seen:&mut HashSet<String>)->Result<Vec<UserListing>,String>{
    let docs=data.get("documents").and_then(|v|v.as_array()).ok_or("Response missing documents array")?;
    let mut out=Vec::new();
    for doc in docs {
        let id=doc.get("id").and_then(|v|v.as_str()).unwrap_or("").trim();
        let title=doc.get("title").and_then(|v|v.as_str()).unwrap_or("").trim();
        if id.is_empty()||title.is_empty() { continue; }
        let Some(place)=first_target_city(doc) else { continue; };
        if !seen.insert(id.to_owned()){continue;}
        let company=doc.get("company").and_then(|v|v.get("name")).and_then(|v|v.as_str()).unwrap_or("").trim();
        out.push(UserListing{
            title:title.into(),company:company.into(),place,
            // Search URL is a fallback, *not* a verified direct vacancy URL.
            url:detail_url(id).unwrap_or_else(||search_url(id,title,company)),description:String::new(),source_id:id.into(),
        });
    }
    Ok(out)
}
pub fn fetch()->Result<Discovery,String>{fetch_with_cache(&HashMap::new())}
pub fn fetch_with_cache(cached:&HashMap<String,String>)->Result<Discovery,String>{
    fetch_with_cache_cancel(cached,&||false)
}
pub fn fetch_with_cache_cancel(cached:&HashMap<String,String>,cancelled:&dyn Fn()->bool)->Result<Discovery,String>{
    let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(25))
        .user_agent("JobupDesktop/0.1 (personal Swiss job search)")
        .redirect(reqwest::redirect::Policy::limited(2)).build().map_err(|e|e.to_string())?;
    let mut result=Discovery{source:"jobs_ch".into(),pages_fetched:0,inspected:0,listings:Vec::new(),warnings:Vec::new(),detail_success:0,detail_cached:0,detail_failed:0};
    let mut seen=HashSet::new();
    for page in 1..=MAX_PAGES {
        if cancelled() { return Err("Refresh cancelled; no Jobs.ch records saved".into()); }
        if result.listings.len()>=MAX_LISTINGS{break;}
        if page>1{std::thread::sleep(Duration::from_millis(250));}
        if cancelled() { return Err("Refresh cancelled; no Jobs.ch records saved".into()); }
        let response=match client.get(build_url(page)).send(){
            Ok(r)=>r,Err(e)=>{if page==1{return Err(format!("Jobs.ch request failed: {e}"))}result.warnings.push(format!("Page {page}: {e}"));break}
        };
        if !response.status().is_success(){
            let msg=format!("Jobs.ch page {page}: HTTP {}",response.status());
            if page==1{return Err(msg)}result.warnings.push(msg);break;
        }
        let data:serde_json::Value=match response.json(){
            Ok(v)=>v,Err(e)=>{if page==1{return Err(format!("Jobs.ch invalid JSON: {e}"))}result.warnings.push(format!("Page {page} invalid JSON: {e}"));break}
        };
        let entries=match parse_page(&data,&mut seen){
            Ok(v)=>v,Err(e)=>{if page==1{return Err(e)}result.warnings.push(e);break}
        };
        let num_pages=data.get("numPages").and_then(|v|v.as_u64()).unwrap_or(0);
        result.pages_fetched+=1;
        result.inspected+=entries.len();
        let room=MAX_LISTINGS.saturating_sub(result.listings.len());
        result.listings.extend(entries.into_iter().take(room));
        if num_pages==0 || u64::from(page)>=num_pages {break;}
    }
    // Only actual UUID detail pages are requested; never treat a search page as detail.
    // Missing or blocked detail pages remain Unverified in the existing classifier.
    let mut requested=0usize;
    let mut failures_net=0usize;
    let mut failures_http=0usize;
    let mut failures_size=0usize;
    let mut failures_parse=0usize;
    for listing in result.listings.iter_mut() {
        if cancelled() { return Err("Refresh cancelled; no Jobs.ch records saved".into()); }
        let Some(url)=detail_url(&listing.source_id) else { continue; };
        if let Some(desc)=cached.get(&listing.source_id) {
            if !desc.trim().is_empty() {
                listing.description=desc.clone();
                result.detail_cached+=1;
                result.detail_success+=1;
                continue;
            }
        }
        if requested>=20 { continue; }
        if requested>0 { std::thread::sleep(Duration::from_millis(200)); }
        requested+=1;
        if cancelled() { return Err("Refresh cancelled; no Jobs.ch records saved".into()); }
        let response=match client.get(&url).send(){
            Ok(r)=>r,
            Err(_)=>{failures_net+=1;continue;}
        };
        if !response.status().is_success()
            || response.url().host_str()!=Some("www.jobs.ch")
            || !response.url().path().starts_with("/fr/offres-emplois/detail/") {
            failures_http+=1;
            continue;
        }
        let html=match response.text(){
            Ok(t) if t.len()<=1_500_000=>t,
            _=>{failures_size+=1;continue;}
        };
        if let Some(desc)=extract_ld_json_description(&html){
            listing.description=desc;
            result.detail_success+=1;
        } else {failures_parse+=1;}
    }
    result.detail_failed=failures_net+failures_http+failures_size+failures_parse;
    if result.detail_failed>0 {
        result.warnings.push(format!(
            "Jobs.ch detail fetch: {} fetched, {} cached; {} network, {} HTTP/redirect, {} invalid/oversized, {} missing JSON-LD",
            result.detail_success.saturating_sub(result.detail_cached), result.detail_cached,
            failures_net, failures_http, failures_size, failures_parse
        ));
    }
    Ok(result)
}
#[cfg(test)] mod tests {
use super::*;
#[test] fn filters_vs_vd(){let mut seen=HashSet::new();let v=serde_json::json!({"documents":[{"id":"1","title":"Vendeur","locations":[{"cantonCode":"ZH","city":"Zurich"}]},{"id":"2","title":"Assistant","locations":[{"cantonCode":"VS","city":"Sion"}]}]});let r=parse_page(&v,&mut seen).unwrap();assert_eq!(r.len(),1);assert_eq!(r[0].place,"Sion");}
#[test] fn same_id_deduplicated(){let mut seen=HashSet::new();let v=serde_json::json!({"documents":[{"id":"2","title":"Assistant","locations":[{"cantonCode":"VD","city":"Lausanne"}]}]});assert_eq!(parse_page(&v,&mut seen).unwrap().len(),1);assert!(parse_page(&v,&mut seen).unwrap().is_empty());}
#[test] fn missing_documents_is_failure(){let mut seen=HashSet::new();assert!(parse_page(&serde_json::json!({}),&mut seen).is_err());}
#[test] fn search_link_is_not_faked_detail(){let u=search_url("example123","Vendeur 80%","Boutique SA");assert!(u.contains("term="));assert!(!u.contains("/detail/"));}
#[test] fn detail_url_rejects_non_uuid(){assert!(detail_url("not-an-id").is_none());}
#[test] fn detail_url_uses_vacancy_path(){assert_eq!(detail_url("a6a10b48-c5ce-4392-93ba-0aefae34d436").unwrap(),"https://www.jobs.ch/fr/offres-emplois/detail/a6a10b48-c5ce-4392-93ba-0aefae34d436/");}
#[test] fn detail_url_uses_offres_emplois_path(){
    let id="79919c1d-0042-4319-ade3-7e2f942d0cd8";
    let url=detail_url(id).unwrap();
    assert!(url.contains("/fr/offres-emplois/detail/"));
    assert!(!url.contains("/fr/emplois/detail/"));
    assert!(url.ends_with(&format!("{id}/")));
}
#[test] fn url_contains_region_filters(){let u=build_url(1);assert!(u.contains("regionIds=52"));assert!(u.contains("regionIds=57"));assert!(u.contains("page=1"));}
}
