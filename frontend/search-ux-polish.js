/* Jobup: Search collection feedback, without changing native collector dispatch. */
(() => {
  'use strict';
  const byId = id => document.getElementById(id);
  let installed = false;

  function mount() {
    if (installed) return true;
    const page = byId('view-search');
    const form = byId('search-config');
    const panel = byId('search-progress-panel');
    const run = byId('search-run');
    const cancel = byId('search-cancel');
    const progress = byId('search-progress');
    const message = byId('search-progress-text');
    const statusList = byId('search-source-status');
    const providers = byId('search-sources');
    if (![page,form,panel,run,cancel,progress,message,statusList,providers].every(Boolean)) return false;
    installed = true;

    const hero = document.createElement('div');
    hero.id = 'search-ux-hero';
    hero.dataset.state = 'idle';
    hero.innerHTML = '<span class="search-ux-ring" aria-hidden="true"></span>' +
      '<div class="search-ux-hero-content"><strong id="search-ux-title">En attente de recherche</strong>' +
      '<span id="search-ux-detail">Sélectionnez les plateformes et lancez la recherche.</span></div>' +
      '<span id="search-ux-time" aria-label="Temps écoulé">00:00</span>';
    const firstMessage = panel.querySelector('#search-progress-text');
    firstMessage.parentNode.insertBefore(hero, firstMessage);

    const line = document.createElement('div');
    line.id = 'search-ux-summary';
    line.setAttribute('aria-live', 'polite');
    progress.insertAdjacentElement('afterend',line);

    const explain = document.createElement('p');
    explain.className = 'search-ux-website-explain';
    explain.textContent = 'SwissDevJobs et eFinancialCareers : collecteurs Firefox Playwright disponibles. Utilisez les cases a cocher ou les boutons Site officiel.';
    providers.insertAdjacentElement('afterend',explain);

    const title = byId('search-ux-title');
    const detail = byId('search-ux-detail');
    const time = byId('search-ux-time');
    let startedAt = 0;
    let timer = null;
    let active = false;
    let everRan = false;

    const formatTime = ms => {
      const seconds = Math.max(0,Math.floor(ms / 1000));
      const mm = String(Math.floor(seconds / 60)).padStart(2,'0');
      const ss = String(seconds % 60).padStart(2,'0');
      return `${mm}:${ss}`;
    };
    function refresh() {
      const raw = (message.textContent || '').trim();
      const done = Math.max(0,Number(progress.value)||0);
      const total = Math.max(0,Number(progress.max)||0);
      const completed = statusList.querySelectorAll('[data-result-source]').length;
      const match = raw.match(/^Collecte\s+(.+?)\s*\(\s*(\d+)\s*\/\s*(\d+)\s*\)/i);
      if (active) {
        title.textContent = match ? `Collecte : ${match[1]}` : (raw.startsWith('Jobup') ? 'Collecte : Jobup' : 'Recherche en cours');
        detail.textContent = raw || 'Connexion au site...';
        time.textContent = formatTime(Date.now()-startedAt);
        line.textContent = total ? `${done} sur ${total} plateforme(s) traitée(s) | ${completed} rapport(s) disponible(s)` : 'Démarrage de la collecte';
      } else if (everRan) {
        detail.textContent = raw || 'Dernière recherche terminée.';
        line.textContent = total ? `${done} sur ${total} plateforme(s) traitée(s) | ${completed} rapport(s) disponible(s)` : `${completed} rapport(s) disponible(s)`;
      }
    }
    function begin() {
      if (active) return;
      active = true;
      everRan = true;
      startedAt = Date.now();
      hero.dataset.state = 'running';
      panel.classList.add('search-ux-busy');
      title.textContent = 'Connexion aux plateformes...';
      time.textContent = '00:00';
      refresh();
      timer = window.setInterval(refresh,1000);
    }
    function end() {
      if (!active) return;
      active = false;
      if (timer !== null) {window.clearInterval(timer);timer=null;}
      panel.classList.remove('search-ux-busy');
      const raw = message.textContent || '';
      const interrupted = /interrompue|arr[êe]t|annul/i.test(raw);
      const failure = /^Erreur\b|Impossible/i.test(raw);
      hero.dataset.state = failure ? 'error' : interrupted ? 'cancelled' : 'done';
      title.textContent = failure ? 'Recherche en erreur' : interrupted ? 'Recherche interrompue' : 'Recherche terminée';
      refresh();
    }

    // Observe the actual Search controls and text; the visual indicator never
    // replaces or simulates a Tauri command or provider progress event.
    const buttonObserver = new MutationObserver(() => run.disabled ? begin() : end());
    buttonObserver.observe(run,{attributes:true,attributeFilter:['disabled']});
    const messageObserver = new MutationObserver(refresh);
    messageObserver.observe(message,{childList:true,characterData:true,subtree:true});
    const meterObserver = new MutationObserver(refresh);
    meterObserver.observe(progress,{attributes:true,attributeFilter:['value','max']});
    const statusesObserver = new MutationObserver(refresh);
    statusesObserver.observe(statusList,{childList:true,subtree:true,attributes:true,attributeFilter:['data-status']});
    form.addEventListener('submit',() => window.queueMicrotask(() => {
      // The native search module is responsible for deciding whether a search can start.
      if (run.disabled) begin(); else refresh();
    }));
    cancel.addEventListener('click',() => {if (active) {title.textContent='Annulation demandée';detail.textContent='La requête HTTP en cours peut encore se terminer.';}});
    refresh();
    return true;
  }

  // Search is mounted dynamically by Step 1, usually at DOMContentLoaded.
  function bootstrap() {
    if (mount()) return;
    const watcher = new MutationObserver(() => {if (mount()) watcher.disconnect();});
    watcher.observe(document.body,{childList:true,subtree:true});
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded',bootstrap,{once:true});
  else bootstrap();
})();
