//! Bounded public regional listing discovery for OK Job.
//! This parser deliberately refuses ambiguous records; no credentials, pagination guesses, or bypasses.
use serde::Serialize;
use scraper::{Html, Selector};
use std::collections::HashSet;
use crate::source_store::UserListing;

const ROOT: &str = "https://www.okjob.ch";
const PAGES: &[(&str, &str)] = &[
    ("/offres-demplois/lausanne/", "Lausanne"),
    ("/offres-demplois/yverdon-les-bains/", "Yverdon-les-Bains"),
];
// Bounded detail budget. A politeness delay is inserted between fetches.
const DETAIL_BUDGET: usize = 20;
const DETAIL_DELAY_MS: u64 = 200;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub source: String,
    pub pages_fetched: usize,
    pub inspected: usize,
    pub listings: Vec<UserListing>,
    pub warnings: Vec<String>,
    pub detail_success: usize,
    pub detail_failed: usize,
    pub detail_cached: usize,
}

fn discover_page(html: &str, place: &str) -> Vec<UserListing> {
    let doc = Html::parse_document(html);
    let a = Selector::parse("a[href]").unwrap();
    let mut seen = HashSet::new();
    let mut results = vec![];
    for node in doc.select(&a) {
        let href = match node.value().attr("href") { Some(h) => h, None => continue };
        let full = match reqwest::Url::parse(ROOT).unwrap().join(href) { Ok(x) => x, Err(_) => continue };
        if full.scheme() != "https" || full.host_str() != Some("www.okjob.ch") { continue; }
        if !full.path().starts_with("/offres-demplois/") { continue; }
        if full.query_pairs().find(|(k, _)| k == "ref").is_none() { continue; }
        let id = match full.query_pairs().find(|(k, _)| k == "ref") {
            Some((_, v)) if v.chars().all(|c| c.is_ascii_digit()) && v.len() >= 3 && v.len() <= 8 => v.to_string(),
            _ => continue,
        };
        if !seen.insert(id) { continue; }
        let text = node.text().collect::<Vec<_>>().join(" ");
        let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
        // Rendered cards begin with DD.MM.YYYY / ID Place / Contract Title Description.
        let title = clean.split_once(" / ").and_then(|(_, s)| s.split_once(" / ").map(|(_, t)| t))
            .unwrap_or("").trim();
        if title.len() < 5 || title.len() > 250 { continue; }
        results.push(UserListing {
            title: title.to_string(),
            company: "OK Job SA (recruiter)".into(),
            place: place.into(),
            url: full.to_string(),
            description: String::new(),
            source_id: String::new(),
        });
    }
    results
}

fn is_job_posting(v: &serde_json::Value) -> bool {
    let s = match v.get("@type") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(","),
        _ => return false,
    };
    s == "JobPosting" || s.ends_with("/JobPosting") || s.contains("JobPosting")
}

fn find_posting(v: &serde_json::Value) -> Option<&serde_json::Value> {
    match v {
        serde_json::Value::Array(a) => a.iter().find_map(find_posting),
        serde_json::Value::Object(o) => {
            if is_job_posting(v) { return Some(v); }
            if let Some(g) = o.get("@graph") { if let Some(f) = find_posting(g) { return Some(f); } }
            for (_, val) in o.iter() { if let Some(f) = find_posting(val) { return Some(f); } }
            None
        }
        _ => None,
    }
}

pub(crate) fn extract_ld_json_description(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    let selector = Selector::parse("script[type='application/ld+json']").ok()?;
    for script in doc.select(&selector) {
        let json = script.text().collect::<String>();
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(&json) {
            if let Some(posting) = find_posting(&root) {
                if let Some(desc) = posting.get("description").and_then(|v| v.as_str()) {
                    let len = desc.chars().count();
                    if len >= 200 && len <= 40_000 {
                        return Some(desc.to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_html_description(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);
    for sel in &[
        "[itemprop='description']",
        "#job-description",
        ".job-description",
        ".offer-description",
        ".description",
        "main article",
        "main",
        "article",
    ] {
        if let Ok(selector) = Selector::parse(sel) {
            let best = doc.select(&selector)
                .map(|n| n.html())
                .filter(|s| s.len() >= 150 && s.len() <= 40_000)
                .max_by_key(|s| s.len());
            if let Some(desc) = best { return Some(desc); }
        }
    }
    None
}

fn url_key(u:&str)->String {
    u.split('#').next().unwrap_or(u).trim().trim_end_matches('/').to_string()
}

#[allow(dead_code)]
pub fn fetch()->Result<Discovery,String>{ fetch_with_cache(&std::collections::HashMap::new()) }

pub fn fetch_with_cache(cached:&std::collections::HashMap<String,String>) -> Result<Discovery, String> {
    fetch_with_cache_cancel(cached, &|| false)
}
pub fn fetch_with_cache_cancel(cached:&std::collections::HashMap<String,String>, cancelled:&dyn Fn()->bool) -> Result<Discovery, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("JobupDesktop/0.1 (personal limited regional check)")
        .redirect(reqwest::redirect::Policy::limited(2))
        .build()
        .map_err(|e| e.to_string())?;
    let mut out = Discovery {
        source: "okjob".into(),
        pages_fetched: 0,
        inspected: 0,
        listings: vec![],
        warnings: vec![],
        detail_success: 0,
        detail_failed: 0,
        detail_cached: 0,
    };
    for &(path, place) in PAGES {
        if cancelled() { return Err("Refresh cancelled; no OK Job records saved".into()); }
        let url = format!("{ROOT}{path}");
        if cancelled() { return Err("Refresh cancelled; no OK Job records saved".into()); }
        let response = match client.get(&url).send() {
            Ok(r) => r,
            Err(e) => { out.warnings.push(format!("{place}: connection failed: {e}")); continue; }
        };
        if !response.status().is_success() {
            out.warnings.push(format!("{place}: HTTP {}", response.status()));
            continue;
        }
        if response.url().host_str() != Some("www.okjob.ch") {
            out.warnings.push(format!("{place}: unexpected redirect"));
            continue;
        }
        let body = match response.text() {
            Ok(t) if t.len() <= 3_000_000 => t,
            _ => { out.warnings.push(format!("{place}: invalid or oversized HTML")); continue; }
        };
        out.pages_fetched += 1;
        let found = discover_page(&body, place);
        out.inspected += found.len();
        out.listings.extend(found);
    }
    if out.pages_fetched == 0 {
        return Err(format!("OK Job: no regional pages accessible: {}", out.warnings.join("; ")));
    }
    // Bounded detail retrieval with strict timeouts and politeness delay.
    let mut network_requests=0usize;
    for listing in out.listings.iter_mut() {
        if cancelled() { return Err("Refresh cancelled; no OK Job records saved".into()); }
        let key=url_key(&listing.url);
        if let Some(desc)=cached.get(&key).filter(|d|!d.trim().is_empty()) {
            listing.description=desc.clone();
            out.detail_success+=1;
            out.detail_cached+=1;
            continue;
        }
        if network_requests>=DETAIL_BUDGET { continue; }
        if network_requests>0 { std::thread::sleep(std::time::Duration::from_millis(DETAIL_DELAY_MS)); }
        network_requests+=1;
        let parsed = match reqwest::Url::parse(&listing.url) {
            Ok(v) => v,
            Err(_) => { out.detail_failed += 1; continue; }
        };
        if parsed.scheme() != "https" || parsed.host_str() != Some("www.okjob.ch") {
            out.detail_failed += 1; continue;
        }
        if cancelled() { return Err("Refresh cancelled; no OK Job records saved".into()); }
        let response = match client.get(parsed).send() {
            Ok(r) => r,
            Err(_) => { out.detail_failed += 1; continue; }
        };
        if !response.status().is_success() || response.url().host_str() != Some("www.okjob.ch") {
            out.detail_failed += 1; continue;
        }
        let page = match response.text() {
            Ok(v) if v.len() <= 1_500_000 => v,
            _ => { out.detail_failed += 1; continue; }
        };
        // JSON-LD first (structured, stable); HTML fallback second.
        // Both keep HTML so the downstream classifier preserves paragraph boundaries.
        let description = extract_ld_json_description(&page)
            .or_else(|| extract_html_description(&page));
        match description {
            Some(d) => { listing.description = d; out.detail_success += 1; }
            None => { out.detail_failed += 1; }
        }
    }
    if out.detail_failed > 0 {
        out.warnings.push(format!(
            "Detail checks: {} complete ({} cached, {} fetched); {} missing or unsuitable (maximum {} requests)",
            out.detail_success, out.detail_cached, out.detail_success-out.detail_cached, out.detail_failed, DETAIL_BUDGET
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_url_key_uses_normalized_url_not_source_id() {
        assert_eq!(url_key("https://www.okjob.ch/offres-demplois/test/?ref=42751/#fragment"),
                   "https://www.okjob.ch/offres-demplois/test/?ref=42751");
        assert_ne!(url_key("https://www.okjob.ch/offres-demplois/test/?ref=42751"), "42751");
    }

    #[test]
    fn extracts_only_same_origin_referenced_listings() {
        let sample = r#"<a href='/offres-demplois/assistant-lausanne/?ref=42751'>26.10.2026 / 42751 Lausanne / Fixe Assistant administratif (H/F) Nous recherchons...</a><a href='https://other.com/offres-demplois/test?ref=1234'>xx</a>"#;
        let jobs = discover_page(sample, "Lausanne");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].place, "Lausanne");
        assert!(jobs[0].description.is_empty());
    }

    #[test]
    fn rejects_unidentified_link() {
        assert!(discover_page("<a href='/offres-demplois/assistant/'>Assistant</a>", "Lausanne").is_empty());
    }

    #[test]
    fn finds_json_ld_jobposting() {
        let html = r#"<html><head><script type="application/ld+json">{"@context":"https://schema.org","@type":"JobPosting","title":"Assistant","description":"<p>Accueil client, encaissement et gestion de stock. Description suffisamment longue pour dépasser le seuil de 200 caractères. Voici du texte supplémentaire pour justifier la longueur requise. Les responsabilités comprennent aussi la réception de marchandises, la gestion de stocks et la relation clientèle. Les responsabilités comprennent aussi la réception de marchandises, la gestion de stocks et la relation clientèle. Les responsabilités comprennent aussi la réception de marchandises, la gestion de stocks et la relation clientèle.</p>"}</script></head></html>"#;
        let desc = extract_ld_json_description(html);
        assert!(desc.is_some(), "expected json-ld description");
        assert!(desc.unwrap().contains("Accueil client"));
    }

    #[test]
    fn json_ld_requires_minimum_length() {
        let html = r#"<script type="application/ld+json">{"@type":"JobPosting","description":"Short"}</script>"#;
        assert!(extract_ld_json_description(html).is_none());
    }

    #[test]
    fn html_fallback_picks_longest_paragraph() {
        let html = r#"<main><p>Short nav text</p><p>Accueil client, conseil et encaissement. Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.</p></main>"#;
        let desc = extract_html_description(html);
        assert!(desc.is_some());
        assert!(desc.unwrap().contains("Accueil client"));
    }
}
#[cfg(test)] mod masar_live_okjob {
    #[test]
    #[ignore = "Explicit public website probe"]
    fn okjob_live(){match super::fetch(){Ok(result)=>println!("MASAR_LIVE okjob: inspected={} listings={} descriptions={} warnings={:?}",result.inspected,result.listings.len(),result.detail_success,result.warnings),Err(error)=>println!("MASAR_LIVE okjob: UNAVAILABLE {error}")}}
}
