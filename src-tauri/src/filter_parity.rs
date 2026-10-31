//! Phase 3 parity baseline; intentionally reproduces legacy prefilter.py behavior.
//! Not a qualifications verifier. Do NOT use this to label jobs fully verified.
use serde::{Deserialize,Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

#[derive(Deserialize)]
struct Rules {
    exclude_keywords: Vec<String>,
    city_whitelist: Vec<String>,
    vs_cities: Vec<String>,
    vs_threshold: f64,
    vd_threshold: f64,
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct FilterResult { pub keep: bool, pub reason: String }

fn normalize(s: &str)-> String {
    s.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c))
      .flat_map(char::to_lowercase).filter(char::is_ascii_alphanumeric).collect()
}
fn rules()->Rules { serde_json::from_str(include_str!("../resources/filter_rules.json")).expect("versioned legacy filtering rules") }

// Normalize one locality rather than substring-matching place names.
fn municipality_key(place: &str) -> String {
    let head = place.split([',', '(', '/']).next().unwrap_or(place);
    head.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_ascii_alphanumeric() {c} else {' '})
        .collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}
fn locality_match(place:&str,city:&str)->bool {
    let location=municipality_key(place);
    let target=municipality_key(city);
    location==target || location.starts_with(&format!("{target} "))
}

fn text(v:&Value,key:&str)->String {v.get(key).and_then(Value::as_str).unwrap_or("").to_string()}
fn max_grade(v:&Value)->f64 {
    v.get("grades").or_else(||v.get("employmentGrades"))
        .and_then(Value::as_array).map(|a|a.iter().filter_map(Value::as_f64).fold(0f64,f64::max)).unwrap_or(0f64)
}
pub fn evaluate(job:&Value)->FilterResult {evaluate_with(job,&rules())}
fn evaluate_with(j:&Value,r:&Rules)->FilterResult {
    let title=text(j,"title");
    let company=j.get("company").and_then(|v|v.as_str().or_else(||v.get("name").and_then(Value::as_str))).unwrap_or("");
    let place=text(j,"place");
    let haystack=normalize(&format!("{title} {company}"));
    for kw in &r.exclude_keywords {
        if haystack.contains(&normalize(kw)) {return FilterResult{keep:false,reason:format!("excluded '{kw}'")}}
    }
    if !r.city_whitelist.iter().any(|city|locality_match(&place,city)){return FilterResult{keep:false,reason:format!("city not in whitelist: {place}")}}
    let canton=if r.vs_cities.iter().any(|city|locality_match(&place,city)){"VS"}else{"VD"};
    let threshold=if canton=="VS"{r.vs_threshold}else{r.vd_threshold};
    let grade=max_grade(j);
    if grade<threshold{return FilterResult{keep:false,reason:format!("grade {} < {} ({canton})",grade as i64,threshold as i64)}}
    FilterResult{keep:true,reason:"kept".into()}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_python_reference_fixtures(){
        let fixtures:Vec<Value>=serde_json::from_str(include_str!("../resources/parity_cases.json")).unwrap();
        let r=rules();
        for j in fixtures {
            let actual=evaluate_with(&j,&r);
            // Explicit, approved exceptions to frozen parity: three corrected VS municipalities.
            let corrected=matches!(j["place"].as_str(),Some("Conthey"|"Ardon"|"Vétroz"));
            if corrected {
                assert!(actual.keep,"corrected municipality should be retained: {}",j);
            } else {
                assert_eq!(actual.keep,j["python_keep"].as_bool().unwrap(),"{}",j);
                assert_eq!(actual.reason,j["python_reason"].as_str().unwrap(),"{}",j);
            }
        }
    }
    #[test]
    fn corrected_vs_municipalities_and_threshold() {
        for place in ["Conthey", "Ardon", "Vétroz", "Vetroz (VS)", "Conthey, VS"] {
            let j=serde_json::json!({"title":"Vendeur","company":"Magasin SA","place":place,"grades":[40]});
            assert!(evaluate(&j).keep, "{place}: {}", evaluate(&j).reason);
            let low=serde_json::json!({"title":"Vendeur","company":"Magasin SA","place":place,"grades":[39]});
            assert!(evaluate(&low).reason.contains("(VS)"));
        }
    }
    #[test]
    fn vd_threshold_and_no_substring_city_matches() {
        let ok=serde_json::json!({"title":"Vendeur","company":"Magasin SA","place":"Lausanne","grades":[70]});
        assert!(evaluate(&ok).keep);
        let low=serde_json::json!({"title":"Vendeur","company":"Magasin SA","place":"Lausanne","grades":[69]});
        assert!(evaluate(&low).reason.contains("(VD)"));
        let bogus=serde_json::json!({"title":"Vendeur","company":"Magasin SA","place":"Faux-Sionville","grades":[100]});
        assert!(!evaluate(&bogus).keep);
    }
    #[test]
    fn no_description_means_no_qualification_verification(){
        let j=serde_json::json!({"title":"Vendeur","company":"Magasin","place":"Sion","grades":[100]});
        assert!(evaluate(&j).keep);
        assert!(j.get("description").is_none());
    }
}
