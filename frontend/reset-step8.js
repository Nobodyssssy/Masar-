/* Jobup: deliberately destructive clear-all-test-data flow with verified backup. */
(()=>{'use strict';
const el=id=>document.getElementById(id);
const invoke=(name,args={})=>{const f=window.__TAURI__?.core?.invoke;if(!f)throw Error('Commandes Tauri indisponibles');return f(name,args);};
const markup=`<div class="s8-danger-zone"><h4>Zone de réinitialisation</h4><p>Efface toutes les annonces, candidatures, décisions, notes, historiques, relances, profils enregistrés et diagnostics locaux. Cette action ne peut pas être annulée depuis l'interface. Une sauvegarde SQLite est créée automatiquement avant la suppression.</p><p class="s8-important">Les sauvegardes et fichiers JSON déjà exportés sur le disque ne sont pas supprimés.</p><button type="button" id="s8-reset-open" class="s8-danger-button">Effacer toutes les données de test…</button></div>
<dialog class="s8-confirm-dialog" id="s8-dialog" aria-labelledby="s8-confirm-title" aria-describedby="s8-confirm-description">
  <h3 id="s8-confirm-title">Supprimer toutes les données Jobup ?</h3>
  <p id="s8-confirm-description">Cela effacera les offres collectées (même récentes), le cache, les candidatures, notes, relances, historiques, paramètres et profils. L'action est définitive dans l'application. <strong>Une sauvegarde SQLite sera créée avant la suppression.</strong></p>
  <label class="s8-ack"><input type="checkbox" id="s8-understand"> J'ai compris que les candidatures et leurs historiques seront supprimés.</label>
  <label class="s8-type">Tapez <strong>EFFACER</strong> pour confirmer <input id="s8-confirm-input" autocomplete="off" placeholder="EFFACER" spellcheck="false"></label>
  <div class="s8-dialog-actions"><button type="button" id="s8-cancel">Annuler</button><button type="button" id="s8-commit" class="s8-danger-button" disabled>Confirmer la suppression</button></div>
  <p class="s8-dialog-status" id="s8-dialog-status" role="status" aria-live="polite"></p>
</dialog>`;
let deleting=false;
function activityInProgress(){
  // The normal Refresh Jobs workflow and the Step 1 Search workflow.
  let legacy=false;try{legacy=typeof refreshInProgress!=='undefined'&&refreshInProgress;}catch{}
  const search=el('search-run'),cancel=el('search-cancel');
  return legacy||(!!search&&!!cancel&&search.disabled&&!cancel.disabled);
}
function localCleanup(){
 const keys=[];
 for(let i=0;i<localStorage.length;i++){const key=localStorage.key(i);if(key&&key.startsWith('jobup_'))keys.push(key);}
 for(const key of keys)localStorage.removeItem(key);
 // Diagnostics also exist as in-memory globals until the page reloads.
}
function mount(){const settings=el('s7-data');if(!settings)return;
 if(el('s8-reset-open'))return;
 settings.insertAdjacentHTML('beforeend',markup);
 const cleanup=document.createElement('button');cleanup.type='button';cleanup.textContent='Nettoyer les annonces de plus de 90 jours sans historique';
 settings.append(cleanup);
 const browserSetup=document.createElement('button');browserSetup.type='button';browserSetup.textContent='Installer / réparer Firefox pour les collecteurs';settings.append(browserSetup);
 browserSetup.addEventListener('click',async()=>{if(deleting||activityInProgress())return;browserSetup.disabled=true;el('s7-message').textContent='Installation de Firefox en cours (connexion Internet nécessaire, délai maximum 5 minutes)…';try{el('s7-message').textContent=await invoke('setup_firefox_collector');}catch(e){el('s7-message').textContent=String(e);}finally{browserSetup.disabled=false;}});

 cleanup.addEventListener('click',async()=>{
   if(activityInProgress()||deleting)return;
   if(!confirm('Supprimer les annonces collectées il y a plus de 90 jours sans candidature liée, note, décision, historique de tri ou lien de doublon ? Une sauvegarde vérifiée sera créée. Les candidatures et historiques restent conservés.'))return;
   cleanup.disabled=true;deleting=true;
   try{const report=await invoke('cleanup_scraped_cache',{days:90});el('s7-message').textContent=`${report.removed} annonces supprimées. Sauvegarde : ${report.backupPath}`;}
   catch(e){el('s7-message').textContent='Nettoyage annulé : '+e;}
   finally{cleanup.disabled=false;deleting=false;}
 });
 const dialog=el('s8-dialog'),button=el('s8-commit'),typed=el('s8-confirm-input'),checked=el('s8-understand'),message=el('s8-dialog-status');
 const update=()=>{button.disabled=deleting||!checked.checked||typed.value!=='EFFACER';};
 el('s8-reset-open').addEventListener('click',()=>{
   if(activityInProgress()){const notice=el('s7-message');if(notice)notice.textContent='Terminez ou annulez la collecte en cours avant de réinitialiser les données.';return;}
   typed.value='';checked.checked=false;message.textContent='';update();dialog.showModal();typed.focus();
 });
 typed.addEventListener('input',update);checked.addEventListener('change',update);
 el('s8-cancel').addEventListener('click',()=>{if(!deleting)dialog.close();});
 dialog.addEventListener('cancel',e=>{if(deleting)e.preventDefault();});
 button.addEventListener('click',async()=>{
   if(deleting||!checked.checked||typed.value!=='EFFACER')return;
   if(activityInProgress()){message.textContent='Collecte en cours. Veuillez terminer ou annuler la collecte.';return;}
   // Ensure browser storage is available before mutating the database.
   try{void localStorage.length;}catch{message.textContent='Stockage local indisponible : réinitialisation annulée.';return;}
   deleting=true;update();el('s8-cancel').disabled=true;message.textContent='Création de la sauvegarde et nettoyage SQLite…';
   try{
     const result=await invoke('reset_all_jobup_data',{confirmation:'EFFACER'});
     if(!result||!result.backupPath)throw Error('Aucune confirmation de sauvegarde reçue');
     message.textContent=`Base vidée (${result.rowsCleared} lignes). Sauvegarde : ${result.backupPath}. Nettoyage des caches…`;
     localCleanup();
     // Do not allow old in-memory data to repopulate SQLite. Reload immediately.
     window.location.reload();
   }catch(err){message.textContent='Réinitialisation incomplète ou annulée : '+String(err)+' — si une sauvegarde est indiquée, conservez-la.';
     deleting=false;el('s8-cancel').disabled=false;update();}
 });
}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',mount,{once:true});else mount();
})();
