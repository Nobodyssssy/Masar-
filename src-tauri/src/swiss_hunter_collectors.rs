//! Optional public-web collector paths inspired by Swiss Job Hunter's provider behavior.
//! Designed independently in Rust. No authenticated endpoints, CAPTCHA bypass or automated submissions.
use crate::source_store::UserListing;
use chrono::{DateTime, Utc};
use reqwest::blocking::Client;
use reqwest::Url;
use unicode_normalization::UnicodeNormalization;
use scraper::{Html, Selector};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

const MAX_PAGES: usize = 3;
const MAX_LISTINGS: usize = 80;
const MAX_DETAILS: usize = 24;
const MAX_HTML: usize = 3_000_000;
const GAP: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct CollectedJob { pub listing: UserListing, pub published_at: Option<String>, pub grades: Vec<u32> }
#[derive(Debug)]
pub struct Discovery { pub source: String, pub pages: usize, pub inspected: usize, pub details: usize, pub jobs: Vec<CollectedJob>, pub warnings: Vec<String> }
impl Discovery { fn new(source: &str) -> Self { Self { source: source.to_string(), pages:0, inspected:0, details:0, jobs:Vec::new(), warnings:Vec::new() } } }
fn client()->Result<Client,String>{
    Client::builder().timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::limited(2))
        .user_agent("JobupDesktop/0.1 (personal Swiss vacancy research)")
        .build().map_err(|e|e.to_string())
}
fn get(client:&Client,url:&str,allowed:&str)->Result<String,String>{
    let response=client.get(url).send().map_err(|e|format!("request failed: {e}"))?;
    let status=response.status();
    if !status.is_success(){return Err(format!("HTTP {status} for {allowed} — no access-control bypass attempted"));}
    let final_url=response.url();
    if final_url.scheme()!="https" {return Err("insecure redirect rejected".into());}
    let domain=final_url.host_str().unwrap_or("");
    if domain!=allowed && !domain.ends_with(&format!(".{allowed}")){return Err(format!("unexpected redirect host: {domain}"));}
    if response.content_length().is_some_and(|v|v>MAX_HTML as u64){return Err("response exceeds safe size".into());}
    let data=response.text().map_err(|e|e.to_string())?;
    if data.len()>MAX_HTML {return Err("response exceeds safe size".into());}
    if data.contains("Just a moment...")||data.contains("cf-challenge") {return Err("Cloudflare access challenge: blocked".into());}
    Ok(data)
}
fn json_ld(doc:&Html)->Vec<Value>{
    let Ok(sel)=Selector::parse("script[type='application/ld+json']") else {return vec![]};
    doc.select(&sel).filter_map(|s|serde_json::from_str::<Value>(&s.text().collect::<String>()).ok()).collect()
}
fn posting(v:&Value)->bool {
    match v.get("@type") {Some(Value::String(t))=>t=="JobPosting" || t.ends_with("/JobPosting"),Some(Value::Array(a))=>a.iter().any(|x|x.as_str()==Some("JobPosting")),_=>false}
}
fn walk_postings(v:&Value, output:&mut Vec<Value>){
    if posting(v) {output.push(v.clone());return;}
    match v {Value::Array(items)=>for a in items{walk_postings(a,output)},Value::Object(obj)=>{
        // ItemList elements wrap individual postings inside `item`.
        for (key,val) in obj { if key=="itemListElement"||key=="item"||key=="@graph"||key=="mainEntity" {walk_postings(val,output);} }
    },_=>{}}
}
fn read_text<'a>(v:&'a Value,key:&str)->&'a str{v.get(key).and_then(Value::as_str).unwrap_or("")}
fn city_from_json(v:&Value)->String {
    let Some(loc)=v.get("jobLocation") else{return String::new()};
    let first=loc.as_array().and_then(|a|a.first()).unwrap_or(loc);
    let Some(address)=first.get("address") else {return String::new()};
    let city=read_text(address,"addressLocality");let region=read_text(address,"addressRegion");
    if !city.is_empty(){city.to_string()}else{region.to_string()}
}
fn grade_percent(s:&str)->Vec<u32>{
    let Ok(re)=regex::Regex::new(r"(?i)\b(\d{1,3})\s*%?\s*[-–—]\s*(\d{1,3})\s*%|\b(\d{1,3})\s*%") else{return vec![]};
    let mut grades:Vec<u32>=re.captures_iter(s).flat_map(|m|(1..=3).filter_map(move |i|m.get(i).and_then(|s|s.as_str().parse::<u32>().ok()))).filter(|n|*n<=100).take(3).collect();
    grades.dedup(); grades
}
fn date_from_iso(s:&str)->Option<String>{
    if let Ok(ts)=DateTime::parse_from_rfc3339(s){return Some(ts.with_timezone(&Utc).format("%Y-%m-%d").to_string());}
    chrono::NaiveDate::parse_from_str(s.get(..10)?,"%Y-%m-%d").ok().map(|d|d.format("%Y-%m-%d").to_string())
}
fn safe_job_url(raw:&str,host:&str)->Option<String>{
    let url=Url::parse(raw).ok()?;
    if url.scheme()!="https" || url.host_str().is_none_or(|x|x!=host&& !x.ends_with(&format!(".{host}"))) || !url.username().is_empty() || url.password().is_some(){return None;}
    Some(url.to_string())
}
fn parse_posting(v:&Value, host:&str, fallback_url:&str, workload:&str)->Option<CollectedJob>{
    let title=read_text(v,"title").trim();if title.is_empty()||title.len()>300{return None;}
    let rawurl=read_text(v,"url");let url=safe_job_url(if rawurl.is_empty(){fallback_url}else{rawurl},host)?;
    let company=read_text(v.get("hiringOrganization").unwrap_or(&Value::Null),"name").to_string();
    let mut desc=read_text(v,"description").trim().to_string();
    if desc.starts_with("We are looking for ") && desc.ends_with(" to join our team.") {desc.clear();}
    if desc.len()<180 || desc.len()>40_000 {desc.clear();}
    Some(CollectedJob{listing:UserListing{title:title.to_string(),company,place:city_from_json(v),url,description:desc,source_id:read_text(v.get("identifier").unwrap_or(&Value::Null),"value").to_string()},published_at:date_from_iso(read_text(v,"datePosted")),grades:grade_percent(workload)})
}
fn details_from_html(html:&str, host:&str,url:&str)->Option<CollectedJob>{
    let doc=Html::parse_document(html);let mut posts=vec![];for root in json_ld(&doc){walk_postings(&root,&mut posts);}
    posts.into_iter().find_map(|v|parse_posting(&v,host,url,""))
}
fn parse_jobcloud_search(html:&str,host:&str)->Result<Vec<CollectedJob>,String>{
    let doc=Html::parse_document(html);
    let mut posts=vec![];
    for v in json_ld(&doc){walk_postings(&v,&mut posts);}
    if posts.is_empty(){
        let empty=json_ld(&doc).iter().any(|v|v.get("@type").and_then(Value::as_str)==Some("ItemList") && v.get("itemListElement").and_then(Value::as_array).is_some_and(|items|items.is_empty()));
        if empty{return Ok(vec![]);}
        return Err("search HTML has no structured JobPosting results; parser or site may have changed".into());
    }
    // The visible result cards carry accurate city/workload even where JSON-LD does not.
    let mut cards=std::collections::HashMap::<String,(String,String)>::new();
    if let Ok(sel)=Selector::parse("[data-cy='serp-item']") {
        let links=Selector::parse("a[href*='/detail/']").unwrap();
        let p=Selector::parse("p").unwrap();
        let sib=Selector::parse("span").unwrap();
        let re=regex::Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F-]{23,}").unwrap();
        for card in doc.select(&sel) {
            let Some(url)=card.select(&links).find_map(|a|a.value().attr("href")) else {continue};
            let Some(id)=re.find(url).map(|m|m.as_str().to_ascii_lowercase()) else {continue};
            let mut city=String::new();let mut workload=String::new();
            for paragraph in card.select(&p) {
                let Some(parent)=paragraph.parent().and_then(scraper::ElementRef::wrap) else {continue};
                let labels=parent.select(&sib).map(|e|e.text().collect::<String>().to_ascii_lowercase()).collect::<Vec<_>>();
                let value=paragraph.text().collect::<Vec<_>>().join(" ");
                if labels.iter().any(|x|x.contains("place of work")||x.contains("lieu de travail")||x.contains("arbeitsort")){city=value.clone();}
                if labels.iter().any(|x|x.contains("workload")||x.contains("taux d'occupation")||x.contains("pensum")){workload=value;}
            }
            cards.insert(id,(city,workload));
        }
    }
    let re=regex::Regex::new(r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
    let mut out=vec![];
    for v in posts {
        let url=read_text(&v,"url");
        let uuid=re.find(url).map(|m|m.as_str().to_lowercase()).or_else(||v.get("identifier").and_then(|x|x.get("value")).and_then(Value::as_str).map(str::to_lowercase));
        let extra=uuid.as_ref().and_then(|id|cards.get(id));
        if let Some(mut job)=parse_posting(&v,host,url,extra.map(|(_,w)|w.as_str()).unwrap_or("")) {
            if let Some((city,_))=extra {if !city.trim().is_empty(){job.listing.place=city.trim().to_string();}}
            out.push(job);
        }
    }
    Ok(out)
}
fn find_postcode(client:&Client,city:&str)->Result<String,String>{
    if city.trim().is_empty(){return Err("JobScout24 requires a location for a scoped search".into());}
    let mut url=Url::parse("https://www.jobscout24.ch/en/jobsearch/Cities/").unwrap();url.query_pairs_mut().append_pair("namePart",city);
    let text=get(client,url.as_str(),"jobscout24.ch")?;
    let parsed:Value=serde_json::from_str(&text).map_err(|_|"JobScout24 city lookup did not return JSON".to_string())?;
    let arr=parsed.as_array().ok_or("JobScout24 city lookup is not a list")?;
    let fold=|s:&str|s.nfd().filter(|c|!unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase();
    // Do NOT use the first suggestion silently: that can target the wrong municipality.
    arr.iter().find(|x|fold(read_text(x,"Text"))==fold(city)).and_then(|x|x.get("Value")).and_then(Value::as_str).map(str::to_string)
        .ok_or_else(||format!("No exact JobScout24 postcode for {city}; refusing nationwide fallback"))
}
fn jobscout(client:&Client,query:&str,location:&str,cancel:&dyn Fn()->bool)->Result<Discovery,String>{
    let mut out=Discovery::new("jobscout24");let mut seen=HashSet::new();
    let cities=if location.trim().is_empty(){vec![String::new()]} else {location.split(',').map(str::trim).filter(|x|!x.is_empty()).take(2).map(str::to_string).collect()};
    let re=regex::Regex::new(r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
    let item_sel=Selector::parse("li.job-list-item[data-job-detail-url]").unwrap();
    for city in cities {if cancel(){return Err("Refresh cancelled before staging".into());}
        let postcode=if city.is_empty(){String::new()}else{find_postcode(client,&city)?};
        for page in 1..=MAX_PAGES {
            if cancel(){return Err("Refresh cancelled before staging".into());}
            if out.pages>0 {std::thread::sleep(GAP);}
            let mut url=Url::parse("https://www.jobscout24.ch/en/jobs/").unwrap();
            url.query_pairs_mut().append_pair("ft",query).append_pair("psz",&postcode).append_pair("p",&page.to_string());
            let body=match get(client,url.as_str(),"jobscout24.ch") {Ok(v)=>v,Err(e)=>{if page>1 && e.contains("404"){break;}if page==1&&out.jobs.is_empty(){return Err(e);}out.warnings.push(e);break;}};
            out.pages+=1;let document=Html::parse_document(&body);let mut uuids=vec![];
            for item in document.select(&item_sel) {
                if let Some(u)=item.value().attr("data-job-detail-url").and_then(|x|re.find(x)){let id=u.as_str().to_lowercase();if seen.insert(id.clone()){uuids.push(id);}}
            }
            if uuids.is_empty(){if body.contains("job-list-item") || out.pages>1 {break;}return Err("JobScout24 search response has no recognized job cards".into());}
            for id in uuids {if out.jobs.len()>=MAX_LISTINGS{break;}
                if cancel(){return Err("Refresh cancelled before staging".into());}
                std::thread::sleep(GAP);
                let link=format!("https://www.jobscout24.ch/en/job/{id}/");
                match get(client,&link,"jobscout24.ch").ok().and_then(|h|details_from_html(&h,"jobscout24.ch",&link)) {
                    Some(mut job)=>{job.listing.source_id=id;out.details+=usize::from(!job.listing.description.is_empty());out.jobs.push(job);},
                    None=>out.warnings.push(format!("One detail page unavailable or missing JobPosting JSON-LD: {id}")),
                }
            }
            if out.jobs.len()>=MAX_LISTINGS{break;}
        }
    }
    out.inspected=out.jobs.len();Ok(out)
}
fn jobcloud(client:&Client,source:&str,keywords:&str,location:&str,cancel:&dyn Fn()->bool)->Result<Discovery,String>{
    let (host,base)=if source=="jobs_ch"{("jobs.ch","https://www.jobs.ch/en/vacancies/")}else{("jobup.ch","https://www.jobup.ch/en/jobs/")};
    let mut out=Discovery::new(source);let mut seen=HashSet::new();
    let searches=if keywords.trim().is_empty(){vec!["".to_string()]}else{keywords.split(',').map(str::trim).filter(|s|!s.is_empty()).take(3).map(str::to_string).collect()};
    for keyword in searches {for page in 1..=MAX_PAGES {
        if cancel(){return Err("Refresh cancelled before staging".into());}
        if out.pages>0 {std::thread::sleep(GAP);}
        let mut url=Url::parse(base).unwrap();url.query_pairs_mut().append_pair("term",&keyword).append_pair("page",&page.to_string());
        if !location.trim().is_empty(){url.query_pairs_mut().append_pair("location",location);}
        let body=match get(client,url.as_str(),host){Ok(v)=>v,Err(e)=>{if page==1&&out.jobs.is_empty(){return Err(e);}out.warnings.push(e);break;}};
        out.pages+=1;let jobs=match parse_jobcloud_search(&body,host){Ok(j)=>j,Err(e)=>{if out.jobs.is_empty(){return Err(e);}out.warnings.push(e);break;}};
        // Detect the known JobCloud failure mode where a query parameter is ignored.
        // Do not misreport a full unrelated search page as valid keyword results.
        if !keyword.trim().is_empty() && jobs.len()>=20 {
            let parts:Vec<String>=keyword.split_whitespace().filter(|t|t.len()>2).map(|x|x.to_lowercase()).collect();
            if !parts.is_empty() && !jobs.iter().any(|j| {
                let s=format!("{} {}",j.listing.title,j.listing.description).to_lowercase();
                parts.iter().any(|p|s.contains(p))
            }) { return Err(format!("{source} returned 20+ unrelated vacancies; keyword filter may be ignored by provider")); }
        }
        let n=jobs.len();let mut fresh=0;
        for job in jobs {if out.jobs.len()>=MAX_LISTINGS{break;}
            if !seen.insert(job.listing.url.clone()){continue;}
            fresh+=1;out.jobs.push(job);
        }
        if fresh==0||n<20||out.jobs.len()>=MAX_LISTINGS{break;}
    }}
    for job in out.jobs.iter_mut().filter(|j|j.listing.description.is_empty()).take(MAX_DETAILS) {
        if cancel(){return Err("Refresh cancelled before staging".into());}
        std::thread::sleep(GAP);
        if let Ok(body)=get(client,&job.listing.url,host) {
            if let Some(enriched)=details_from_html(&body,host,&job.listing.url) {
                if !enriched.listing.description.is_empty(){job.listing.description=enriched.listing.description;out.details+=1;}
                if job.published_at.is_none(){job.published_at=enriched.published_at;}
            }
        }
    }
    out.inspected=out.jobs.len();Ok(out)
}
fn indeed(client:&Client,keywords:&str,location:&str,days:u32,cancel:&dyn Fn()->bool)->Result<Discovery,String>{
    let mut out=Discovery::new("indeed_ch");let mut seen=HashSet::new();
    let terms=if keywords.trim().is_empty(){vec!["vente".to_string(),"logistique".to_string()]}else{keywords.split(',').map(str::trim).filter(|s|!s.is_empty()).take(3).map(str::to_string).collect()};
    for term in terms {for page in 0..MAX_PAGES {
        if cancel(){return Err("Refresh cancelled before staging".into());}
        if out.pages>0{std::thread::sleep(GAP);}
        let mut url=Url::parse("https://ch.indeed.com/rss").unwrap();
        url.query_pairs_mut().append_pair("q",&term).append_pair("l",if location.trim().is_empty(){"Valais"}else{location}).append_pair("fromage",&days.to_string()).append_pair("sort","date").append_pair("start",&(page*10).to_string());
        let body=match get(client,url.as_str(),"indeed.com") {Ok(v)=>v,Err(e)=>{if out.jobs.is_empty(){return Err(e);}out.warnings.push(e);break;}};
        let doc=roxmltree::Document::parse(&body).map_err(|_|"Indeed RSS did not return valid XML (blocked or discontinued feed)".to_string())?;
        let items:Vec<_>=doc.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="item").collect();
        if items.is_empty(){if out.jobs.is_empty(){return Err("Indeed RSS returned no item elements; endpoint may be unavailable".into());}break;}
        out.pages+=1;
        let mut fresh=0;
        for item in items {if out.jobs.len()>=MAX_LISTINGS{break;}
            let field=|key:&str|item.children().find(|x|x.is_element()&&x.tag_name().name()==key).and_then(|x|x.text()).unwrap_or("").trim().to_string();
            let title=field("title");let url=field("link");
            if title.is_empty()||title.len()>300||safe_job_url(&url,"indeed.com").is_none()||!seen.insert(url.clone()){continue;}
            let company={let x=field("company");if x.is_empty(){field("source")}else{x}};
            let city=field("city");let state=field("state");let place=if city.is_empty(){state}else{city};
            let description=field("description");
            let description=html_escape::decode_html_entities(&description).to_string();
            let pubdate=DateTime::parse_from_rfc2822(&field("pubDate")).ok().map(|x|x.with_timezone(&Utc).format("%Y-%m-%d").to_string());
            out.jobs.push(CollectedJob{listing:UserListing{title,company,place,url,description:if description.len()>=150&&description.len()<=40_000 {description}else{String::new()},source_id:field("jobkey")},published_at:pubdate,grades:vec![]});fresh+=1;
        }
        if fresh==0||out.jobs.len()>=MAX_LISTINGS{break;}
    }}
    out.inspected=out.jobs.len();Ok(out)
}
include!("swiss_extra_collectors.rs");
include!("swiss_firefox_collectors.rs");
pub fn collect(source:&str,keywords:&str,location:&str,days:u32,cancel:&dyn Fn()->bool)->Result<Discovery,String>{
    if ![1,3,7,14,30].contains(&days)||keywords.len()>220||location.len()>140{return Err("Invalid search parameters".into());}
    let client=client()?;
    match source {
        "jobup"|"jobs_ch"=>jobcloud(&client,source,keywords,location,cancel),
        "jobscout24"=>jobscout(&client,keywords,location,cancel),
        "indeed_ch"=>indeed(&client,keywords,location,days,cancel),
        "zuri_jobs"=>collect_zuri(&client,keywords,location,cancel),
        "michael_page"=>collect_michael_page(&client,keywords,location,cancel),
        "linkedin_jobs"=>collect_linkedin(&client,keywords,location,days,cancel),
        "swissdevjobs"|"efinancialcareers"=>collect_firefox(source,keywords,location,days,cancel),
        "job_room"=>Err("Job-Room does not expose a verified public vacancy-search API. Its documented Jobs API requires employer credentials. Open Job-Room manually; no listings invented.".into()),
        _=>Err("Unsupported collector source".into()),
    }
}
#[cfg(test)] mod tests {use super::*;
    #[test]fn parse_jobcloud_jsonld() {
        let sample=r#"<script type="application/ld+json">{"@type":"ItemList","itemListElement":[{"item":{"@type":"JobPosting","title":"Conseiller clientèle","url":"https://www.jobs.ch/en/vacancies/detail/a12a1234-b123-c123-d123-123456789abc/","datePosted":"2026-10-26T10:00:00+02:00","hiringOrganization":{"name":"Shop SA"},"jobLocation":{"address":{"addressLocality":"Sion"}}}}]}</script>"#;
        let jobs=parse_jobcloud_search(sample,"jobs.ch").unwrap();assert_eq!(jobs.len(),1);assert_eq!(jobs[0].listing.place,"Sion");assert_eq!(jobs[0].published_at.as_deref(),Some("2026-10-26"));
    }
    #[test]fn safe_urls(){assert!(safe_job_url("https://other.example/", "jobs.ch").is_none());assert!(safe_job_url("http://www.jobs.ch/", "jobs.ch").is_none());assert!(safe_job_url("https://www.jobs.ch/en/vacancies/detail/a", "jobs.ch").is_some());}
    #[test]fn workload(){assert_eq!(grade_percent("80–100%"),vec![80,100]);assert_eq!(grade_percent("80%–100%"),vec![80,100]);}
    #[test]fn no_fabricated_empty_success(){let r=collect("job_room","vente","Sion",3,&||false);assert!(r.is_err());}
}

#[cfg(test)] mod masar_live_verification {
    use super::*;
    #[test]
    #[ignore = "Live public websites; run explicitly with --ignored --nocapture"]
    fn swiss_sources_live() {
        for (source,keyword,location) in [
            ("jobup","vendeur","Sion"),("jobs_ch","logistique","Lausanne"),
            ("jobscout24","administration","Lausanne"),("linkedin_jobs","warehouse","Switzerland"),
            ("michael_page","administration",""),("zuri_jobs","Sachbearbeiter","Zürich"),
            ("swissdevjobs","",""),("efinancialcareers","",""),
        ] {
            let start=std::time::Instant::now();
            match collect(source,keyword,location,7,&||false) {
                Ok(result)=>println!("MASAR_LIVE {source}: parsed={} descriptions={} pages={} warnings={} elapsed={}s",result.jobs.len(),result.details,result.pages,result.warnings.join("; "),start.elapsed().as_secs()),
                Err(error)=>println!("MASAR_LIVE {source}: UNAVAILABLE {error} elapsed={}s",start.elapsed().as_secs()),
            }
        }
    }
}


#[cfg(test)] mod masar_cancel_tests {
    #[test] fn cancellation_prevents_network_work(){for source in ["jobup","jobs_ch","jobscout24","linkedin_jobs","michael_page","zuri_jobs","swissdevjobs","efinancialcareers"]{assert!(super::collect(source,"","",7,&||true).unwrap_err().to_lowercase().contains("cancel"),"{source}");}}
}
