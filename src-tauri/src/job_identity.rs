//! Provider-neutral job records and conservative duplicate preview.
//! No database operations. False positives are intentionally more costly than misses.
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SourceJob {
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub company: String,
    pub place: String,
    pub url: String,
    #[serde(default)] pub grades: Vec<u32>,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub employer_application_url: Option<String>,
    #[serde(default)] pub published_at: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct DuplicateResult {
    pub relation: String, // same_listing | same_opportunity | possible_duplicate | distinct
    pub reason: String,
}
fn fold(s:&str)->String {
    s.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase).collect()
}
fn words(s:&str)->Vec<String> {
    fold(s).split(|c:char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w|w.to_owned()).collect()
}
fn key(s:&str)->String { words(s).join(" ") }
fn title_words(s:&str)->BTreeSet<String> {
    words(s).into_iter().filter(|w| !matches!(w.as_str(), "de"|"du"|"des"|"d"|"le"|"la"|"les"|"et"|"en"|"a"|"the"|"of"|"for"|"m"|"f"|"x"|"h"|"feminin"|"masculin"|"100"|"80"|"60"|"50"|"40"))
        .collect()
}
fn title_overlap(a:&str,b:&str)->f32 {
    let a=title_words(a);let b=title_words(b);
    if a.is_empty() || b.is_empty() {return 0.;}
    a.intersection(&b).count() as f32 / a.union(&b).count() as f32
}
pub fn canonical_url(value:&str)->String {
    let Ok(mut url)=reqwest::Url::parse(value.trim())else{return value.trim().to_string();};
    url.set_fragment(None);
    let mut pairs:Vec<(String,String)>=url.query_pairs().filter(|(name,_)|{
        let name=name.to_ascii_lowercase();!name.starts_with("utm_")&&!matches!(name.as_str(),"gclid"|"fbclid"|"ref"|"source"|"trackingid")
    }).map(|(k,v)|(k.into_owned(),v.into_owned())).collect();
    pairs.sort();url.set_query(None);
    if !pairs.is_empty(){url.query_pairs_mut().extend_pairs(pairs);}
    let path=url.path().trim_end_matches('/').to_string();url.set_path(&path);
    url.to_string().trim_end_matches('/').to_string()
}
fn grade_compatible(a:&[u32], b:&[u32])->bool {
    a.is_empty()||b.is_empty()||a.iter().any(|x|b.contains(x))
}
pub fn compare(a:&SourceJob,b:&SourceJob)->DuplicateResult {
    let result=|relation:&str,reason:&str| DuplicateResult{relation:relation.into(),reason:reason.into()};
    if a.source==b.source && !a.source_id.is_empty() && a.source_id==b.source_id {
        return result("same_listing","Identical provider and listing ID");
    }
    if let (Some(x),Some(y))=(&a.employer_application_url,&b.employer_application_url) {
        if !x.is_empty() && !y.is_empty() && canonical_url(x)==canonical_url(y) {
            return result("same_opportunity","Identical employer application destination");
        }
    }
    if !a.url.is_empty() && !b.url.is_empty() && canonical_url(&a.url)==canonical_url(&b.url) {
        return result("same_opportunity","Identical canonical listing URL");
    }
    let company=key(&a.company)==key(&b.company) && !key(&a.company).is_empty();
    let location=key(&a.place)==key(&b.place) && !key(&a.place).is_empty();
    let overlap=title_overlap(&a.title,&b.title);
    if company && location && overlap>=0.90 && grade_compatible(&a.grades,&b.grades) {
        return result("possible_duplicate","Employer, locality and role closely match; do not merge automatically without shared listing/application identity");
    }
    if company && location && overlap>=0.55 {
        return result("possible_duplicate","Similar employer, location and role; manual review needed");
    }
    result("distinct","No sufficiently strong matching identity")
}
#[cfg(test)]
mod tests {
 use super::*;
 fn j(src:&str,id:&str,title:&str)->SourceJob {SourceJob{source:src.into(),source_id:id.into(),title:title.into(),company:"Société Étoile".into(),place:"Vétroz".into(),url:format!("https://{src}.example/jobs/{id}"),grades:vec![80,100],description:None,employer_application_url:None,published_at:None}}
 #[test] fn same_id_only_with_same_source(){assert_eq!(compare(&j("jobup","1","Vendeur"),&j("jobup","1","Vendeur")).relation,"same_listing");assert_ne!(compare(&j("jobup","1","Vendeur"),&j("indeed","1","Vendeur")).relation,"same_listing");}
 #[test] fn cross_site_same_application_url(){let mut a=j("jobup","1","Vendeur");let mut b=j("jobs","2","Conseiller vente");a.employer_application_url=Some("https://employer.ch/jobs/55?utm_source=jobup".into());b.employer_application_url=Some("https://employer.ch/jobs/55?utm_source=jobs".into());assert_eq!(compare(&a,&b).relation,"same_opportunity")}
 #[test] fn similar_titles_do_not_auto_merge(){assert_eq!(compare(&j("jobup","1","Vendeur"),&j("jobs","2","Vendeur")).relation,"possible_duplicate")}
 #[test] fn different_occupation_is_distinct(){assert_eq!(compare(&j("jobup","1","Vendeur"),&j("jobs","2","Magasinier")).relation,"distinct")}
 #[test] fn different_company_is_distinct(){let a=j("jobup","1","Vendeur");let mut b=j("jobs","2","Vendeur");b.company="Autre entreprise".into();assert_eq!(compare(&a,&b).relation,"distinct")}
 #[test] fn different_location_is_distinct(){let a=j("jobup","1","Vendeur");let mut b=j("jobs","2","Vendeur");b.place="Sion".into();assert_eq!(compare(&a,&b).relation,"distinct")}
 #[test] fn accent_normalization(){assert_eq!(key("Vétroz"),key("Vetroz"))}
}

#[cfg(test)] mod masar_canonical_tests {
    #[test] fn preserves_query_job_identity(){assert_ne!(super::canonical_url("https://ch.indeed.com/viewjob?jk=one"),super::canonical_url("https://ch.indeed.com/viewjob?jk=two"));assert_eq!(super::canonical_url("https://example.com/jobs/One?utm_source=test#x"),super::canonical_url("https://example.com/jobs/One"));assert_ne!(super::canonical_url("https://example.com/jobs/One"),super::canonical_url("https://example.com/jobs/one"));}
}
