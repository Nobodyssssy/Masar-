/* Jobup — Step 3/7 Applications. UI only; SQLite remains authoritative. */
(()=>{
'use strict';
const API={invoke:(cmd,args)=>{
  const fn=window.__TAURI__?.core?.invoke;
  if(!fn)throw Error('Application native Tauri requise');
  return fn(cmd,args||{});
}};
const h=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const el=(id)=>document.getElementById(id);
const STAGES={new:'À postuler',applied:'Envoyée · en attente',screening:'Réponse / sélection',interview:'Entretien',offer:'Offre reçue',rejected:'Refusée',withdrawn:'Retirée'};
const statusFilter={all:'Toutes',new:'À postuler',applied:'Envoyées',screening:'Réponses',interview:'Entretiens',offer:'Offres',closed:'Clôturées',due:'Relances dues'};
const shell=`<section id="view-applications-step3" class="view applications3">
 <div class="page-heading"><div><h2>Mes candidatures</h2><p>À postuler → Envoyée → Réponse → Entretien / Offre / Clôture</p></div><div class="page-toolbar"><button type="button" id="app3-refresh">Actualiser</button><button type="button" id="app3-go-review">Ouvrir Review</button></div></div>
 <div class="app3-stats" id="app3-stats" aria-label="Statistiques des candidatures"></div>
 <div class="app3-toolbar"><label>Statut <select id="app3-filter">${Object.entries(statusFilter).map(([k,v])=>`<option value="${h(k)}">${h(v)}</option>`).join('')}</select></label><label>Rechercher <input id="app3-query" type="search" placeholder="Titre, société, lieu…"></label><label>Tri <select id="app3-sort"><option value="recent">Activité récente</option><option value="company">Entreprise A–Z</option><option value="follow">Prochaine relance</option></select></label><span id="app3-count" role="status"></span></div>
 <div class="app3-workspace"><div class="app3-list-panel"><div class="app3-list-heading">Candidatures enregistrées</div><div id="app3-list" role="listbox" aria-label="Liste des candidatures"></div></div><div class="app3-detail" id="app3-detail" aria-live="polite"><div class="app3-empty">Choisissez une candidature pour voir ses documents, son statut et son historique.</div></div></div>
 <div id="app3-message" class="app3-message" role="status" aria-live="polite"></div>
</section>`;
let started=false,active=false,loading=false,saving=false,items=[],selected='',activityToken=0;
const getRow=()=>items.find(x=>x.jobId===selected);
const toast=(message,error=false)=>{const t=el('app3-message');if(t){t.textContent=String(message);t.classList.toggle('error',error);}};
function initialize(){if(started)return;const main=document.querySelector('main');if(!main)return;
  main.insertAdjacentHTML('beforeend',shell);
  const previousNavigate=window.jobupNavigate;
  window.jobupNavigate=(page)=>['tracker','applications','applications-step3'].includes(page)?navigate():typeof previousNavigate==='function'?previousNavigate(page):undefined;
  for(const trigger of document.querySelectorAll('.side-link[data-page="tracker"],.tab[data-tab="tracker"]')){
    trigger.addEventListener('click',event=>{event.preventDefault();event.stopImmediatePropagation();navigate();},true);
  }
  el('app3-refresh').addEventListener('click',refresh);
  el('app3-go-review').addEventListener('click',()=>window.jobupNavigate?.('review'));
  for(const id of ['app3-filter','app3-sort'])el(id).addEventListener('change',renderList);
  el('app3-query').addEventListener('input',renderList);
  started=true;
}
function navigate(){initialize();if(!started)return;active=true;
  document.querySelectorAll('.view').forEach(v=>v.classList.toggle('active',v.id==='view-applications-step3'));
  document.querySelectorAll('.side-link,.tab').forEach(x=>x.classList.toggle('active',x.dataset.page==='tracker'||x.dataset.tab==='tracker'));
  const title=el('desktop-page-title');if(title)title.textContent='Mes candidatures';
  refresh();
}
async function refresh(){if(loading)return;loading=true;toast('Chargement des candidatures…');
  try{const result=await API.invoke('applications_step3_list');items=result.items||[];
   const boxes=[['Suivies',result.summary?.tracked,''],['À postuler',result.summary?.toApply,''],['Envoyées',result.summary?.applied,'info'],['Relances dues',result.summary?.followUpsDue,'warning'],['Entretiens',result.summary?.interviews,''],['Offres',result.summary?.offers,'success']];
   el('app3-stats').innerHTML=boxes.map(([label,value,cls])=>`<div class="app3-stat ${cls}"><strong>${Number(value||0)}</strong><small>${h(label)}</small></div>`).join('');
   if(result.truncated)toast('Plus de 5 000 candidatures : affichage partiel, l’archive reste intacte.',true);else toast(`${items.length} dossier(s) chargé(s).`);
   if(window.jobupRequestedApplicationId && items.some(x=>x.jobId===window.jobupRequestedApplicationId)){selected=window.jobupRequestedApplicationId;window.jobupRequestedApplicationId=null;}
   if(!items.some(x=>x.jobId===selected))selected=items[0]?.jobId||'';
   renderList();renderDetail();
  }catch(err){toast('Erreur SQLite / Tauri : '+err,true);}finally{loading=false;}}
function filtered(){const f=el('app3-filter').value,q=el('app3-query').value.trim().toLocaleLowerCase('fr');const now=new Date().toLocaleDateString('sv-SE');
 return items.filter(x=>{
   if(f==='closed'&&!['rejected','withdrawn'].includes(x.status))return false;
   if(f==='due'&&!(x.followUpDate && x.followUpDate<=now && ['applied','screening'].includes(x.status)))return false;
   if(!['all','closed','due'].includes(f)&&x.status!==f)return false;
   return !q||(x.title+' '+x.company+' '+x.place+' '+x.source).toLocaleLowerCase('fr').includes(q);
 }).sort((a,b)=>{
   const sort=el('app3-sort').value;
   if(sort==='company')return a.company.localeCompare(b.company,'fr');
   if(sort==='follow')return (a.followUpDate||'9999').localeCompare(b.followUpDate||'9999');
   return (b.appliedDate||'').localeCompare(a.appliedDate||'');
 });
}
function renderList(){if(!started)return;const arr=filtered();el('app3-count').textContent=`${arr.length} affichée(s) / ${items.length}`;const list=el('app3-list');list.replaceChildren();
 if(!arr.length){list.innerHTML='<div class="app3-empty">Aucune candidature pour ces filtres. Vous pouvez approuver des annonces dans Review.</div>';return;}
 for(const x of arr){const row=document.createElement('button');row.type='button';row.className='app3-row'+(x.jobId===selected?' selected':'');row.setAttribute('role','option');row.setAttribute('aria-selected',String(x.jobId===selected));row.innerHTML=`<span class="app3-row-main"><strong>${h(x.title||'Offre importée')}</strong><small>${h(x.company)} · ${h(x.place)} · ${h(x.source)}</small><span class="app3-tag ${h(x.status)}">${h(STAGES[x.status]||x.status)}</span></span><span class="app3-row-meta">${x.followUpDate?`Relance ${h(x.followUpDate)}`:''}</span>`;
   row.addEventListener('click',()=>{if(saving)return;selected=x.jobId;renderList();renderDetail();});list.append(row);
 }
}
function renderDetail(){const d=el('app3-detail'),x=getRow();if(!d)return;
 if(!x){d.innerHTML='<div class="app3-empty">Aucune candidature sélectionnée.</div>';return;}
 d.innerHTML=`<div class="app3-detail-heading"><small>${h(x.source)} · ${h(x.company)}</small><h3>${h(x.title||'Offre importée')}</h3><p>${h(x.place)}</p><div class="app3-detail-actions"><button type="button" id="app3-open" ${/^https:\/\//.test(x.listingUrl)?'':'disabled'}>Annonce originale ↗</button><button type="button" id="app3-quick-applied" class="primary" ${x.status==='new'?'':'hidden'}>Marquer comme envoyée</button></div></div>
 <form id="app3-form" class="app3-form"><label>Étape de candidature <select name="status" id="app3-status" required>${STAGES[x.status]?'':`<option value="" selected disabled>Statut hérité « ${h(x.status)} » — choisir une étape</option>`}${Object.entries(STAGES).map(([k,v])=>`<option value="${h(k)}" ${x.status===k?'selected':''}>${h(v)}</option>`).join('')}</select></label>
 <div class="app3-form-pair"><label>CV utilisé (profil)<input name="cvProfile" maxlength="120" list="app3-profiles" value="${h(x.cvProfile)}" placeholder="Ex. Vente"></label><datalist id="app3-profiles"><option value="Vente"></option><option value="Logistique"></option><option value="Administration commerciale"></option></datalist><label>Méthode de candidature <input name="applicationMethod" maxlength="200" value="${h(x.applicationMethod)}" placeholder="Site employeur, email…"></label></div>
 <div class="app3-form-pair"><label>Référence CV / chemin local<input name="cvFile" maxlength="1024" value="${h(x.cvFile)}" placeholder="C:\\…\\CV.pdf"></label><label>Lettre / document utilisé<input name="letterFile" maxlength="1024" value="${h(x.letterFile)}" placeholder="Chemin local facultatif"></label></div>
 <div class="app3-form-pair"><label>Date d’envoi (si déjà postulé)<input name="appliedDate" type="date" value="${h(x.appliedDate)}" ${x.appliedDate?'readonly title="Date déjà enregistrée : historique protégé"':''}></label><label>Prochaine relance<input name="followUpDate" type="date" value="${h(x.followUpDate)}"></label></div><label>Entretien (date/heure locale)<input name="interviewAt" type="datetime-local" value="${h(x.interviewAt)}"></label>
 <label>Contact employeur<input name="contact" maxlength="300" value="${h(x.contact)}" placeholder="Nom, email ou référence"></label>
 <label>Notes de candidature<textarea name="note" maxlength="10000" rows="4" placeholder="Réponse de l'employeur, prochaines étapes…">${h(x.note)}</textarea></label>
 <div class="app3-editor-footer"><button class="primary" type="submit" id="app3-save">Enregistrer dans SQLite</button><span class="app3-tip">L'enregistrement ne postule pas à votre place et n'envoie aucun email.</span></div></form>
 <div class="app3-history"><h4>Historique de candidature</h4><div id="app3-history">${(x.history||[]).length?([...x.history].reverse().map(evt=>`<div class="app3-event"><b>${h(STAGES[evt.to]||evt.to||'Mise à jour')}</b><small>${h(evt.date||'Date inconnue')} · ${h(evt.from||'')}</small></div>`).join('')):'<p class="hint">Aucun changement de statut enregistré.</p>'}</div><div id="app3-audit"></div></div>`;
 el('app3-form').addEventListener('submit',e=>{e.preventDefault();save();});
 el('app3-open').addEventListener('click',()=>openOriginal(x.listingUrl));
 el('app3-quick-applied').addEventListener('click',()=>{el('app3-status').value='applied';save();});
 fetchActivity(x.jobId);
}
async function fetchActivity(id){const serial=++activityToken;try{const data=await API.invoke('applications_step3_activity',{jobId:id});if(serial!==activityToken||selected!==id)return;
 const target=el('app3-audit');if(!target)return;
 const changes=(data||[]).slice(0,20);if(changes.length){target.innerHTML='<h4>Modifications des documents et informations</h4>'+changes.map(e=>`<div class="app3-event"><b>Enregistrement</b><small>${h(e.recordedAt||'')}</small></div>`).join('');}}
 catch(err){console.warn('Application activity:',err);}}
function openOriginal(url){if(!/^https:\/\//.test(url))return;
 if(typeof window.openExternalUrl==='function')window.openExternalUrl(url);
 else if(window.__TAURI__?.opener?.openUrl)window.__TAURI__.opener.openUrl(url).catch(e=>toast(e,true));
 else window.open(url,'_blank','noopener,noreferrer');
}
async function save(){if(saving)return;const x=getRow();if(!x)return;
 const form=el('app3-form');const data=Object.fromEntries(new FormData(form).entries());
 if(x.status==='new'&&data.status==='applied'&&!confirm(`Avez-vous réellement soumis votre candidature pour « ${x.title} » ?\n\nCette étape enregistrera la candidature comme envoyée. Aucun envoi automatique ne sera effectué.`))return;
 if(x.status==='new'&&!['new','applied','withdrawn'].includes(data.status)){toast('Veuillez d’abord marquer la candidature comme Envoyée.',true);return;}
 saving=true;const btn=el('app3-save');if(btn)btn.disabled=true;
 try{await API.invoke('applications_step3_save',{update:{jobId:x.jobId,revision:x.revision,...data}});
   if(typeof window.jobupReviewReloadLegacy==='function'){try{await window.jobupReviewReloadLegacy();}catch(err){console.warn('Dashboard refresh',err);}}
   await refresh();toast('Candidature et historique enregistrés dans SQLite.');
 }catch(err){toast('Échec : '+err,true);if(btn)btn.disabled=false;}finally{saving=false;}
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',initialize,{once:true});else initialize();
window.jobupOpenApplication=(jobId)=>{
  if(typeof jobId!=="string"||!jobId)return;
  window.jobupRequestedApplicationId=jobId;
  navigate();
  if(items.some(x=>x.jobId===jobId)){selected=jobId;renderList();renderDetail();}
};
})();
