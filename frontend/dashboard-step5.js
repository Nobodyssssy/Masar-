/* Step 5 of 7 — true-data Dashboard. Read-only: no application/status mutations. */
(()=>{'use strict';
const el=id=>document.getElementById(id);
const invoke=(cmd,args={})=>{const f=window.__TAURI__?.core?.invoke;if(!f)throw new Error('Tauri indisponible');return f(cmd,args);};
const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const iso=d=>`${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;
const today=()=>iso(new Date());
const dayDate=s=>{if(!/^\d{4}-\d{2}-\d{2}$/.test(s||''))return null;const [y,m,d]=s.split('-').map(Number);const v=new Date(y,m-1,d);return iso(v)===s?v:null;};
const validDate=s=>dayDate(String(s||'').slice(0,10));
const state={apps:null,followups:null,review:null,search:null,month:new Date(new Date().getFullYear(),new Date().getMonth(),1),picked:today(),loading:false,ready:false};
const sourceNames=['Jobup','Jobs.ch','Indeed Suisse','Emploi en Suisse','JobScout24','OK Job','100000jobs','Job-Room','LinkedIn Jobs','Michael Page','Züri Jobs','SwissDevJobs','eFinancialCareers'];
const main=`<section id="step5-dashboard" aria-label="Vue d'ensemble de Masar">
<div class="s5-heading"><div><h2>Vue d’ensemble</h2><p>Vos prochaines actions, candidatures et sources — données locales de Masar.</p></div><div class="s5-controls"><span id="s5-updated" class="hint" aria-live="polite"></span><button type="button" id="s5-reload">Actualiser</button><button type="button" class="primary" id="s5-search">Rechercher des offres</button></div></div>
<div class="s5-metrics" id="s5-metrics" aria-label="Indicateurs principaux"></div>
<div class="s5-overview"><section class="s5-card s5-actions"><div class="s5-card-head"><div><h3>Actions prioritaires</h3><p>Relances en retard, entretiens et dossiers à traiter.</p></div><button type="button" id="s5-followups">Toutes les relances</button></div><div id="s5-action-list" class="s5-action-list"></div></section>
<section class="s5-card s5-calendar"><div class="s5-card-head"><div><h3>Calendrier</h3><p>Relances, entretiens et envois enregistrés.</p></div><div class="s5-month-nav"><button type="button" id="s5-prev" aria-label="Mois précédent">‹</button><button type="button" id="s5-now">Aujourd'hui</button><button type="button" id="s5-next" aria-label="Mois suivant">›</button></div></div><div class="s5-calendar-label" id="s5-month"></div><div class="s5-dayheads"><span>Lu</span><span>Ma</span><span>Me</span><span>Je</span><span>Ve</span><span>Sa</span><span>Di</span></div><div class="s5-month-grid" id="s5-month-grid"></div><div class="s5-legend"><span>● Relance</span><span>● Entretien</span><span>● Envoi</span></div><div class="s5-selected-events" id="s5-events" aria-live="polite"></div></section></div>
<div class="s5-bottom"><section class="s5-card"><div class="s5-card-head"><div><h3>Candidatures envoyées par semaine</h3><p>Huit dernières semaines · date d’envoi enregistrée</p></div></div><div class="s5-weekbars" id="s5-weekbars"></div></section><section class="s5-card"><div class="s5-card-head"><div><h3>État des candidatures</h3><p>Étapes actuelles, sans déduire de résultats inexistants.</p></div><button type="button" id="s5-apps">Mes candidatures</button></div><div id="s5-stage-bars" class="s5-stage-bars"></div></section></div>
<div class="s5-bottom"><section class="s5-card"><div class="s5-card-head"><div><h3>Activité récente</h3><p>Transitions réellement enregistrées.</p></div></div><div id="s5-recent"></div></section><section class="s5-card"><div class="s5-card-head"><div><h3>État des sources</h3><p>Dernière collecte connue dans les diagnostics locaux.</p></div><button type="button" id="s5-sources">Diagnostics</button></div><div id="s5-source-state"></div></section></div>
<p id="s5-integrity" class="s5-integrity" aria-live="polite"></p>
</section>`;
function navigate(page){if(typeof window.jobupNavigate==='function')window.jobupNavigate(page);}
function openApplication(id){if(typeof window.jobupOpenApplication==='function')window.jobupOpenApplication(id);else navigate('tracker');}
function mount(){if(state.ready)return;const host=el('view-dashboard');if(!host)return;
 host.insertAdjacentHTML('afterbegin',main);state.ready=true;
 el('s5-search').addEventListener('click',()=>navigate('search'));
 el('s5-reload').addEventListener('click',reload);
 el('s5-apps').addEventListener('click',()=>navigate('tracker'));
 el('s5-followups').addEventListener('click',()=>navigate('followups'));
 el('s5-sources').addEventListener('click',()=>navigate('sources'));
 el('s5-prev').addEventListener('click',()=>shift(-1));el('s5-next').addEventListener('click',()=>shift(1));
 el('s5-now').addEventListener('click',()=>{state.month=new Date(new Date().getFullYear(),new Date().getMonth(),1);state.picked=today();drawCalendar();});
 // Capture sidebar click before legacy handlers; do not create a competing dashboard router.
 for(const b of document.querySelectorAll('.side-link[data-page="dashboard"],.tab[data-tab="dashboard"]')){
  b.addEventListener('click',e=>{e.preventDefault();e.stopImmediatePropagation();navigate('dashboard');reload();},true);
 }
 reload();
}
const number=value=>Number(value??0).toLocaleString('fr-CH');
const fact=(label,value,kind,action,note)=>({label,value,kind,action,note});
function renderMetrics(){const apps=state.apps,fu=state.followups,rv=state.review,sc=state.search;
 const items=apps?.items||[];const pending=Number(fu?.summary?.overdue||0)+Number(fu?.summary?.dueToday||0);
 const interviews=items.filter(x=>validDate(x.interviewAt)&&x.interviewAt.slice(0,10)>=today()&&!['rejected','withdrawn'].includes(x.status)).length;
 const cards=[
  fact('Publiées (7 jours)',sc?number(sc.matched):'—','blue','search','Dates de publication connues, toutes sources'),
  fact('À examiner',rv?number(rv.total):'—','blue','review','Toutes les offres découvertes non traitées'),
  fact('À postuler',apps?number(items.filter(x=>x.status==='new').length):'—','normal','tracker','Candidatures approuvées mais non envoyées'),
  fact('Envoyées (historique)',apps?number(items.filter(x=>validDate(x.appliedDate)).length):'—','green','tracker','Date d’envoi effectivement enregistrée'),
  fact('Relances dues',fu?number(pending):'—','amber','followups','Aujourd’hui ou en retard · relances ouvertes'),
  fact('Entretiens prévus',apps?number(interviews):'—','purple','followups','Date future enregistrée dans Applications'),
 ];
 const root=el('s5-metrics');root.replaceChildren();for(const f of cards){const btn=document.createElement('button');btn.type='button';btn.className=`s5-metric ${f.kind}`;btn.innerHTML=`<span>${esc(f.label)}</span><strong>${esc(f.value)}</strong><small>${esc(f.note)}</small>`;btn.addEventListener('click',()=>navigate(f.action));root.append(btn);}
}
function addAction(node,kind,label,headline,meta,handler){const b=document.createElement('button');b.type='button';b.className='s5-action';b.innerHTML=`<span class="s5-priority ${esc(kind)}">${esc(label)}</span><span class="s5-action-text"><strong>${esc(headline)}</strong><small>${esc(meta)}</small></span><span class="s5-arrow">›</span>`;b.addEventListener('click',handler);node.append(b);}
function renderActions(){const root=el('s5-action-list');root.replaceChildren();const app=state.apps?.items||[],fu=state.followups?.items||[],t=today(),list=[];
 for(const x of fu){if(x.state!=='open'||!validDate(x.dueDate))continue;if(x.dueDate<t)list.push({priority:0,kind:'late',tag:'En retard',name:x.title,meta:`${x.company} · relance ${x.dueDate}`,id:x.jobId,route:'followups'});else if(x.dueDate===t)list.push({priority:1,kind:'due',tag:"Aujourd'hui",name:x.title,meta:`${x.company} · relance aujourd'hui`,id:x.jobId,route:'followups'});}
 for(const x of app){if(!validDate(x.interviewAt)||x.interviewAt.slice(0,10)<t||['rejected','withdrawn'].includes(x.status))continue;list.push({priority:2,kind:'interview',tag:'Entretien',name:x.title,meta:`${x.company} · ${x.interviewAt.replace('T',' ')}`,id:x.jobId,route:'app'});}
 for(const x of app.filter(x=>x.status==='new').slice(0,10))list.push({priority:3,kind:'apply',tag:'À postuler',name:x.title,meta:`${x.company} · ${x.place}`,id:x.jobId,route:'app'});
 list.sort((a,b)=>a.priority-b.priority||a.meta.localeCompare(b.meta,'fr'));
 for(const a of list.slice(0,8))addAction(root,a.kind,a.tag,a.name,a.meta,()=>a.route==='app'?openApplication(a.id):navigate('followups'));
 if(state.review?.total>0)addAction(root,'review','Review','Examiner les offres découvertes',`${number(state.review.total)} offre(s) non traitée(s)`,()=>navigate('review'));
 if(!root.children.length){const msg=document.createElement('p');msg.className='s5-empty';msg.textContent='Aucune action urgente dans les dossiers enregistrés.';root.append(msg);}
}
function eventList(){const all=[];for(const a of state.apps?.items||[]){
 if(validDate(a.appliedDate))all.push({date:a.appliedDate.slice(0,10),type:'applied',text:'Candidature envoyée',app:a});
 if(validDate(a.interviewAt))all.push({date:a.interviewAt.slice(0,10),type:'interview',text:'Entretien',app:a});
 }
 const byId=new Map((state.apps?.items||[]).map(x=>[x.jobId,x]));for(const x of state.followups?.items||[]){if(!validDate(x.dueDate)||!['open','completed'].includes(x.state))continue;const a=byId.get(x.jobId)||x;all.push({date:x.dueDate,type:x.state==='completed'?'completed':'due',text:x.state==='completed'?'Relance effectuée':'Relance à effectuer',app:a});}
 return all;
}
function shift(m){state.month=new Date(state.month.getFullYear(),state.month.getMonth()+m,1);state.picked=iso(state.month);drawCalendar();}
function drawCalendar(){const grid=el('s5-month-grid');if(!grid)return;grid.replaceChildren();const ev=eventList();const sums=new Map();for(const e of ev){if(!sums.has(e.date))sums.set(e.date,[]);sums.get(e.date).push(e);}
 const y=state.month.getFullYear(),m=state.month.getMonth(),days=new Date(y,m+1,0).getDate(),pad=(new Date(y,m,1).getDay()+6)%7;
 el('s5-month').textContent=state.month.toLocaleDateString('fr-CH',{month:'long',year:'numeric'});
 for(let i=0;i<pad;i++){const blank=document.createElement('div');blank.className='s5-day empty';grid.append(blank);}
 for(let n=1;n<=days;n++){const date=iso(new Date(y,m,n)),items=sums.get(date)||[],btn=document.createElement('button');btn.type='button';btn.className='s5-day'+(date===today()?' today':'')+(date===state.picked?' picked':'');btn.setAttribute('aria-label',`${date}: ${items.length} activité(s)`);btn.innerHTML=`<span>${n}</span><span class="s5-dots">${[...new Set(items.map(e=>e.type))].slice(0,4).map(k=>`<i class="${esc(k)}"></i>`).join('')}</span>`;btn.addEventListener('click',()=>{state.picked=date;drawCalendar();});grid.append(btn);}
 drawSelected(sums.get(state.picked)||[]);
}
function drawSelected(events){const root=el('s5-events');root.replaceChildren();const title=document.createElement('strong');title.textContent=state.picked===today()?"Aujourd'hui · "+state.picked:state.picked;root.append(title);
 if(!events.length){const p=document.createElement('p');p.className='hint';p.textContent='Aucune activité enregistrée ce jour.';root.append(p);return;}
 for(const e of events){const b=document.createElement('button');b.type='button';b.className='s5-event';b.innerHTML=`<span class="s5-event-dot ${esc(e.type)}"></span><span><strong>${esc(e.text)}</strong><small>${esc(e.app.company||'')} · ${esc(e.app.title||'')}</small></span>`;b.addEventListener('click',()=>openApplication(e.app.jobId));root.append(b);}
}
function monday(d){const x=new Date(d.getFullYear(),d.getMonth(),d.getDate());x.setDate(x.getDate()-(x.getDay()+6)%7);return x;}
function renderWeekly(){const root=el('s5-weekbars');root.replaceChildren();if(!state.apps){root.innerHTML='<p class="hint">Historique des candidatures indisponible.</p>';return;}
 const current=monday(new Date()),keys=[];for(let i=7;i>=0;i--){const d=new Date(current);d.setDate(d.getDate()-i*7);keys.push(iso(d));}
 const counts=new Map(keys.map(k=>[k,0]));for(const x of state.apps.items){const d=validDate(x.appliedDate);if(!d)continue;const k=iso(monday(d));if(counts.has(k))counts.set(k,counts.get(k)+1);}
 const max=Math.max(1,...counts.values());for(const [date,count] of counts){const b=document.createElement('div');b.className='s5-bar-unit';const week=dayDate(date);b.innerHTML=`<b>${count}</b><div class="s5-bar-rail"><div style="height:${Math.max(count>0?5:0,count/max*100)}%" class="s5-bar-fill"></div></div><small>${week.toLocaleDateString('fr-CH',{day:'2-digit',month:'2-digit'})}</small>`;b.title=`Semaine du ${date} : ${count} candidature(s) envoyée(s)`;root.append(b);}
}
function renderStages(){const root=el('s5-stage-bars');root.replaceChildren();if(!state.apps){root.innerHTML='<p class="hint">Données d’applications indisponibles.</p>';return;}
 const defs=[['new','À postuler'],['applied','Envoyées / en attente'],['screening','Réponse / sélection'],['interview','Entretien'],['offer','Offre'],['rejected','Refus'],['withdrawn','Retrait']];const counts=new Map(defs.map(([k])=>[k,0]));for(const a of state.apps.items){if(counts.has(a.status))counts.set(a.status,counts.get(a.status)+1);}
 const max=Math.max(1,...counts.values());for(const [status,label] of defs){const n=counts.get(status);const line=document.createElement('div');line.className='s5-stage';line.innerHTML=`<span>${esc(label)}</span><div><i style="width:${n/max*100}%"></i></div><strong>${number(n)}</strong>`;root.append(line);}
}
function renderRecent(){const root=el('s5-recent');root.replaceChildren();if(!state.apps){root.innerHTML='<p class="hint">Activité indisponible.</p>';return;}
 const seen=[];for(const x of state.apps.items)for(const e of (x.history||[])){if(!validDate(e.date))continue;seen.push({date:e.date,stage:e.to||e.status||'',app:x});}
 seen.sort((a,b)=>b.date.localeCompare(a.date));for(const x of seen.slice(0,8)){const b=document.createElement('button');b.type='button';b.className='s5-recent-row';b.innerHTML=`<span><strong>${esc(x.app.title)}</strong><small>${esc(x.app.company)} · ${esc(x.stage)}</small></span><time>${esc(x.date)}</time>`;b.addEventListener('click',()=>openApplication(x.app.jobId));root.append(b);}
 if(!root.children.length)root.innerHTML='<p class="s5-empty">Aucune transition datée enregistrée.</p>';
}
function renderSources(){const root=el('s5-source-state');root.replaceChildren();let saved={};try{saved=JSON.parse(localStorage.getItem('jobup_collection_diagnostics_v1')||'{}');if(!saved||typeof saved!=='object'||Array.isArray(saved))saved={};}catch{}
 const status={Success:'Opérationnel',Working:'Opérationnel',Partial:'Partiel',Blocked:'Bloqué',Failed:'Échec',Unavailable:'Indisponible',Cancelled:'Annulé',Running:'En cours'};
 for(const name of sourceNames){const d=saved[name]||{},s=d.status||'NotChecked';const row=document.createElement('div');row.className='s5-source';const cell=document.createElement('span');cell.className='s5-source-dot '+({Success:'ok',Working:'ok',Partial:'warn',Blocked:'bad',Failed:'bad',Unavailable:'unknown',Running:'warn'}[s]||'unknown');const left=document.createElement('strong');left.textContent=name;const right=document.createElement('span');right.textContent=status[s]||'Non vérifiée';row.append(cell,left,right);const timestamp=d.attemptedAt;if(timestamp&&!Number.isNaN(Date.parse(timestamp)))row.title=`Dernier essai : ${new Date(timestamp).toLocaleString('fr-CH')}`;if(d.message)row.title+=(row.title?'\n':'')+String(d.message);root.append(row);}
}
async function reload(){if(!state.ready||state.loading)return;state.loading=true;const btn=el('s5-reload');btn.disabled=true;el('s5-updated').textContent='Chargement depuis SQLite…';
 const queries=[['apps','applications_step3_list',{}],['followups','followups_step4_list',{}],['review','review_step2_inspect',{filter:{scope:'all',decision:'pending',limit:1,offset:0}}],['search','inspect_search_results',{filters:{sources:[],keywords:'',location:'',daysBack:7,includeUnknownDates:false,classification:'',limit:1}}]];
 const settled=await Promise.allSettled(queries.map(([,cmd,args])=>invoke(cmd,args)));const errors=[];
 for(let i=0;i<queries.length;i++){const [key]=queries[i],r=settled[i];state[key]=r.status==='fulfilled'?r.value:null;if(r.status==='rejected')errors.push(`${key}: ${String(r.reason).slice(0,110)}`);}
 renderMetrics();renderActions();drawCalendar();renderWeekly();renderStages();renderRecent();renderSources();
 const partial=(state.apps?.truncated||state.followups?.truncated||(state.search?.scanned>10000));el('s5-integrity').textContent=[partial?'Données partielles : limites de lecture atteintes. Les dossiers conservés dans SQLite restent intacts.':'',...errors.map(e=>'Source indisponible : '+e)].filter(Boolean).join(' · ');
 el('s5-updated').textContent=errors.length?'Données partielles':`Actualisé à ${new Date().toLocaleTimeString('fr-CH',{hour:'2-digit',minute:'2-digit'})}`;btn.disabled=false;state.loading=false;
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
