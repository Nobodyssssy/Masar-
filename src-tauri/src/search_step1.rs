//! Read-only search of staged vacancies. Fetch parameters are controlled by each
//! provider's existing collector; this query never relabels or promotes records.
use chrono::{NaiveDate, Local};
use unicode_normalization::UnicodeNormalization;
use std::collections::HashMap;
use rusqlite::Connection;
use serde::{Deserialize,Serialize};
use serde_json::Value;
use crate::job_identity::{compare,SourceJob};

#[derive(Debug,Deserialize,Default)]
#[serde(rename_all="camelCase")]
pub struct SearchFilters {
    #[serde(default)] pub sources:Vec<String>,
    #[serde(default)] pub hide_expired:bool,
    #[serde(default)] pub eligibility_rules:Option<crate::eligibility_rules::Rules>,
    #[serde(default)] pub facts:crate::job_facts::FactFilter,
    #[serde(default)] pub cantons:Vec<String>,
    #[serde(default)] pub radius_km:Option<u32>,
    #[serde(default)] pub radius_center:String,
    #[serde(default)] pub include_unknown_locations:bool,
    #[serde(default)] pub ad_languages:Vec<String>,
    #[serde(default)] pub working_languages:Vec<String>,
    #[serde(default)] pub language_all:bool,
    #[serde(default)] pub include_unknown_languages:bool,
    #[serde(default)] pub title_only:bool,
    #[serde(default)] pub keyword_all:bool,
    #[serde(default)] pub max_grade:Option<u32>,
    #[serde(default)] pub date_from:Option<String>,
    #[serde(default)] pub date_to:Option<String>,
    #[serde(default)] pub keywords:String,
    #[serde(default)] pub location:String,
    #[serde(default)] pub min_grade:Option<u32>,
    #[serde(default)] pub days_back:Option<u32>,
    #[serde(default)] pub include_unknown_dates:bool,
    #[serde(default)] pub classification:String,
    #[serde(default)] pub exclude_keywords:String,
    #[serde(default)] pub sort_by:String,
    #[serde(default)] pub limit:Option<usize>,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct SearchHit {
    pub observation:crate::listing_freshness::Observation,
    pub facts:crate::job_facts::Facts,
    pub ad_language:String,pub working_languages:Vec<(String,String)>,
    pub source:String,pub source_id:String,pub title:String,pub company:String,
    pub place:String,pub listing_url:String,pub published_at:Option<String>,
    pub retrieved_at:String,pub grades:Vec<u32>,pub classification:String,
    pub description_available:bool,pub possible_duplicates:Vec<String>,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct SearchResponse {pub items:Vec<SearchHit>,pub matched:usize,pub scanned:usize,pub capped:bool,pub unknown_publication_dates:usize,pub unknown_workloads:usize,pub duplicate_groups:usize}

const SOURCES:&[&str]=&["jobup","jobs_ch","indeed_ch","emploi_en_suisse","jobscout24","okjob","100000jobs","job_room","zuri_jobs","michael_page","linkedin_jobs","swissdevjobs","efinancialcareers"];
fn normalize(s:&str)->String {
    s.nfd().filter(|c|!unicode_normalization::char::is_combining_mark(*c))
       .flat_map(char::to_lowercase).collect()
}
fn date_part(s:&str)->Option<NaiveDate>{NaiveDate::parse_from_str(s.get(..10)?,"%Y-%m-%d").ok()}
pub fn matches_text(hay:&str,needle:&str)->bool {
    if needle.trim().is_empty(){return true;}
    let folded=normalize(hay);
    static TOKENS:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let tokens=TOKENS.get_or_init(||regex::Regex::new(r#""([^"]+)"|([^\s"]+)"#).expect("Static keyword expression"));
    needle.split(',').any(|group|{
        let query=normalize(group.trim());
        if query.is_empty(){return false;}
        let literal=tokens.captures_iter(group).all(|part|folded.contains(&normalize(part.get(1).or_else(||part.get(2)).unwrap().as_str())));
        if literal{return true;}
        if query=="warehouse"{return folded.split(|c:char|!c.is_alphabetic()).any(|word|["lager","lagerarbeit","lagermitarbeit","lagerist","lagerlogist","lagerbewirtschaft","lagerhaltung","lagerwirtschaft","lagerfach","lagerhelfer"].iter().any(|root|word==*root||(*root!="lager"&&word.starts_with(root)))||word.starts_with("entrepot")||word.starts_with("magasinier")||word.starts_with("warehous"));}
        false
    })
}
pub fn inspect(db:&Connection, filter:SearchFilters)->Result<SearchResponse,String>{
    if filter.keywords.len()>220 || filter.exclude_keywords.len()>220 || filter.location.len()>140 || filter.sources.len()>13 {
        return Err("Search query too large".into());
    }
    if filter.days_back.is_some()&&(filter.date_from.is_some()||filter.date_to.is_some()){return Err("Choisissez une période relative ou des dates personnalisées, pas les deux".into());}
    if filter.sources.iter().any(|s| !SOURCES.contains(&s.as_str())) {return Err("Unsupported provider filter".into())}
    if let Some(rules)=&filter.eligibility_rules{rules.validate()?;}
    filter.facts.validate()?;
    crate::geography::validate(&filter.cantons,filter.radius_km,&filter.radius_center)?;
    if filter.ad_languages.iter().chain(filter.working_languages.iter()).any(|s| !["fr","de","en"].contains(&s.as_str())) {return Err("Invalid language".into());}
    if filter.date_from.as_deref().is_some_and(|s|date_part(s).is_none()) || filter.date_to.as_deref().is_some_and(|s|date_part(s).is_none()) {return Err("Invalid custom date".into());}
    if filter.date_from.as_ref().zip(filter.date_to.as_ref()).is_some_and(|(a,b)|a>b) {return Err("Invalid date range".into());}
    if filter.max_grade.is_some_and(|v|v>100) || filter.min_grade.zip(filter.max_grade).is_some_and(|(a,b)|a>b) {return Err("Invalid workload range".into());}
    if filter.min_grade.is_some_and(|v|v>100) {return Err("Invalid workload minimum".into())}
    if filter.days_back.is_some_and(|v| ![1,3,7,14,30].contains(&v)) {return Err("Invalid date interval".into())}
    if !filter.classification.is_empty() && !["eligible","unverified","excluded"].contains(&filter.classification.as_str()) {return Err("Invalid classification".into())}
    if !filter.sort_by.is_empty() && !["published","retrieved","company","title","location","workload","relevance"].contains(&filter.sort_by.as_str()) {
        return Err("Invalid sort mode".into());
    }
    let max=filter.limit.unwrap_or(150);
    if max==0||max>500 {return Err("Limit must be between 1 and 500".into())}
    // Bounded read; historical records remain untouched. Caps are reported explicitly.
    let mut stmt=db.prepare(r#"SELECT s.source,s.source_id,s.title,s.company,s.place,
       s.listing_url,s.published_at,s.retrieved_at,s.grades_json,
       COALESCE(e.classification,'unverified'),
       CASE WHEN s.description IS NOT NULL AND length(trim(s.description)) > 0 THEN 1 ELSE 0 END,
       substr(COALESCE(s.description,''),1,8000)
       FROM source_listings s LEFT JOIN source_eligibility e
         ON e.source=s.source AND e.source_id=s.source_id
       ORDER BY COALESCE(s.published_at,s.retrieved_at) DESC,s.source,s.source_id LIMIT 10001"#)
         .map_err(|e|e.to_string())?;
    let rows=stmt.query_map([],|r|{
        Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
            r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,
            r.get::<_,Option<String>>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,
            r.get::<_,String>(9)?,r.get::<_,i64>(10)?,r.get::<_,String>(11)?))
    }).map_err(|e|e.to_string())?;
    let has_observations:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='listing_observations')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    let mut items=Vec::new();let mut scanned=0;let mut unknown=0;let mut unknown_workloads=0;
    let now=Local::now();
    let relative_days=filter.days_back.filter(|_|filter.date_from.is_none()&&filter.date_to.is_none());
    for row in rows {
        let (source,source_id,title,company,place,url,published_at,retrieved_at,grades_raw,mut classification,has_desc,description)=row.map_err(|e|e.to_string())?;
        scanned+=1;
        if matches!(source.as_str(),"swissdevjobs"|"efinancialcareers")&&!crate::swiss_hunter_collectors::valid_browser_job_url(&source,&url){continue;}
        if scanned>10000 {break;}
        if !filter.sources.is_empty()&&!filter.sources.contains(&source){continue;}
        let publication=published_at.as_deref().and_then(date_part);
        if publication.is_none() {unknown+=1;}
        if relative_days.is_some_and(|days|!crate::publication_window::matches(published_at.as_deref(),days,filter.include_unknown_dates,now)){continue;}
        // Provider metadata may be sparse. The indexed description is bounded to 8k characters.
        if let Some(date)=publication {
            if filter.date_from.as_deref().and_then(date_part).is_some_and(|d|date<d) || filter.date_to.as_deref().and_then(date_part).is_some_and(|d|date>d){continue;}
        }else if (filter.date_from.is_some()||filter.date_to.is_some())&&!filter.include_unknown_dates {continue;}
        let text=if filter.title_only{title.clone()}else{format!("{title} {company} {place} {description}")};
        let keyword_match=if filter.keyword_all{filter.keywords.split(',').all(|s|matches_text(&text,s))}else{matches_text(&text,&filter.keywords)};
        if !keyword_match || (filter.radius_km.is_none()&&!crate::geography::postal_matches(&place,&filter.location)){continue;}
        if !crate::geography::matches(&place,&filter.cantons,filter.radius_km,&filter.radius_center,filter.include_unknown_locations){continue;}
        let facts=crate::job_facts::extract(&title,&company,&description);
        if !filter.facts.matches(&facts,has_desc!=0){continue;}
        let ad_language=crate::swiss_filters::ad_language(&description);
        let working_languages=crate::swiss_filters::working_languages(&description);
        if !crate::swiss_filters::language_matches(&ad_language,&working_languages,&filter.ad_languages,&filter.working_languages,filter.language_all,filter.include_unknown_languages){continue;}
        if !filter.exclude_keywords.is_empty() && matches_text(&text,&filter.exclude_keywords){continue;}
        let grades:Vec<u32>=serde_json::from_str::<Value>(&grades_raw).ok()
            .and_then(|v|v.as_array().map(|a|a.iter().filter_map(|v|v.as_u64().map(|n|n as u32)).collect()))
            .unwrap_or_default();
        if let Some(rules)=&filter.eligibility_rules{classification=rules.evaluate(&title,&place,&description,&grades,&facts);}
        if !filter.classification.is_empty()&&classification!=filter.classification{continue;}
        if filter.eligibility_rules.is_some()&&filter.classification.is_empty()&&classification=="excluded"{continue;}
        if filter.min_grade.is_some_and(|g|!grades.is_empty()&&grades.iter().all(|&n|n<g)){continue;}
        if filter.max_grade.is_some_and(|g|!grades.is_empty()&&grades.iter().all(|&n|n>g)){continue;}
        if grades.is_empty() {unknown_workloads+=1;}
        let observation=if has_observations{crate::listing_freshness::get(db,&source,&source_id)?}else{crate::listing_freshness::Observation::default()};
        if filter.hide_expired&&observation.availability=="expired"{continue;}
        items.push(SearchHit{observation,facts,ad_language,working_languages,source,source_id,title,company,place,listing_url:url,published_at,retrieved_at,grades,classification,description_available:has_desc!=0,possible_duplicates:Vec::new()});
    }
    let matched=items.len();
    // Sort all matching records before choosing a display window. Unknown dates
    // always come after known dates when sorting by publication.
    match filter.sort_by.as_str() {
        "location"=>items.sort_by(|a,b|normalize(&a.place).cmp(&normalize(&b.place))),
        "workload"=>items.sort_by(|a,b|b.grades.iter().max().cmp(&a.grades.iter().max())),
        "relevance"=>items.sort_by_key(|a|{let title=normalize(&a.title);std::cmp::Reverse(filter.keywords.split(',').flat_map(str::split_whitespace).filter(|word|title.contains(&normalize(word))).count())}),
        "retrieved"=>items.sort_by(|a,b|b.retrieved_at.cmp(&a.retrieved_at)),
        "company"=>items.sort_by(|a,b|normalize(&a.company).cmp(&normalize(&b.company)).then_with(||a.title.cmp(&b.title))),
        "title"=>items.sort_by(|a,b|normalize(&a.title).cmp(&normalize(&b.title)).then_with(||a.company.cmp(&b.company))),
        _=>items.sort_by(|a,b|b.published_at.as_deref().and_then(date_part)
            .cmp(&a.published_at.as_deref().and_then(date_part))
            .then_with(||b.retrieved_at.cmp(&a.retrieved_at))),
    }
    // Find potential duplicates among *all matched records*, rather than only
    // the first displayed page. Index by company and locality to avoid O(n^2)
    // comparisons on a 10,000-record result set. This creates warnings only.
    let mut groups:HashMap<String,Vec<usize>>=HashMap::new();
    for (idx,hit) in items.iter().enumerate() {
        let company=normalize(&hit.company).trim().to_string();
        let locality=normalize(&hit.place).trim().to_string();
        if company.is_empty()||locality.is_empty()||company=="unknown"||company=="unknown company" {continue;}
        groups.entry(format!("{company}\u{1f}{locality}")).or_default().push(idx);
    }
    let make=|h:&SearchHit| SourceJob{source:h.source.clone(),source_id:h.source_id.clone(),title:h.title.clone(),company:h.company.clone(),place:h.place.clone(),url:h.listing_url.clone(),grades:h.grades.clone(),description:None,employer_application_url:None,published_at:h.published_at.clone()};
    let mut dup_groups=0;
    for group in groups.values() {
        if group.len()<2 {continue;}
        let mut matching=false;
        for (i,&left) in group.iter().enumerate() {
            for &right in group.iter().skip(i+1) {
                if items[left].source==items[right].source {continue;}
                let result=compare(&make(&items[left]),&make(&items[right]));
                if result.relation=="possible_duplicate"||result.relation=="same_opportunity" {
                    let a=format!("{}:{}",items[right].source,items[right].source_id);
                    let b=format!("{}:{}",items[left].source,items[left].source_id);
                    items[left].possible_duplicates.push(a);
                    items[right].possible_duplicates.push(b);
                    matching=true;
                }
            }
        }
        if matching {dup_groups+=1;}
    }
    items.truncate(max);
    Ok(SearchResponse{items,matched,scanned,capped:scanned>10000||matched>max,unknown_publication_dates:unknown,unknown_workloads,duplicate_groups:dup_groups})
}
#[cfg(test)]mod tests{
    use super::*;
    fn sample_db()->Connection {
        let db=Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE source_listings (source TEXT, source_id TEXT, title TEXT, company TEXT, place TEXT, listing_url TEXT, published_at TEXT, retrieved_at TEXT, grades_json TEXT, description TEXT); CREATE TABLE source_eligibility(source TEXT, source_id TEXT, classification TEXT);").unwrap();
        db
    }
    fn stage(db:&Connection, source:&str, id:&str, title:&str,company:&str,description:&str,posted:Option<&str>) {
        db.execute("INSERT INTO source_listings VALUES (?1,?2,?3,?4,'Sion',?5,?6,'2026-10-27 10:00:00','[80,100]',?7)",rusqlite::params![source,id,title,company,format!("https://example.ch/{source}/{id}"),posted,description]).unwrap();
        db.execute("INSERT INTO source_eligibility VALUES (?1,?2,'unverified')",rusqlite::params![source,id]).unwrap();
    }
    #[test]fn searches_description_with_accent_folding_and_exclusions(){
        let db=sample_db();stage(&db,"jobup","1","Conseiller","Étoile SA","Encaissement, clientèle et accueil",None);
        let filter=SearchFilters{keywords:"clientele".into(),include_unknown_dates:true,..Default::default()};
        assert_eq!(inspect(&db,filter).unwrap().matched,1);
        let filter=SearchFilters{exclude_keywords:"encaissement".into(),..Default::default()};
        assert_eq!(inspect(&db,filter).unwrap().matched,0);
    }
    #[test]fn matches_duplicates_outside_display_limit(){
        let db=sample_db();
        stage(&db,"jobup","a","Vendeur 80%","Étoile SA","Accueil",None);
        stage(&db,"jobs_ch","b","Vendeur (H/F)","Etoile SA","Accueil",None);
        let f=SearchFilters{limit:Some(1),..Default::default()};
        let found=inspect(&db,f).unwrap();
        assert_eq!(found.matched,2);
        assert_eq!(found.items.len(),1);
        assert_eq!(found.items[0].possible_duplicates.len(),1);
    }
    #[test]fn rejects_unknown_sort(){
        let db=sample_db();
        let f=SearchFilters{sort_by:"random".into(),..Default::default()};
        assert!(inspect(&db,f).unwrap_err().contains("sort"));
    }
    #[test]fn known_filters(){assert!(date_part("2026-10-26T00:00:00").is_some());assert!(date_part("not-a-date").is_none());assert!(matches_text("Vendeur Sion","vendeur"));}
    #[test]fn refuses_invalid_filters(){let db=Connection::open_in_memory().unwrap();let f=SearchFilters{days_back:Some(31),..Default::default()};assert!(inspect(&db,f).is_err());}
    #[test]fn safe_empty_results(){let db=Connection::open_in_memory().unwrap();db.execute_batch("CREATE TABLE source_listings(source TEXT,source_id TEXT,title TEXT,company TEXT,place TEXT,listing_url TEXT,published_at TEXT,retrieved_at TEXT,grades_json TEXT,description TEXT);CREATE TABLE source_eligibility(source TEXT,source_id TEXT,classification TEXT);").unwrap();let r=inspect(&db,SearchFilters::default()).unwrap();assert!(r.items.is_empty());}
    #[test]fn warns_but_does_not_merge(){let db=Connection::open_in_memory().unwrap();db.execute_batch("CREATE TABLE source_listings(source TEXT,source_id TEXT,title TEXT,company TEXT,place TEXT,listing_url TEXT,published_at TEXT,retrieved_at TEXT,grades_json TEXT,description TEXT);CREATE TABLE source_eligibility(source TEXT,source_id TEXT,classification TEXT);INSERT INTO source_listings VALUES ('jobup','1','Vendeur 80%','Coop','Sion','https://a.example/1',NULL,'2026-10-25','[80]',NULL),('jobs_ch','2','Vendeur 80%','Coop','Sion','https://b.example/2',NULL,'2026-10-25','[80]',NULL);").unwrap();let r=inspect(&db,SearchFilters::default()).unwrap();assert_eq!(r.items.len(),2);assert_eq!(r.items[0].possible_duplicates.len(),1);}
}

#[cfg(test)] mod masar_phrase_tests {
    #[test] fn exact_phrase_requires_order(){assert!(super::matches_text("Assistant service client","\"service client\""));assert!(!super::matches_text("Client service assistant","\"service client\""));}
}

#[cfg(test)] mod multilingual_warehouse_tests {
    #[test] fn warehouse_has_explicit_swiss_language_equivalents(){assert!(super::matches_text("Lagermitarbeiter Montage","warehouse"));assert!(super::matches_text("Gestion du stock en entrepôt","warehouse"));assert!(!super::matches_text("Marketing Manager","warehouse"));assert!(!super::matches_text("Schlagermusik Marketing","warehouse"));assert!(!super::matches_text("Karl Lagerfeld Marketing","warehouse"));assert!(!super::matches_text("Lagermitarbeiter","\"warehouse\""));}
}

#[cfg(test)] mod conflicting_publication_filters {
    #[test] fn conflicting_date_controls_are_rejected(){let db=rusqlite::Connection::open_in_memory().unwrap();let filters=super::SearchFilters{days_back:Some(1),date_from:Some("2026-09-01".into()),..Default::default()};assert!(super::inspect(&db,filters).unwrap_err().contains("relative"));}
}
