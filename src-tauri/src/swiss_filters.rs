use unicode_normalization::UnicodeNormalization;

pub fn fold(value:&str)->String {
    value.nfd().filter(|c|!unicode_normalization::char::is_combining_mark(*c)).flat_map(char::to_lowercase).collect()
}

pub fn location_matches(place:&str, query:&str)->bool {
    if query.trim().is_empty(){return true;}
    let place=fold(place);
    query.split(',').map(str::trim).filter(|s|!s.is_empty()).any(|q|{
        let q=fold(q);
        let aliases:&[&str]=match q.as_str(){
            "sion"|"sitten"=>&["sion","sitten"],
            "biel"|"bienne"=>&["biel","bienne"],
            "fribourg"|"freiburg"=>&["fribourg","freiburg"],
            "geneva"|"geneve"|"genf"=>&["geneva","geneve","genf"],
            "zurich"=>&["zurich","zuerich"],
            _=>&[],
        };
        if aliases.is_empty(){place.contains(&q)}else{aliases.iter().any(|s|place.contains(s))}
    })
}

pub fn ad_language(text:&str)->String {
    let text=format!(" {} ",fold(text));
    let scores=[("fr",[" vous "," votre "," nous "," une "," recherche "," poste "," travail "," pour "]),
        ("de",[" sie "," ihre "," wir "," und "," suchen "," aufgaben "," arbeit "," fur "]),
        ("en",[" you "," your "," we "," and "," looking "," role "," work "," the "])];
    let mut ranked:Vec<_>=scores.iter().map(|(lang,words)|(*lang,words.iter().filter(|w|text.contains(**w)).count())).collect();
    ranked.sort_by(|a,b|b.1.cmp(&a.1));
    if ranked[0].1>=3 && ranked[0].1>ranked[1].1 {ranked[0].0.into()}else{"unknown".into()}
}

pub fn working_languages(text:&str)->Vec<(String,String)> {
    let text=fold(text).replace(" and ",";").replace(" et ",";").replace(" und ",";");
    let names=[("fr",["francais","french","franzosisch"]),("de",["allemand","german","deutsch"]),("en",["anglais","english","englisch"])];
    names.iter().filter_map(|(code,words)|{
        let mut level=0;
        for sentence in text.split(['.',';','\n','!']) {
            if !words.iter().any(|word|sentence.split(|c:char|!c.is_alphabetic()).any(|s|s==*word)){continue;}
            let negated=["not required","not mandatory","nicht erforderlich","nicht notwendig","non requis","pas obligatoire"].iter().any(|s|sentence.contains(s));
            let preferred=["souhaite","un atout","apprecie","preferred","a plus","von vorteil","wunschenswert"].iter().any(|s|sentence.contains(s));
            let required=["obligatoire","exige","requis","required","mandatory","must ","erforderlich","zwingend","voraussetzung"].iter().any(|s|sentence.contains(s));
            level=level.max(if negated{1}else if preferred{2}else if required{3}else{1});
        }
        if level==0{None}else{Some((code.to_string(),match level{3=>"required",2=>"preferred",_=>"mentioned"}.into()))}
    }).collect()
}

pub fn language_matches(ad:&str, requirements:&[(String,String)], ads:&[String], working:&[String], all:bool, unknown:bool)->bool {
    if !ads.is_empty() && !ads.iter().any(|s|s==ad) && !(unknown&&ad=="unknown"){return false;}
    if working.is_empty(){return true;}
    let known:Vec<_>=requirements.iter().filter(|(_,level)|level=="required").map(|(code,_)|code).collect();
    if known.is_empty(){return unknown;}
    if all{working.iter().all(|s|known.contains(&s))}else{working.iter().any(|s|known.contains(&s))}
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn aliases_and_multiple_cities(){assert!(location_matches("Sitten, VS","Sion, Lausanne"));assert!(location_matches("Genève","Geneva"));assert!(!location_matches("Bern","Sion"));}
    #[test] fn ad_is_not_requirement(){let text="Wir suchen Sie fur Ihre Arbeit und Ihre Aufgaben. English required.";assert_eq!(ad_language(text),"de");let req=working_languages(text);assert_eq!(req,vec![("en".into(),"required".into())]);}
    #[test] fn preferred_and_negated_are_not_mandatory(){assert_eq!(working_languages("Français souhaité. German not required."),vec![("fr".into(),"preferred".into()),("de".into(),"mentioned".into())]);}
    #[test] fn unknown_is_honest(){assert_eq!(ad_language("Developer 80%"),"unknown");assert!(working_languages("Developer 80%").is_empty());}
    #[test] fn language_and_or_unknown(){let req=working_languages("French required. English mandatory.");let selected=vec!["fr".into(),"de".into()];assert!(language_matches("en",&req,&[],&selected,false,false));assert!(!language_matches("en",&req,&[],&selected,true,false));assert!(!language_matches("en",&[],&[],&selected,false,false));assert!(language_matches("en",&[],&[],&selected,false,true));}
}
