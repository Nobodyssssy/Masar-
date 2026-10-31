//! Authorized JSON feeds for the seven user-selected Swiss sources.
//! No undisclosed endpoints, scraping, or server-side credential persistence.
use crate::source_store::UserListing;
use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;

const SOURCES: [&str;7] = ["jobs_ch","indeed_ch","emploi_en_suisse","jobscout24","okjob","100000jobs","job_room"];

fn validate_endpoint(endpoint:&str,source:&str)->Result<reqwest::Url,String>{
    if !SOURCES.contains(&source){return Err("Source not supported by authorized-feed connector".into());}
    let url=reqwest::Url::parse(endpoint).map_err(|_|"Enter an absolute HTTPS API URL")?;
    if url.scheme()!="https" || url.host_str().is_none() || url.port().is_some() || !url.username().is_empty() || url.password().is_some() {
       return Err("Feed must be an HTTPS URL without credentials or a custom port".into());
    }
    let host=url.host_str().unwrap().to_lowercase();
    if host=="localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.parse::<IpAddr>().is_ok() {
       return Err("Use the public DNS hostname supplied by the provider".into());
    }
    Ok(url)
}

fn first_str<'a>(v:&'a Value,keys:&[&str])->Option<&'a str>{keys.iter().find_map(|k|v.get(*k).and_then(Value::as_str)).filter(|s|!s.trim().is_empty())}
fn listings_from_json(v:Value)->Result<Vec<UserListing>,String>{
    let arr=v.as_array().or_else(|| ["jobs","results","listings","items","data"].iter().find_map(|k|v.get(*k).and_then(Value::as_array)))
       .ok_or("Feed must return a JSON array or a 'jobs'/'results'/'items' array")?;
    if arr.len()>2000{return Err("Feed has over 2000 records. Configure a smaller page/query".into());}
    let mut output=Vec::with_capacity(arr.len());
    for (i,item) in arr.iter().enumerate(){
       let url=first_str(item,&["url","listingUrl","link","jobUrl"]).ok_or_else(||format!("Listing {} has no URL",i+1))?;
       let title=first_str(item,&["title","jobTitle","name"]).ok_or_else(||format!("Listing {} has no title",i+1))?;
       output.push(UserListing{title:title.into(),company:first_str(item,&["company","companyName","employer"]).unwrap_or("").into(),place:first_str(item,&["place","location","city"]).unwrap_or("").into(),url:url.into(),description:first_str(item,&["description","descriptionHtml","body"]).unwrap_or("").into(),source_id:first_str(item,&["sourceId","source_id","id"]).unwrap_or("").into()});
    }
    if output.is_empty(){return Err("No vacancies in feed; nothing saved".into());}
    Ok(output)
}

pub fn fetch(source:&str,endpoint:&str,key:&str,auth_type:&str)->Result<Vec<UserListing>,String>{
    let url=validate_endpoint(endpoint,source)?;
    if key.len()>4096{return Err("API key too long".into());}
    if !["none","bearer","x-api-key"].contains(&auth_type){return Err("Unknown authorization method".into());}
    let client=reqwest::blocking::Client::builder().timeout(Duration::from_secs(25))
       .redirect(reqwest::redirect::Policy::none()).user_agent("JobupLocal/1.0 authorized-feed")
       .build().map_err(|e|e.to_string())?;
    let mut request=client.get(url).header("Accept","application/json");
    if !key.is_empty(){request=match auth_type{
        "bearer"=>request.bearer_auth(key),
        "x-api-key"=>request.header("x-api-key",key),
        _=>return Err("Select an authorization method for the supplied key".into())
    };}
    let response=request.send().map_err(|e|format!("Feed request failed: {e}"))?;
    if !response.status().is_success(){return Err(format!("Feed returned HTTP {}",response.status()));}
    if let Some(len)=response.content_length(){if len>2_000_000{return Err("Feed response exceeds 2 MB".into());}}
    let bytes=response.bytes().map_err(|e|e.to_string())?;
    if bytes.len()>2_000_000{return Err("Feed response exceeds 2 MB".into());}
    let data:Value=serde_json::from_slice(&bytes).map_err(|_|"Feed did not return valid JSON")?;
    listings_from_json(data)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn rejects_unsafe_endpoints(){assert!(validate_endpoint("http://okjob.ch/api","okjob").is_err());assert!(validate_endpoint("https://127.0.0.1/feed","okjob").is_err());assert!(validate_endpoint("https://api.example.com:444/jobs","okjob").is_err());}
    #[test] fn rejects_unapproved_provider(){assert!(validate_endpoint("https://api.example.com/jobs","jooble_ch").is_err());}
    #[test] fn parses_common_fields(){let v=serde_json::json!({"jobs":[{"title":"Vente","company":"A","location":"Sion","link":"https://www.okjob.ch/jobs/1"}]});let r=listings_from_json(v).unwrap();assert_eq!(r[0].place,"Sion");assert_eq!(r[0].title,"Vente");}
    #[test] fn requires_title_and_url(){assert!(listings_from_json(serde_json::json!([{"title":"A"}])).is_err());}
}
