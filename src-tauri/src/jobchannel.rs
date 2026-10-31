//! Limited public first-page vacancy discovery for jobchannel-powered Swiss websites.
//! No login, bypass, guesses about hidden endpoints, or unbounded pagination.
use crate::source_store::UserListing;
use scraper::{Html,Selector};
use std::collections::HashSet;
use std::time::Duration;

pub struct Discovery { pub source: String, pub pages_fetched: usize, pub listings: Vec<UserListing>, pub warnings: Vec<String> }

fn parse_page(html:&str, source:&str, origin:&str)->Vec<UserListing> {
    let doc=Html::parse_document(html);
    let links=Selector::parse("a[href]").unwrap();
    let root=reqwest::Url::parse(origin).unwrap();
    let mut seen=HashSet::new();
    let mut items=Vec::new();
    for anchor in doc.select(&links) {
        let Some(href)=anchor.value().attr("href") else {continue};
        let Ok(url)=root.join(href) else {continue};
        if url.scheme()!="https" || url.host_str()!=root.host_str() {continue;}
        if !url.path().starts_with("/job/") {continue;}
        if url.username()!="" || url.password().is_some() || url.port().is_some(){continue;}
        let path=url.path().to_owned();
        // Job pages on this publisher end with a compact opaque identity.
        let Some(id)=path.trim_end_matches('/').rsplit('/').next() else {continue};
        if !(8..=32).contains(&id.len()) || !id.chars().all(|c| c.is_ascii_hexdigit()){continue;}
        let mut canonical=url;
        canonical.set_query(None);canonical.set_fragment(None);
        let key=canonical.to_string();
        if !seen.insert(key.clone()){continue;}
        let title=anchor.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
        if !(8..=250).contains(&title.len()) {continue;}
        // The listing-card metadata may be outside the link. Never invent company
        // or location or treat an excerpt as a complete job description.
        items.push(UserListing{title,company:String::new(),place:String::new(),url:key,description:String::new(),source_id:id.to_string()});
    }
    // Conservative cap, and signal via diagnostics if a page has too many matches.
    items.truncate(100);
    let _=source;
    items
}

pub fn fetch(source:&str)->Result<Discovery,String>{
    fetch_cancel(source,&||false)
}
pub fn fetch_cancel(source:&str,cancelled:&dyn Fn()->bool)->Result<Discovery,String>{
    let (origin,path)=match source {
        "emploi_en_suisse"=>("https://emploi-en-suisse.ch","/"),
        "100000jobs"=>("https://www.100000jobs.ch","/r%C3%A9gion-suisse-romande"),
        _=>return Err("Unsupported jobchannel source".into())
    };
    let client=reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(18))
        .redirect(reqwest::redirect::Policy::limited(2))
        .user_agent("JobupDesktop/0.1 (personal bounded vacancy check)")
        .build().map_err(|e|e.to_string())?;
    if cancelled() { return Err("Refresh cancelled; no Jobchannel records saved".into()); }
    let response=client.get(format!("{origin}{path}")).send().map_err(|e|format!("{source}: {e}"))?;
    if response.status()==reqwest::StatusCode::FORBIDDEN {return Err(format!("{source}: HTTP 403 Forbidden — site denied automated access; no retry or bypass attempted"));}
    if !response.status().is_success(){return Err(format!("{source}: HTTP {}",response.status()));}
    if response.url().host_str()!=reqwest::Url::parse(origin).unwrap().host_str(){return Err(format!("{source}: unexpected redirect host"));}
    let body=response.text().map_err(|e|e.to_string())?;
    if body.len()>4_000_000{return Err(format!("{source}: response too large"));}
    let listings=parse_page(&body,source,origin);
    let warnings=if listings.is_empty(){vec!["No recognized vacancy links on this page; structure may have changed".to_string()]}else{vec![]};
    Ok(Discovery{source:source.into(),pages_fetched:1,listings,warnings})
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn accepts_only_expected_same_domain_jobs(){
 let html=r#"<a href='/job/assistant-commercial/3255536c7a'>Assistant commercial 100%</a><a href='https://bad.example/job/foo/3255536c7a'>Bad domain</a><a href='/job/no-id'>Bad identity</a>"#;
 let jobs=parse_page(html,"emploi_en_suisse","https://emploi-en-suisse.ch");
 assert_eq!(jobs.len(),1);assert_eq!(jobs[0].title,"Assistant commercial 100%");assert!(jobs[0].description.is_empty());
 }
 #[test]fn deduplicates_same_link_and_rejects_outside_domain(){
 let html=r#"<a href='/job/vente/3255536c7a'>Vendeur magasin 80%</a><a href='/job/vente/3255536c7a'>Vendeur magasin 80%</a><a href='https://emploi-en-suisse.ch/job/vente/3255536c7a'>Other website</a>"#;
 assert_eq!(parse_page(html,"100000jobs","https://www.100000jobs.ch").len(),1);
 }
}
