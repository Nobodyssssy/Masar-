/* Search page (Step 1/7). Uses registered Tauri commands; no scraping in WebView. */
(()=>{
'use strict';
const PROVIDERS=[
  ['jobup','Jobup','collector','https://www.jobup.ch/fr/'],
  ['jobs_ch','Jobs.ch','collector','https://www.jobs.ch/fr/'],
  ['indeed_ch','Indeed Suisse','website','https://ch.indeed.com/'],
  ['emploi_en_suisse','Emploi en Suisse','website','https://emploi-en-suisse.ch/'],
  ['jobscout24','JobScout24','collector','https://www.jobscout24.ch/fr/'],
  ['okjob','OK Job','collector','https://www.okjob.ch/offres-demplois/'],
  ['100000jobs','100000jobs','website','https://www.100000jobs.ch/'],
  ['job_room','Job-Room','website','https://www.job-room.ch/index.html'],
  ['zuri_jobs','Züri Jobs','collector','https://www.zueri.jobs/jobs'],
  ['michael_page','Michael Page','collector','https://www.michaelpage.ch/jobs'],
  ['linkedin_jobs','LinkedIn Jobs','collector','https://www.linkedin.com/jobs/'],
  ['swissdevjobs','SwissDevJobs','collector','https://swissdevjobs.ch/jobs'],
  ['efinancialcareers','eFinancialCareers','collector','https://www.efinancialcareers.ch/jobs']
];
const $s=q=>document.querySelector('#view-search '+q);
let running=false,stopping=false,profiles=[],lastResults=[],resultQuerySerial=0;
const call=(cmd,args={})=>window.__TAURI__.core.invoke(cmd,args);
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function mount(){
  if(document.getElementById('view-search'))return;
  const main=document.querySelector('main');if(!main)return;
  const section=document.createElement('section');section.className='view';section.id='view-search';
  section.innerHTML=`<div class="search-heading"><div><h2>Recherche d’offres</h2><p>Collecte sur les sources disponibles et filtrage local des annonces conservées dans SQLite.</p></div><button type="button" id="search-open-sources">Diagnostics des sources</button></div>
  <div class="search-columns"><form id="search-config" class="search-panel">
    <div class="search-section-title">Profil de recherche</div>
    <div class="search-form-row"><label for="search-profile">Profil enregistré</label><select id="search-profile"><option value="">Profil par défaut</option></select></div>
    <div class="search-form-row"><label for="search-profile-name">Nom du profil</label><input id="search-profile-name" placeholder="Ex. Vente VS / VD" maxlength="65"></div>
    <div class="search-actions-inline"><button type="button" id="search-save-profile">Enregistrer</button><button type="button" id="search-delete-profile">Supprimer</button></div>
    <div class="search-actions-inline"><button type="button" id="search-duplicate-profile">Dupliquer</button><button type="button" id="search-rename-profile">Renommer</button><button type="button" id="search-export-profile">Exporter</button><button type="button" id="search-import-profile">Importer</button><input id="search-profile-file" type="file" accept="application/json,.json" hidden></div>
    <div class="search-form-row"><label for="search-preset">Règles d’éligibilité</label><select id="search-preset"><option value="">Toutes les offres — aucune règle personnelle</option><option value="current">Mon éligibilité actuelle (VS / VD)</option></select></div>
<fieldset id="search-rule-editor" hidden><legend>Modifier les règles du profil</legend><label class="search-checkbox"><input type="checkbox" data-rule="mandatoryCfc" checked> Exclure CFC / EFZ obligatoire sans équivalence</label><label class="search-checkbox"><input type="checkbox" data-rule="mandatoryDegrees" checked> Exclure Bachelor / Master obligatoire</label><label class="search-checkbox"><input type="checkbox" data-rule="mandatoryForklift" checked> Exclure permis cariste obligatoire</label><label class="search-checkbox"><input type="checkbox" data-rule="outboundCalls" checked> Exclure prospection téléphonique obligatoire</label><label class="search-checkbox"><input type="checkbox" data-rule="alcoholSales" checked> Exclure vente d’alcool</label><label class="search-checkbox"><input type="checkbox" data-rule="restaurantService" checked> Exclure service en restauration</label><label class="search-checkbox"><input type="checkbox" data-rule="targetFamiliesOnly" > Limiter aux familles vente, logistique et administration</label><div class="search-form-row"><label for="search-rule-vs">Taux minimum Valais</label><input id="search-rule-vs" type="number" min="0" max="100" value="40"><label for="search-rule-vd">Taux minimum Vaud</label><input id="search-rule-vd" type="number" min="0" max="100" value="70"></div><p class="search-muted">CFC souhaité : pas de rejet. Équivalence : à vérifier. Ces choix sont enregistrés dans le profil ; aucune annonce n’est réécrite.</p></fieldset>    <p class="search-muted">Ces règles changent la visibilité seulement. Les offres collectées et les décisions sont conservées.</p>
    <hr><div class="search-section-title">Période et filtres</div>
    <div class="search-form-row"><label for="search-days">Publication</label><select id="search-days"><option value="1">24 h / date du jour</option><option value="3" selected>3 derniers jours</option><option value="7">7 derniers jours</option><option value="14">14 derniers jours</option><option value="30">30 derniers jours</option><option value="custom">Dates personnalisées</option></select></div>
    <div class="search-form-row"><label for="search-keywords">Mots-clés</label><input id="search-keywords" placeholder="vente, administration..." maxlength="220"></div>
    <div class="search-form-row"><label for="search-exclude">Exclure mots</label><input id="search-exclude" placeholder="ex. stage, apprentissage" maxlength="220"></div>
    <div class="search-form-row"><label for="search-location">Ville / région</label><input id="search-location" placeholder="Toute la Suisse, ou Sion, Lausanne..." maxlength="140"></div>
<div class="search-form-row"><label for="search-cantons">Cantons (aucune sélection : toute la Suisse)</label><select id="search-cantons" multiple size="5"><option value="AG">Argovie (AG)</option><option value="AI">Appenzell Rhodes-Intérieures (AI)</option><option value="AR">Appenzell Rhodes-Extérieures (AR)</option><option value="BE">Berne (BE)</option><option value="BL">Bâle-Campagne (BL)</option><option value="BS">Bâle-Ville (BS)</option><option value="FR">Fribourg (FR)</option><option value="GE">Genève (GE)</option><option value="GL">Glaris (GL)</option><option value="GR">Grisons (GR)</option><option value="JU">Jura (JU)</option><option value="LU">Lucerne (LU)</option><option value="NE">Neuchâtel (NE)</option><option value="NW">Nidwald (NW)</option><option value="OW">Obwald (OW)</option><option value="SG">Saint-Gall (SG)</option><option value="SH">Schaffhouse (SH)</option><option value="SO">Soleure (SO)</option><option value="SZ">Schwytz (SZ)</option><option value="TG">Thurgovie (TG)</option><option value="TI">Tessin (TI)</option><option value="UR">Uri (UR)</option><option value="VD">Vaud (VD)</option><option value="VS">Valais (VS)</option><option value="ZG">Zoug (ZG)</option><option value="ZH">Zurich (ZH)</option></select></div><div class="search-form-row"><label for="search-radius">Rayon en km (facultatif)</label><input id="search-radius" type="number" min="0" max="500"><label for="search-radius-center">Centre du rayon (ville / NPA)</label><input id="search-radius-center" placeholder="1950"></div><label class="search-checkbox"><input id="search-unknown-locations" type="checkbox"> Inclure les lieux inconnus</label><p class="search-muted">Filtres locaux sur le lieu de travail fourni. Rayon approximatif à vol d’oiseau, données GeoNames locales ; aucune adresse employeur n’est substituée.</p>    <div class="search-form-row"><label for="search-grade">Taux minimum</label><select id="search-grade"><option value="">Aucun minimum</option><option value="0">0 %</option><option value="40">40 %</option><option value="50">50 %</option><option value="70">70 %</option><option value="80">80 %</option><option value="100">100 %</option></select></div>
    <div class="search-form-row"><label for="search-max-grade">Taux maximum</label><input id="search-max-grade" type="number" min="0" max="100" placeholder="100"></div>
    <div class="search-form-row"><label for="search-date-from">Publication du / au</label><input id="search-date-from" type="date"><input id="search-date-to" type="date"></div>
    <label class="search-checkbox"><input id="search-keyword-all" type="checkbox"> Tous les groupes de mots-clés (ET, séparés par virgules)</label>
    <label class="search-checkbox"><input id="search-title-only" type="checkbox"> Rechercher dans le titre uniquement</label>
    <div class="search-form-row"><label for="search-ad-languages">Langue de l’annonce</label><select id="search-ad-languages" multiple size="3"><option value="fr">Français</option><option value="de">Allemand</option><option value="en">Anglais</option></select></div>
    <div class="search-form-row"><label for="search-working-languages">Langues de travail explicitement requises</label><select id="search-working-languages" multiple size="3"><option value="fr">Français</option><option value="de">Allemand</option><option value="en">Anglais</option></select></div>
    <label class="search-checkbox"><input id="search-language-all" type="checkbox"> Toutes les langues de travail sélectionnées (ET)</label>
    <label class="search-checkbox"><input id="search-unknown-languages" type="checkbox" checked> Inclure les langues inconnues</label>
    <p class="search-muted">Sans sélection : aucune restriction. La langue de l’annonce n’est jamais une exigence de travail. Détection conventionnelle : vérifiez le texte original.</p>
<div class="search-form-row"><label for="search-fact-work_modes">Mode de travail</label><select id="search-fact-work_modes" multiple size="3"><option value="onsite">Sur site</option><option value="hybrid">Hybride</option><option value="remote">À distance</option><option value="unknown">Inconnu</option></select></div><div class="search-form-row"><label for="search-fact-contracts">Contrat</label><select id="search-fact-contracts" multiple size="3"><option value="permanent">CDI</option><option value="temporary">CDD / temporaire</option><option value="internship">Stage</option><option value="apprenticeship">Apprentissage</option><option value="contract">Mandat / freelance</option><option value="unknown">Inconnu</option></select></div><div class="search-form-row"><label for="search-fact-seniorities">Expérience indiquée</label><select id="search-fact-seniorities" multiple size="3"><option value="entry">Débutant</option><option value="junior">Junior</option><option value="mid">Intermédiaire</option><option value="senior">Senior</option><option value="unknown">Inconnue</option></select></div><div class="search-form-row"><label for="search-fact-categories">Famille de métiers (titre)</label><select id="search-fact-categories" multiple size="3"><option value="retail">Vente</option><option value="logistics">Logistique</option><option value="administration">Administration</option><option value="technology">Technologie</option><option value="finance">Finance</option><option value="unknown">Inconnue</option></select></div><div class="search-form-row"><label for="search-employer-kind">Employeur</label><select id="search-employer-kind"><option value="">Tous</option><option value="agency">Agence identifiée</option><option value="unknown">Non identifié</option></select></div><div class="search-form-row"><label for="search-salary">Salaire explicite minimum CHF</label><input id="search-salary" type="number" min="0" max="2000000"><select id="search-salary-period"><option value="">Période non filtrée</option><option value="year">Par an explicite</option><option value="month">Par mois explicite</option></select></div><label class="search-checkbox"><input id="search-description-only" type="checkbox"> Description disponible uniquement</label><label class="search-checkbox"><input id="search-facts-unknown" type="checkbox"> Inclure les métadonnées inconnues</label><p class="search-muted">Métadonnées explicites et indices de titre seulement. Les ambiguïtés restent inconnues ; vérifiez l’annonce.</p>    <div class="search-form-row"><label for="search-sort">Trier par</label><select id="search-sort"><option value="published">Publication récente</option><option value="retrieved">Dernière collecte</option><option value="company">Entreprise (A–Z)</option><option value="title">Intitulé (A–Z)</option><option value="location">Lieu</option><option value="workload">Taux décroissant</option><option value="relevance">Mots du titre correspondants</option></select></div>
    <div class="search-form-row"><label for="search-class">Éligibilité</label><select id="search-class"><option value="">Toutes</option><option value="eligible">Éligible</option><option value="unverified">À vérifier</option><option value="excluded">Exclue</option></select></div>
    <label class="search-checkbox"><input id="search-hide-expired" type="checkbox"> Masquer les annonces confirmées expirées (404 / 410)</label>
    <label class="search-checkbox"><input id="search-unknown-dates" type="checkbox"> Inclure les annonces sans date de publication connue</label>
    <hr><div class="search-section-title">Plateformes <span class="search-muted">13 sources</span></div>
    <div id="search-sources" class="search-source-grid"></div>
    <div class="search-actions-inline"><button type="button" id="search-all">Tout cocher</button><button type="button" id="search-active">Sources opérationnelles</button></div>
    <p class="search-notice"><strong>Important :</strong> les dates inconnues sont masquées par défaut; vous pouvez les inclure sans leur attribuer une fausse date. Les plateformes en mode Site officiel n'effectuent aucune collecte automatique. La disponibilité doit être vérifiée en direct. Les règles personnelles restent facultatives. Les filtres locaux ne suppriment aucune annonce.</p>
    <div class="search-actions-inline"><button class="primary" id="search-run" type="submit">Rechercher les offres</button><button id="search-cancel" type="button" disabled>Annuler</button><button id="search-filter-only" type="button">Afficher les offres enregistrées</button></div>
  </form><div class="search-results-area"><div class="search-panel" id="search-progress-panel"><div class="search-section-title">Progression et diagnostics</div><div id="search-progress-text" aria-live="polite">Aucune collecte lancée.</div><progress id="search-progress" max="1" value="0"></progress><div id="search-source-status" class="search-status-list"></div></div>
    <div class="search-panel"><div class="search-results-head"><div><strong>Offres retrouvées</strong><p id="search-result-info" class="search-muted">Sélectionnez vos filtres et lancez la recherche.</p></div><button id="search-all-discovered" type="button">Historique complet (sans filtres)</button><button id="search-filter-refresh" type="button">Actualiser la liste</button></div><div id="search-result-list" class="search-result-list"></div></div>
  </div></div>`;
  main.appendChild(section);
  const primaryActions=$s('#search-run').parentElement;primaryActions.classList.add('search-primary-actions');$s('#search-config').prepend(primaryActions);
  const side=document.querySelector('.sidebar-nav');
  const newButton=document.createElement('button');newButton.type='button';newButton.className='side-link';newButton.dataset.page='search';newButton.innerHTML='<span class="side-icon">⌕</span>Recherche';
  if(side){side.insertBefore(newButton,side.children[1]||null);newButton.addEventListener('click',()=>navigate());}
  else{const nav=document.querySelector('header.top .tabs')||document.querySelector('header.top');if(nav){newButton.className='tab';newButton.textContent='Recherche';nav.append(newButton);newButton.addEventListener('click',navigate);}}
  $s('#search-sources').innerHTML=PROVIDERS.map(([id,label,mode,url])=>`<div class="search-source"><input type="checkbox" data-source="${id}" ${mode==='collector'?'':'disabled'} ${mode==='collector'&&['jobup','jobs_ch','okjob','jobscout24'].includes(id)?'checked':''}><span>${esc(label)}</span><small>${mode==='collector'?'Collecteur disponible (à vérifier en direct)':'Site officiel uniquement'}</small><button type="button" class="search-official-site" data-official="${id}" aria-label="Ouvrir ${esc(label)} sur le site officiel">Site officiel ↗</button></div>`).join('');
  $s('#search-sources').addEventListener('click',async event=>{const b=event.target.closest('button[data-official]');if(!b)return;const provider=PROVIDERS.find(x=>x[0]===b.dataset.official);if(!provider)return;try{await call('open_official_job_site',{source:provider[0]});}catch(e){progress('Ouverture du site impossible : '+String(e)+' · '+provider[3]);}});
  $s('#search-config').addEventListener('submit',event=>{event.preventDefault();runSearch();});
  $s('#search-filter-only').addEventListener('click',refreshResults);$s('#search-filter-refresh').addEventListener('click',refreshResults);
  $s('#search-cancel').addEventListener('click',async()=>{stopping=true;progress('Arrêt demandé — la requête HTTP en cours peut encore se terminer.');try{await call('cancel_jobup_refresh');}catch{} try{await call('cancel_swiss_hunter_refresh');}catch{} });
  $s('#search-active').addEventListener('click',()=>setChecked(['jobup','jobs_ch','okjob','jobscout24']));
  $s('#search-all').addEventListener('click',()=>setChecked(PROVIDERS.map(x=>x[0])));
  const noneButton=document.createElement('button');
  noneButton.type='button';noneButton.id='search-none';noneButton.textContent='Tout d\u00e9cocher';
  $s('#search-all').insertAdjacentElement('afterend',noneButton);
  noneButton.addEventListener('click',()=>setChecked([]));
  $s('#search-save-profile').addEventListener('click',saveProfile);
  $s('#search-delete-profile').addEventListener('click',deleteProfile);
  $s('#search-profile').addEventListener('change',()=>{const profile=profiles.find(p=>p.name===$s('#search-profile').value);if(profile)applyConfig(profile);else applyConfig({name:'',daysBack:3,eligibilityPreset:'',sources:['jobup','jobs_ch','okjob','jobscout24']});});
  $s('#search-open-sources').addEventListener('click',()=>{if(window.jobupNavigate)window.jobupNavigate('sources');else document.querySelector('#btn-swiss-sources')?.click();});
  $s('#search-all-discovered').addEventListener('click',()=>refreshResults(true));
  $s('#search-duplicate-profile').addEventListener('click',()=>{const name=prompt('Nom du nouveau profil',($s('#search-profile-name').value||'Profil')+' — copie');if(name?.trim()){$s('#search-profile-name').value=name.trim();saveProfile();}});
  $s('#search-rename-profile').addEventListener('click',async()=>{const old=$s('#search-profile').value;if(!old){progress('Sélectionnez un profil enregistré.');return;}const name=prompt('Nouveau nom du profil',old)?.trim();if(!name||name===old)return;if(name.length>65||profiles.some(p=>p.name===name)){progress('Nom trop long ou déjà utilisé.');return;}const previous=profiles;profiles=profiles.map(p=>p.name===old?{...p,name}:p);try{await persistProfiles();renderProfiles();$s('#search-profile').value=name;$s('#search-profile-name').value=name;progress('Profil renommé.');}catch(e){profiles=previous;renderProfiles();progress('Renommage annulé : '+e);}});
  $s('#search-export-profile').addEventListener('click',()=>{const blob=new Blob([JSON.stringify({format:'masar-search-profile',version:1,profile:config()},null,2)],{type:'application/json'});const url=URL.createObjectURL(blob);const link=document.createElement('a');link.href=url;link.download='masar-search-profile.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);});
  $s('#search-import-profile').addEventListener('click',()=>$s('#search-profile-file').click());
  $s('#search-profile-file').addEventListener('change',async event=>{try{const file=event.target.files[0];if(!file)return;if(file.size>32000)throw new Error('Fichier trop volumineux');const data=JSON.parse(await file.text());if(data.format!=='masar-search-profile'||data.version!==1||!data.profile||typeof data.profile.name!=='string'||!Array.isArray(data.profile.sources))throw new Error('Profil Masar invalide');applyConfig(data.profile);progress('Profil importé pour inspection. Enregistrer pour le conserver.');}catch(e){progress('Import refusé : '+e);}finally{event.target.value='';}});
  $s('#search-preset').addEventListener('change',()=>$s('#search-rule-editor').hidden=$s('#search-preset').value!=='current');
  $s('#search-days').addEventListener('change',()=>{if($s('#search-days').value!=='custom'){$s('#search-date-from').value='';$s('#search-date-to').value='';}});
  for(const id of ['#search-date-from','#search-date-to'])$s(id).addEventListener('change',()=>{if($s('#search-date-from').value||$s('#search-date-to').value)$s('#search-days').value='custom';});
  loadProfiles();
}
function navigate(){
  if(window.jobupNavigate)window.jobupNavigate('search');
  else document.querySelectorAll('.view').forEach(view=>view.classList.toggle('active',view.id==='view-search'));
  document.querySelectorAll('[data-page]').forEach(el=>el.classList.toggle('active',el.dataset.page==='search'));
  const title=document.getElementById('desktop-page-title');if(title)title.textContent='Recherche';
}
function setChecked(ids){document.querySelectorAll('#search-sources [data-source]').forEach(box=>box.checked=!box.disabled && ids.includes(box.dataset.source));}
function selected(id){return Array.from($s(id).selectedOptions).map(o=>o.value);}
function factConfig(){return {workModes:selected('#search-fact-work_modes'),contracts:selected('#search-fact-contracts'),seniorities:selected('#search-fact-seniorities'),categories:selected('#search-fact-categories'),employerKind:$s('#search-employer-kind').value,minSalaryChf:$s('#search-salary').value!==''?Number($s('#search-salary').value):null,salaryPeriod:$s('#search-salary-period').value,descriptionOnly:$s('#search-description-only').checked,includeUnknown:$s('#search-facts-unknown').checked};}
function eligibilityRules(){if($s('#search-preset').value!=='current')return null;const rules={valaisMinimum:Number($s('#search-rule-vs').value),vaudMinimum:Number($s('#search-rule-vd').value)};for(const box of document.querySelectorAll('#search-rule-editor [data-rule]'))rules[box.dataset.rule]=box.checked;return rules;}
function config(){return {hideExpired:$s('#search-hide-expired').checked,eligibilityRules:eligibilityRules(),facts:factConfig(),cantons:selected('#search-cantons'),radiusKm:$s('#search-radius').value!==''?Number($s('#search-radius').value):null,radiusCenter:$s('#search-radius-center').value.trim(),includeUnknownLocations:$s('#search-unknown-locations').checked,eligibilityPreset:$s('#search-preset').value,adLanguages:selected('#search-ad-languages'),workingLanguages:selected('#search-working-languages'),languageAll:$s('#search-language-all').checked,includeUnknownLanguages:$s('#search-unknown-languages').checked,titleOnly:$s('#search-title-only').checked,keywordAll:$s('#search-keyword-all').checked,maxGrade:$s('#search-max-grade').value!==''?Number($s('#search-max-grade').value):null,dateFrom:$s('#search-date-from').value||null,dateTo:$s('#search-date-to').value||null,name:$s('#search-profile-name').value.trim(),daysBack:$s('#search-days').value==='custom'?null:Number($s('#search-days').value),sources:Array.from(document.querySelectorAll('#search-sources input:checked:not(:disabled)')).map(x=>x.dataset.source),keywords:$s('#search-keywords').value.trim(),excludeKeywords:$s('#search-exclude').value.trim(),sortBy:$s('#search-sort').value,location:$s('#search-location').value.trim(),minGrade:$s('#search-grade').value?Number($s('#search-grade').value):null,classification:$s('#search-class').value,includeUnknownDates:$s('#search-unknown-dates').checked};}
function applyConfig(p){$s('#search-hide-expired').checked=p.hideExpired===true;const facts=p.facts||{};for(const [id,key] of [['work_modes','workModes'],['contracts','contracts'],['seniorities','seniorities'],['categories','categories']])for(const o of $s('#search-fact-'+id).options)o.selected=Array.isArray(facts[key])&&facts[key].includes(o.value);$s('#search-employer-kind').value=facts.employerKind||'';$s('#search-salary').value=facts.minSalaryChf??'';$s('#search-salary-period').value=facts.salaryPeriod||'';$s('#search-description-only').checked=facts.descriptionOnly===true;$s('#search-facts-unknown').checked=facts.includeUnknown===true;for(const o of $s('#search-cantons').options)o.selected=Array.isArray(p.cantons)&&p.cantons.includes(o.value);$s('#search-radius').value=p.radiusKm??'';$s('#search-radius-center').value=p.radiusCenter||'';$s('#search-unknown-locations').checked=p.includeUnknownLocations===true;
 $s('#search-preset').value=p.eligibilityPreset||'';$s('#search-rule-editor').hidden=$s('#search-preset').value!=='current';const rules=p.eligibilityRules||{};for(const box of document.querySelectorAll('#search-rule-editor [data-rule]'))box.checked=rules[box.dataset.rule]??box.dataset.rule!=='targetFamiliesOnly';$s('#search-rule-vs').value=rules.valaisMinimum??40;$s('#search-rule-vd').value=rules.vaudMinimum??70;
 for(const [id,key] of [['#search-ad-languages','adLanguages'],['#search-working-languages','workingLanguages']])for(const option of $s(id).options)option.selected=Array.isArray(p[key])&&p[key].includes(option.value);
 for(const [id,key] of [['#search-language-all','languageAll'],['#search-title-only','titleOnly'],['#search-keyword-all','keywordAll']])$s(id).checked=p[key]===true;
 $s('#search-unknown-languages').checked=p.includeUnknownLanguages!==false;
 $s('#search-max-grade').value=p.maxGrade??'';$s('#search-date-from').value=p.dateFrom||'';$s('#search-date-to').value=p.dateTo||'';
 $s('#search-profile-name').value=p.name||'';$s('#search-days').value=p.dateFrom||p.dateTo?'custom':String(p.daysBack||3);$s('#search-keywords').value=p.keywords||'';$s('#search-exclude').value=p.excludeKeywords||'';$s('#search-sort').value=p.sortBy||'published';$s('#search-location').value=p.location||'';$s('#search-grade').value=p.minGrade!=null?String(p.minGrade):'';$s('#search-class').value=p.classification||'';$s('#search-unknown-dates').checked=p.includeUnknownDates===true;setChecked(p.sources||['jobup','jobs_ch','okjob']);}
async function loadProfiles(){try{const data=await call('db_load');profiles=Array.isArray(data.settings?.searchProfiles)?data.settings.searchProfiles:exampleProfiles();renderProfiles();}catch(e){progress('Profils disponibles après le chargement de SQLite.');}}
function exampleProfiles(){const base={daysBack:7,includeUnknownDates:true,includeUnknownLanguages:true,sources:['jobup','jobs_ch','jobscout24'],eligibilityPreset:''};return [
 {name:'All Switzerland — No Restrictions'},
 {name:'My Current Eligibility',eligibilityPreset:'current',location:'Sion, Lausanne'},
 {name:'Valais + Vaud',cantons:['VS','VD']},
 {name:'French-speaking Switzerland',cantons:['GE','VD','VS','FR','NE','JU'],adLanguages:['fr']},
 {name:'English-speaking Opportunities',workingLanguages:['en']},
 {name:'Retail & Sales',keywords:'vente, vendeur, retail, sales'},
 {name:'Logistics & Warehouse',keywords:'logistique, warehouse, lager'},
 {name:'Entry-Level Administration',keywords:'administration, assistant administratif, sachbearbeiter'}
 ].map(p=>({...base,...p}));}
function renderProfiles(){const select=$s('#search-profile');select.innerHTML='<option value="">Profil par défaut</option>'+profiles.map(p=>`<option value="${esc(p.name)}">${esc(p.name)}</option>`).join('');}
async function persistProfiles(){const original=await call('db_load');const newSettings={...(original.settings||{}),searchProfiles:profiles};await call('db_settings',{settings:newSettings,savedAt:new Date().toISOString()});if(typeof settings!=='undefined')settings=newSettings;}
async function saveProfile(){const p=config();if(!p.name){progress('Indiquez un nom de profil avant d’enregistrer.');return;}const existing=profiles.findIndex(a=>a.name===p.name);if(existing<0 && profiles.length>=30){progress('Maximum 30 profils.');return;}
 const nextProfiles=profiles.map(x=>({...x}));if(existing>=0)nextProfiles[existing]=p;else nextProfiles.push(p);
 const previous=profiles;profiles=nextProfiles;
 try{await persistProfiles();renderProfiles();$s('#search-profile').value=p.name;progress('Profil enregistré dans SQLite.');}catch(e){profiles=previous;renderProfiles();progress('Échec de l’enregistrement : '+e);}}
async function deleteProfile(){const name=$s('#search-profile').value;if(!name)return;const previous=profiles;profiles=profiles.filter(p=>p.name!==name);try{await persistProfiles();renderProfiles();applyConfig({name:'',daysBack:3,eligibilityPreset:'',sources:['jobup','jobs_ch','okjob','jobscout24']});progress('Profil supprimé.');}catch(e){profiles=previous;renderProfiles();progress('Échec de la suppression : '+e);}}
function progress(message){$s('#search-progress-text').textContent=message;}
function status(source,state,message){const container=$s('#search-source-status');let row=container.querySelector(`[data-result-source="${source}"]`);if(!row){row=document.createElement('div');row.className='search-source-result';row.dataset.resultSource=source;container.appendChild(row);}row.textContent=`${PROVIDERS.find(p=>p[0]===source)?.[1]||source}: ${state} — ${message}`;row.dataset.status=state;}
function resultCount(result){return result.fetched??result.inspected??0;}
async function runSearch(){
 if(running)return;
 const cfg=config();if(!cfg.sources.length){progress('Sélectionnez au moins une plateforme.');return;}
 running=true;stopping=false;$s('#search-run').disabled=true;$s('#search-cancel').disabled=false;$s('#search-source-status').replaceChildren();
 const bar=$s('#search-progress');bar.value=0;bar.max=cfg.sources.length;
 let done=0;
 let unlisten=null;
 try{
  if(window.__TAURI__?.event?.listen)unlisten=await window.__TAURI__.event.listen('jobup-refresh-progress',evt=>{
    const data=evt.payload||{};const detail=typeof data.completed==='number'?` · ${data.completed}/${data.total??'?'}`:'';
    progress(`Jobup : ${data.message||data.phase||'Collecte'}${detail}`);
  });
  for(const source of cfg.sources){
   if(stopping){status(source,'Annulé','Non lancé');break;}
   const provider=PROVIDERS.find(p=>p[0]===source);
   if(provider?.[2]==='website'){status(source,'Site officiel','Collecte indisponible. Utilisez le bouton Site officiel pour consulter cette plateforme.');done++;bar.value=done;continue;}
   progress(`Collecte ${provider?.[1]||source} (${done+1}/${cfg.sources.length})`);
   status(source,'En cours','Connexion et traitement');const start=performance.now();
   try{
    let report;
    if(['jobup','jobs_ch'].includes(source))report=await call('refresh_swiss_hunter_source',{source,keywords:cfg.keywords,location:cfg.location,daysBack:cfg.daysBack??30});
    else if(source==='jobup')report=await call('commit_jobup_complete',{daysBack:cfg.daysBack,detailBudget:15});
    else if(source==='jobs_ch')report=await call('refresh_jobs_ch');
    else if(source==='okjob')report=await call('refresh_okjob',{keywords:cfg.keywords,location:cfg.location});
    else if(['jobscout24','zuri_jobs','michael_page','linkedin_jobs','swissdevjobs','efinancialcareers'].includes(source))report=await call('refresh_swiss_hunter_source',{source,keywords:cfg.keywords,location:cfg.location,daysBack:cfg.daysBack??30});
    else report=await call('refresh_jobchannel',{source});
    const warnings=report.warnings||[];
    status(source,warnings.length?'Partiel':'Succès',`${resultCount(report)} annonces · ${report.inserted??report.newCount??0} nouvelles · ${report.previouslyKnown??report.unchanged??0} connues · ${report.descriptionsRetrieved??report.detailsRequested??0} descriptions · ${Math.round(performance.now()-start)} ms${warnings.length?' · '+warnings.slice(0,2).join('; '):''}`);
    const label=provider[1];if(typeof setCollectorStatus==='function')setCollectorStatus(label,warnings.length?'Partial':'Success',{fetched:resultCount(report),newCount:report.inserted??report.newCount??0,revised:report.revised??0,eligible:report.eligible,excluded:report.excluded,unverified:report.unverified,descriptions:report.descriptionsRetrieved??report.detailsRequested,durationMs:Math.round(performance.now()-start),message:warnings.join('; ')||'Search page completed'});
   }catch(e){const msg=String(e);const blocked=/403|429|Forbidden|Too Many Requests|Cloudflare|access challenge|blocked/i.test(msg);const unavailable=/unknown command|command.*not found|invalid command|does not expose a verified public vacancy-search API/i.test(msg);const cancelled=/cancelled|canceled/i.test(msg)||stopping;status(source,cancelled?'Annulé':blocked?'Bloqué':unavailable?'Indisponible':'Échec',msg.slice(0,180));if(typeof setCollectorStatus==='function')setCollectorStatus(provider[1],blocked?'Blocked':unavailable?'Unavailable':cancelled?'Cancelled':'Failed',{message:msg.slice(0,220)});}
   done++;bar.value=done;
  }
  progress(stopping?'Collecte interrompue. Les sources déjà enregistrées sont conservées.':`Recherche terminée : ${done}/${cfg.sources.length} sources traitées. Contrôlez les diagnostics ci-dessus.`);
  await refreshResults();
 }catch(e){progress('Erreur : '+String(e));}
 finally{if(unlisten)try{await unlisten();}catch{}running=false;$s('#search-run').disabled=false;$s('#search-cancel').disabled=true;}
}
async function refreshResults(allDiscovered=false){
 const currentQuery=++resultQuerySerial;
 const cfg=allDiscovered===true?{sources:[],keywords:'',excludeKeywords:'',sortBy:'published',location:'',radiusCenter:'',classification:'',includeUnknownDates:true,includeUnknownLanguages:true}:config();const container=$s('#search-result-list');container.textContent='Lecture des annonces conservées dans SQLite…';
 window.masarPublicationContext={id:Date.now(),allHistory:allDiscovered===true,daysBack:cfg.daysBack??null,dateFrom:cfg.dateFrom||null,dateTo:cfg.dateTo||null,includeUnknownDates:cfg.includeUnknownDates===true};
 const publicationScope=allDiscovered===true?'Historique complet — aucun filtre de recherche actif':cfg.dateFrom||cfg.dateTo?`Publication du ${cfg.dateFrom||'début'} au ${cfg.dateTo||'aujourd’hui'}`:`Publication : ${cfg.daysBack===1?'24 h (dates sans heure : aujourd’hui)':cfg.daysBack+' jours'}${cfg.includeUnknownDates?' + dates inconnues':''}`;

 try{
  const data=await call('inspect_search_results',{filters:{sources:cfg.sources,keywords:cfg.keywords,excludeKeywords:cfg.excludeKeywords,sortBy:cfg.sortBy,location:cfg.location,minGrade:cfg.minGrade,daysBack:cfg.daysBack,includeUnknownDates:cfg.includeUnknownDates,hideExpired:cfg.hideExpired,facts:cfg.facts,cantons:cfg.cantons,radiusKm:cfg.radiusKm,radiusCenter:cfg.radiusCenter,includeUnknownLocations:cfg.includeUnknownLocations,eligibilityRules:cfg.eligibilityRules,classification:cfg.classification,adLanguages:cfg.adLanguages,workingLanguages:cfg.workingLanguages,languageAll:cfg.languageAll,includeUnknownLanguages:cfg.includeUnknownLanguages,titleOnly:cfg.titleOnly,keywordAll:cfg.keywordAll,maxGrade:cfg.maxGrade,dateFrom:cfg.dateFrom,dateTo:cfg.dateTo,limit:250}});
  if(currentQuery!==resultQuerySerial)return;
  lastResults=data.items||[];container.replaceChildren();
  $s('#search-result-info').textContent=`${publicationScope} · ${data.matched} annonces correspondent aux filtres · ${data.unknownPublicationDates} dates de publication inconnues examinées · ${data.unknownWorkloads} taux inconnus dans les résultats · ${data.duplicateGroups} groupes avec doublons possibles${data.capped?' · résultat limité':''}. Filtrage local, sans modification des classifications.`;
  if(!lastResults.length){container.textContent='Aucune annonce enregistrée ne correspond à ces critères.';return;}
  for(const item of lastResults){const card=document.createElement('article');card.className='search-hit';
   const head=document.createElement('div');head.className='search-hit-title';const name=document.createElement('strong');name.textContent=item.title;const state=document.createElement('span');state.className='search-state';state.dataset.state=item.classification;state.textContent=item.classification;head.append(name,state);
   const meta=document.createElement('p');meta.className='search-muted';meta.textContent=`${item.company||'Entreprise inconnue'} · ${item.place||'Lieu inconnu'} · ${item.source} · ${item.grades.length?item.grades.join('–')+' %':'Taux inconnu'}`;
   const dates=document.createElement('p');dates.className='search-muted';dates.textContent=`Publication : ${item.publishedAt?.slice(0,10)||'inconnue'} · Collectée : ${item.retrievedAt?.slice(0,10)||'inconnue'} · Description : ${item.descriptionAvailable?'disponible':'non disponible'}`;
   const languages=document.createElement('p');languages.className='search-muted';languages.textContent=`Langue de l’annonce : ${item.adLanguage||'unknown'} · Langues de travail : ${(item.workingLanguages||[]).map(([code,level])=>code+' ('+level+')').join(', ')||'inconnues'}`;card.append(head,meta,dates,languages);
   if(item.possibleDuplicates?.length){const duplicate=document.createElement('p');duplicate.className='search-duplicate';duplicate.textContent='⚠ Doublon possible : '+item.possibleDuplicates.slice(0,3).join(', ')+' (aucune fusion automatique)';card.append(duplicate);}
   const open=document.createElement('button');open.type='button';open.textContent='Voir l’annonce originale ↗';open.addEventListener('click',()=>openListing(item.listingUrl));card.append(open);
   const freshness=document.createElement('p');freshness.className='search-muted';freshness.textContent=`Première observation : ${item.observation?.firstSeenAt||'inconnue'} · Dernière observation : ${item.observation?.lastSeenAt||item.retrievedAt||'inconnue'} · Dernier lien joignable : ${item.observation?.lastVerifiedAt||'non vérifié'} · État : ${item.observation?.availability||'unknown'}`;card.append(freshness);
   const verify=document.createElement('button');verify.type='button';verify.textContent='Vérifier le lien';verify.addEventListener('click',async()=>{verify.disabled=true;try{const result=await call('verify_vacancy_link',{source:item.source,sourceId:item.sourceId});freshness.textContent='Vérification du lien : '+result+' (joignable ne garantit pas que le poste reste ouvert)';}catch(e){freshness.textContent='Vérification impossible : '+e;}finally{verify.disabled=false;}});card.append(verify);
   container.append(card);
  }
 }catch(e){if(currentQuery===resultQuerySerial)container.textContent='Impossible de charger les annonces : '+String(e);}
}
async function openListing(url){if(!/^https:\/\//i.test(String(url||''))){progress('Lien original invalide.');return;}try{await call('open_external_url',{url});}catch(e){progress('Ouverture externe impossible : '+String(e));}}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
