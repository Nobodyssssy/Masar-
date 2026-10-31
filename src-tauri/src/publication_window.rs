use chrono::{DateTime,Duration,Local,Utc};

pub fn matches(value:Option<&str>,days:u32,include_unknown:bool,now:DateTime<Local>)->bool{
    let Some(value)=value else{return include_unknown;};
    if value.len()>10 {
        if let Ok(date)=DateTime::parse_from_rfc3339(value){return date.with_timezone(&Utc)>=now.with_timezone(&Utc)-Duration::days(days as i64)&&date.with_timezone(&Utc)<=now.with_timezone(&Utc);}
    }
    match value.get(..10).and_then(|s|chrono::NaiveDate::parse_from_str(s,"%Y-%m-%d").ok()){
        Some(date)=>date>=now.date_naive()-Duration::days(days.saturating_sub(1) as i64)&&date<=now.date_naive(),
        None=>include_unknown,
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn collection_date_never_makes_old_publication_recent(){let now=DateTime::parse_from_rfc3339("2026-10-31T16:00:00+01:00").unwrap().with_timezone(&Local);assert!(!matches(Some("2026-09-01"),1,true,now));assert!(matches(Some("2026-10-31"),1,false,now));assert!(!matches(Some("2026-10-30"),1,false,now));assert!(matches(Some("2026-10-30T17:00:00+01:00"),1,false,now));assert!(!matches(Some("2026-10-30T15:00:00+01:00"),1,false,now));assert!(matches(None,1,true,now));}
}
