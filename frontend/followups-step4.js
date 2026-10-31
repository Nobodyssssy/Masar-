/* Step 4/7 — Follow-ups. No automated email or network access. */
(()=>{'use strict';
const invoke=(name,args={})=>{const fn=window.__TAURI__?.core?.invoke;if(!fn)throw Error('Jobup/Tauri requis pour les relances');return fn(name,args);};
const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const byId=id=>document.getElementById(id);
const isoLocal=()=>{const d=new Date();return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`};
const dateOf=iso=>{if(!/^\d{4}-\d\d-\d\d$/.test(iso||''))return null;const [y,m,d]=iso.split('-').map(Number);const obj=new Date(y,m-1,d);return obj.getFullYear()===y&&obj.getMonth()===m-1&&obj.getDate()===d?obj:null;};
const isoOf=date=>`${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,'0')}-${String(date.getDate()).padStart(2,'0')}`;
const frDate=iso=>{const d=dateOf((iso||'').slice(0,10));return d?d.toLocaleDateString('fr-CH',{day:'numeric',month:'long',year:'numeric'}):'Date inconnue';};
const states={open:'À relancer',completed:'Terminée',dismissed:'Sans relance',paused:'Réponse / dossier avancé',unscheduled:'Date manquante'};
const actions={complete:'Relance effectuée',dismiss:'Relance inutile',postpone:'Reportée',reopen:'Réactivée'};
const shell=`<section class="view followups4" id="view-followups-step4">
<div class="page-heading"><div><h2>Relances et rendez-vous</h2><p>Actions à effectuer · Candidatures suivies · Aucun email envoyé automatiquement</p></div><div class="page-toolbar"><button type="button" id="fu4-reload">Actualiser</button><button type="button" id="fu4-go-apps">Mes candidatures</button></div></div>
<div class="fu4-stats" id="fu4-stats" aria-label="Résumé des relances"></div>
<div class="fu4-preferences"><strong>Règle par défaut</strong><label><input type="number" id="fu4-delay" min="1" max="30" value="5"> jours</label><select id="fu4-mode"><option value="business">ouvrables (lundi–vendredi)</option><option value="calendar">calendaires</option></select><button type="button" id="fu4-settings-save">Enregistrer la règle</button><small>Les jours fériés cantonaux ne sont pas calculés. Les reports effectués ici n’écrasent pas la date initiale conservée dans Applications.</small></div>
<div class="fu4-tabs" role="tablist" aria-label="Vue du calendrier"><button type="button" data-fu-view="month" class="active">Mois</button><button type="button" data-fu-view="week">Semaine</button><button type="button" data-fu-view="agenda">Agenda</button></div>
<div class="fu4-calendar card"><div class="fu4-month-head"><button type="button" id="fu4-prev" aria-label="Période précédente">‹</button><strong id="fu4-month-label"></strong><button type="button" id="fu4-next" aria-label="Période suivante">›</button><button type="button" id="fu4-today">Aujourd’hui</button><small><span class="fu4-dot due"></span>Relance <span class="fu4-dot interview"></span>Entretien <span class="fu4-dot applied"></span>Envoi <span class="fu4-dot completed"></span>Terminée</small></div><div id="fu4-cal-grid" class="fu4-cal-grid"></div><div id="fu4-day-info" class="fu4-day-info" aria-live="polite"></div></div>
<div class="fu4-toolbar"><label>Afficher <select id="fu4-filter"><option value="pending">Relances ouvertes</option><option value="overdue">En retard</option><option value="today">Aujourd’hui</option><option value="upcoming">À venir</option><option value="completed">Terminées</option><option value="dismissed">Inutiles</option><option value="paused">Suspendues</option><option value="all">Toutes les candidatures</option></select></label><label>Rechercher <input id="fu4-search" placeholder="Entreprise, poste ou commune" type="search"></label><span id="fu4-count"></span></div>
<div class="fu4-workspace"><div class="fu4-list" id="fu4-list" role="listbox" aria-label="Liste des relances"></div><aside id="fu4-detail" class="fu4-detail" aria-live="polite"><p>Choisissez une candidature à relancer.</p></aside></div><div class="fu4-message" id="fu4-message" role="status" aria-live="polite"></div>
</section>`;
let initialized=false,fetching=false,working=false,items=[],settings={delayDays:5,delayMode:'business'},selected='',displayMode='month',cursor=new Date(),focusDay='',messageToken=0;
cursor=new Date(cursor.getFullYear(),cursor.getMonth(),1);
const getSelected=()=>items.find(v=>v.jobId===selected);
const currentStatus=item=>{if(item.state==='open'&&item.dueDate){const today=isoLocal();return item.dueDate<today?'overdue':item.dueDate===today?'today':'upcoming';}return item.state;};
const msg=(s,error=false)=>{const x=byId('fu4-message');if(!x)return;x.textContent=String(s);x.classList.toggle('error',error);};
const showApp=id=>{if(typeof window.jobupOpenApplication==='function')window.jobupOpenApplication(id);else window.jobupNavigate?.('tracker');};
function mount(){if(initialized)return;const main=document.querySelector('main');if(!main)return;
 main.insertAdjacentHTML('beforeend',shell);
 const side=document.querySelector('.sidebar-nav');const b=document.createElement('button');b.type='button';b.dataset.page='followups';b.className=side?'side-link':'tab';b.id='fu4-side';b.innerHTML=side?'<span class="side-icon">◷</span> Relances':'Relances';
 if(side){const apps=side.querySelector('[data-page="tracker"]');if(apps)apps.after(b);else side.append(b);}else{document.querySelector('header .tabs')?.append(b);}b.addEventListener('click',navigate);
 const original=window.jobupNavigate;
 window.jobupNavigate=(page)=>['followups','followups-step4'].includes(page)?navigate():typeof original==='function'?original(page):undefined;
 byId('fu4-reload').addEventListener('click',refresh);
 byId('fu4-go-apps').addEventListener('click',()=>window.jobupNavigate?.('tracker'));
 byId('fu4-settings-save').addEventListener('click',saveSettings);
 byId('fu4-filter').addEventListener('change',renderList);byId('fu4-search').addEventListener('input',renderList);
 for(const b of document.querySelectorAll('[data-fu-view]'))b.addEventListener('click',()=>{displayMode=b.dataset.fuView;document.querySelectorAll('[data-fu-view]').forEach(x=>x.classList.toggle('active',x===b));renderCalendar();});
 byId('fu4-prev').addEventListener('click',()=>shift(-1));byId('fu4-next').addEventListener('click',()=>shift(1));
 byId('fu4-today').addEventListener('click',()=>{const today=new Date();cursor=new Date(today.getFullYear(),today.getMonth(),1);focusDay=isoLocal();renderCalendar();});
 initialized=true;
 mountDashboardPreview();
}
function navigate(){mount();if(!initialized)return;document.querySelectorAll('.view').forEach(v=>v.classList.toggle('active',v.id==='view-followups-step4'));document.querySelectorAll('.side-link,.tab').forEach(v=>v.classList.toggle('active',v.dataset.page==='followups'||v.id==='fu4-side'));const title=byId('desktop-page-title');if(title)title.textContent='Relances';refresh();}
async function refresh(){if(fetching)return;fetching=true;msg('Chargement des échéances depuis SQLite…');try{
 const data=await invoke('followups_step4_list');items=data.items||[];settings=data.settings||settings;
 byId('fu4-delay').value=settings.delayDays;byId('fu4-mode').value=settings.delayMode;
 let stats=data.summary||{};const values=[['En retard',stats.overdue,'urgent'],['Aujourd’hui',stats.dueToday,'warning'],['À venir',stats.upcoming,''],['Terminées',stats.completed,'success'],['Suspendues',stats.pausedAfterReply,''],['Entretiens',stats.interviews,'info']];
 byId('fu4-stats').innerHTML=values.map(([label,n,tone])=>`<div class="fu4-stat ${tone}"><strong>${Number(n||0)}</strong><small>${esc(label)}</small></div>`).join('');
 if(!items.some(x=>x.jobId===selected))selected=items.find(x=>x.state==='open')?.jobId||items[0]?.jobId||'';
 if(data.truncated)msg('Plus de 5 000 candidatures. Liste partielle : les dossiers plus anciens restent sauvegardés.',true);else msg(`${items.length} candidature(s) avec historique d’envoi.`);
 renderCalendar();renderList();renderDetail();renderDashboard();
}catch(err){msg('Impossible de charger les relances : '+err,true);}finally{fetching=false;}}
function filtered(){const f=byId('fu4-filter').value,q=byId('fu4-search').value.toLocaleLowerCase('fr').trim();return items.filter(item=>{
 const s=currentStatus(item);
 if(f==='pending'&&item.state!=='open')return false;
 if(f==='overdue'&&s!=='overdue')return false;
 if(f==='today'&&s!=='today')return false;
 if(f==='upcoming'&&s!=='upcoming')return false;
 if(['completed','dismissed','paused'].includes(f)&&item.state!==f)return false;
 return !q||(item.title+' '+item.company+' '+item.place).toLocaleLowerCase('fr').includes(q);
}).sort((a,b)=>{const rank=v=>currentStatus(v)==='overdue'?0:currentStatus(v)==='today'?1:2;return rank(a)-rank(b)||String(a.dueDate||'9999').localeCompare(String(b.dueDate||'9999'))||a.title.localeCompare(b.title,'fr');});}
function renderList(){if(!initialized)return;const rows=filtered();byId('fu4-count').textContent=`${rows.length} dossier(s)`;const root=byId('fu4-list');root.replaceChildren();if(!rows.length){root.innerHTML='<p class="fu4-empty">Aucune relance pour les filtres choisis.</p>';return;}
 for(const x of rows){const node=document.createElement('button');node.type='button';node.className='fu4-row'+(x.jobId===selected?' selected':'');node.setAttribute('role','option');node.setAttribute('aria-selected',String(x.jobId===selected));const s=currentStatus(x);
 node.innerHTML=`<strong>${esc(x.title||'Offre')}</strong><small>${esc(x.company)} · ${esc(x.place)}</small><span class="fu4-row-foot"><em class="fu4-status ${esc(s)}">${esc(s==='overdue'?'En retard':s==='today'?"Aujourd’hui":s==='upcoming'?'À venir':states[x.state]||x.state)}</em><span>${esc(x.dueDate||'—')}</span></span>`;
 node.addEventListener('click',()=>{selected=x.jobId;renderList();renderDetail();});root.append(node);}}
function addEvent(events,date,type,item){if(!dateOf(date))return;events.push({date,type,item});}
function calendarEvents(){const out=[];for(const x of items){if(x.state==='open'&&x.dueDate)addEvent(out,x.dueDate,'due',x);if(x.state==='completed'&&x.dueDate)addEvent(out,x.dueDate,'completed',x);if(x.interviewAt)addEvent(out,x.interviewAt.slice(0,10),'interview',x);if(x.appliedDate)addEvent(out,x.appliedDate,'applied',x);}return out;}
function dayContent(date,events){const day=events.filter(x=>x.date===date);return day.map(v=>`<button class="fu4-day-row" data-job="${esc(v.item.jobId)}" type="button"><span class="fu4-dot ${esc(v.type)}"></span>${esc(v.item.company)} · ${esc(v.item.title)} <small>${v.type==='due'?'Relance':v.type==='completed'?'Relance effectuée':v.type==='interview'?'Entretien':'Envoi'}</small></button>`).join('')||'<p class="hint">Aucune activité enregistrée pour ce jour.</p>';}
function daySelect(date){focusDay=date;renderCalendar();}
function renderCalendar(){if(!initialized)return;const ev=calendarEvents(),root=byId('fu4-cal-grid');root.replaceChildren();const first=new Date(cursor.getFullYear(),cursor.getMonth(),1),today=isoLocal();
 let start,end;if(displayMode==='month'){
 start=new Date(first);start.setDate(start.getDate()-(start.getDay()+6)%7);end=new Date(first.getFullYear(),first.getMonth()+1,0);const remaining=(7-((end.getDay()+6)%7)-1);end.setDate(end.getDate()+remaining);
 }else if(displayMode==='week'){
 const anchor=dateOf(focusDay)||new Date(cursor.getFullYear(),cursor.getMonth(),1);start=new Date(anchor);start.setDate(start.getDate()-(start.getDay()+6)%7);end=new Date(start);end.setDate(end.getDate()+6);
 }else{start=new Date(first);end=new Date(first.getFullYear(),first.getMonth()+1,0);}
 byId('fu4-month-label').textContent=displayMode==='week'?`${frDate(isoOf(start))} – ${frDate(isoOf(end))}`:first.toLocaleDateString('fr-CH',{month:'long',year:'numeric'});
 root.className='fu4-cal-grid '+displayMode;
 if(displayMode!=='agenda'){for(const day of ['Lun','Mar','Mer','Jeu','Ven','Sam','Dim']){const th=document.createElement('div');th.className='fu4-dow';th.textContent=day;root.append(th);}}
 for(let d=new Date(start);d<=end;d.setDate(d.getDate()+1)){
 const ds=isoOf(d),day=ev.filter(e=>e.date===ds),counts={due:0,interview:0,applied:0};for(const e of day)counts[e.type]++;
 if(displayMode==='agenda'){
  if(!day.length)continue;const node=document.createElement('button');node.type='button';node.className='fu4-agenda-day';node.innerHTML=`<b>${esc(frDate(ds))}</b><span>${day.map(e=>`<span class="fu4-dot ${esc(e.type)}"></span>${esc(e.item.company)} · ${e.type==='due'?'Relance':e.type==='completed'?'Terminée':e.type==='interview'?'Entretien':'Candidature'}`).join(' &nbsp; ')}</span>`;node.addEventListener('click',()=>daySelect(ds));root.append(node);continue;
 }
 const node=document.createElement('button');node.type='button';node.className='fu4-day'+(ds===today?' today':'')+(ds===focusDay?' chosen':'')+(d.getMonth()!==first.getMonth()?' outer':'');node.setAttribute('aria-label',`${frDate(ds)} : ${day.length} activité(s)`);
 node.innerHTML=`<span>${d.getDate()}</span><div class="fu4-dots">${Object.entries(counts).filter(([,n])=>n).map(([kind,n])=>`<span class="fu4-dot ${kind}" title="${kind}: ${n}"></span>`).join('')}</div>`;
 node.addEventListener('click',()=>daySelect(ds));root.append(node);
 }
 if(displayMode==='agenda'&&!root.children.length)root.innerHTML='<p class="fu4-empty">Aucun événement pour ce mois.</p>';
 const chosen=focusDay||today;byId('fu4-day-info').innerHTML=`<strong>${esc(frDate(chosen))}</strong>${dayContent(chosen,ev)}`;
 byId('fu4-day-info').querySelectorAll('[data-job]').forEach(b=>b.addEventListener('click',()=>{selected=b.dataset.job;byId('fu4-filter').value='all';renderList();renderDetail();}));
}
function shift(direction){if(displayMode==='week'){
 const day=dateOf(focusDay)||new Date(cursor.getFullYear(),cursor.getMonth(),1);day.setDate(day.getDate()+direction*7);focusDay=isoOf(day);cursor=new Date(day.getFullYear(),day.getMonth(),1);
 }else{cursor=new Date(cursor.getFullYear(),cursor.getMonth()+direction,1);focusDay="";}renderCalendar();}
function draft(x){const posting=x.title||'le poste concerné';const applied=x.appliedDate?` le ${frDate(x.appliedDate)}`:'';const company=x.company?` auprès de ${x.company}`:'';
 const subject=`Suivi de ma candidature – ${posting}`;
 const body=`Bonjour,\n\nJe me permets de revenir vers vous concernant ma candidature au poste de ${posting}${company}, transmise${applied}.\n\nJe reste très intéressé(e) par cette opportunité et souhaitais savoir si vous aviez des informations sur l'avancement du recrutement.\n\nJe reste à votre disposition pour tout renseignement complémentaire.\n\nJe vous remercie de votre retour et vous adresse mes meilleures salutations.\n\n[Votre nom]`;
 return {subject,body};}
function renderDetail(){const d=byId('fu4-detail');if(!d)return;const x=getSelected();if(!x){d.innerHTML='<p class="fu4-empty">Sélectionnez une candidature.</p>';return;}
 const s=currentStatus(x),editable=x.state==='open';
 d.innerHTML=`<small>${esc(x.company)} · ${esc(x.place)}</small><h3>${esc(x.title)}</h3><div class="fu4-detail-meta"><span class="fu4-status ${esc(s)}">${esc(s==='overdue'?'En retard':s==='today'?"Aujourd’hui":s==='upcoming'?'À venir':states[x.state])}</span><span>Envoyée : ${esc(x.appliedDate||'—')}</span><span>Relance : ${esc(x.dueDate||'—')}</span></div><div class="fu4-detail-actions"><button type="button" id="fu4-open-app">Ouvrir la candidature</button><button type="button" id="fu4-open-post" ${/^https:\/\//.test(x.listingUrl)?'':'disabled'}>Annonce originale ↗</button></div>
 ${x.interviewAt?`<p class="fu4-notice">Entretien enregistré : ${esc(x.interviewAt.replace('T',' '))}</p>`:''}
 ${x.state==='paused'?'<p class="fu4-notice">Une réponse ou nouvelle étape a été enregistrée. La relance est suspendue, sans modifier le statut de la candidature.</p>':''}
 <div class="fu4-action-area"><h4>Décision de relance</h4><div class="fu4-actions"><button id="fu4-complete" ${editable?'':'disabled'}>Marquer effectuée</button><button id="fu4-dismiss" ${editable?'':'disabled'}>Inutile</button><button id="fu4-reopen" ${x.state==='completed'||x.state==='dismissed'?'':'disabled'}>Réactiver</button></div><label>Nouvelle échéance <input id="fu4-postpone-date" type="date" min="${esc(isoLocal())}" value="${esc(x.dueDate>=isoLocal()?x.dueDate:isoLocal())}"></label><button id="fu4-postpone" ${editable?'':'disabled'}>Reporter à cette date</button></div>
 <div class="fu4-draft"><h4>Proposition d’email en français (à vérifier avant envoi)</h4><label>Objet<input type="text" id="fu4-draft-subject" maxlength="250"></label><label>Message<textarea id="fu4-draft-text" rows="9"></textarea></label><button type="button" id="fu4-copy">Copier objet + message</button><p>Aucun destinataire n'est inventé. L'email n'est jamais envoyé automatiquement.</p></div><div class="fu4-audit"><h4>Historique des relances</h4><div id="fu4-events" class="hint">Chargement…</div></div>`;
 const templ=draft(x);byId('fu4-draft-subject').value=templ.subject;byId('fu4-draft-text').value=templ.body;
 byId('fu4-open-app').addEventListener('click',()=>showApp(x.jobId));
 byId('fu4-open-post').addEventListener('click',()=>{if(/^https:\/\//.test(x.listingUrl)){if(typeof window.openExternalUrl==='function')window.openExternalUrl(x.listingUrl);else window.open(x.listingUrl,'_blank','noopener,noreferrer');}});
 byId('fu4-complete').addEventListener('click',()=>change('complete',x));byId('fu4-dismiss').addEventListener('click',()=>change('dismiss',x));byId('fu4-reopen').addEventListener('click',()=>change('reopen',x));byId('fu4-postpone').addEventListener('click',()=>change('postpone',x));
 byId('fu4-copy').addEventListener('click',async()=>{const text=`Objet : ${byId('fu4-draft-subject').value}\n\n${byId('fu4-draft-text').value}`;try{await navigator.clipboard.writeText(text);msg('Brouillon copié. Envoyez-le vous-même après vérification.');}catch{byId('fu4-draft-text').focus();byId('fu4-draft-text').select();msg('Copie indisponible : sélectionnez et copiez manuellement le texte.');}});
 loadEvents(x.jobId);
}
async function loadEvents(id){try{const rows=await invoke('followups_step4_events',{jobId:id});if(selected!==id)return;byId('fu4-events').innerHTML=rows.length?rows.map(e=>`<div class="fu4-event"><b>${esc(actions[e.action]||e.action)}</b><small>${esc(e.recordedAt)} · ${esc(e.current?.dueDate||'')}</small></div>`).join(''):'Aucune décision de relance enregistrée.';}catch(err){if(selected===id)byId('fu4-events').textContent='Historique indisponible : '+err;}}
async function change(action,x){if(working)return;const date=byId('fu4-postpone-date').value;if(action==='postpone'&&(!date||date<=x.dueDate))return msg('Choisissez une date ultérieure à l’échéance actuelle.',true);
 if(action==='complete'&&!window.confirm('Avez-vous réellement effectué la relance ?'))return;
 if(action==='dismiss'&&!window.confirm('Marquer cette relance comme non nécessaire ? Vous pourrez la réactiver.'))return;
 working=true;try{await invoke('followups_step4_action',{update:{jobId:x.jobId,expectedRevision:x.revision,action,dueDate:action==='postpone'||action==='reopen'?date:null}});await refresh();msg(actions[action]+' · Action enregistrée dans SQLite.');}catch(err){msg('Enregistrement refusé : '+err,true);}finally{working=false;}}
async function saveSettings(){const delay=Number(byId('fu4-delay').value),mode=byId('fu4-mode').value;if(!Number.isInteger(delay)||delay<1||delay>30)return msg('La durée doit être comprise entre 1 et 30 jours.',true);try{await invoke('followups_step4_settings',{settings:{delayDays:delay,delayMode:mode}});await refresh();msg('Règle enregistrée. Les dates déjà définies dans Applications sont conservées.');}catch(err){msg('Paramètres non enregistrés : '+err,true);}}
// Dashboard: compact, genuine monthly calendar remains visible and actionable.
function mountDashboardPreview(){const dashboard=byId('view-dashboard');if(!dashboard||byId('fu4-dashboard'))return;
 const block=document.createElement('div');block.className='fu4-dashboard';block.id='fu4-dashboard';block.innerHTML='<div class="fu4-dash-head"><div><strong>Relances et rendez-vous</strong><p>Calendrier des candidatures enregistrées dans SQLite</p></div><button id="fu4-dash-open" type="button">Gérer les relances</button></div><div id="fu4-dash-main"><p class="hint">Chargement du calendrier…</p></div>';
 const first=dashboard.querySelector('.dash');if(first)first.after(block);else dashboard.prepend(block);
 byId('fu4-dash-open').addEventListener('click',navigate);
 // Refresh dashboard after first native data load; no timers or background monitoring.
 try{invoke('followups_step4_list').then(data=>{items=data.items||[];renderDashboard();}).catch(()=>{byId('fu4-dash-main').innerHTML='<p class="hint">Ouvrez « Relances » pour charger vos échéances.</p>';});}catch{byId('fu4-dash-main').innerHTML='<p class="hint">Le calendrier sera disponible dans l’application Windows.</p>';}
}
function renderDashboard(){const panel=byId('fu4-dash-main');if(!panel)return;
 const today=isoLocal(),first=new Date(),month=new Date(first.getFullYear(),first.getMonth(),1),end=new Date(first.getFullYear(),first.getMonth()+1,0).getDate();
 const events=calendarEvents();const counts={};for(const ev of events){counts[ev.date]=(counts[ev.date]||0)+1;}
 const upcoming=items.filter(x=>x.state==='open'&&x.dueDate).sort((a,b)=>a.dueDate.localeCompare(b.dueDate)).slice(0,5);
 const days=[];for(let n=1;n<=end;n++){const date=isoOf(new Date(month.getFullYear(),month.getMonth(),n));days.push(`<button class="fu4-dash-day ${date===today?'today':''} ${counts[date]?'has-events':''}" data-day="${date}" type="button" title="${counts[date]||0} activité(s)">${n}${counts[date]?'<i></i>':''}</button>`);}
 panel.innerHTML=`<div class="fu4-dashboard-body"><div class="fu4-dash-month"><strong>${esc(month.toLocaleDateString('fr-CH',{month:'long',year:'numeric'}))}</strong><div class="fu4-dash-grid">${days.join('')}</div></div><div class="fu4-dash-queue"><strong>Prochaines actions</strong>${upcoming.length?upcoming.map(x=>`<button type="button" data-job="${esc(x.jobId)}"><span>${esc(x.company)} · ${esc(x.title)}</span><small>${esc(x.dueDate)}${x.dueDate<today?' · En retard':''}</small></button>`).join(''):'<p class="hint">Aucune relance à effectuer.</p>'}</div></div>`;
 panel.querySelectorAll('[data-day]').forEach(b=>b.addEventListener('click',()=>{focusDay=b.dataset.day;cursor=dateOf(focusDay)||month;cursor=new Date(cursor.getFullYear(),cursor.getMonth(),1);navigate();}));
 panel.querySelectorAll('[data-job]').forEach(b=>b.addEventListener('click',()=>{selected=b.dataset.job;navigate();}));
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
