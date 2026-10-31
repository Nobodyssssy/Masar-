// Firefox-backed discovery for two JavaScript-rendered public job boards.
// This module is included by swiss_hunter_collectors.rs and shares its
// CollectedJob/Discovery types. No new database or eligibility implementation.
#[derive(serde::Deserialize)]
struct FirefoxJob {
    title: String,
    #[serde(default)] company: String,
    #[serde(default)] place: String,
    url: String,
    #[serde(default)] description: String,
    #[serde(default)] source_id: String,
    published_at: Option<String>,
    #[serde(default)] grades: Vec<u32>,
}
#[derive(serde::Deserialize)]
struct FirefoxReport {
    ok: bool,
    error: Option<String>,
    #[serde(default)] pages: usize,
    #[serde(default)] details: usize,
    #[serde(default)] items: Vec<FirefoxJob>,
    #[serde(default)] warnings: Vec<String>,
}
fn collect_firefox(source: &str, keywords: &str, location: &str, days: u32,
    cancel: &dyn Fn() -> bool) -> Result<Discovery, String> {
    use std::process::{Command, Stdio};
    if !matches!(source, "swissdevjobs" | "efinancialcareers") {
        return Err("Unknown Firefox collector".into());
    }
    if cancel() { return Err("Collection cancelled".into()); }
    let project_root=crate::browser_runtime::directory()?;
    let script=project_root.join("firefox-job-collectors.cjs");
    if !script.is_file() || !project_root.join("node.exe").is_file(){return Err("Bundled Firefox helper missing. Repair the Masar installation.".into());}
    // A private temp JSON file avoids stdout pipe deadlocks with large job descriptions.
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|e|e.to_string())?.as_nanos();
    let output = std::env::temp_dir().join(format!("jobup-firefox-{}-{nonce}.json", std::process::id()));
    let mut cmd = Command::new(project_root.join("node.exe"));
    cmd.current_dir(&project_root).arg(&script)
        .arg("--source").arg(source)
        .arg("--keywords").arg(keywords)
        .arg("--location").arg(location)
        .arg("--days").arg(days.to_string())
        .arg("--out").arg(&output)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        // Avoid a console window popping up during normal Tauri use.
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().map_err(|e|format!("Cannot start Node.js Firefox helper: {e}"))?;
    let started = std::time::Instant::now();
    let status = loop {
        if cancel() || started.elapsed() > Duration::from_secs(210) {
            #[cfg(windows)] {
                use std::os::windows::process::CommandExt;
                let _=Command::new("taskkill.exe").args(["/PID",&child.id().to_string(),"/T","/F"]).creation_flags(0x08000000).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
            let _ = child.kill(); let _ = child.wait();
            let _ = std::fs::remove_file(&output);
            return Err(if cancel() { "Firefox collection cancelled" } else { "Firefox collection timed out after 210 seconds" }.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(150)),
            Err(error) => {let _=child.kill();let _=child.wait();let _=std::fs::remove_file(&output);return Err(format!("Firefox process error: {error}"));}
        }
    };
    let data = std::fs::read_to_string(&output).map_err(|e|format!(
        "Firefox helper produced no results file (exit={status}). Verify npx playwright install firefox: {e}"));
    let _ = std::fs::remove_file(&output);
    let data = data?;
    if data.len() > 5_000_000 { return Err("Firefox collector report exceeded size limit".into()); }
    let report: FirefoxReport = serde_json::from_str(&data).map_err(|e|format!("Invalid Firefox collector report: {e}"))?;
    if !report.ok { return Err(report.error.unwrap_or_else(||"Firefox collector failed".into())); }
    if !status.success() { return Err(format!("Firefox helper exited unsuccessfully: {status}")); }
    if report.pages == 0 { return Err("Firefox returned no parsed search pages".into()); }
    let expected_host = if source=="swissdevjobs" {"swissdevjobs.ch"} else {"efinancialcareers.ch"};
    let mut out = Discovery::new(source);
    out.pages = report.pages;
    out.warnings = report.warnings;
    let mut known = HashSet::new();
    for raw in report.items.into_iter().take(40) {
        let title = raw.title.trim();
        if title.is_empty() || title.len() > 300 { continue; }
        let Some(url) = safe_job_url(&raw.url, expected_host) else { continue; };
        if !valid_browser_job_url(source,&url){continue;}
        if !known.insert(url.clone()) { continue; }
        let description = if (180..=40_000).contains(&raw.description.len()) {raw.description} else {String::new()};
        let grades = raw.grades.into_iter().filter(|n| (0..=100).contains(n)).take(3).collect();
        out.jobs.push(CollectedJob {
            listing: UserListing {
                title: title.into(), company: raw.company.trim().chars().take(200).collect(),
                place: raw.place.trim().chars().take(160).collect(), url,
                description, source_id: raw.source_id.chars().take(250).collect(),
            },
            published_at: raw.published_at.as_deref().and_then(date_from_iso), grades,
        });
    }
    out.inspected = out.jobs.len();
    out.details = report.details.min(out.inspected);
    Ok(out)
}
#[cfg(test)] mod firefox_tests {
    #[test] fn stable_source_names() {
        assert!(matches!("swissdevjobs", "swissdevjobs" | "efinancialcareers"));
        assert!(matches!("efinancialcareers", "swissdevjobs" | "efinancialcareers"));
    }
    #[test] fn firefox_json_contract() {
        let j: super::FirefoxReport = serde_json::from_str(r#"{"ok":true,"pages":1,"items":[{"title":"Vendeur","url":"https://swissdevjobs.ch/jobs/x","description":""}]}"#).unwrap();
        assert_eq!(j.items.len(), 1);
        assert_eq!(j.pages, 1);
    }
}

pub fn valid_browser_job_url(source:&str,url:&str)->bool{
    let Ok(url)=Url::parse(url)else{return false;};
    let pattern=if source=="efinancialcareers"{r"\.id[0-9]{5,}/?$"}else if source=="swissdevjobs"{r"^/(?:[a-z]{2}/)?jobs/[^/?#]{10,}/?$"}else{return false;};
    regex::Regex::new(pattern).is_ok_and(|r|r.is_match(url.path()))
}
#[cfg(test)] mod masar_browser_quality_tests {
    #[test] fn rejects_navigation_links(){assert!(!super::valid_browser_job_url("efinancialcareers","https://www.efinancialcareers.ch/jobs/in-indien"));assert!(!super::valid_browser_job_url("swissdevjobs","https://swissdevjobs.ch/jobs/JavaScript/all"));assert!(super::valid_browser_job_url("efinancialcareers","https://www.efinancialcareers.ch/jobs-Switzerland-Zurich-Developer.id24846055"));}
}
