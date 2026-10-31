use std::{collections::HashMap,sync::OnceLock};
use crate::swiss_filters::fold;

type Place=(String,String,String,f64,f64);
struct Index {places:Vec<Place>,names:HashMap<String,Vec<usize>>}
fn key(text:&str)->String {fold(text).split(|c:char|!c.is_alphanumeric()).filter(|s|!s.is_empty()).collect::<Vec<_>>().join(" ")}
fn index()->&'static Index {
    static INDEX:OnceLock<Index>=OnceLock::new();
    INDEX.get_or_init(||{
        let places:Vec<Place>=serde_json::from_str(include_str!("swiss_postal_places.json")).expect("Bundled Swiss place data must parse");
        let mut names:HashMap<String,Vec<usize>>=HashMap::new();
        for (i,place) in places.iter().enumerate(){for name in [&place.0,&place.1]{names.entry(key(name)).or_default().push(i);}}
        for (alias,canonical) in [("sitten","sion"),("bienne","biel bienne"),("biel","biel bienne"),("freiburg","fribourg"),("geneva","geneve"),("genf","geneve"),("zuerich","zurich")] {
            if let Some(ids)=names.get(canonical).cloned(){names.entry(alias.into()).or_default().extend(ids);}
        }
        Index{places,names}
    })
}
fn resolve(text:&str)->Vec<usize>{
    let text=key(text);let words:Vec<_>=text.split_whitespace().collect();let mut found=vec![];
    for size in (1..=words.len().min(6)).rev(){for parts in words.windows(size){if let Some(ids)=index().names.get(&parts.join(" ")){found.extend(ids);}}}
    found.sort_unstable();found.dedup();found
}
fn distance(a:&Place,b:&Place)->f64 {
    let dlat=(b.3-a.3).to_radians();let dlon=(b.4-a.4).to_radians();
    let value=(dlat/2.0).sin().powi(2)+a.3.to_radians().cos()*b.3.to_radians().cos()*(dlon/2.0).sin().powi(2);
    6371.0*2.0*value.sqrt().asin()
}
pub fn validate(cantons:&[String],radius:Option<u32>,center:&str)->Result<(),String>{
    const CODES:&[&str]=&["AG","AI","AR","BE","BL","BS","FR","GE","GL","GR","JU","LU","NE","NW","OW","SG","SH","SO","SZ","TG","TI","UR","VD","VS","ZG","ZH"];
    if cantons.len()>26||cantons.iter().any(|s|!CODES.contains(&s.as_str())){return Err("Invalid Swiss canton".into());}
    if radius.is_some_and(|r|r>500){return Err("Radius must be between 0 and 500 km".into());}
    if radius.is_some(){
        let centers=resolve(center);
        if centers.is_empty(){return Err("Radius centre is unknown. Enter a Swiss municipality or postal code.".into());}
        let first=&index().places[centers[0]];
        if centers.iter().any(|&i|distance(first,&index().places[i])>15.0){return Err("Radius centre is ambiguous. Enter its postal code.".into());}
    }
    Ok(())
}
pub fn matches(place:&str,cantons:&[String],radius:Option<u32>,center:&str,unknown:bool)->bool{
    if cantons.is_empty()&&radius.is_none(){return true;}
    let places=resolve(place);
    if places.is_empty(){
        if radius.is_none()&&!cantons.is_empty(){let tokens:Vec<_>=place.split(|c:char|!c.is_alphabetic()).collect();if cantons.iter().any(|c|tokens.contains(&c.as_str())){return true;}}
        return unknown;
    }
    let centers=if radius.is_some(){resolve(center)}else{vec![]};
    places.iter().any(|&i|{let p=&index().places[i];(cantons.is_empty()||cantons.contains(&p.2))&&radius.is_none_or(|r|centers.iter().any(|&c|distance(p,&index().places[c])<=r as f64))})
}
pub fn postal_matches(place:&str,query:&str)->bool{
    if crate::swiss_filters::location_matches(place,query){return true;}
    let ids=resolve(place);
    query.split(',').map(str::trim).filter(|q|q.len()==4&&q.chars().all(|c|c.is_ascii_digit())).any(|q|ids.iter().any(|&i|index().places[i].0==q))
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn dataset_covers_all_cantons(){let mut codes=index().places.iter().map(|p|p.2.clone()).collect::<Vec<_>>();codes.sort();codes.dedup();assert_eq!(codes.len(),26);}
    #[test] fn swiss_aliases_and_postal(){assert!(matches("Sitten",&["VS".into()],None,"",false));assert!(matches("Bienne",&["BE".into()],None,"",false));assert!(postal_matches("Sion","1950"));assert!(!matches("Lausanne",&["VS".into()],None,"",false));}
    #[test] fn radius_and_unknown(){assert!(validate(&[],Some(15),"1950").is_ok());assert!(matches("Sierre",&[],Some(20),"1950",false));assert!(!matches("Genève",&[],Some(20),"1950",false));assert!(!matches("Unknown",&["VS".into()],None,"",false));assert!(matches("Unknown",&["VS".into()],None,"",true));}
}
