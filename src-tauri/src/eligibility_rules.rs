use serde::Deserialize;
use crate::{swiss_filters::fold,job_facts::Facts};

#[derive(Debug,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Rules {
    #[serde(default="yes")] pub mandatory_cfc:bool,
    #[serde(default="yes")] pub mandatory_degrees:bool,
    #[serde(default="yes")] pub mandatory_forklift:bool,
    #[serde(default="yes")] pub outbound_calls:bool,
    #[serde(default="yes")] pub alcohol_sales:bool,
    #[serde(default="yes")] pub restaurant_service:bool,
    #[serde(default)] pub target_families_only:bool,
    #[serde(default="vs_min")] pub valais_minimum:u32,
    #[serde(default="vd_min")] pub vaud_minimum:u32,
}
fn yes()->bool{true} fn vs_min()->u32{40} fn vd_min()->u32{70}
fn contains(text:&str,words:&[&str])->bool{words.iter().any(|word|text.contains(word))}
impl Rules {
    pub fn validate(&self)->Result<(),String>{if self.valais_minimum>100||self.vaud_minimum>100{Err("Personal workload thresholds must be 0–100".into())}else{Ok(())}}
    pub fn evaluate(&self,title:&str,place:&str,description:&str,grades:&[u32],facts:&Facts)->String{
        let title=fold(title);let text=fold(description);let mut verify=description.trim().is_empty();
        if self.alcohol_sales&&(contains(&title,&["caviste","sommelier","wine sales"])||contains(&text,&["vente d'alcool","vente de boissons alcoolisees","verkauf von alkohol"])){return "excluded".into();}
        if self.restaurant_service&&(contains(&title,&["serveur","serveuse","restaurant service","restaurantfach"])||contains(&text,&["service en restauration","service en salle","restaurant service duties"])){return "excluded".into();}
        for sentence in text.split(['.',';','\n','!']) {
            let required=contains(sentence,&["obligatoire","exige","requis","required","mandatory","must ","titulaire","erforderlich","zwingend","voraussetzung"]);
            let preferred=contains(sentence,&["souhaite","un atout","preferred","von vorteil","wunschenswert","not required","pas obligatoire","non requis","nicht erforderlich"]);
            let equivalent=contains(sentence,&["equivalent","experience equivalente","oder gleichwertig","or equivalent"]);
            for (enabled,present) in [
                (self.mandatory_cfc,sentence.split(|c:char|!c.is_alphabetic()).any(|s|s=="cfc"||s=="efz")),
                (self.mandatory_degrees,contains(sentence,&["bachelor","master","universitaire","hochschulabschluss"])),
                (self.mandatory_forklift,contains(sentence,&["permis cariste","permis de cariste","certificat cariste","forklift licen","forklift certif","staplerausweis"])),
                (self.outbound_calls,contains(sentence,&["prospection telephonique","outbound calls","telephone prospecting","telefonakquise"])),
            ] {
                if enabled&&present&&!preferred {if required&&!equivalent{return "excluded".into();}verify=true;}
            }
        }
        if self.target_families_only&&!matches!(facts.category.as_str(),"retail"|"logistics"|"administration"){return "excluded".into();}
        let minimum=if crate::geography::matches(place,&["VS".into()],None,"",false){self.valais_minimum}else if crate::geography::matches(place,&["VD".into()],None,"",false){self.vaud_minimum}else{0};
        if minimum>0 {if grades.is_empty(){verify=true;}else if grades.iter().all(|&g|g<minimum){return "excluded".into();}}
        if verify{"unverified".into()}else{"eligible".into()}
    }
}
#[cfg(test)] mod tests {
    use super::*;
    fn rules()->Rules{serde_json::from_str("{}").unwrap()}
    fn classify(r:&Rules,text:&str)->String{r.evaluate("Vendeur","Sion",text,&[80],&crate::job_facts::extract("Vendeur","Shop",text))}
    #[test] fn mandatory_preferred_equivalent(){assert_eq!(classify(&rules(),"CFC obligatoire."),"excluded");assert_eq!(classify(&rules(),"CFC souhaité."),"eligible");assert_eq!(classify(&rules(),"CFC obligatoire ou expérience équivalente."),"unverified");}
    #[test] fn editable_rules_do_not_change_records(){let mut r=rules();r.mandatory_cfc=false;assert_eq!(classify(&r,"CFC obligatoire."),"eligible");r.mandatory_cfc=true;assert_eq!(classify(&r,"CFC obligatoire."),"excluded");}
    #[test] fn regional_workload_only_in_preset(){let r=rules();let facts=crate::job_facts::extract("Vendeur","Shop","Votre travail");assert_eq!(r.evaluate("Vendeur","Lausanne","Votre travail",&[60],&facts),"excluded");assert_eq!(r.evaluate("Vendeur","Sion","Votre travail",&[60],&facts),"eligible");}
}
