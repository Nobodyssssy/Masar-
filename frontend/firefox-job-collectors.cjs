'use strict';
// Jobup: optional headless Firefox collector. Public job pages only.
// No account, cookie injection, anti-bot evasion, or job submissions.
const fs = require('node:fs');
const { URL } = require('node:url');

const MAX_JOBS = 40;
const MAX_DETAILS = 16;
const PAUSE_MS = 750;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function text(x, max = 500) { return String(x || '').replace(/\s+/g, ' ').trim().slice(0, max); }
function fold(x) { return text(x).normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase(); }
function jobMatches(title, keywords) {
  const parts = text(keywords, 220).split(',').map(x => x.trim()).filter(Boolean).slice(0, 3);
  if (!parts.length) return true;
  return parts.some(part => part.split(/\s+/).filter(Boolean).every(word => fold(title).includes(fold(word))));
}
function secureUrl(candidate, site) {
  try {
    const value = new URL(candidate, `https://${site}/`);
    if (value.protocol !== 'https:' || (value.hostname !== site && value.hostname !== `www.${site}`) || value.username || value.password) return null;
    value.hash = '';
    return value.href;
  } catch { return null; }
}
function extractGrades(...values) {
  const v = values.join(' ');
  // Covers: 80%, 80%-100%, 80-100%, and 80–100 %.
  const matches = [...v.matchAll(/\b(\d{1,3})\s*(?:[-–—]\s*(\d{1,3})\s*)?%/g)];
  return [...new Set(matches.flatMap(m => [Number(m[1]), ...(m[2] ? [Number(m[2])] : [])]).filter(n => n >= 0 && n <= 100))].slice(0, 3);
}
function sanitizeDate(value) {
  const match = /^\d{4}-\d{2}-\d{2}/.exec(text(value, 50));
  if (!match) return null;
  const date = match[0];
  const timestamp=Date.parse(`${date}T00:00:00Z`);return Number.isNaN(timestamp)||new Date(timestamp).toISOString().slice(0,10)!==date ? null : date;
}
function argValue(args, name, defaultValue = '') {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] != null ? args[i + 1] : defaultValue;
}
async function cardValue(node, selector, max = 400) {
  try { return text(await node.locator(selector).first().innerText({ timeout: 1200 }), max); }
  catch { return ''; }
}
async function attribute(node, selector, attr) {
  try { return await node.locator(selector).first().getAttribute(attr, { timeout: 1200 }) || ''; }
  catch { return ''; }
}
async function readSwissCard(node) {
  const a = 'a[title][href^="/jobs/"]';
  const href = await attribute(node, a, 'href');
  const url = secureUrl(href, 'swissdevjobs.ch');
  if (!url) return null;
  const raw = text(await attribute(node, a, 'title'), 400);
  const m = raw.match(/^(.*?) job in (.+)$/i);
  const title = text(m ? m[1] : raw, 300);
  if (!title) return null;
  // Don't guess employer from URL slugs: titles and employer tokens are ambiguous.
  const company = await cardValue(node, '[data-test*="company"], [class*="company-name"], [class*="companyName"]', 200);
  const place = text(m ? m[2] : await cardValue(node, '[data-test*="city"], [class*="location"]', 160), 160);
  const workload = await cardValue(node, '[class*="workload"], [class*="employment"]', 80);
  return { title, company, place, url, description: '', source_id: url.split('/jobs/')[1] || '', published_at: null, grades: extractGrades(title, workload) };
}
async function readFinanceCard(node) {
  const title = await cardValue(node, 'h3', 300);
  const href = await attribute(node, 'a[href*="/jobs/"]', 'href');
  const url = secureUrl(href, 'efinancialcareers.ch');
  if (!title || !url) return null;
  const rawPlace = await cardValue(node, '[class*="location"]', 200);
  const place = text(rawPlace.replace(/\b(Festanstellung|Teilzeit|Vertrag|Contract|Freelance|Befristet)\b.*$/i, ''), 160);
  const actualCompany = await cardValue(node, '[class*="company"], [data-testid*="company"]', 200);
  let company = actualCompany;
  if (!company) {
    const lines = (await node.innerText({ timeout: 1200 })).split(/\n/).map(x => text(x)).filter(Boolean);
    const index = lines.findIndex(x => x === 'Speichern');
    if (index >= 0) company = text(lines[index + 1], 200);
  }
  const date = await attribute(node, 'time[datetime]', 'datetime');
  const workload = await cardValue(node, '[class*="employment-type"], [class*="workload"]', 90);
  return { title, company, place, url, description: '', source_id: (url.match(/\.id(\d+)(?:\?|$)/) || [])[1] || url, published_at: sanitizeDate(date), grades: extractGrades(title, workload) };
}
async function detailsSwiss(page, job) {
  const selectors = ['[class*="job-description"]', '[data-test*="description"]', '[class*="description"]', 'article', 'main'];
  for (const selector of selectors) {
    const content = await cardValue(page, selector, 40000);
    if (content.length < 220) continue;
    if ((selector === 'main' || selector === 'article') && !fold(content).includes(fold(job.title).slice(0, 30))) continue;
    return content;
  }
  return '';
}
async function detailsFinance(page, job) {
  // Swiss Job Hunter observes descriptions in embedded application/json branding data.
  const scripts = await page.locator('script[type="application/json"]').all();
  for (const script of scripts.slice(0, 16)) {
    let outer;
    try { outer = JSON.parse(await script.textContent() || ''); } catch { continue; }
    if (!outer || typeof outer !== 'object') continue;
    for (const [key, value] of Object.entries(outer)) {
      if (!key.includes('branding') || !value || typeof value !== 'object') continue;
      let inner = value.body;
      try { if (typeof inner === 'string') inner = JSON.parse(inner); } catch { continue; }
      const raw = inner?.data?.description;
      if (typeof raw !== 'string' || raw.length < 180) continue;
      // DOM parser runs inside the same loaded page; no external HTML dependencies.
      const description = await page.evaluate(html => {
        const div = document.createElement('div'); div.innerHTML = html;
        return div.textContent || '';
      }, raw);
      if (text(description, 40000).length >= 180) return text(description, 40000);
    }
  }
  const description = await cardValue(page, '[class*="job-content"], [class*="job-description"], [class*="description"]', 40000);
  return description.length >= 220 ? description : '';
}
// A source URL is a job only if it matches that site's documented job path.
// Finance jobs often use /jobs-Switzerland-Zurich-....id123, NOT /jobs/.
function normalizedJobUrl(href, source) {
  const host = source === 'swissdevjobs' ? 'swissdevjobs.ch' : 'efinancialcareers.ch';
  const u = secureUrl(href, host);
  if (!u) return null;
  const parsed = new URL(u);
  if (source === 'swissdevjobs') {
    if (!/^\/(?:[a-z]{2}\/)?jobs\/[^/?#]{10,}\/?$/i.test(parsed.pathname)) return null;
  } else if (!/\.id\d{5,}\/?$/i.test(parsed.pathname)) {
    return null;
  }
  // Canonicalizing marketing query parameters avoids duplicate vacancy IDs.
  for(const key of [...parsed.searchParams.keys()])if(/^utm_|^ref$|^source$/i.test(key))parsed.searchParams.delete(key);
  return parsed.href;
}
function matchesLocation(place, location) {
  if (!place || !location.trim()) return true; // Unknown stays unknown, never fabricates city.
  const aliases = location.split(',').map(x => fold(x.trim())).filter(Boolean);
  return aliases.length === 0 || aliases.some(x => fold(place).includes(x) || fold(place).replace(/ue/g,'u').includes(x.replace(/ue/g,'u')));
}
function cleanHtml(value) {
  return text(String(value || '').replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi,' ')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi,' ')
    .replace(/<[^>]*>/g,' ')
    .replace(/&(?:nbsp|#160);/gi,' ').replace(/&amp;/gi,'&')
    .replace(/&quot;/gi,'"').replace(/&#(?:39|x27);/gi,"'")
    .replace(/&lt;/gi,'<').replace(/&gt;/gi,'>'), 40000);
}
function xmlField(item, field) {
  const escaped = field.replace(/[.*+?^${}()|[\]\\]/g,'\\$&');
  const match = new RegExp('<'+escaped+'(?:\\s[^>]*)?>([\\s\\S]*?)<\\/'+escaped+'>','i').exec(item);
  return match ? match[1].trim().replace(/^<!\[CDATA\[([\s\S]*?)\]\]>$/,'$1').trim() : '';
}
function parseSwissRss(xml, keywords, location, days, nowMs = Date.now()) {
  if (!/<rss\b|<feed\b/i.test(xml)) throw new Error('SwissDevJobs RSS was not an RSS document');
  const entries = xml.match(/<item(?:\s[^>]*)?>[\s\S]*?<\/item>/gi) || [];
  if (!entries.length) throw new Error('SwissDevJobs RSS has no job items');
  const out=[];
  for (const entry of entries.slice(0,350)) {
    const rawTitle = cleanHtml(xmlField(entry,'title'));
    const link = normalizedJobUrl(cleanHtml(xmlField(entry,'link')),'swissdevjobs');
    if (!rawTitle || !link) continue;
    const title=text(rawTitle.split(/\s+@\s+/)[0].replace(/\s*\[CHF[^\]]*\]\s*$/i,''),300);
    const companyPart=rawTitle.includes(' @ ') ? rawTitle.split(' @ ').slice(1).join(' @ ').replace(/\s*\[CHF[^\]]*\].*$/i,'').trim() : '';
    const description=cleanHtml(xmlField(entry,'description'));
    const dateStr=xmlField(entry,'pubDate');
    let published_at=null;
    if (dateStr) { const date=new Date(dateStr); if(!Number.isNaN(date.getTime())) published_at=date.toISOString().slice(0,10); }
    // SwissDevJobs RSS can carry long-lived vacancies. Do not label old ones as recent.
    if (published_at && nowMs-new Date(`${published_at}T00:00:00Z`).getTime() > (days+1)*86400000) continue;
    let place='';
    const m=/\b(?:Full-time|Part-time|Permanent employee|Vollzeit|Teilzeit)[^\n]{0,80}?[·•]\s*([^·•\n<]{2,75})/i.exec(description);
    if(m) place=text(m[1],160);
    if (!jobMatches(title,keywords) || !matchesLocation(place,location)) continue;
    out.push({title,company:text(companyPart,200),place,url:link,description,
      source_id:new URL(link).pathname.split('/jobs/').pop(),published_at,
      grades:extractGrades(title,description.slice(0,180))});
    if(out.length>=MAX_JOBS) break;
  }
  return out;
}
async function browserCardSnapshot(page, source) {
  // One DOM round trip instead of many 1.2s selector waits per card.
  return page.evaluate((src) => {
    const t = (v,n=400)=>String(v||'').replace(/\s+/g,' ').trim().slice(0,n);
    const out=[];
    const seen=new Set();
    const selector = src==='swissdevjobs' ? 'a[href*="/jobs/"]' : 'a[href*=".id"], a[href*="/jobs/"]';
    for (const a of [...document.querySelectorAll(selector)].filter(a=>{
      try{const path=new URL(a.getAttribute('href'),location.href).pathname;return src==='swissdevjobs'?/^\/(?:[a-z]{2}\/)?jobs\/[^/?#]{10,}\/?$/i.test(path):/\.id\d{5,}\/?$/i.test(path);}catch{return false;}
    }).slice(0,200)) {
      const href=a.getAttribute('href')||'';
      if(seen.has(href))continue; seen.add(href);
      const card=a.closest('[data-test="card-body"], [data-test*="card"], [class*="job-card"], [class*="jobCard"], article, li') || a.parentElement?.parentElement || a.parentElement;
      const read=selector=>t(card?.querySelector(selector)?.textContent||'');
      const rawTitle=t(a.getAttribute('title'),350);
      const head=read('h3,h2,h4');
      const title=src==='swissdevjobs'
        ? t((rawTitle.match(/^(.*?) job in /i)||[])[1]||head||a.textContent,300)
        : t(head||a.textContent,300);
      if(!title || /^(save|apply|read more|mehr|details|jetzt bewerben|speichern)$/i.test(title))continue;
      const place = src==='swissdevjobs'
        ? t((rawTitle.match(/ job in (.+)$/i)||[])[1] || read('[data-test*="city"],[class*="location"],[class*="city"]'),160)
        : t(read('[class*="location"],[data-testid*="location"]'),160);
      const company=t(read('[data-test*="company"],[class*="company-name"],[class*="companyName"],[class*="company"],[data-testid*="company"]'),200);
      const workload=t(read('[class*="workload"],[class*="employment"]'),90);
      const published=card?.querySelector('time[datetime]')?.getAttribute('datetime')||'';
      out.push({href,title,company,place,workload,published});
    }
    return out;
  },source);
}
const BUDGET_MS=65000;
const NAV_MS=12000;
const DETAIL_MS=6500;
async function gotoBounded(page,url,timeout=NAV_MS) {
  return page.goto(url,{waitUntil:'domcontentloaded',timeout});
}
async function collect(source, keywords, location, days) {
  if (!['swissdevjobs','efinancialcareers'].includes(source)) throw new Error('Unsupported Firefox source');
  if (![1,3,7,14,30].includes(days)) throw new Error('Unsupported days value');
  const { firefox } = require('playwright');
  let browser;
  const started=Date.now();
  const remains=()=>BUDGET_MS-(Date.now()-started);
  const output={ok:true,source,pages:0,details:0,items:[],warnings:[]};
  try {
    browser=await firefox.launch({headless:true,timeout:15000});
    const context=await browser.newContext({locale:'fr-CH',viewport:{width:1440,height:900}});
    const page=await context.newPage();
    page.setDefaultTimeout(2200);
    const target=source==='swissdevjobs'?new URL('https://swissdevjobs.ch/jobs'):new URL('https://www.efinancialcareers.ch/jobs');
    if(source==='swissdevjobs') {
      if(keywords.trim())target.searchParams.set('position',keywords.split(',')[0].trim());
      if(location.trim())target.searchParams.set('city',location.split(',')[0].trim());
    } else {
      const keyword=text(keywords.split(',')[0],140);
      if(keyword)target.pathname=`/jobs/${encodeURIComponent(keyword.toLowerCase().replace(/\s+/g,'-'))}`;
      target.searchParams.set('q',keyword);
      if(location.trim())target.searchParams.set('location',location.split(',')[0].trim());
      for (const [k,v] of Object.entries({countryCode:'CH',radius:'40',radiusUnit:'km',pageSize:'15',currencyCode:'CHF',language:'de',enableVectorSearch:'true'}))target.searchParams.set(k,v);
    }
    const collected=new Map();
    const pagesToCheck=source==='swissdevjobs'?1:2;
    let missingCards=false;
    for(let n=1;n<=pagesToCheck && collected.size<MAX_JOBS && remains()>16000;n++) {
      const url=new URL(target);
      if(source==='efinancialcareers')url.searchParams.set('page',String(n));
      const resp=await gotoBounded(page,url.href,Math.min(NAV_MS,Math.max(1500,remains()-8000)));
      const code=resp?.status()||0;
      if([401,403,429].includes(code))throw new Error(`Access denied (HTTP ${code})`);
      if(code===404 && n>1)break;
      if(code>=400)throw new Error(`Search request failed (HTTP ${code})`);
      const host=new URL(page.url()).hostname;
      const expected=source==='swissdevjobs'?'swissdevjobs.ch':'efinancialcareers.ch';
      if(host!==expected && host!==`www.${expected}`)throw new Error(`Unexpected search host: ${host}`);
      let candidates=[];
      for(let retry=0;retry<4 && remains()>14000;retry++) {
        candidates=await browserCardSnapshot(page,source);
        if(candidates.length)break;
        await sleep(900);
      }
      if(!candidates.length){missingCards=true;break;}
      output.pages++;
      let added=0;
      for(const candidate of candidates) {
        const url=normalizedJobUrl(candidate.href,source);
        if(!url)continue;
        if(!jobMatches(candidate.title,keywords)||!matchesLocation(candidate.place,location))continue;
        if(collected.has(url))continue;
        collected.set(url,{
          title:text(candidate.title,300),company:text(candidate.company,200),place:text(candidate.place,160),url,
          description:'',source_id:source==='swissdevjobs'?new URL(url).pathname.split('/jobs/').pop() : ((url.match(/\.id(\d+)/)||[])[1]||url),
          published_at:sanitizeDate(candidate.published), grades:extractGrades(candidate.title,candidate.workload)
        });
        added++;
        if(collected.size>=MAX_JOBS)break;
      }
      if(!added && n>1)break;
      await sleep(250);
    }
    if(source==='swissdevjobs' && missingCards && remains()>7000) {
      // The official RSS feed is a non-browser fallback for changed SPA selectors.
      try {
        const r=await context.request.get('https://swissdevjobs.ch/rss',{timeout:Math.min(8000,Math.max(1500,remains()-4000))});
        if(!r.ok())throw new Error(`RSS HTTP ${r.status()}`);
        for(const item of parseSwissRss(await r.text(),keywords,location,days))collected.set(item.url,item);
        output.pages=1;
        output.warnings.push('SwissDevJobs search cards unavailable; used its official RSS feed.');
      } catch(e) {throw new Error('No recognizable SwissDevJobs search cards; RSS fallback failed: '+text(e?.message||e,170));}
    }
    if(!output.pages) {
      throw new Error(source==='efinancialcareers'
        ? 'No recognizable eFinancialCareers job links (site layout or availability may have changed)'
        : 'SwissDevJobs returned no recognizable vacancy links');
    }
    // Details are optional; NEVER let one detail stall the whole collection.
    const detailPage=await context.newPage();
    detailPage.setDefaultTimeout(1700);
    const detailLimit=Math.min(6,MAX_DETAILS);
    for(const item of [...collected.values()].slice(0,detailLimit)) {
      if(remains()<8500){output.warnings.push('Detail enrichment stopped at the time budget; listings retained.');break;}
      if(item.description.length>=180){output.details++;continue;}
      try {
        const response=await gotoBounded(detailPage,item.url,Math.min(DETAIL_MS,Math.max(1500,remains()-4500)));
        if(!response?.ok()){output.warnings.push(`One detail returned HTTP ${response?.status()||'?'}`);continue;}
        const content=source==='swissdevjobs'?await detailsSwiss(detailPage,item):await detailsFinance(detailPage,item);
        if(content && content.length<=40000){item.description=content;output.details++;}
      }catch {output.warnings.push('One detail could not be loaded; listing retained.');}
    }
    output.items=[...collected.values()];if(output.items.length>=MAX_JOBS)output.warnings.push('Result cap reached (40 vacancies); collection may be partial.');
    if(source==='efinancialcareers' && !output.items.length)output.warnings.push('No postings match these keywords and location, or results were not recognized.');
    if(output.items.length && output.items.every(item=>!item.published_at))output.warnings.push('Publication dates unavailable; enable unknown dates in Search.');
    return output;
  } finally {
    if(browser) {
      try {await Promise.race([browser.close(),sleep(5000)]);} catch {}
    }
  }
}
async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) {
    const assert = require('node:assert/strict');
    assert.deepEqual(extractGrades('80–100%'), [80,100]);
    assert.equal(jobMatches('Assistant de vente', 'vente'),true);
    assert.equal(jobMatches('Technicien informatique', 'vente'),false);
    assert.equal(secureUrl('https://evil.test/jobs/', 'swissdevjobs.ch'),null);
    assert.equal(secureUrl('/jobs/example', 'swissdevjobs.ch'),'https://swissdevjobs.ch/jobs/example');
    assert.equal(normalizedJobUrl('/jobs/Vertec-AG-Fachperson-technische-Dokumentation-wmd','swissdevjobs'), 'https://swissdevjobs.ch/jobs/Vertec-AG-Fachperson-technische-Dokumentation-wmd');
    assert.equal(normalizedJobUrl('/jobs-Switzerland-Zurich-Sales__Segment_Manager_Retail_Banking.id24846055','efinancialcareers'),'https://efinancialcareers.ch/jobs-Switzerland-Zurich-Sales__Segment_Manager_Retail_Banking.id24846055');
    assert.equal(normalizedJobUrl('https://other.example/jobs-Pretend.id24846055','efinancialcareers'),null);
    assert.equal(normalizedJobUrl('/jobs/in-indien','efinancialcareers'),null);
    assert.equal(normalizedJobUrl('/jobs/JavaScript/all','swissdevjobs'),null);
    assert.equal(matchesLocation('Zürich, Schweiz','Zuerich'),true);
    const rss='<?xml version="1.0"?><rss><channel><item><title><![CDATA[Junior Developer @ Example SA [CHF 80 000] ]]></title><link>https://swissdevjobs.ch/jobs/Example-SA-Junior-Developer?utm_source=our_rss_feed</link><pubDate>Mon, 26 Oct 2026 12:00:00 GMT</pubDate><description><![CDATA[<b>Requirements:</b> Javascript and TypeScript]]></description></item></channel></rss>';
    const parsed = parseSwissRss(rss,'developer','',7,Date.parse('2026-10-27T12:00:00Z'));
    assert.equal(parsed.length,1);
    assert.equal(parsed[0].company,'Example SA');
    assert.equal(parseSwissRss(rss,'accounting','',7,Date.parse('2026-10-27T12:00:00Z')).length,0);
    assert.equal(parseSwissRss(rss,'developer','',7,Date.parse('2026-11-15T12:00:00Z')).length,0);
    console.log('PASS: Firefox helper URL recognition, RSS/date filtering, and safety tests');
    return;
  }
  const reportPath = argValue(args,'out');
  if (!reportPath) throw new Error('Missing output path');
  const watchdog = setTimeout(() => {
    try { fs.writeFileSync(reportPath,JSON.stringify({ok:false,error:'Firefox collection reached the 80-second safety limit; no data was deleted.'}),'utf8'); } catch {}
    process.exit(124);
  },80000);
  let result;
  try {
    const source = argValue(args, 'source');
    const keywords = argValue(args,'keywords');
    const location = argValue(args,'location');
    const days = Number(argValue(args,'days','3'));
    result = await collect(source,keywords,location,days);
  } catch (error) {
    result = { ok: false, error: text(error?.message || error, 400) };
  }
  clearTimeout(watchdog);
  fs.writeFileSync(reportPath,JSON.stringify(result),'utf8');
  if (!result.ok) process.exitCode = 1;
}
main().catch(e => { console.error(text(e?.message || e)); process.exitCode = 1; });
