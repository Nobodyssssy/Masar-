/* Jobup Review: SQLite-backed triage. No automatic applications or AI. */
(()=>{'use strict';
const rootId='view-triage';
let items=[],offset=0,total=0,selectedKey='',loading=false,started=false,requestSerial=0,mode='split';
const invoke=(name,args={})=>window.__TAURI__.core.invoke(name,args);
const $=id=>document.querySelector('#'+rootId+' '+id);
const htmlEscape=x=>String(x??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const key=x=>x.source+'\x1F'+x.sourceId;
const stateName={pending:'À examiner',later:'Plus tard',skipped:'Écartée',approved:'Vers candidatures'};
const eligibility={eligible:'Validée · règles historiques',unverified:'À vérifier',excluded:'Écartée · règles historiques'};
const isVisible=()=>document.getElementById(rootId)?.classList.contains('active');
const errors=error=>{const label=String(error).replace(/^Error:\s*/,'');$('#triage-message').textContent=label;$('#triage-message').setAttribute('data-error','true');};
function formatDate(x){if(!x)return 'Inconnue';return String(x).slice(0,10);}
function announce(x){$('#triage-message').textContent=x;$('#triage-message').removeAttribute('data-error');}
function mount(){
 if(document.getElementById(rootId))return;
 const section=document.createElement('section');section.id=rootId;section.className='view';
 section.innerHTML=`<div class="triage-heading"><div><h2>Review · Trier les offres</h2><p>Décidez rapidement. Les candidatures restent séparées de l’éligibilité et de la découverte.</p></div><div class="triage-actions"><button id="triage-layout" type="button">Vue tableau</button><button id="triage-reload" type="button">Actualiser</button></div></div>
 <div class="triage-explain">Toutes les offres découvertes sont visibles par défaut. La file d’éligibilité personnelle est facultative et masque les métiers hors cible. Les offres « À vérifier » ne sont <strong>pas</strong> considérées éligibles. Aucun enregistrement n’est supprimé.</div><form id="triage-filters" class="triage-filters"><label>Recherche<input id="triage-query" placeholder="Titre, société, description" maxlength="220"></label><label>Source<select id="triage-source"><option value="">Toutes les sources</option>${[['jobup','Jobup'],['jobs_ch','Jobs.ch'],['okjob','OK Job'],['indeed_ch','Indeed Suisse'],['emploi_en_suisse','Emploi en Suisse'],['jobscout24','JobScout24'],['100000jobs','100000jobs'],['job_room','Job-Room'],['linkedin_jobs','LinkedIn Jobs'],['michael_page','Michael Page'],['zuri_jobs','Züri Jobs'],['swissdevjobs','SwissDevJobs'],['efinancialcareers','eFinancialCareers']].map(([v,t])=>`<option value="${v}">${t}</option>`).join('')}</select></label><label>File de traitement<select id="triage-scope"><option value="priority">Mon éligibilité actuelle (facultatif)</option><option value="eligible">Éligibles uniquement</option><option value="targeted_unverified">À vérifier · métiers ciblés</option><option value="unverified">Toutes les offres à vérifier</option><option value="excluded">Exclues (audit)</option><option value="all" selected>Toutes les offres découvertes</option></select></label><label>Éligibilité (avancé)<select id="triage-class"><option value="">Toutes</option><option value="eligible">Éligibles</option><option value="unverified">À vérifier</option><option value="excluded">Exclues</option></select></label><label>Décision<select id="triage-decision"><option value="pending">À examiner</option><option value="later">Plus tard</option><option value="skipped">Écartées</option><option value="approved">Vers candidatures</option><option value="">Toutes</option></select></label><label>Ville<input id="triage-location" placeholder="Sion, Lausanne" maxlength="140"></label><label>Publication<select id="triage-days"><option value="">Toutes les dates</option><option value="1">24 h / date du jour</option><option value="3">3 jours</option><option value="7">7 jours</option><option value="14">14 jours</option><option value="30">30 jours</option></select></label><label><input id="triage-unknown-dates" type="checkbox"> Inclure les dates inconnues</label><label>Du<input id="triage-from" type="date"></label><label>Au<input id="triage-to" type="date"></label><label>Trier<select id="triage-sort"><option value="retrieved">Dernière collecte</option><option value="published">Publication</option><option value="title">Titre A–Z</option><option value="company">Entreprise A–Z</option></select></label><button type="submit">Filtrer</button></form>
 <p id="triage-filter-summary" role="status"></p><p class="triage-explain">Inventaire global, toutes dates : les classifications historiques ne sont pas des recommandations. Une offre sans description reste souvent à vérifier ; elle n’est pas automatiquement sans intérêt.</p><div class="triage-overview" id="triage-overview" role="status" aria-live="polite">Chargement du bilan des offres…</div><div class="triage-summary"><span id="triage-count">Chargement…</span><div><button id="triage-bulk-later" type="button">Plus tard (sélection)</button><button id="triage-bulk-skip" type="button">Écarter (sélection)</button></div></div><div id="triage-message" role="status" aria-live="polite">Aucune modification automatique des candidatures.</div><button id="triage-show-all" type="button" hidden>Afficher les annonces masquées</button>
 <div id="triage-workspace" class="triage-workspace"><div class="triage-list"><div class="triage-list-head"><input type="checkbox" id="triage-select-all" aria-label="Sélectionner les annonces visibles"><span>Offres découvertes</span><small>J / K : naviguer · A : approuver · S : écarter · L : plus tard · Z : annuler</small></div><div id="triage-items" role="listbox" aria-label="Offres à examiner"></div><button id="triage-more" type="button">Charger 50 offres supplémentaires</button></div><aside id="triage-detail" class="triage-detail" aria-live="polite"><div class="triage-placeholder">Sélectionnez une annonce pour voir les exigences, la description et les décisions possibles.</div></aside></div>`;
 document.querySelector('main').append(section);
 const side=document.querySelector('.sidebar-nav');
 const nav=document.createElement('button');nav.type='button';nav.id='triage-nav';nav.className=side?'side-link':'tab';nav.dataset.page='triage';
 nav.innerHTML='<span class="side-icon">☷</span> Review';
 if(side){const search=side.querySelector('[data-page="search"]');const discover=side.querySelector('[data-page="discover"]');const anchor=discover||side.querySelector('[data-page="tracker"]');if(search)search.after(nav);else if(anchor)anchor.before(nav);else side.append(nav);}
 else{document.querySelector('header.top .tabs')?.append(nav);}
 nav.addEventListener('click',navigate);
 const previous=window.jobupNavigate;
 if(typeof previous==='function')window.jobupNavigate=page=>page==='review'||page==='triage'?navigate():previous(page);
 $('#triage-filters').addEventListener('submit',e=>{e.preventDefault();reload();});
 $('#triage-reload').addEventListener('click',reload);
 $('#triage-days').addEventListener('change',()=>{if($('#triage-days').value){$('#triage-from').value='';$('#triage-to').value='';}reload();});
 for(const id of ['#triage-from','#triage-to'])$(id).addEventListener('change',()=>{$('#triage-days').value='';});
 $('#triage-unknown-dates').addEventListener('change',reload);
 $('#triage-scope').addEventListener('change',()=>{$('#triage-class').value='';reload();});
 $('#triage-class').addEventListener('change',()=>{if($('#triage-class').value){$('#triage-scope').value='all';}reload();});
 $('#triage-show-all').addEventListener('click',()=>{$('#triage-scope').value='all';$('#triage-class').value='';reload();});
 $('#triage-layout').addEventListener('click',()=>{mode=mode==='split'?'table':'split';$('#triage-workspace').classList.toggle('table-mode',mode==='table');$('#triage-layout').textContent=mode==='split'?'Vue tableau':'Vue détaillée';});
 $('#triage-more').addEventListener('click',()=>load(false));
 $('#triage-select-all').addEventListener('change',e=>{document.querySelectorAll('#triage-items .triage-mark').forEach(box=>{box.checked=e.target.checked;});});
 $('#triage-bulk-later').addEventListener('click',()=>batch('later'));
 $('#triage-bulk-skip').addEventListener('click',()=>batch('skipped'));
 document.addEventListener('keydown',onKey);
 started=true;
}
function navigate(){
 if(!started)mount();
 document.querySelectorAll('.view').forEach(v=>v.classList.toggle('active',v.id===rootId));
 document.querySelectorAll('.side-link,header .tab').forEach(x=>x.classList.toggle('active',x.id==='triage-nav'));
 const title=document.getElementById('desktop-page-title');if(title)title.textContent='Review';
 const context=window.masarPublicationContext;
 if(context&&context.id!==appliedSearchContext){appliedSearchContext=context.id;$('#triage-scope').value='all';$('#triage-class').value='';$('#triage-days').value=context.daysBack?String(context.daysBack):'';$('#triage-from').value=context.dateFrom||'';$('#triage-to').value=context.dateTo||'';$('#triage-unknown-dates').checked=context.includeUnknownDates===true;reload();}
 else if(!items.length)reload();
}
let appliedSearchContext=0;
function filters(){const val=id=>$(id).value.trim();return {daysBack:val('#triage-days')?Number(val('#triage-days')):null,includeUnknownDates:$('#triage-unknown-dates').checked,source:val('#triage-source')||null,classification:val('#triage-class')||null,scope:val('#triage-scope'),decision:val('#triage-decision')||null,query:val('#triage-query')||null,location:val('#triage-location')||null,dateFrom:val('#triage-from')||null,dateTo:val('#triage-to')||null,sort:val('#triage-sort'),limit:50,offset};}
async function reload(){const period=$('#triage-days').value;const from=$('#triage-from').value,to=$('#triage-to').value;$('#triage-filter-summary').textContent=period?`File filtrée par publication : ${period==='1'?'24 h (dates sans heure : aujourd’hui)':period+' jours'}${$('#triage-unknown-dates').checked?' + dates inconnues':''}. La date de collecte n’est jamais utilisée.`:from||to?`File filtrée par publication : ${from||'début'} → ${to||'aujourd’hui'}.`:'Historique complet — aucune restriction de publication. Le dernier choix dans Recherche est repris lorsque vous ouvrez cette page depuis une nouvelle recherche.';offset=0;items=[];selectedKey='';$('#triage-items').replaceChildren();$('#triage-detail').textContent='Sélectionnez une annonce.';$('#triage-select-all').checked=false;$('#triage-show-all').hidden=true;await load(true);}
async function load(reset){if(loading)return;loading=true;const serial=++requestSerial;$('#triage-more').disabled=true;try{
 const resp=await invoke('review_step2_inspect',{filter:filters()});if(serial!==requestSerial)return;
 total=resp.total;const unique=new Set(items.map(key));for(const item of resp.items){if(!unique.has(key(item))){items.push(item);unique.add(key(item));}}
 offset+=resp.items.length;renderList();$('#triage-more').hidden=offset>=total;$('#triage-count').textContent=`${total} dans la file sélectionnée · ${items.length} affichée(s) / ${total}`;
 const stats=resp.overview;
 if(stats){$('#triage-overview').replaceChildren();for(const [label,value,cls] of [['Découvertes',stats.discovered,''],['File prioritaire',stats.priority,'priority'],['Validées · ancien préréglage',stats.eligible,'eligible'],['À vérifier',stats.unverified,'unverified'],['Exclues',stats.excluded,'excluded']]){
   const span=document.createElement('span');span.className='triage-overview-item '+cls;const strong=document.createElement('strong');strong.textContent=value;const title=document.createElement('small');title.textContent=label;span.append(strong,title);$('#triage-overview').append(span);
 }}
 $('#triage-show-all').hidden=total!==0||$('#triage-scope').value==='all';
 if(!total){$('#triage-detail').textContent='Aucune annonce dans cette file. Changez de vue pour examiner les offres non vérifiées ou exclues.';}
 if(!selectedKey&&items.length)select(items[0]);
 announce(reset?'Liste actualisée.':`${resp.items.length} annonce(s) supplémentaires chargées.`);
 }catch(e){errors('Impossible de charger Review : '+e);}finally{loading=false;$('#triage-more').disabled=false;}}
function renderList(){const parent=$('#triage-items');parent.replaceChildren();for(const item of items){const entry=document.createElement('div');entry.className='triage-row';entry.dataset.key=key(item);entry.tabIndex=0;entry.setAttribute('role','option');entry.setAttribute('aria-selected',String(selectedKey===key(item)));
 entry.innerHTML=`<input class="triage-mark" type="checkbox" aria-label="Sélectionner ${htmlEscape(item.title)}"><div class="triage-row-main"><strong>${htmlEscape(item.title||'Sans intitulé')}</strong><div class="triage-meta">${htmlEscape(item.company||'Employeur inconnu')} · ${htmlEscape(item.place||'Lieu inconnu')} · ${htmlEscape(item.source)}</div><div class="triage-badges"><span class="triage-tag ${htmlEscape(item.classification)}">${htmlEscape(eligibility[item.classification]||item.classification)}</span><span class="triage-tag decision">${htmlEscape(stateName[item.decision]||'À examiner')}</span>${item.duplicateCount?'<span class="triage-tag duplicate">Doublon possible</span>':''}</div></div><small class="triage-date" title="Date de publication">${htmlEscape(formatDate(item.publishedAt))}</small>`;
 entry.addEventListener('click',e=>{if(e.target.closest('input'))return;select(item);});entry.addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();select(item);}});parent.append(entry);}
}
function select(item){selectedKey=key(item);document.querySelectorAll('#triage-items .triage-row').forEach(row=>{const active=row.dataset.key===selectedKey;row.classList.toggle('selected',active);row.setAttribute('aria-selected',String(active));});const p=$('#triage-detail');p.replaceChildren();
 const el=(tag,text,cls)=>{const e=document.createElement(tag);e.textContent=text||'';if(cls)e.className=cls;p.append(e);return e;};
 el('div',`${item.source.toUpperCase()} · ${eligibility[item.classification]||item.classification}`,'triage-eyebrow');el('h3',item.title||'Annonce');el('p',[item.company,item.place,'Publication : '+formatDate(item.publishedAt),'Collecte : '+formatDate(item.retrievedAt)].filter(Boolean).join(' · '),'triage-meta');
 if(item.duplicateCount)el('p',`${item.duplicateCount} autre(s) annonce(s) de titre/entreprise/lieu identiques, sur une autre plateforme. Vérifier manuellement. Aucune fusion automatique.`,'triage-dup-notice');
 el('h4','Pourquoi ce classement ?');el('p',item.legacyReason||'Critères non vérifiés — consulter l’annonce originale.','triage-reason');
 if(item.evidence && JSON.stringify(item.evidence)!=='[]'&&JSON.stringify(item.evidence)!=='null')el('pre',JSON.stringify(item.evidence,null,2),'triage-evidence');
 el('h4','Description de l’offre');const description=String(item.description||'');let cleaned=description;
 if(description.includes('<')){const doc=new DOMParser().parseFromString(description.replace(/<\/(?:p|div|h[1-6]|li)>/gi,'\n'), 'text/html');cleaned=doc.body.textContent||'';}
 el('div',cleaned.trim()||'Description indisponible : ne pas considérer les exigences comme vérifiées.','triage-description');
 const buttons=el('div','','triage-buttons');const button=(label,act,css)=>{const b=document.createElement('button');b.type='button';b.textContent=label;b.className=css||'';b.disabled=busy; b.addEventListener('click',act);buttons.append(b);return b;};
 if(/^https:\/\//i.test(item.listingUrl)){button('Annonce originale ↗',()=>openOriginal(item.listingUrl));}
 const eligibleForPromotion=['eligible','unverified','excluded'].includes(item.classification)&&item.detailStatus==='description_found'&&description.trim().length>0;
 if(eligibleForPromotion)button(item.inReview?'Déjà dans Review classique':'Approuver → À postuler',()=>action('approved',item),'primary');
 if(item.decision!=='approved') {button('Plus tard',()=>action('later',item));button('Écarter',()=>action('skipped',item));}
 if(item.decision!=='approved') button('Annuler la dernière décision',()=>action('undo',item));
 const notes=el('label','Note personnelle','triage-note-label');const textarea=document.createElement('textarea');textarea.id='triage-note';textarea.value=item.note||'';textarea.maxLength=5000;textarea.rows=3;textarea.placeholder='Motif, question à vérifier, décision…';notes.append(textarea);
 button('Enregistrer la note',()=>action('note',item,textarea.value));
 if(item.classification==='excluded')el('p','Le préréglage personnel déconseille cette offre. Après lecture, vous pouvez néanmoins choisir de postuler manuellement.','triage-safety');
 else if(!eligibleForPromotion)el('p','Approbation indisponible : description complète ou classification vérifiable nécessaire. Vous pouvez reporter ou écarter cette offre.','triage-safety');
 if(item.decision==='approved')el('p','Déjà ajoutée à la liste des offres à postuler. Les statuts de candidature ne sont pas modifiés ici.','triage-safety');
}
let busy=false;
function openOriginal(url){if(!/^https:\/\//.test(url))return; if(typeof window.openExternalUrl==='function')window.openExternalUrl(url);else if(window.__TAURI__?.opener?.openUrl)window.__TAURI__.opener.openUrl(url).catch(e=>errors(e));else window.open(url,'_blank','noopener,noreferrer');}
async function action(kind,item,note){if(busy)return;busy=true;try{
 if(kind==='approved'){
   if(item.classification==='unverified'){
     const confirmed=window.confirm(`Cette offre est À VÉRIFIER. Vous devez lire l’annonce originale et confirmer personnellement toutes les conditions obligatoires.\n\n${item.title}\n\nLa déplacer vers À postuler ?`);if(!confirmed)return;
   }else if(!window.confirm(`Avez-vous vérifié les conditions obligatoires de cette offre ?\n\n${item.title}\n\nL’ajouter aux offres à postuler ?`))return;
   await invoke('review_step2_approve',{source:item.source,sourceId:item.sourceId,allowUnverified:item.classification==='unverified',allowExcluded:item.classification==='excluded'});
   // Update the existing legacy tables through an installer-registered bridge.
   if(typeof window.jobupReviewReloadLegacy==='function'){
      try{await window.jobupReviewReloadLegacy();}catch(e){console.warn('Legacy refresh:',e);}
   }
 }else if(kind==='undo'){
   await invoke('review_step2_undo',{source:item.source,sourceId:item.sourceId});
 }else if(kind==='note'){
   await invoke('review_step2_note',{source:item.source,sourceId:item.sourceId,note:note??''});
 }else{
   await invoke('review_step2_decide',{source:item.source,sourceId:item.sourceId,decision:kind});
 }
 announce(`Décision enregistrée : ${kind==='approved'?'à postuler':kind==='note'?'note':kind==='undo'?'retour en arrière':stateName[kind]||kind}.`);
 await reload();
 const chosen=items.find(x=>key(x)===key(item));if(chosen)select(chosen);
 }catch(e){errors('Impossible d’enregistrer : '+e);}finally{busy=false;}}
async function batch(decision){if(busy)return;const marked=Array.from(document.querySelectorAll('#triage-items .triage-row')).filter(el=>el.querySelector('.triage-mark')?.checked).map(el=>items.find(x=>key(x)===el.dataset.key)).filter(Boolean);
 if(!marked.length)return announce('Sélectionnez des annonces avant une action groupée.');
 if(marked.length>100)return errors('Sélection maximale : 100 offres.');
 if(marked.some(x=>x.decision==='approved'))return errors('Une offre est déjà approuvée; retirez-la de la sélection.');
 if(!confirm(`${decision==='skipped'?'Écarter':'Reporter'} ${marked.length} annonce(s) ? Cette action est réversible individuellement.`))return;
 busy=true;try{const n=await invoke('review_step2_bulk',{items:marked.map(x=>({source:x.source,sourceId:x.sourceId})),decision});await reload();announce(`${n} annonce(s) modifiée(s).`);}catch(e){errors(e);}finally{busy=false;}
}
function onKey(e){if(!isVisible()||e.ctrlKey||e.metaKey||e.altKey)return;const active=document.activeElement;if(active&&['INPUT','TEXTAREA','SELECT'].includes(active.tagName))return;
 const i=items.findIndex(x=>key(x)===selectedKey);if(e.key==='/' ){e.preventDefault();$('#triage-query').focus();return;}
 if((e.key==='j'||e.key==='ArrowDown')&&i<items.length-1){e.preventDefault();select(items[i+1]);document.querySelector('#triage-items .triage-row.selected')?.scrollIntoView({block:'nearest'});}
 if((e.key==='k'||e.key==='ArrowUp')&&i>0){e.preventDefault();select(items[i-1]);document.querySelector('#triage-items .triage-row.selected')?.scrollIntoView({block:'nearest'});}
 const current=items.find(x=>key(x)===selectedKey);if(!current)return;
 if(e.key==='a'){e.preventDefault();if(['eligible','unverified','excluded'].includes(current.classification)&&current.description&&current.detailStatus==='description_found')action('approved',current);}
 if(e.key==='l'){e.preventDefault();action('later',current);}
 if(e.key==='s'){e.preventDefault();action('skipped',current);}
 if(e.key==='z'){e.preventDefault();action('undo',current);}
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
