/* Jobup Step 7/7 — Settings and release preparation. Zero new dependencies.
 * Existing Rust/SQLite APIs are authoritative. No fake toggles for unimplemented features.
 */
(()=>{'use strict';
const el=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const invoke=(name,args={})=>{if(typeof window.__TAURI__?.core?.invoke!=='function')throw Error('Masar doit être ouvert via Tauri');return window.__TAURI__.core.invoke(name,args);};
const KEYS={density:'jobup_step7_density',font:'jobup_step7_font',motion:'jobup_step7_motion',days:'jobup_step7_default_search_days'};
const get=(key,fallback)=>{try{return localStorage.getItem(key)??fallback;}catch{return fallback;}};
const store=(key,value)=>{try{localStorage.setItem(key,String(value));return true;}catch{return false;}};
const fixedSourceCount=13;
const state={loading:false,started:false,profiles:[],lastBackup:'',settings:{delayDays:5,delayMode:'business'}};
function note(message,error=false){const target=el('s7-message');if(target){target.textContent=String(message);target.classList.toggle('error',!!error);}}
function choose(page){if(typeof window.jobupNavigate==='function')window.jobupNavigate(page);}
const shell=`<section id="view-settings-step7" class="view s7-settings" aria-label="Paramètres Masar">
 <div class="s7-heading"><div><h2>Paramètres</h2><p>Préférences réelles, sauvegarde de SQLite et état de votre installation locale.</p></div><button type="button" id="s7-reload">Actualiser</button></div>
 <div class="s7-layout"><nav class="s7-jump" aria-label="Sections des paramètres">
 <a href="#s7-appearance">Apparence</a><a href="#s7-search">Recherche</a><a href="#s7-followups">Relances</a><a href="#s7-eligibility">Éligibilité</a><a href="#s7-data">Données</a><a href="#s7-sources">Sources et état</a>
 </nav><div class="s7-sections">
 <section class="s7-card" id="s7-appearance"><div class="s7-card-head"><div><h3>Apparence et confort</h3><p>Ces réglages concernent uniquement l'interface sur cet appareil.</p></div><span class="s7-tag">Local</span></div>
 <div class="s7-control"><label for="s7-theme">Thème</label><select id="s7-theme"><option value="dark">Sombre</option><option value="light">Clair</option></select></div>
 <div class="s7-control"><label for="s7-density">Densité</label><select id="s7-density"><option value="comfortable">Confortable</option><option value="compact">Compacte</option></select></div>
 <div class="s7-control"><label for="s7-font">Taille du texte</label><select id="s7-font"><option value="13">Normale</option><option value="14">Agrandie</option><option value="15">Très lisible</option></select></div>
 <div class="s7-control"><label for="s7-motion">Animations de l'application</label><select id="s7-motion"><option value="system">Suivre Windows</option><option value="reduce">Réduire</option></select></div>
 </section>
 <section class="s7-card" id="s7-search"><div class="s7-card-head"><div><h3>Recherche et profils enregistrés</h3><p>Le moteur de recherche utilise les règles Rust existantes. Les profils sont conservés dans SQLite.</p></div><span class="s7-tag">SQLite + local</span></div>
 <div class="s7-control"><label for="s7-days">Période proposée à l'ouverture</label><select id="s7-days"><option value="1">1 jour</option><option value="3">3 jours</option><option value="7">7 jours</option></select></div>
 <p class="s7-tip">La période par défaut s'applique à la page Recherche sur ce PC. Seul le collecteur Jobup reçoit actuellement ce paramètre côté réseau ; les autres sites ne le prennent pas forcément en charge.</p>
 <div class="s7-control"><span>Profils sauvegardés dans SQLite</span><span id="s7-profile-count" class="s7-value">Chargement…</span></div><div class="s7-profile-list" id="s7-profiles" role="list"></div>
 <button type="button" id="s7-open-search">Modifier les profils et filtres dans Recherche</button>
 </section>
 <section class="s7-card" id="s7-followups"><div class="s7-card-head"><div><h3>Règle de relance</h3><p>Utilise la fonction native existante de l'étape Relances. Les dates manuelles déjà enregistrées sont conservées.</p></div><span class="s7-tag">SQLite</span></div>
 <div class="s7-control"><label for="s7-delay">Délai après envoi</label><div class="s7-inline"><input id="s7-delay" type="number" min="1" max="30" step="1" value="5"><select id="s7-delay-mode"><option value="business">Jours ouvrables (lun–ven)</option><option value="calendar">Jours calendaires</option></select></div></div>
 <p class="s7-tip">Le calcul n'exclut pas encore les jours fériés des cantons. Chaque candidature peut conserver sa date de relance spécifique.</p><button class="primary" type="button" id="s7-save-followups">Enregistrer la règle dans SQLite</button>
 </section>
 <section class="s7-card" id="s7-eligibility"><div class="s7-card-head"><div><h3>Éligibilité</h3><p>Le préréglage personnel est facultatif. Ses règles et seuils sont modifiables dans Recherche puis enregistrés dans chaque profil.</p></div><span class="s7-tag muted">Facultatif</span></div>
 <div class="s7-eligibility"><div><b>Valais</b><span>Seuil de référence : 40 %</span></div><div><b>Vaud</b><span>Seuil de référence : 70 %</span></div><div><b>Qualifications</b><span>CFC exigé ou souhaité, diplômes et permis obligatoires distingués</span></div><div><b>Autres règles</b><span>Familles de métiers, secteurs exclus, localisation et démarchage téléphonique</span></div></div>
 <p class="s7-tip">Dans Recherche, activez Mon éligibilité actuelle pour modifier les règles. Sans ce préréglage, aucune règle personnelle ne masque les offres.</p>
 <button type="button" id="s7-open-review">Examiner les décisions et leurs motifs</button>
 </section>
 <section class="s7-card" id="s7-data"><div class="s7-card-head"><div><h3>Sauvegarde et données</h3><p>La base SQLite contient vos candidatures, notes, statuts et historiques. Faites une sauvegarde avant une mise à jour.</p></div><span class="s7-tag">Local</span></div>
 <div class="s7-data-actions"><button type="button" class="primary" id="s7-backup">Créer une sauvegarde SQLite</button><button type="button" id="s7-tools">Outils d'importation / exportation</button></div>
 <div id="s7-backup-result" class="s7-backup-result" role="status">Aucune sauvegarde demandée dans cette session.</div>
 <p class="s7-tip">Une sauvegarde SQLite complète est distincte de l'exportation legacy de <code>reviewed.json</code>. Aucune restauration destructive ni suppression automatique n'est lancée ici.</p>
 </section>
 <section class="s7-card" id="s7-sources"><div class="s7-card-head"><div><h3>Sources et état de livraison</h3><p>Treize plateformes suivies ; certaines ne peuvent pas être collectées automatiquement.</p></div><span class="s7-tag">${fixedSourceCount} sources</span></div>
 <div class="s7-source-grid"><div><b>Jobup, Jobs.ch, JobScout24, OK Job</b><span>Collecteurs existants</span></div><div><b>LinkedIn Jobs, Züri Jobs, Michael Page</b><span>Collecteurs publics experimentaux</span></div><div><b>Indeed, Emploi en Suisse, 100000jobs, Job-Room</b><span>Site officiel uniquement</span></div><div><b>SwissDevJobs, eFinancialCareers</b><span>Firefox facultatif — setup dans Données</span></div></div>
 <p class="s7-tip">Les collecteurs sont a verifier en direct. Les plateformes sans collecte disposent d'un lien vers leur site officiel dans Recherche et Sources. Aucun contournement des blocages de sites.</p><button type="button" id="s7-open-sources">Voir les diagnostics réels</button>
 </section></div></div>
 <div class="s7-message" id="s7-message" role="status" aria-live="polite"></div>
 </section>`;
function applyVisual(){
  const theme=el('settings-theme');const choice=get('jobup-ui-theme','dark')==='light'?'light':'dark';
  if(el('s7-theme'))el('s7-theme').value=theme?.value==='light'?'light':choice;
  const density=get(KEYS.density,'comfortable')==='compact'?'compact':'comfortable';document.documentElement.dataset.jobupDensity=density;if(el('s7-density'))el('s7-density').value=density;
  const font=['13','14','15'].includes(get(KEYS.font,'13'))?get(KEYS.font,'13'):'13';document.documentElement.style.setProperty('--s7-font-size',font+'px');if(el('s7-font'))el('s7-font').value=font;
  const motion=get(KEYS.motion,'system')==='reduce'?'reduce':'system';document.documentElement.dataset.jobupMotion=motion;if(el('s7-motion'))el('s7-motion').value=motion;
  const days=['1','3','7'].includes(get(KEYS.days,'3'))?get(KEYS.days,'3'):'3';if(el('s7-days'))el('s7-days').value=days;const searchDays=el('search-days');if(searchDays)searchDays.value=days;
}
async function refresh(){if(state.loading)return;state.loading=true;const b=el('s7-reload');if(b)b.disabled=true;
  try{const [profiles,followups]=await Promise.allSettled([invoke('db_load'),invoke('followups_step4_list')]);
    if(profiles.status==='fulfilled'){
      const rows=profiles.value?.settings?.searchProfiles;state.profiles=Array.isArray(rows)?rows.filter(x=>x&&typeof x.name==='string').slice(0,30):[];
      el('s7-profile-count').textContent=state.profiles.length+' profil(s)';
      const host=el('s7-profiles');host.replaceChildren();
      if(!state.profiles.length){const row=document.createElement('p');row.className='s7-tip';row.textContent='Aucun profil nommé. Utilisez Recherche pour en créer un.';host.append(row);}else for(const row of state.profiles){const button=document.createElement('button');button.type='button';button.className='s7-profile';button.textContent=row.name;button.title='Ouvrir Recherche pour modifier ce profil';button.addEventListener('click',()=>{choose('search');const selection=el('search-profile');if(selection){selection.value=row.name;selection.dispatchEvent(new Event('change'));}});host.append(button);}
    }else{el('s7-profile-count').textContent='Indisponible';el('s7-profiles').textContent='Impossible de lire les profils SQLite.';}
    if(followups.status==='fulfilled'){
      const settings=followups.value?.settings||{};state.settings={delayDays:Number(settings.delayDays)||5,delayMode:settings.delayMode==='calendar'?'calendar':'business'};
      el('s7-delay').value=String(state.settings.delayDays);el('s7-delay-mode').value=state.settings.delayMode;
    }
    if(profiles.status==='rejected'||followups.status==='rejected')note('Lecture partielle : vérifiez que les commandes Rust des étapes 1 et 4 sont installées.',true);
    else note('Préférences chargées depuis Masar et SQLite.');
  }catch(err){note('Chargement impossible : '+String(err),true);}finally{state.loading=false;if(b)b.disabled=false;}
}
async function saveFollowups(){const delay=Number(el('s7-delay').value),mode=el('s7-delay-mode').value;
 if(!Number.isInteger(delay)||delay<1||delay>30||!['business','calendar'].includes(mode)){note('Choisissez un délai entier entre 1 et 30 jours.',true);return;}
 const button=el('s7-save-followups');button.disabled=true;
 try{await invoke('followups_step4_settings',{settings:{delayDays:delay,delayMode:mode}});note('Règle de relance enregistrée dans SQLite. Les échéances déjà personnalisées ne changent pas.');}
 catch(err){note('Enregistrement refusé : '+String(err),true);}
 finally{button.disabled=false;}
}
async function backup(){const button=el('s7-backup');button.disabled=true;el('s7-backup-result').textContent='Création de la sauvegarde SQLite…';
 try{const path=await invoke('db_backup_auto');if(typeof path!=='string'||!path)throw Error('Chemin de sauvegarde manquant');state.lastBackup=path;el('s7-backup-result').textContent='Sauvegarde créée : '+path;note('Sauvegarde SQLite enregistrée.');}
 catch(err){el('s7-backup-result').textContent='Échec de la sauvegarde : '+String(err);note('La sauvegarde n’a pas été confirmée.',true);}
 finally{button.disabled=false;}
}
function openTools(){const tools=el('desktop-tools');if(!tools){note('Les outils existants ne sont pas disponibles dans cette version.',true);return;}tools.open=true;tools.querySelector('summary')?.focus();note('Outils ouverts dans la barre supérieure.');}
function bind(){
 el('s7-reload').addEventListener('click',refresh);
 el('s7-theme').addEventListener('change',()=>{const old=el('settings-theme');if(old){old.value=el('s7-theme').value;old.dispatchEvent(new Event('change'));}else{document.documentElement.dataset.theme=el('s7-theme').value;store('jobup-ui-theme',el('s7-theme').value);}note('Thème appliqué.');});
 el('s7-density').addEventListener('change',()=>{const value=el('s7-density').value;store(KEYS.density,value);document.documentElement.dataset.jobupDensity=value;note('Densité appliquée.');});
 el('s7-font').addEventListener('change',()=>{const value=el('s7-font').value;store(KEYS.font,value);document.documentElement.style.setProperty('--s7-font-size',value+'px');note('Taille du texte mise à jour.');});
 el('s7-motion').addEventListener('change',()=>{const value=el('s7-motion').value;store(KEYS.motion,value);document.documentElement.dataset.jobupMotion=value;note('Préférence de mouvement appliquée.');});
 el('s7-days').addEventListener('change',()=>{const value=el('s7-days').value;store(KEYS.days,value);const days=el('search-days');if(days)days.value=value;note('Période proposée enregistrée sur cet appareil.');});
 el('s7-open-search').addEventListener('click',()=>choose('search'));
 el('s7-open-review').addEventListener('click',()=>choose('review'));
 el('s7-open-sources').addEventListener('click',()=>choose('sources'));
 el('s7-save-followups').addEventListener('click',saveFollowups);
 el('s7-backup').addEventListener('click',backup);
 el('s7-tools').addEventListener('click',openTools);
}
function navigate(){document.querySelectorAll('.view').forEach(v=>v.classList.toggle('active',v.id==='view-settings-step7'));
 document.querySelectorAll('.side-link').forEach(b=>b.classList.toggle('active',b.dataset.page==='settings'));
 const title=el('desktop-page-title');if(title)title.textContent='Paramètres';refresh();}
function mount(){if(state.started)return;const main=document.querySelector('main');if(!main)return;
 main.insertAdjacentHTML('beforeend',shell);state.started=true;
 const old=el('view-settings');if(old)old.classList.remove('active');
 const previousNavigate=window.jobupNavigate;
 window.jobupNavigate=(page)=>page==='settings'?navigate():typeof previousNavigate==='function'?previousNavigate(page):undefined;
 applyVisual();bind();
 if(document.querySelector('.side-link[data-page="settings"]')?.classList.contains('active'))navigate();
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
