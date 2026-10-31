// Additional Swiss Job Hunter-compatible public HTML/guest collectors.
// No login, cookies, browser impersonation, access-control bypass, or hidden API credentials.
// The two Playwright-only sites are exposed as official website links instead.
fn extra_terms(input: &str) -> Vec<String> {
    let terms: Vec<String> = input.split(',').map(str::trim).filter(|s| !s.is_empty()).take(3).map(str::to_string).collect();
    if terms.is_empty() { vec![String::new()] } else { terms }
}
fn extra_text(el: &scraper::ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}
fn extra_find(el: &scraper::ElementRef<'_>, selector: &str) -> String {
    Selector::parse(selector).ok().and_then(|s| el.select(&s).next().map(|e| extra_text(&e))).unwrap_or_default()
}
fn extra_link(raw: &str, base: &str, host: &str) -> Option<String> {
    let base = Url::parse(base).ok()?;
    let joined = base.join(raw).ok()?;
    safe_job_url(joined.as_str(), host)
}
fn extra_matches(haystack: &str, search: &str) -> bool {
    let folded = |text: &str| text.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase();
    let haystack = folded(haystack);
    search.split_whitespace().filter(|t| t.len() >= 2).all(|term| haystack.contains(&folded(term)))
}
fn extra_push(out: &mut Discovery, seen: &mut HashSet<String>, job: CollectedJob) -> bool {
    if out.jobs.len() >= MAX_LISTINGS || !seen.insert(job.listing.url.clone()) { return false; }
    out.jobs.push(job);
    true
}
fn extra_enrich(client: &Client, out: &mut Discovery, host: &str, cancel: &dyn Fn()->bool) -> Result<(),String> {
    for job in out.jobs.iter_mut().filter(|j| j.listing.description.is_empty()).take(MAX_DETAILS) {
        if cancel() { return Err("Refresh cancelled before database staging".into()); }
        std::thread::sleep(GAP);
        if let Ok(body) = get(client, &job.listing.url, host) {
            if let Some(full) = details_from_html(&body, host, &job.listing.url) {
                if !full.listing.description.is_empty() { job.listing.description = full.listing.description; out.details += 1; }
                if job.published_at.is_none() { job.published_at = full.published_at; }
            } else {
                let doc = Html::parse_document(&body);
                let s = Selector::parse("[class*='job-description'],[class*='job-content'],[class*='description']").unwrap();
                if let Some(el) = doc.select(&s).next() {
                    let body = extra_text(&el);
                    if (180..=40_000).contains(&body.len()) { job.listing.description = body; out.details += 1; }
                }
            }
        }
    }
    Ok(())
}
/// Zueri.jobs primarily publishes jobs in the City of Zurich; don't pretend its
/// Zurich-only catalogue is a relevant source for Sion searches.
fn collect_zuri(client: &Client, keywords: &str, location: &str, cancel: &dyn Fn()->bool) -> Result<Discovery,String> {
    let mut out=Discovery::new("zuri_jobs");
    if !location.trim().is_empty() && !["zurich", "zürich", "zuerich", "zh"].iter().any(|s| location.to_lowercase().contains(s)) {
        out.warnings.push("Zueri.Jobs covers the City of Zurich; local search outside Zurich has no applicable vacancies. Open the website if desired.".into());
        return Ok(out);
    }
    let mut seen=HashSet::new();
    let cards=Selector::parse("div.job-listings-item").unwrap();
    let link=Selector::parse("a.job-details-link").unwrap();
    let next=Selector::parse("a[rel='next'],.pagination .next").unwrap();
    for term in extra_terms(keywords) {
        for page in 1..=MAX_PAGES {
            if cancel() { return Err("Refresh cancelled before staging".into()); }
            if out.pages>0 { std::thread::sleep(GAP); }
            let mut url=Url::parse("https://www.zueri.jobs/jobs").unwrap();
            url.query_pairs_mut().append_pair("q", &term).append_pair("l", "Zürich");
            if page>1 { url.query_pairs_mut().append_pair("page", &page.to_string()); }
            let body=match get(client,url.as_str(),"zueri.jobs") { Ok(s)=>s,Err(e)=> { if out.pages==0 {return Err(e);} out.warnings.push(e);break; } };
            out.pages+=1;
            let doc=Html::parse_document(&body);let mut candidates=Vec::new();
            for root in json_ld(&doc) {
                let mut posts=Vec::new();walk_postings(&root,&mut posts);
                for value in posts {
                    // A search page itself is not a job URL. Never fabricate a listing link.
                    if read_text(&value,"url").trim().is_empty() { continue; }
                    if let Some(job)=parse_posting(&value,"zueri.jobs",url.as_str(),"") { candidates.push(job); }
                }
            }
            if candidates.is_empty() {
                for card in doc.select(&cards) {
                    let Some(a)=card.select(&link).next() else {continue};
                    let Some(href)=a.value().attr("href") else {continue};
                    let Some(target)=extra_link(href,"https://www.zueri.jobs", "zueri.jobs") else {continue};
                    let title=extra_find(&a,"h3,h2");let title=if title.is_empty(){extra_text(&a)}else{title};
                    if title.is_empty(){continue;}
                    let company=card.select(&Selector::parse("div.job-employer-logo img").unwrap()).next().and_then(|img|img.value().attr("alt")).unwrap_or("").to_string();
                    candidates.push(CollectedJob{listing:UserListing{title,company,place:"Zürich".into(),url:target,description:String::new(),source_id:String::new()},published_at:None,grades:Vec::new()});
                }
            }
            if candidates.is_empty() { if out.pages==1 {return Err("Zueri.Jobs page has no recognized job entries; check markup or query".into());} break; }
            let mut fresh=0;
            for job in candidates {
                if !term.is_empty() && !extra_matches(&format!("{} {}",job.listing.title,job.listing.description),&term) {continue;}
                fresh+=usize::from(extra_push(&mut out,&mut seen,job));
            }
            if fresh==0 || out.jobs.len()>=MAX_LISTINGS || doc.select(&next).next().is_none() {break;}
        }
    }
    extra_enrich(client,&mut out,"zueri.jobs",cancel)?;
    out.inspected=out.jobs.len();Ok(out)
}
fn collect_michael_page(client: &Client, keywords: &str, location: &str, cancel: &dyn Fn()->bool) -> Result<Discovery,String> {
    let mut out=Discovery::new("michael_page");let mut seen=HashSet::new();
    let rows=Selector::parse(".view-content .views-row").unwrap();
    let links=Selector::parse("h3 a,h2 a").unwrap();
    for term in extra_terms(keywords) {
        for page in 0..MAX_PAGES {
            if cancel() {return Err("Refresh cancelled before staging".into());}
            if out.pages>0 {std::thread::sleep(GAP);}
            let mut url=Url::parse("https://www.michaelpage.ch/jobs").unwrap();
            url.query_pairs_mut().append_pair("search", &term);
            if !location.trim().is_empty() {url.query_pairs_mut().append_pair("location", location);}
            if page>0 {url.query_pairs_mut().append_pair("page",&page.to_string());}
            let body=match get(client,url.as_str(),"michaelpage.ch") {Ok(s)=>s,Err(e)=> {if e.contains("HTTP 404") && page>0 {break;}if out.pages==0 {return Err(e);}out.warnings.push(e);break;}};
            out.pages+=1;let doc=Html::parse_document(&body);let mut fresh=0;
            for row in doc.select(&rows) {
                let Some(a)=row.select(&links).next() else {continue};
                let Some(href)=a.value().attr("href") else {continue};
                let Some(target)=extra_link(href,"https://www.michaelpage.ch", "michaelpage.ch") else {continue};
                let title=extra_text(&a);
                if title.is_empty() || (!term.is_empty()&&!extra_matches(&title,&term)) {continue;}
                let place=extra_find(&row,".job-location");
                fresh+=usize::from(extra_push(&mut out,&mut seen,CollectedJob{listing:UserListing{title,company:"Michael Page".into(),place,url:target,description:String::new(),source_id:String::new()},published_at:None,grades:Vec::new()}));
            }
            if out.pages==1 && out.jobs.is_empty() {return Err("Michael Page returned no parseable search cards; markup or search parameters may have changed".into());}
            if fresh==0 || out.jobs.len()>=MAX_LISTINGS {break;}
        }
    }
    extra_enrich(client,&mut out,"michaelpage.ch",cancel)?;
    out.inspected=out.jobs.len();Ok(out)
}
fn parse_linkedin_cards(body:&str)->Vec<CollectedJob>{
    let document=Html::parse_document(body);let mut jobs=Vec::new();let mut seen=HashSet::new();
    let selector=Selector::parse("li").unwrap();let link=Selector::parse("a.base-card__full-link").unwrap();let time=Selector::parse("time[datetime]").unwrap();
    let idre=regex::Regex::new(r"(?i)/jobs/view/(?:[^/?#]*-)?([0-9]{6,})").unwrap();
    for li in document.select(&selector){
        let title=extra_find(&li,"h3.base-search-card__title");if title.is_empty(){continue;}
        let Some(href)=li.select(&link).next().and_then(|a|a.value().attr("href"))else{continue;};
        let Some(url)=extra_link(href,"https://www.linkedin.com","linkedin.com")else{continue;};
        let Some(id)=idre.captures(&url).and_then(|c|c.get(1).map(|m|m.as_str().to_string()))else{continue;};
        if !seen.insert(id.clone()){continue;}
        let company=extra_find(&li,"h4.base-search-card__subtitle");let place=extra_find(&li,".job-search-card__location");
        let published_at=li.select(&time).next().and_then(|t|t.value().attr("datetime")).and_then(date_from_iso);
        jobs.push(CollectedJob{listing:UserListing{title,company,place,url:format!("https://www.linkedin.com/jobs/view/{id}/"),description:String::new(),source_id:id},published_at,grades:Vec::new()});
    }
    jobs
}
fn collect_linkedin(client: &Client, keywords: &str, location: &str, days: u32, cancel: &dyn Fn()->bool) -> Result<Discovery,String> {
    let mut out=Discovery::new("linkedin_jobs");let mut seen=HashSet::new();
    for term in extra_terms(keywords) {
        for page in 0..MAX_PAGES {
            if cancel(){return Err("Refresh cancelled before staging".into());}
            if out.pages>0 {std::thread::sleep(GAP);}
            let mut url=Url::parse("https://www.linkedin.com/jobs-guest/jobs/api/seeMoreJobPostings/search").unwrap();
            url.query_pairs_mut().append_pair("keywords",&term)
                .append_pair("location",if location.trim().is_empty(){"Switzerland"}else{location})
                .append_pair("f_TPR",match days {1=>"r86400",3=>"r259200",14=>"r1209600",30=>"r2592000",_=>"r604800"})
                .append_pair("start",&(page*25).to_string()).append_pair("count","25");
            let body=match get(client,url.as_str(),"linkedin.com") {Ok(s)=>s,Err(e)=> {if out.pages==0{return Err(e);}out.warnings.push(e);break;}};
            out.pages+=1;let mut fresh=0;let jobs=parse_linkedin_cards(&body);let found=jobs.len();
            for job in jobs {fresh+=usize::from(extra_push(&mut out,&mut seen,job));}
            if found==0 {if out.pages==1{return Err("LinkedIn guest search did not contain recognized job cards".into());}break;}
            if fresh==0||out.jobs.len()>=MAX_LISTINGS {break;}
        }
    }
    // Public guest description endpoint; leave unknown if blocked or markup changed.
    let descr=Selector::parse(".show-more-less-html__markup,.description__text").unwrap();
    for job in out.jobs.iter_mut().take(MAX_DETAILS) {
        if cancel(){return Err("Refresh cancelled before staging".into());}
        std::thread::sleep(GAP);
        let id=&job.listing.source_id;
        let url=format!("https://www.linkedin.com/jobs-guest/jobs/api/jobPosting/{id}");
        if let Ok(body)=get(client,&url,"linkedin.com") {
            let doc=Html::parse_document(&body);
            if let Some(el)=doc.select(&descr).next(){let text=extra_text(&el);if (100..=40_000).contains(&text.len()){job.listing.description=text;out.details+=1;}}
        }
    }
    out.inspected=out.jobs.len();
    let before=out.jobs.len();
    out.jobs.retain(|job|crate::search_step1::matches_text(&format!("{} {}",job.listing.title,job.listing.description),keywords));
    if out.jobs.len()<before{out.warnings.push(format!("{} unrelated LinkedIn cards rejected after description enrichment",before-out.jobs.len()));}
    if keywords.trim().eq_ignore_ascii_case("warehouse"){out.warnings.push("Relevance uses explicit warehouse/Lager/entrepôt/magasinier text; advertisement language is filtered separately".into());}
    if out.pages>=MAX_PAGES||out.inspected>=MAX_LISTINGS{out.warnings.push("LinkedIn page/result budget reached; collection may be partial".into());}
    Ok(out)
}
#[cfg(test)] mod extra_tests {
    use super::*;
    #[test] fn firefox_browser_sources_are_registered() {
        assert!(matches!("swissdevjobs", "swissdevjobs" | "efinancialcareers"));
        assert!(matches!("efinancialcareers", "swissdevjobs" | "efinancialcareers"));
    }
    #[test] fn normalizes_relative_and_rejects_off_domain_links() {
        assert_eq!(extra_link("/jobs/test", "https://www.zueri.jobs", "zueri.jobs").as_deref(),Some("https://www.zueri.jobs/jobs/test"));
        assert!(extra_link("https://other.invalid/jobs", "https://www.zueri.jobs", "zueri.jobs").is_none());
    }
    #[test] fn keyword_matching_is_accent_insensitive() {assert!(extra_matches("Responsable clientèle à Genève", "clientele"));}
}

#[cfg(test)] mod linkedin_fixture_tests {
    #[test] fn german_warehouse_card_is_not_rejected_before_enrichment(){let jobs=super::parse_linkedin_cards(include_str!("../tests/fixtures/linkedin-guest.html"));assert_eq!(jobs.len(),1);assert_eq!(jobs[0].listing.title,"Lagermitarbeiter/in");assert_eq!(jobs[0].listing.url,"https://www.linkedin.com/jobs/view/1234567890/");assert!(crate::search_step1::matches_text(&jobs[0].listing.title,"warehouse"));}
    #[test] fn marketing_card_without_warehouse_text_is_unrelated(){assert!(!crate::search_step1::matches_text("Marketing specialist Campaigns and branding","warehouse"));}
    #[test]
    #[ignore="Explicit live LinkedIn relevance verification"]
    fn linkedin_relevant_swiss_jobs(){
        let client=super::client().unwrap();let jobs=super::collect_linkedin(&client,"warehouse","Switzerland",7,&||false).unwrap();
        assert!(!jobs.jobs.is_empty(),"No relevant Swiss warehouse vacancies were retained");
        for job in &jobs.jobs{assert!(crate::search_step1::matches_text(&format!("{} {}",job.listing.title,job.listing.description),"warehouse"));assert!(job.listing.url.starts_with("https://www.linkedin.com/jobs/view/"));}
        println!("LinkedIn verified: {} relevant listings; {} descriptions; {} pages; {} inspected",jobs.jobs.len(),jobs.details,jobs.pages,jobs.inspected);
        for job in jobs.jobs.iter().take(3){println!("Public vacancy evidence: {} | {} | {}",job.listing.title,job.listing.place,job.listing.url);}
    }
}
