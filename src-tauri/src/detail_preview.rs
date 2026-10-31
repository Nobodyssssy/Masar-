//! Conservative preview-only detail fetch. No database writes, no automatic classification.
//! Jobup page markup was not verifiable from this environment: treat absent JSON-LD as unavailable.
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

const MAX_HTML_BYTES: usize = 2_000_000;
const MAX_DESCRIPTION_CHARS: usize = 100_000;

#[derive(Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct DetailPreview {
    pub id: String,
    pub url: String,
    pub status: String,
    pub title: Option<String>,
    pub description_html: Option<String>,
    pub extraction: Option<String>,
    pub warning: Option<String>,
}

fn valid_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().enumerate().all(|(i,c)| match i {
        8|13|18|23 => c == b'-',
        _ => c.is_ascii_hexdigit(),
    })
}

fn job_url(id:&str)->Result<String,String> {
    if !valid_id(id) { return Err("Invalid job ID: expected UUID".into()); }
    Ok(format!("https://www.jobup.ch/fr/emplois/detail/{id}/"))
}

fn is_job_posting(v:&Value)->bool {
    match v.get("@type") {
        Some(Value::String(s)) => s == "JobPosting" || s.ends_with("/JobPosting"),
        Some(Value::Array(a)) => a.iter().any(|x| x.as_str().is_some_and(|s| s == "JobPosting" || s.ends_with("/JobPosting"))),
        _ => false,
    }
}
fn find_posting(v:&Value)->Option<&Value> {
    if is_job_posting(v) {return Some(v)}
    match v {
        Value::Array(a) => a.iter().find_map(find_posting),
        Value::Object(o) => o.get("@graph").and_then(find_posting),
        _ => None,
    }
}

fn extract(html:&str)->Option<(Option<String>,String)> {
    let doc = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("script[type='application/ld+json']").ok()?;
    for script in doc.select(&selector) {
        let json = script.text().collect::<String>();
        if let Ok(root) = serde_json::from_str::<Value>(&json) {
            if let Some(posting) = find_posting(&root) {
                if let Some(description) = posting.get("description").and_then(Value::as_str) {
                    if !description.trim().is_empty() && description.chars().count() <= MAX_DESCRIPTION_CHARS {
                        let title = posting.get("title").and_then(Value::as_str).map(str::to_owned);
                        return Some((title,description.to_owned()));
                    }
                }
            }
        }
    }
    None
}

pub fn fetch_detail(id:String)->Result<DetailPreview,String>{
    let url=job_url(&id)?;
    let client=reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent("JobupDesktop/0.1 (local detail preview)")
        .build().map_err(|e| format!("HTTP client: {e}"))?;
    let mut result=DetailPreview {id,url:url.clone(),status:"unavailable".into(),title:None,description_html:None,extraction:None,warning:None};
    let response=match client.get(&url).send() {
        Ok(r)=>r,
        Err(e)=>{result.warning=Some(format!("Request failed: {e}"));return Ok(result)}
    };
    if !response.status().is_success() {
        result.warning=Some(format!("Page returned HTTP {}", response.status()));return Ok(result);
    }
    if response.url().host_str()!=Some("www.jobup.ch") {
        result.warning=Some("Redirected outside www.jobup.ch; refused result".into());return Ok(result);
    }
    if !response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|v|v.to_str().ok()).is_some_and(|s|s.to_ascii_lowercase().contains("text/html")) {
        result.warning=Some("Response is not HTML".into());return Ok(result);
    }
    if response.content_length().is_some_and(|n| n > MAX_HTML_BYTES as u64) {
        result.warning=Some("Page exceeds size limit".into());return Ok(result);
    }
    let mut limited=response.take((MAX_HTML_BYTES+1) as u64);
    use std::io::Read;
    let mut bytes=Vec::new();
    if let Err(e)=limited.read_to_end(&mut bytes) {
        result.warning=Some(format!("Unable to read page: {e}"));return Ok(result);
    }
    if bytes.len()>MAX_HTML_BYTES {result.warning=Some("Page exceeds size limit".into());return Ok(result)}
    let html=match String::from_utf8(bytes) {Ok(s)=>s,Err(_)=>{result.warning=Some("Page was not UTF-8".into());return Ok(result)}};
    if let Some((title,description))=extract(&html) {
        result.status="description_found".into(); result.title=title;
        result.description_html=Some(description);result.extraction=Some("json_ld_jobposting".into());
    } else {result.warning=Some("No usable JSON-LD JobPosting description found; leave eligibility unverified".into());}
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn restricts_id() {
        assert!(job_url("09c2d49a-207f-4eda-bc9e-2ddc6346dae9").is_ok());
        assert!(job_url("https://example.com/").is_err());
        assert!(job_url("../foo").is_err());
    }
    #[test] fn extracts_jobposting_only() {
        let html=r#"<script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"Organization","name":"X"},{"@type":"JobPosting","title":"Vendeur","description":"<p>Expérience souhaitée</p>"}]}</script>"#;
        let (title,desc)=extract(html).unwrap();
        assert_eq!(title.as_deref(),Some("Vendeur"));
        assert_eq!(desc,"<p>Expérience souhaitée</p>");
    }
    #[test] fn rejects_missing_description() {
        assert!(extract(r#"<script type="application/ld+json">{"@type":"JobPosting","title":"X"}</script>"#).is_none());
        assert!(extract("<html><body>Just content</body></html>").is_none());
    }
    #[test] fn rejects_non_jobposting() {
        assert!(extract(r#"<script type="application/ld+json">{"@type":"Article","description":"wrong"}</script>"#).is_none());
    }
}
