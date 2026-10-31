/* Jobup Sources 6/7: truthful per-source diagnostics and read-only SQLite quality audit. */
(()=>{'use strict';
const doc=document,el=id=>doc.getElementById(id);
const SOURCE_NAMES=[
 ['Jobup','jobup','https://www.jobup.ch/fr/','collector'],
 ['Jobs.ch','jobs_ch','https://www.jobs.ch/fr/','collector'],
 ['Indeed Suisse','indeed_ch','https://ch.indeed.com/','website'],
 ['Emploi en Suisse','emploi_en_suisse','https://emploi-en-suisse.ch/','website'],
 ['JobScout24','jobscout24','https://www.jobscout24.ch/fr/','collector'],
 ['OK Job','okjob','https://www.okjob.ch/offres-demplois/','collector'],
 ['100000jobs','100000jobs','https://www.100000jobs.ch/','website'],
 ['Job-Room','job_room','https://www.job-room.ch/index.html','website'],
 ['Züri Jobs','zuri_jobs','https://www.zueri.jobs/jobs','collector'],
 ['Michael Page','michael_page','https://www.michaelpage.ch/jobs','collector'],
 ['LinkedIn Jobs','linkedin_jobs','https://www.linkedin.com/jobs/','collector'],
 ['SwissDevJobs','swissdevjobs','https://swissdevjobs.ch/jobs','collector'],
 ['eFinancialCareers','efinancialcareers','https://www.efinancialcareers.ch/jobs','collector']
];
const DIAG='jobup_collection_diagnostics_v1',HISTORY='jobup_sources_step6_history_v1';
const state={ready:false,selected:'Jobup',inventory:{},history:{},status:{},loading:false,filter:'all',sort:'default',error:null};
const invoke=(cmd,args={})=>{const f=window.__TAURI__?.core?.invoke;if(!f)throw Error('Interface Rust indisponible');return f(cmd,args)};
const fmt=n=>n===undefined||n===null?'—':Number(n).toLocaleString('fr-CH');
const formatDate=(value)=>{if(!value)return '—';const raw=String(value).trim();const normalized=/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(raw)?raw.replace(' ','T')+'Z':raw;const date=new Date(normalized);return Number.isNaN(date.getTime())?String(value):date.toLocaleString('fr-CH',{day:'2-digit',month:'2-digit',year:'numeric',hour:'2-digit',minute:'2-digit'});};
const statusLabel={Success:'Opérationnel',Working:'Opérationnel (ancien état)',Partial:'Partiel',Blocked:'Bloqué',Failed:'Échec',Unavailable:'Indisponible',Cancelled:'Annulé',Running:'En cours',NotChecked:'Non vérifié'};
function readObject(key){try{const value=JSON.parse(localStorage.getItem(key)||'{}');return value&&typeof value==='object'&&!Array.isArray(value)?value:{}}catch{return {}}}
function report(){return {exportedAt:new Date().toISOString(),origin:'Jobup desktop (local)',sourceAttempts:state.status,sourceHistory:state.history,sqliteInventory:state.inventory};}
function td(label,value){const d=doc.createElement('div');d.className='s6-fact';const l=doc.createElement('span');l.textContent=label;const v=doc.createElement('strong');v.textContent=value;d.append(l,v);return d;}
function make(tag,cls,text){const n=doc.createElement(tag);if(cls)n.className=cls;if(text!==undefined)n.textContent=text;return n;}
function button(text,handler,cls){const b=make('button',cls,text);b.type='button';b.addEventListener('click',handler);return b;}
function statusKind(s){return ({Success:'good',Partial:'partial',Blocked:'bad',Failed:'bad',Cancelled:'neutral',Unavailable:'neutral',Running:'partial',Working:'partial'}[s]||'neutral');}
function selectedEntry(){return SOURCE_NAMES.find(x=>x[0]===state.selected)||SOURCE_NAMES[0]}
function navigate(page){window.jobupNavigate?.(page)}
function wrapDiagnostics(){if(typeof setCollectorStatus!=='function')return;
 const original=setCollectorStatus;
 setCollectorStatus=function(name,status,details={}){
   original(name,status,details);
   if(!SOURCE_NAMES.some(x=>x[0]===name))return;
   // A refresh phase is not a completed attempt. Store terminal events only.
   const end=['Success','Partial','Blocked','Failed','Cancelled','Unavailable'].includes(status);
   if(end){const h=readObject(HISTORY),now=new Date().toISOString();const rows=Array.isArray(h[name])?h[name]:[];
     rows.unshift({at:now,status,fetched:details.fetched??details.checked??null,newCount:details.newCount??null,descriptions:details.descriptions??null,eligible:details.eligible??null,unverified:details.unverified??null,excluded:details.excluded??null,durationMs:details.durationMs??null,message:String(details.message||'').slice(0,400)});
     h[name]=rows.slice(0,15);try{localStorage.setItem(HISTORY,JSON.stringify(h));}catch{}
   }
   if(state.ready){syncLocal();render();if(end)refreshInventory();}
 };
}
function syncLocal(){state.status=readObject(DIAG);state.history=readObject(HISTORY);}
const rootMarkup=`<div id="s6-sources" aria-label="Diagnostics des 13 sources">
 <div class="s6-heading"><div><h2>Sources d’offres</h2><p>État réel de la collecte, qualité des descriptions et données conservées dans SQLite.</p></div>
 <div class="s6-actions"><button type="button" class="primary" id="s6-search">Rechercher des offres</button><button type="button" id="s6-audit">Actualiser l’audit</button></div></div>
 <div id="s6-banner" class="s6-banner" role="status" aria-live="polite"></div>
 <div class="s6-overview" id="s6-overview"></div>
 <div class="s6-layout"><div class="s6-left"><div class="s6-switch"><button type="button" data-mode="all">Toutes</button><button type="button" data-mode="issues">Problèmes</button></div><div id="s6-providers" class="s6-providers"></div></div>
 <section class="s6-detail" aria-label="Détails de la source"><div id="s6-detail"></div></section></div>
 <div class="s6-bottom"><section class="s6-card"><div class="s6-card-head"><h3>Historique des tentatives</h3><p>Enregistré sur cet appareil à partir de l’installation de cette page.</p></div><div id="s6-history"></div></section>
 <section class="s6-card"><div class="s6-card-head"><h3>Interprétation des chiffres</h3><p>À distinguer avant de diagnostiquer une panne de collecte.</p></div>
 <p><strong>Collectées</strong> : annonces récupérées lors de la dernière tentative. <strong>En SQLite</strong> : annonces conservées depuis toutes les collectes, dédoublonnées par source et identifiant.</p>
 <p><strong>Vu le</strong> : dernière récupération locale, ce n’est pas la date de publication de l’offre. <strong>Publié le</strong> n’existe que si la source le fournit.</p>
 <p>Une réponse HTTP 403 est <strong>Bloquée</strong>, pas « 0 emploi ». Une source sans collecteur reste <strong>Indisponible</strong>. Les estimations de descriptions courtes sont des indicateurs de qualité, pas des verdicts de qualification.</p>
 <div class="s6-actions"><button id="s6-copy" type="button">Copier le rapport technique (JSON)</button><button id="s6-legacy" type="button">Importation / outils existants</button></div><textarea id="s6-report-fallback" hidden readonly aria-label="Rapport technique à copier"></textarea>
 </section></div>
 </div>`;
function mount(){if(state.ready)return;const host=el('view-sources');if(!host)return;
 host.insertAdjacentHTML('afterbegin',rootMarkup);state.ready=true;
 el('s6-search').addEventListener('click',()=>navigate('search'));
 el('s6-audit').addEventListener('click',refreshInventory);
 el('s6-copy').addEventListener('click',async()=>{
  const text=JSON.stringify(report(),null,2);try{await navigator.clipboard.writeText(text);banner('Rapport technique copié. Il ne contient pas les annonces complètes.',false);}catch{
   const fallback=el('s6-report-fallback');fallback.hidden=false;fallback.value=text;fallback.focus();fallback.select();banner('Copie automatique impossible. Copiez le texte sélectionné ci-dessous.',true);
  }
 });
 el('s6-legacy').addEventListener('click',()=>{
  host.classList.toggle('s6-show-legacy');const legacy=el('swiss-sources-panel');if(legacy){legacy.hidden=false;try{renderSwissSources();renderCollectorDiagnostics();}catch{}}
 });
 for(const b of doc.querySelectorAll('.s6-switch button'))b.addEventListener('click',()=>{state.filter=b.dataset.mode;renderProviders();});
 // Use click navigation to reload view after it becomes visible; no extra routing layer.
 const sidebar=doc.querySelector('.side-link[data-page="sources"]');if(sidebar)sidebar.addEventListener('click',()=>{syncLocal();render();refreshInventory();});
 wrapDiagnostics();syncLocal();render();refreshInventory();
}
function banner(message,warning){const root=el('s6-banner');root.textContent=message||'';root.classList.toggle('warning',!!warning);root.hidden=!message;}
function renderOverview(){const host=el('s6-overview');host.replaceChildren();const rows=Object.values(state.inventory),sum=key=>rows.reduce((acc,v)=>acc+Number(v[key]||0),0);
 const known=SOURCE_NAMES.filter(x=>['Success','Partial','Working'].includes(state.status[x[0]]?.status)).length;
 const failure=SOURCE_NAMES.filter(x=>['Blocked','Failed'].includes(state.status[x[0]]?.status)).length;
 const metrics=[['Sites configurés',String(SOURCE_NAMES.length)],['Collectes avec résultat',fmt(known)],['Bloqués ou en échec',fmt(failure)],['Annonces en SQLite',fmt(sum('storedListings'))],['Descriptions disponibles',fmt(sum('descriptionsAvailable'))]];
 for(const [label,value]of metrics)host.append(td(label,value));
}
function renderProviders(){const host=el('s6-providers');host.replaceChildren();const entries=SOURCE_NAMES.filter(x=>state.filter==='all'||['Blocked','Failed','Partial','Cancelled','Unavailable'].includes(state.status[x[0]]?.status)||!state.status[x[0]]?.status);
 for(const [name,key,url,capability]of entries){const stateRow=state.status[name]||{},status=stateRow.status||(capability==='website'?'Unavailable':'NotChecked'),db=state.inventory[key]||{};
  const b=button('',()=>{state.selected=name;render();},'s6-provider'+(state.selected===name?' selected':''));b.setAttribute('aria-pressed',String(state.selected===name));
  const label=make('div','s6-provider-name');label.append(make('strong','',name),make('span','s6-badge '+statusKind(status),statusLabel[status]||status));
  const stats=make('div','s6-provider-stats');stats.textContent=`En SQLite : ${fmt(db.storedListings??0)} · Descriptions : ${fmt(db.descriptionsAvailable??0)}`;
  const hint=make('div','s6-provider-hint');hint.textContent=status==='Blocked'?'Accès refusé par le site':capability==='website'?'Collecteur natif absent':status==='Running'?'Collecte en cours':`Dernier essai : ${formatDate(stateRow.attemptedAt)}`;
  b.append(label,stats,hint);host.append(b);
 }
}
function renderDetail(){const root=el('s6-detail');root.replaceChildren();const [name,key,url,capability]=selectedEntry(),d=state.status[name]||{},m=state.inventory[key]||{},status=d.status||'NotChecked';
 const head=make('div','s6-card-head');const label=make('div');label.append(make('h3','',name),make('p','',`État : ${statusLabel[status]||status} · ${capability==='collector'?'Collecteur natif présent':capability==='website'?'Site officiel uniquement':'Collecteur absent'}`));head.append(label);
 const open=doc.createElement('a');open.href=url;open.target='_blank';open.rel='noopener noreferrer';open.textContent='Site officiel ↗';open.addEventListener('click',async event=>{event.preventDefault();try{await invoke('open_official_job_site',{source:key});}catch(e){banner('Ouverture impossible : '+String(e)+' · Adresse : '+url,true);}});head.append(open);root.append(head);
 const row=make('div','s6-detail-section');row.append(make('h4','','Dernière tentative (diagnostics locaux)'));
 const grid=make('div','s6-facts');
 for(const [label,v]of [['État',statusLabel[status]||'Non vérifié'],['Tentative',formatDate(d.attemptedAt)],['Dernier succès enregistré',formatDate(d.lastSuccessAt)],['Trouvées (dernière tentative)',fmt(d.fetched??d.checked)],['Nouvelles',fmt(d.newCount)],['Révisées',fmt(d.revised??d.updated)],['Doublons dans le lot',fmt(d.duplicates)],['Durée',d.durationMs===undefined?'—':`${Math.round(Number(d.durationMs))} ms`]])grid.append(td(label,v));
 row.append(grid);if(d.message){const msg=make('p','s6-message',String(d.message));row.append(msg);}root.append(row);
 const inventory=make('div','s6-detail-section');inventory.append(make('h4','','Inventaire et qualité dans SQLite'));
 const facts=make('div','s6-facts');
 for(const [label,k]of [['Total conservé','storedListings'],['Éligibles','eligible'],['À vérifier','unverified'],['Exclues','excluded'],['Sans classification','unclassified'],['Avec description','descriptionsAvailable'],['Descriptions courtes (moins de 150 caractères)','shortDescriptions'],['Publication inconnue','unknownPublicationDates'],['Taux d’activité inconnu','unknownWorkloads'],['Non revues depuis plus de 30 jours','stale30Days'],['Snapshots de révision','revisionSnapshots']])facts.append(td(label,fmt(m[k]??0)));
 facts.append(td('Dernière récupération enregistrée',formatDate(m.lastRetrievedAt)),td('Publication la plus récente renseignée',formatDate(m.lastPublishedAt)));
 inventory.append(facts);root.append(inventory);
 const reasons=Array.isArray(m.exclusionReasons)?m.exclusionReasons:[];
 if(reasons.length){const sec=make('div','s6-detail-section');sec.append(make('h4','','Principales raisons d’exclusion (en SQLite)'));const list=make('ol','s6-reasons');for(const r of reasons){const li=make('li','',`${String(r.reason)} · ${fmt(r.count)}`);list.append(li);}sec.append(list);root.append(sec);}
 if(capability==='website'){root.append(make('p','s6-advice','Cette version ne dispose pas d’un collecteur natif pour ce site. Vous pouvez ouvrir son site et utiliser les outils d’importation existants.'));}
 if(status==='Blocked'){root.append(make('p','s6-advice','Accès refusé. Consultez l’annonce sur son site ou utilisez l’importation manuelle; aucune méthode de contournement n’est utilisée.'));}
}
function renderHistory(){const root=el('s6-history');root.replaceChildren();const all=Array.isArray(state.history[state.selected])?state.history[state.selected]:[];
 if(!all.length){root.append(make('p','s6-muted','Aucun événement enregistré pour cette source depuis l’installation de Step 6. Les dernières tentatives antérieures restent visibles dans le panneau ci-dessus.'));return;}
 const list=make('div','s6-events');for(const item of all.slice(0,12)){const row=make('div','s6-event');row.append(make('span','s6-badge '+statusKind(item.status),statusLabel[item.status]||item.status));
 const body=make('div');body.append(make('strong','',formatDate(item.at)),make('small','',`Trouvées : ${fmt(item.fetched)} · Nouvelles : ${fmt(item.newCount)} · ${item.message||'Aucun détail fourni'}`));row.append(body);list.append(row);}root.append(list);
}
function render(){if(!state.ready)return;renderOverview();renderProviders();renderDetail();renderHistory();}
async function refreshInventory(){if(!state.ready||state.loading)return;state.loading=true;const button=el('s6-audit');button.disabled=true;
 try{const metrics=await invoke('sources_step6_inspect');if(!Array.isArray(metrics))throw Error('Réponse SQLite non reconnue');state.inventory={};for(const m of metrics){if(m&&typeof m.source==='string')state.inventory[m.source]=m;}
  state.error=null;banner('Audit SQLite actualisé. Les compteurs d’inventaire incluent l’historique enregistré.',false);
 }catch(e){state.error=String(e);banner('Impossible de lire l’inventaire SQLite : '+state.error.slice(0,240),true);}finally{state.loading=false;button.disabled=false;syncLocal();render();}
}
if(doc.readyState==='loading')doc.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
