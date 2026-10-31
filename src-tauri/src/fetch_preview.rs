//! Preview-only port of fetch_jobs.py search request and pagination.
//! Never writes to a DB or filesystem; does not mark jobs seen or filter.
use serde::Serialize;
use serde_json::Value;
use chrono::{Local, NaiveDate};

const API: &str = "https://job-search-api.jobup.ch/search/semantic";
const FILTER: &str = "employmentGradeMin=40&employmentPositionIds=3&employmentTypeIds=1&employmentTypeIds=4&employmentTypeIds=5&regionIds=52&regionIds=57&rows=20";
const MAX_PAGES: u32 = 250;
const MAX_DOCUMENTS: usize = 5_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchPreview {
    pub fetched_at: String,
    pub window_from: String,
    pub window_to: String,
    pub pages_fetched: u32,
    pub reported_pages: u32,
    pub jobs: Vec<Value>,
    pub warnings: Vec<String>,
    pub complete: bool,
}

fn build_url(page: u32, start: NaiveDate, end: NaiveDate) -> String {
    // Match Python's URL construction byte for byte (including date encoding).
    format!("{API}?{FILTER}&page={page}&publicationDateFrom={start}%2000%3A00%3A00&publicationDateTo={end}%2023%3A59%3A59")
}

fn decode_page(data: Value) -> Result<(Vec<Value>, u32), String> {
    let obj = data.as_object().ok_or("Jobup response is not an object")?;
    let docs = obj.get("documents").and_then(Value::as_array)
        .ok_or("Jobup response missing documents array")?.clone();
    let num_pages = obj.get("numPages").and_then(Value::as_u64)
        .ok_or("Jobup response missing integer numPages")?;
    if num_pages == 0 || num_pages > u32::MAX as u64 {
        return Err(format!("Invalid Jobup numPages: {num_pages}"));
    }
    Ok((docs, num_pages as u32))
}

pub fn fetch_recent_preview(days_back: u32) -> Result<FetchPreview, String> {
    fetch_recent_with_progress(days_back, &mut |_, _| {})
}

pub fn fetch_recent_with_progress(days_back: u32, progress: &mut dyn FnMut(u32,u32)) -> Result<FetchPreview, String> {
    if days_back > 30 { return Err("daysBack must be between 0 and 30".into()); }
    let today = Local::now().date_naive();
    let from = today.checked_sub_signed(chrono::Duration::days(days_back as i64))
        .ok_or("Invalid date window")?;
    fetch_window(from, today, progress, &|| false)
}

pub fn fetch_recent_cancellable(days_back:u32, progress:&mut dyn FnMut(u32,u32), cancelled:&dyn Fn()->bool)->Result<FetchPreview,String>{
    if days_back>30 {return Err("daysBack must be between 0 and 30".into());}
    let today=Local::now().date_naive();
    let from=today.checked_sub_signed(chrono::Duration::days(days_back as i64)).ok_or("Invalid date window")?;
    fetch_window(from,today,progress,cancelled)
}
fn fetch_window(from: NaiveDate, to: NaiveDate, progress: &mut dyn FnMut(u32,u32), cancelled:&dyn Fn()->bool) -> Result<FetchPreview, String> {
    let agent = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("JobupDesktop/0.1 (local preview)")
        .build().map_err(|e| format!("HTTP client: {e}"))?;
    let mut result = FetchPreview {
        fetched_at: Local::now().to_rfc3339(),
        window_from: from.to_string(), window_to: to.to_string(),
        pages_fetched: 0, reported_pages: 0,
        jobs: Vec::new(), warnings: Vec::new(), complete: false,
    };
    let mut page = 1;
    loop {
        if cancelled(){return Err("Refresh cancelled; nothing saved".into());}
        if page > MAX_PAGES {
            result.warnings.push(format!("Stopped at the safety limit of {MAX_PAGES} pages"));
            break;
        }
        let response = match agent.get(build_url(page, from, to)).send() {
            Ok(v) => v,
            Err(e) => {
                let message = format!("Page {page} request failed: {e}");
                if page == 1 { return Err(message); }
                result.warnings.push(message); break;
            }
        };
        if !response.status().is_success() {
            let message = format!("Page {page} HTTP status {}", response.status());
            if page == 1 { return Err(message); }
            result.warnings.push(message); break;
        }
        let data: Value = match response.json() {
            Ok(v) => v,
            Err(e) => {
                let message = format!("Page {page} invalid JSON: {e}");
                if page == 1 { return Err(message); }
                result.warnings.push(message); break;
            }
        };
        let (docs, pages) = match decode_page(data) {
            Ok(v) => v,
            Err(e) => {
                let message = format!("Page {page}: {e}");
                if page == 1 { return Err(message); }
                result.warnings.push(message); break;
            }
        };
        if page > pages {
            result.warnings.push(format!("Page {page} exceeds reported numPages {pages}"));
            break;
        }
        if result.jobs.len() + docs.len() > MAX_DOCUMENTS {
            result.warnings.push(format!("Stopped before exceeding {MAX_DOCUMENTS} documents"));
            break;
        }
        if result.reported_pages != 0 && result.reported_pages != pages {
            result.warnings.push(format!("numPages changed from {} to {pages} during pagination", result.reported_pages));
        }
        result.reported_pages = pages;
        result.jobs.extend(docs);
        result.pages_fetched += 1;
        progress(result.pages_fetched, result.reported_pages);
        if cancelled(){return Err("Refresh cancelled; nothing saved".into());}
        if page >= pages { result.complete = true; break; }
        page += 1;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_matches_original_python_request() {
        let date = NaiveDate::from_ymd_opt(2026,10,25).unwrap();
        let earlier = NaiveDate::from_ymd_opt(2026,10,24).unwrap();
        assert_eq!(build_url(1, earlier, date),
            "https://job-search-api.jobup.ch/search/semantic?employmentGradeMin=40&employmentPositionIds=3&employmentTypeIds=1&employmentTypeIds=4&employmentTypeIds=5&regionIds=52&regionIds=57&rows=20&page=1&publicationDateFrom=2026-10-24%2000%3A00%3A00&publicationDateTo=2026-10-25%2023%3A59%3A59");
    }
    #[test]
    fn parses_actual_expected_shape() {
        let (docs, pages) = decode_page(serde_json::json!({"documents":[{"id":"123","title":"Vendeur"}],"numPages":3})).unwrap();
        assert_eq!(docs.len(), 1); assert_eq!(pages, 3);
    }
    #[test]
    fn rejects_unexpected_payloads() {
        for v in [serde_json::json!([]),serde_json::json!({"documents":[],"numPages":0}),serde_json::json!({"documents":"oops","numPages":1}),serde_json::json!({"documents":[],"numPages":"2"})] {
            assert!(decode_page(v).is_err());
        }
    }
}
