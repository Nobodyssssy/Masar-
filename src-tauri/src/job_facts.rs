use serde::{Deserialize,Serialize};
use crate::swiss_filters::fold;

#[derive(Debug,Default,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct FactFilter {
    #[serde(default)] pub work_modes:Vec<String>,
    #[serde(default)] pub contracts:Vec<String>,
    #[serde(default)] pub seniorities:Vec<String>,
    #[serde(default)] pub categories:Vec<String>,
    #[serde(default)] pub employer_kind:String,
    #[serde(default)] pub description_only:bool,
    #[serde(default)] pub min_salary_chf:Option<u32>,
    #[serde(default)] pub salary_period:String,
    #[serde(default)] pub include_unknown:bool,
}
#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Facts {pub work_mode:String,pub contract:String,pub seniority:String,pub category:String,pub employer_kind:String,pub salary_chf:Option<u32>,pub salary_period:String}
fn has(text:&str,terms:&[&str])->bool{terms.iter().any(|term|text.contains(term))}
fn choose(text:&str,groups:&[(&str,&[&str])])->String{
    let matches:Vec<_>=groups.iter().filter(|(_,words)|has(text,words)).map(|(name,_)|*name).collect();
    if matches.len()==1{matches[0].into()}else{"unknown".into()}
}
pub fn extract(title:&str,company:&str,description:&str)->Facts {
    let title=fold(title);let description=fold(description);let text=format!(" {title} {description} ");
    let work_mode=if has(&text,&["hybrid work","hybrid role","hybride","hybridarbeit"]){"hybrid"}else if has(&text,&["not remote","no remote","pas de teletravail","kein homeoffice"]){"unknown"}else if has(&text,&["100% remote","fully remote","remote work","remote position","travail a distance","teletravail","homeoffice"]){"remote"}else if has(&text,&["on-site","on site","sur site","vor ort"]){"onsite"}else{"unknown"}.into();
    let contract=choose(&text,&[("permanent",&[" cdi","unbefristet","permanent contract","festanstellung"]),("temporary",&[" cdd"," befristet","temporary contract","temporare"]),("internship",&["internship","stage de ","stagiaire","praktikum"]),("apprenticeship",&["apprentissage","apprenticeship","lehrstelle"]),("contract",&["freelance","contractor","mandat independant"])]);
    let seniority=choose(&title,&[("entry",&["entry-level","entry level","debutant","sans experience","berufseinsteiger"]),("junior",&["junior"]),("mid",&["mid-level","mid level"]),("senior",&["senior","lead ","head of "])]);
    let category=choose(&title,&[("retail",&["vente","vendeur","vendeuse","retail","sales","verkauf"]),("logistics",&["logist","warehouse","magasinier","lager","preparateur de commandes"]),("administration",&["administrat","sachbearbeiter","back-office","back office","secretair"]),("technology",&["software","developer","developpeur","informatik","engineer"]),("finance",&["accountant","comptab","finance","audit","banking"])]);
    let employer_kind=if has(&fold(company),&["adecco","manpower","randstad","michael page","interiman","hays","leteam","le team","kelly services"]){"agency"}else{"unknown"}.into();
    let salary_period=if has(&description,&["per year","per annum","annual salary","annuel","jahrlich","pro jahr","p.a."]){"year"}else if has(&description,&["per month","mensuel","pro monat"]){"month"}else{"unknown"}.into();
    static SALARY:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let salary_chf=Some(SALARY.get_or_init(||regex::Regex::new(r"(?i)\bchf\s*([0-9]{2,3}['’ ,][0-9]{3}|[0-9]{4,6})\b").expect("Static salary expression"))).and_then(|r|r.captures(&text).and_then(|c|c.get(1)).map(|m|m.as_str().chars().filter(char::is_ascii_digit).collect::<String>())).and_then(|s|s.parse::<u32>().ok());
    Facts{work_mode,contract,seniority,category,employer_kind,salary_chf,salary_period}
}
impl FactFilter {
    pub fn validate(&self)->Result<(),String>{
        for (selected,allowed) in [(&self.work_modes,&["onsite","hybrid","remote","unknown"][..]),(&self.contracts,&["permanent","temporary","internship","apprenticeship","contract","unknown"][..]),(&self.seniorities,&["entry","junior","mid","senior","unknown"][..]),(&self.categories,&["retail","logistics","administration","technology","finance","unknown"][..])]{
            if selected.len()>8||selected.iter().any(|s|!allowed.contains(&s.as_str())){return Err("Invalid vacancy metadata filter".into());}
        }
        if !["","agency","direct","unknown"].contains(&self.employer_kind.as_str())||!["","year","month","unknown"].contains(&self.salary_period.as_str())||self.min_salary_chf.is_some_and(|s|s>2_000_000){return Err("Invalid employer or salary filter".into());}
        Ok(())
    }
    pub fn matches(&self,facts:&Facts,description_available:bool)->bool{
        for (selected,value) in [(&self.work_modes,&facts.work_mode),(&self.contracts,&facts.contract),(&self.seniorities,&facts.seniority),(&self.categories,&facts.category)]{
            if !selected.is_empty()&&!selected.contains(value)&&!(self.include_unknown&&value=="unknown"){return false;}
        }
        if self.description_only&&!description_available{return false;}
        if !self.employer_kind.is_empty()&&self.employer_kind!=facts.employer_kind&&!(self.include_unknown&&facts.employer_kind=="unknown"){return false;}
        if let Some(min)=self.min_salary_chf {match facts.salary_chf {Some(s) if s<min=>return false,None if !self.include_unknown=>return false,_=>{}}}
        if !self.salary_period.is_empty()&&self.salary_period!=facts.salary_period&&!(self.include_unknown&&facts.salary_period=="unknown"){return false;}
        true
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn explicit_metadata(){let facts=extract("Junior warehouse assistant CDD","Adecco","Temporary contract, on-site. CHF 60'000 annual salary.");assert_eq!(facts.seniority,"junior");assert_eq!(facts.category,"logistics");assert_eq!(facts.employer_kind,"agency");assert_eq!(facts.salary_chf,Some(60000));assert_eq!(facts.salary_period,"year");assert_eq!(facts.work_mode,"onsite");}
    #[test] fn ambiguity_and_unknown(){let facts=extract("Assistant","Company","CDI ou CDD. No remote work.");assert_eq!(facts.contract,"unknown");assert_eq!(facts.work_mode,"unknown");assert_eq!(facts.salary_chf,None);assert_eq!(facts.employer_kind,"unknown");}
    #[test] fn filters_do_not_fabricate(){let facts=extract("Assistant","Company","");let mut filter=FactFilter::default();filter.work_modes=vec!["remote".into()];assert!(!filter.matches(&facts,false));filter.include_unknown=true;assert!(filter.matches(&facts,false));filter.description_only=true;assert!(!filter.matches(&facts,false));}
}
