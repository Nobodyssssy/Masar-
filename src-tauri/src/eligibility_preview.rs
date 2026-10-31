//! Deterministic eligibility preview. Never writes to SQLite and never treats a missing
//! detail page as evidence that requirements are satisfied.
use serde::Serialize;
use serde_json::Value;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use crate::filter_parity;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence { pub category: String, pub quote: String, pub explanation: String }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityPreview {
    pub classification: String,
    pub legacy_keep: bool,
    pub legacy_reason: String,
    pub evidence: Vec<Evidence>,
    pub notes: Vec<String>,
}

fn fold(s: &str) -> String {
    s.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase).collect::<String>()
}
fn regex(pattern: &str) -> Regex { Regex::new(pattern).expect("static eligibility regex") }
fn has(pattern: &str, s: &str) -> bool { regex(pattern).is_match(s) }
fn add(ev:&mut Vec<Evidence>, cat:&str, quote:&str, why:&str) {
    ev.push(Evidence {category:cat.into(),quote:quote.trim().chars().take(350).collect(),explanation:why.into()});
}
fn job_text(job:&Value, field:&str)->String {
    job.get(field).and_then(Value::as_str).unwrap_or_default().to_owned()
}
fn normalize_html(input:&str)->String {
    let fragment = scraper::Html::parse_fragment(input);
    let text=fragment.root_element().text().collect::<Vec<_>>().join(" ");
    html_escape::decode_html_entities(&text).to_string()
        .split_whitespace().collect::<Vec<_>>().join(" ")
}
fn excerpts(input:&str)->Vec<String> {
    // Preserve DOM block boundaries before flattening. This avoids accidental
    // inheritance of requirement cues from unrelated paragraphs/list entries.
    let fragment=scraper::Html::parse_fragment(input);
    let selector=scraper::Selector::parse("p, li, h1, h2, h3, h4, h5, h6").expect("static selector");
    let mut blocks=vec![];
    for node in fragment.select(&selector) {
        // Avoid double-reading nested paragraph elements within list items.
        if node.value().name()=="li" && node.select(&scraper::Selector::parse("p").unwrap()).next().is_some() {continue;}
        let text=html_escape::decode_html_entities(&node.text().collect::<Vec<_>>().join(" ")).to_string();
        let text=text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() && blocks.last()!=Some(&text) {blocks.push(text);}
    }
    if blocks.is_empty() {blocks.push(normalize_html(input));}
    blocks.into_iter().flat_map(|block| {
        // Split only on sentence-ending punctuation + whitespace; keep abbreviations
        // and decimals if no subsequent whitespace.
        let mut parts=vec![];
        let mut start=0;
        for (idx,c) in block.char_indices() {
            if matches!(c,'!'|'?'|'.'|';') && block[idx+c.len_utf8()..].chars().next().is_some_and(char::is_whitespace) {
                let part=block[start..idx+c.len_utf8()].trim();
                if !part.is_empty(){parts.push(part.to_string());}
                start=idx+c.len_utf8();
            }
        }
        let rest=block[start..].trim();if !rest.is_empty(){parts.push(rest.to_string());}
        parts
    }).take(500).collect()
}

pub fn evaluate(job:&Value, description_html:Option<&str>)->EligibilityPreview {
    let legacy=filter_parity::evaluate(job);
    // The frozen legacy filter also searches the employer name, so a bakery
    // name can reject a retail job before the description is inspected.
    // Exception is narrow: only bakery/pastry employer-name keyword hits,
    // with a clean title and with every other legacy rule still passing.
    // The actual food-service duties below can still definitively exclude it.
    let employer_name=job.get("company")
        .and_then(|v|v.as_str().or_else(||v.get("name").and_then(Value::as_str)))
        .unwrap_or("");
    let title_name=job_text(job,"title");
    let bakery_keywords=["boulangerie","boulanger","patisserie","patissier"];
    let bakery_name_hit=bakery_keywords.iter().any(|k|fold(employer_name).contains(k));
    let bakery_only_exclusion=!legacy.keep
        && legacy.reason.starts_with("excluded '")
        && bakery_keywords.iter().any(|k|legacy.reason==format!("excluded '{k}'"))
        && bakery_name_hit
        && !bakery_keywords.iter().any(|k|fold(&title_name).contains(k));
    let mut clean_job=job.clone();
    if bakery_only_exclusion {
        if let Some(map)=clean_job.as_object_mut() {
            map.insert("company".into(),Value::String("Neutral Employer".into()));
        }
    }
    let effective_legacy=if bakery_only_exclusion {
        filter_parity::evaluate(&clean_job)
    } else {
        filter_parity::evaluate(job)
    };
    let mut out=EligibilityPreview {classification:"unverified".into(),legacy_keep:effective_legacy.keep,
        legacy_reason:effective_legacy.reason.clone(), evidence:vec![],notes:vec![]};
    if !effective_legacy.keep {
        out.classification="excluded".into();
        add(&mut out.evidence,"legacy_filter",&effective_legacy.reason,"Existing strict Python-compatible exclusion rule");
        return out;
    }
    // Narrow occupation-only exclusions, justified by the explicitly named
    // regulated/specialist role. Do not infer missing qualifications for broad
    // labels such as assistant, adviser, operator, or customer service.
    let occupation_title=fold(&job_text(job,"title"));
    let definite_specialist = [
        (r"\b(mecatronicien|diagnosticien d.automobiles|automotive mechatronic)\b", "Specialized automotive occupation"),
        (r"\b(technologue en dispositifs medicaux|medical device technologist)\b", "Specialized medical-device occupation"),
        (r"\b(quality engineer|ingenieur qualite|ingenieur process)\b", "Engineering occupation"),
        (r"\b(assistant technique specialise en salle d.operation|atsso)\b", "Specialized operating-room occupation"),
        (r"\b(team leader bureau technique|responsable bureau technique)\b", "Technical management occupation"),
        (r"\b(expert(?:e)? en caisses? de pension)\b", "Specialist occupational-pension expert role"),
        (r"\b(gestionnaire|gerant|conseillere?|commercial|expert|administrateur|specialiste)\b.*\b(lpp|prevoyance professionnelle|caisses? de pensions?|pension fund|bvg)\b", "Occupational-pension (LPP) specialist role requiring specialist credentials"),
        (r"\b(lpp|prevoyance professionnelle|bvg)\b.*\b(gestionnaire|gerant|conseillere?|commercial|expert|administrateur|specialiste)\b", "Occupational-pension (LPP) specialist role requiring specialist credentials"),
    ];
    if let Some((_,reason))=definite_specialist.iter().find(|(pattern,_)|has(pattern,&occupation_title)) {
        out.classification="excluded".into();
        add(&mut out.evidence,"specialized_occupation_title",&job_text(job,"title"),reason);
        return out;
    }
    let Some(raw)=description_html.filter(|s| !s.trim().is_empty()) else {
        out.notes.push("Full advertisement description unavailable; eligibility cannot be verified".into());
        return out;
    };
    if raw.len()>150_000 {out.notes.push("Description exceeds evaluation limit".into());return out;}
    let readable=normalize_html(raw);
    if readable.len()<80 {
        out.notes.push("Description is too short to verify requirements".into());return out;
    }
    let full=fold(&readable);
    let title=fold(&job_text(job,"title"));
    let employer=fold(&job.get("company").and_then(|v|v.as_str().or_else(||v.get("name").and_then(Value::as_str))).unwrap_or(""));
    // Sector signals require an employment context, not incidental employer-name substrings.
    let sector_role = has(r"(?i)\b(serveur|serveuse|barman|barmaid|sommelier|commis de cuisine|cuisinier|restauration|restaurant|brasserie|cafe|café|traiteur|catering|casino|paris sportifs|betting)\b", &title);
    let sector_employer = has(r"\b(restaurant|brasserie|cafe|hotel|casino|bar)\b", &employer)
        && has(r"\b(restaurant|restauration|cuisine|service en salle|hospitalite|hotellerie|casino|paris sportifs|jeux d'argent)\b", &full);
    if sector_role || sector_employer {
        add(&mut out.evidence,"restricted_sector",&job_text(job,"title"),"Restaurant, food-service or gambling employment");
        out.classification="excluded".into();return out;
    }
    // Controlling/accounting career positions are excluded even when titles do
    // not contain the older prefilter's specific keywords.
    let accounting_role = has(r"\b(controleur(?:se)? de gestion|controle de gestion|controlling|financial controller|finance controller|comptable|fiduciaire|accountant|audit financier|gestionnaire comptable)\b", &title);
    if accounting_role {
        add(&mut out.evidence,"restricted_accounting_career",&job_text(job,"title"),"Accounting, fiduciary or financial controlling career is outside the permitted target roles");
        out.classification="excluded".into(); return out;
    }
    // Evaluate the workplace and actual duties, not incidental food words in
    // an unrelated employer's name. Exclude explicit food-service employment.
    let food_service_context = has(r"\b(restaurant|restauration|service en salle|service a table|cafe-restaurant|brasserie|traiteur|catering|service de restauration|food service|gastronomie|gastronomic)\b", &full);
    let service_duties = has(r"\b(servir les clients a table|service en salle|service a table|preparer des boissons|service de restauration|service des repas|service du petit.dejeuner|service au comptoir|restauration sur place|consommation sur place)\b", &full);
    let restaurant_workplace = has(r"\b(restaurant|brasserie|cafe|cafe-restaurant|traiteur|catering)\b", &employer);
    if (food_service_context && service_duties) || (restaurant_workplace && food_service_context) {
        add(&mut out.evidence,"restricted_food_service",&readable,"Employment responsibilities or workplace indicate restaurant / food service");
        out.classification="excluded".into(); return out;
    }
    // Bakery/food retail can encompass either ordinary retail or excluded
    // food-service tasks: avoid promoting it to Eligible without more evidence.
    let food_retail_ambiguous = has(r"\b(boulangerie|patisserie|boulanger-patissier|tea.room|snack|sandwicherie|bistro|bakery|food and beverage)\b", &format!("{employer} {full}"));
    if food_retail_ambiguous {
        add(&mut out.evidence,"food_sector_review",&job_text(job,"title"),"Food retail/bakery workplace needs confirmation that this is not a restaurant or food-service role");
    }
    let mut ambiguous=food_retail_ambiguous;
    let mut preferred=false;
    let mut verified_equivalence=false;
    let mut positive_duties=false;
    let role_family=has(r"\b(vendeur|vendeuse|vente|caissier|caissiere|logistique|magasinier|gestionnaire de stock|service client|customer service|administration commerciale|assistant administratif|assistante administrative|gestion des commandes|order management|achats|planification|ordonnancement|approvisionnement|supply chain|acheteur|acheteuse|back.?office|saisie|secr[eé]tariat|secr[eé]taire|accueil|conseiller(?:e)? (?:de )?vente|assistant(?:e)? commercial|employ[ée] de commerce|gestionnaire administratif|gestionnaire administrative)\b",&title);
    for sentence in excerpts(raw) {
        let s=fold(&sentence);
        if role_family && has(r"\b(accueil|conseil client|vente|encaissement|mise en rayon|stocks?|inventaire|reception des marchandises|preparation de commandes|suivi de commandes|service clients?|relations? clients?|traitement des dossiers|saisie de donnees|customer service|order processing|warehouse|cashier|stock management)\b",&s) {
            positive_duties=true;
        }
        let must=has(r"\b(obligatoire|exige(?:e|s)?|requis(?:e|es)?|imperatif|indispensable|must have|must possess|required|mandatory|necessaire|necessaires)\b",&s);
        let wish=has(r"\b(souhaite(?:e|s)?|atout|idealement|un plus|apprecie(?:e|s)?|preferred|desirable|nice to have|advantage)\b",&s);
        let equivalence=has(r"\b(ou experience equivalente|ou experience similaire|ou formation equivalente|or equivalent experience)\b",&s);
        let diploma=has(r"\b(cfc|certificat federal de capacite|brevet federal|bachelor|master|licence universitaire|diplome universitaire|university degree|bachelor'?s degree|master'?s degree)\b",&s);
        let forklift=has(r"\b(permis (de )?cariste|certificat cariste|suva (r1|r2)|forklift licen[cs]e)\b",&s);
        let driver=has(r"\b(permis (de conduire )?(b|c|ce|d|be)|categorie (b|c|ce|d|be)|driving licen[cs]e)\b",&s);
        let german=has(r"\b(allemand|deutsch|german)\b",&s) && has(r"\b(courant|maitrise|parfait|fluide|fluent|c1|c2|b2|bilingue|niveau avance)\b",&s);
        let outbound=has(r"\b(prospection telephonique|appels? a froid|cold.calling|telemarketing|outbound calling|demarchage telephonique)\b",&s);
        let alcohol=has(r"\b(vente|distribution|production|fabrication|commercialisation|sales|selling)\b",&s)
            && has(r"\b(alcool|boissons alcoolisees|spiritueux|vins?|bieres?|alcohol)\b",&s);
        let regulated=has(r"\b(avocat|notaire|infirmier|medecin|pharmacien|enseignant diplome|expert comptable|licence de conducteur de train|permis de conduire des trains|train driver licen[cs]e)\b",&s);
        let railway=has(r"\b(conducteur de train|conductrice de train|mecanicien de locomotive|locomotive driver|train driver)\b",&title);
        if railway && has(r"\b(certification|licence|permis|formation ferroviaire|qualification|habilitation)\b",&s) {
            ambiguous=true;
            add(&mut out.evidence,"railway_qualification_check",&sentence,"Railway role: verify profession-specific qualification and employer training route");
        }
        if alcohol && (must || has(r"\b(mission|activite|responsabilite|tache|role|vous serez charge)\b",&s)) {
            add(&mut out.evidence,"alcohol_sales",&sentence,"Alcohol production, distribution or sale is incompatible");
        }
        if outbound && (must || has(r"\b(mission|activite|responsabilite|tache|role|vous serez charge)\b",&s)) {
            add(&mut out.evidence,"mandatory_outbound",&sentence,"Mandatory telephone prospecting is excluded");
        }
        if must && !wish {
            if diploma {
                if equivalence {verified_equivalence=true;add(&mut out.evidence,"equivalent_experience",&sentence,"Explicit equivalent experience accepted; relevance needs assessment");ambiguous=true;}
                else {add(&mut out.evidence,"mandatory_diploma",&sentence,"Mandatory qualification not held");}
            }
            if forklift {add(&mut out.evidence,"mandatory_forklift",&sentence,"Mandatory forklift qualification not held");}
            if german {add(&mut out.evidence,"mandatory_german",&sentence,"Mandatory advanced German requirement not met");}
            if driver {
                if has(r"\b(permis (de conduire )?b|categorie b)\b",&s) && !has(r"\b(c|ce|d|be)\b",&s) {
                    add(&mut out.evidence,"license_b_ok",&sentence,"Category B licence held");
                } else {add(&mut out.evidence,"license_check",&sentence,"Driving licence category or vehicle access requires review");ambiguous=true;}
            }
            if regulated {add(&mut out.evidence,"regulated_profession",&sentence,"Regulated professional qualification requires review");ambiguous=true;}
        } else if (diploma || forklift || german || outbound || driver) && wish {
            preferred=true;
            add(&mut out.evidence,"preferred_requirement",&sentence,"Preferred requirement does not by itself exclude");
        } else if (diploma || forklift || german || outbound || driver) && !must {
            ambiguous=true;
            add(&mut out.evidence,"ambiguous_requirement",&sentence,"Requirement not clearly mandatory or preferred");
        }
    }
    let definitive=out.evidence.iter().any(|e|matches!(e.category.as_str(),"mandatory_diploma"|"mandatory_forklift"|"mandatory_german"|"mandatory_outbound"|"alcohol_sales"));
    if definitive {out.classification="excluded".into();return out;}
    // Full descriptions can still omit actual requirements or contain hidden constraints.
    // Only supported target families can be tentatively accepted, and uncertain requirements
    // remain unverified.
    let target_role=has(r"\b(vendeur|vendeuse|vente|caissier|caissiere|logistique|magasinier|gestionnaire de stock|service client|customer service|administration commerciale|assistant administratif|assistante administrative|gestion des commandes|order management|achats|planification|ordonnancement|approvisionnement|supply chain|acheteur|acheteuse|back.?office|saisie|secr[eé]tariat|secr[eé]taire|accueil|conseiller(?:e)? (?:de )?vente|assistant(?:e)? commercial|employ[ée] de commerce|gestionnaire administratif|gestionnaire administrative)\b",&title);
    if !target_role {out.notes.push("Role is outside explicit preferred job families; manual review needed".into());}
    if verified_equivalence {out.notes.push("Equivalent experience clause requires matching role-specific evidence".into());}
    if ambiguous {out.notes.push("Some requirements are ambiguous and require review".into());}
    if target_role && !positive_duties {out.notes.push("No clear job-duty evidence supporting the preferred CV families".into());}
    if !ambiguous && target_role && positive_duties && readable.len()>=150 {
        out.classification="eligible".into();
        out.notes.push("Provisional rule-based eligibility from available description; not a guarantee of employer acceptance".into());
    } else if preferred {out.notes.push("Preferred-only qualifications do not trigger automatic exclusion".into());}
    out
}

#[cfg(test)]
mod tests {
 use super::*;
 fn j(title:&str)->Value {serde_json::json!({"title":title,"company":"Entreprise neutre","place":"Sion","grades":[100]})}
 fn d(s:&str)->String {format!("<p>Notre société recrute pour renforcer son équipe en Suisse. Les responsabilités sont variées et le travail est organisé dans un environnement professionnel.</p><p>{s}</p>")}
 #[test] fn no_description_is_unverified(){assert_eq!(evaluate(&j("Vendeur"),None).classification,"unverified")}
 #[test] fn mandatory_cfc_excluded(){assert_eq!(evaluate(&j("Vendeur"),Some(&d("CFC obligatoire pour ce poste."))).classification,"excluded")}
 #[test] fn bachelor_required_excluded(){assert_eq!(evaluate(&j("Vendeur"),Some(&d("Bachelor required for this role."))).classification,"excluded")}
 #[test] fn preferred_cfc_not_excluded(){assert_ne!(evaluate(&j("Vendeur"),Some(&d("CFC souhaité, nous valorisons aussi l'expérience en magasin."))).classification,"excluded")}
 #[test] fn equivalent_remains_unverified(){assert_eq!(evaluate(&j("Vendeur"),Some(&d("CFC obligatoire ou expérience équivalente en vente."))).classification,"unverified")}
 #[test] fn forklift_mandatory_excluded(){assert_eq!(evaluate(&j("Magasinier"),Some(&d("Permis cariste obligatoire pour accéder à ce poste."))).classification,"excluded")}
 #[test] fn forklift_preferred_not_excluded(){assert_ne!(evaluate(&j("Magasinier"),Some(&d("Permis cariste souhaité, formation possible."))).classification,"excluded")}
 #[test] fn mandatory_german_excluded(){assert_eq!(evaluate(&j("Vendeur"),Some(&d("Allemand courant obligatoire."))).classification,"excluded")}
 #[test] fn mandatory_outbound_excluded(){assert_eq!(evaluate(&j("Vendeur"),Some(&d("Prospection téléphonique obligatoire."))).classification,"excluded")}
 #[test] fn mandatory_master_french_excluded(){assert_eq!(evaluate(&j("Assistant administratif"),Some(&d("Master exigé pour ce poste."))).classification,"excluded")}
 #[test] fn mandatory_forklift_english_excluded(){assert_eq!(evaluate(&j("Magasinier"),Some(&d("Forklift license required for all warehouse staff."))).classification,"excluded")}
 #[test] fn unavailable_description_must_not_be_eligible(){let out=evaluate(&j("Vendeur"),Some("<p>Poste</p>"));assert_eq!(out.classification,"unverified")}
 #[test] fn missing_role_match_unverified(){assert_eq!(evaluate(&j("Coordinateur"),Some(&d("Accueil et classement de dossiers clients."))).classification,"unverified")}
 #[test] fn html_entities_handled(){assert!(normalize_html("<p>CFC&nbsp;souhaité</p>").contains("CFC"))}
 #[test] fn block_boundary_is_preserved(){let parts=excerpts("<h3>The ideal candidate will have</h3><ul><li>Forklift certificate preferred</li><li>German fluent mandatory</li></ul>");assert_eq!(parts.len(),3);assert_eq!(parts[1],"Forklift certificate preferred");assert_eq!(parts[2],"German fluent mandatory");}
 #[test] fn independent_requirements_do_not_inherit_must(){let result=evaluate(&j("Magasinier"),Some(&d("<ul><li>Permis cariste souhaité</li><li>Allemand courant obligatoire</li></ul>")));assert!(result.evidence.iter().any(|e|e.category=="mandatory_german"));assert!(!result.evidence.iter().any(|e|e.category=="mandatory_forklift"));}
 #[test] fn generic_description_cannot_make_eligible() {
   let r=evaluate(&j("Vendeur"),Some(&d("Nous recherchons un profil motivé pour rejoindre notre équipe.")));
   assert_eq!(r.classification,"unverified");
 }
 #[test] fn positive_retail_duties_can_support_eligibility() {
   let r=evaluate(&j("Vendeur"),Some(&d("Accueil client, conseil client, encaissement et mise en rayon.")));
   assert_eq!(r.classification,"eligible");
 }
 #[test] fn train_operator_requires_manual_profession_check() {
   let r=evaluate(&j("Conducteur de train"),Some(&d("Une certification et une formation ferroviaire sont nécessaires.")));
   assert_ne!(r.classification,"eligible");
 }
 #[test] fn pastry_sales_not_automatically_eligible() {
   let job=serde_json::json!({"title":"Vendeur","company":"La Gourmandine Boulangerie","place":"Savièse","grades":[50]});
   let r=evaluate(&job,Some(&d("Accueil client, conseil client, encaissement et mise en rayon dans notre boulangerie.")));
   assert_eq!(r.classification,"unverified");
   assert!(r.evidence.iter().any(|e|e.category=="food_sector_review"));
 }
 #[test] fn explicit_food_service_is_excluded() {
   let r=evaluate(&j("Vendeur"),Some(&d("Accueil client et encaissement. Missions: service en salle et restauration sur place.")));
   assert_eq!(r.classification,"excluded");
   assert!(r.evidence.iter().any(|e|e.category=="restricted_food_service"));
 }
 #[test] fn unrelated_food_word_does_not_exclude() {
   let job=serde_json::json!({"title":"Vendeur","company":"Pomme Informatique SA","place":"Sion","grades":[80]});
   let r=evaluate(&job,Some(&d("Accueil client, encaissement et gestion de stock de matériel informatique.")));
   assert_eq!(r.classification,"eligible");
 }
 #[test] fn controlling_career_excluded() {
   let r=evaluate(&j("Contrôleur de gestion senior"),Some(&d("Superviser le reporting, les analyses et la planification financière.")));
   assert_eq!(r.classification,"excluded");
 }
 #[test] fn bakery_employer_name_alone_not_definitive() {
   let job=serde_json::json!({"title":"Vendeur","company":"Boulangerie Centrale","place":"Sion","grades":[80]});
   let r=evaluate(&job,Some(&d("Accueil client et encaissement dans un commerce de détail.")));
   assert_eq!(r.classification,"unverified");
 }
 #[test] fn bakery_with_explicit_restaurant_service_still_excluded() {
   let job=serde_json::json!({"title":"Vendeur","company":"Boulangerie Centrale","place":"Sion","grades":[80]});
   let r=evaluate(&job,Some(&d("Accueil client et encaissement. Missions : service en salle et restauration sur place.")));
   assert_eq!(r.classification,"excluded");
   assert!(r.evidence.iter().any(|e|e.category=="restricted_food_service"));
 }
 #[test] fn bakery_exception_does_not_override_city_restriction() {
   let job=serde_json::json!({"title":"Vendeur","company":"Boulangerie Centrale","place":"Zurich","grades":[80]});
   let r=evaluate(&job,Some(&d("Accueil client et encaissement dans un commerce de détail.")));
   assert_eq!(r.classification,"excluded");
   assert!(r.evidence.iter().any(|e|e.category=="legacy_filter"));
 }
 #[test] fn old_filter_always_wins(){let r=evaluate(&j("Cuisinier"),Some(&d("Formation sur place.")));assert_eq!(r.classification,"excluded");assert!(r.evidence.iter().any(|x|x.category=="legacy_filter"));}
}

#[cfg(test)] mod occupation_step12_tests {
 use super::*;
 #[test] fn definite_specialists_without_details(){
  for title in ["Mécatronicien / Diagnosticien d'automobiles", "Technologue en dispositifs médicaux", "Quality Engineer Process", "ATSSO - Assistant technique spécialisé en salle d'opération"] {
   let j=serde_json::json!({"title":title,"company":"Generic SA","place":"Lausanne","grades":[100]});
   let r=evaluate(&j,None);assert_eq!(r.classification,"excluded", "{title}: {:?}",r.notes);
  }
 }
 #[test] fn generic_customer_roles_stay_unverified_without_details(){
  for title in ["Conseiller-ère SAV", "Vendeur(euse)", "Assistant administratif"] {
   let j=serde_json::json!({"title":title,"company":"Generic SA","place":"Sion","grades":[100]});
   assert_eq!(evaluate(&j,None).classification,"unverified");
  }
 }
}

#[cfg(test)]
mod step31_role_tests {
    use super::*;
    #[test]
    fn planning_and_ordonnancement_role_is_target_family() {
        let j=serde_json::json!({"title":"Agent de planification & ordonnancement 80-100%","company":"Adecco","place":"Crissier","grades":[80,100]});
        let r=evaluate(&j,None);
        assert_ne!(r.classification,"excluded");
        assert!(!r.notes.iter().any(|n|n.contains("outside explicit preferred job families")));
    }
    #[test]
    fn lpp_pension_specialist_title_is_excluded() {
        let j=serde_json::json!({"title":"Gestionnaire Commercial LPP FR/ALL 80%-100%","company":"Albedis Lausanne","place":"Lausanne","grades":[80]});
        assert_eq!(evaluate(&j,None).classification,"excluded");
    }
    #[test]
    fn bare_commercial_title_stays_unverified() {
        let j=serde_json::json!({"title":"Commercial terrain","company":"Some SA","place":"Lausanne","grades":[100]});
        assert_eq!(evaluate(&j,None).classification,"unverified");
    }
    #[test]
    fn lpp_only_in_description_does_not_exclude() {
        let j=serde_json::json!({"title":"Vendeur 80%","company":"Boutique SA","place":"Lausanne","grades":[80]});
        let r=evaluate(&j,Some("<p>Accueil client, conseil client, encaissement et mise en rayon. Nous offrons un plan de prévoyance LPP conforme.</p>"));
        assert_ne!(r.classification,"excluded");
    }
}
