'use strict';

import { state, $, allProviders, PROVIDER_LABEL, DL_CLIENTS, dlShort, dlLabel,
         GROUP_ORDER, GROUP_LABELS, providerGroupOf, saveSelectedProviders,
         CATEGORY_ORDER, CATEGORY_LABELS, normalizeCategory, matchesQuality } from './state.js';
import { esc, highlight, shortMagnet, seedBarHTML, relevanceScore } from './utils.js';

// ---------- provider group chips (top bar) ----------
export function renderGroupChips() {
  const wrap = $('#group-chips');
  if (!wrap) return;

  const byGroup = new Map();
  allProviders.forEach((p) => {
    const g = providerGroupOf(p);
    if (!byGroup.has(g)) byGroup.set(g, []);
    byGroup.get(g).push(p);
  });
  const groups = [...GROUP_ORDER, 'custom'].filter((g) => byGroup.has(g));

  const total = allProviders.length;
  const selTotal = allProviders.filter((p) => state.selected.has(p.id)).length;
  const allOn = total > 0 && selTotal === total;

  const chip = (key, label, on, extra) => {
    if (key === 'adult' && state.safeMode) return '';
    return `<button type="button" class="chip group-chip${on ? ' on' : ''}" data-group="${esc(key)}"` +
      ` aria-pressed="${on ? 'true' : 'false'}"` +
      `${extra ? ` title="${esc(extra)}"` : ''}>` +
      `<span class="dot" aria-hidden="true"></span>${esc(label)}</button>`;
  };

  let html = chip('__all__', '全部', allOn);
  html += groups.map((g) => {
    const list = byGroup.get(g);
    const on = list.length > 0 && list.every((p) => state.selected.has(p.id));
    const sel = list.filter((p) => state.selected.has(p.id)).length;
    return chip(g, GROUP_LABELS[g] || g, on, `${sel}/${list.length} 已选`);
  }).join('');
  wrap.innerHTML = html;
}

// ---------- provider chips (settings panel) ----------
export function renderProviderChips() {
  const wrap = $('#provider-chips');
  wrap.innerHTML = '';

  const byGroup = new Map();
  allProviders.forEach((p) => {
    const g = providerGroupOf(p);
    if (!byGroup.has(g)) byGroup.set(g, []);
    byGroup.get(g).push(p);
  });

  const order = [...GROUP_ORDER, 'custom'].filter((g) => byGroup.has(g));
  order.forEach((g) => {
    const list = byGroup.get(g);
    const selCount = list.filter((p) => state.selected.has(p.id)).length;

    const section = document.createElement('div');
    section.className = 'pgroup';
    section.dataset.group = g;

    const head = document.createElement('div');
    head.className = 'pgroup-head';
    head.innerHTML =
      `<span class="pgroup-name">${esc(GROUP_LABELS[g] || g)}` +
      `<span class="pgroup-count">${selCount}/${list.length}</span></span>` +
      `<button type="button" class="pgroup-toggle" data-group="${esc(g)}">` +
      `${selCount === list.length ? '全不选' : '全选'}</button>`;
    section.appendChild(head);

    const chips = document.createElement('div');
    chips.className = 'chips';
    list.forEach((p) => {
      const on = state.selected.has(p.id);
      const el = document.createElement('button');
      el.type = 'button';
      el.className = `chip${on ? ' on' : ''}${p.demo ? ' demo' : ''}`;
      el.dataset.id = p.id;
      el.setAttribute('aria-pressed', on ? 'true' : 'false');
      el.innerHTML = `<span class="dot" aria-hidden="true"></span>${esc(p.name)}`;
      el.onclick = () => {
        // Dynamic import to avoid circular dependency
        import('./main.js').then((m) => m.toggleProvider(p.id));
      };
      chips.appendChild(el);
    });
    section.appendChild(chips);
    wrap.appendChild(section);
  });
}

// Categorize a provider error string into a short human label so the status
// bar can tell "unreachable/blocked" apart from "parse failure (site redesigned)"
// — the two failure modes need different user actions (switch mirror vs wait).
// ponytail: string heuristic, replace with backend error codes if it ever misleads.
function categorizeError(err) {
  const e = String(err || '');
  if (/timeout|timed ?out|econn|enotfound|etimedout|err_|unreachable|refused|getaddrinfo|socket|dns|eai_|reset|blocked|cloudflare|403|429/i.test(e)) return '网络不可达';
  if (/no_?results|no results|parsed|parse|magnet|selector|empty/i.test(e)) return '解析失败（可能改版）';
  return '未知错误';
}

// ---------- status bar ----------
export function renderStatus(providers) {
  const bar = $('#status-bar');
  const entries = Object.entries(providers);
  if (!entries.length) {
    // Show cached status from last search if available
    try {
      const cached = JSON.parse(localStorage.getItem('last-status') || '{}');
      const cachedEntries = Object.entries(cached);
      if (cachedEntries.length) {
        bar.hidden = false;
        bar.innerHTML = cachedEntries.map(([id, s]) => {
          const label = PROVIDER_LABEL[id] || id;
          return `<span class="status-pill cached">` +
            `<span class="status-dot ok" aria-hidden="true"></span>` +
            `<b>${esc(label)}</b><span class="status-meta">上次 ${s.count} 条</span></span>`;
        }).join('');
      } else {
        bar.hidden = true;
      }
    } catch { bar.hidden = true; }
    return;
  }
  bar.hidden = false;
  const okCount = entries.filter(([, s]) => s.status === 'ok').length;
  const sum = `<span class="status-pill status-sum"><b>${okCount}/${entries.length}</b> 已响应</span>`;
  bar.innerHTML = sum + entries.map(([id, s]) => {
    const cls = s.status === 'ok' ? 'ok' : 'err';
    const mark = s.status === 'ok' ? '✓' : '✕';
    const label = PROVIDER_LABEL[id] || id;
    const ms = s.ms != null ? ` · ${s.ms}ms` : '';
    const errFull = s.error ? String(s.error) : '';
    const errCat = s.error ? categorizeError(s.error) : '';
    const detail = s.error ? `${mark} ${errCat}` : `${mark} ${s.count} 条${ms}`;
    return `<span class="status-pill${s.error ? ' err-pill' : ''}"${s.error ? ` title="${esc(label)}：${esc(errFull)}（点击查看）" data-err="${esc(errFull)}"` : ''}>` +
      `<span class="status-dot ${cls}" aria-hidden="true"></span>` +
      `<b>${esc(label)}</b><span class="status-meta">${esc(detail)}</span></span>`;
  }).join('');
}

// ---------- category filter chips ----------
export function renderCategoryFilters() {
  const wrap = $('#category-filters');
  const counts = new Map();
  for (const it of state.groups.values()) {
    const c = normalizeCategory(it.category, it.name);
    counts.set(c, (counts.get(c) || 0) + 1);
  }

  const present = CATEGORY_ORDER.filter((c) => counts.has(c));
  if (state.groups.size === 0 || present.length < 2) {
    wrap.hidden = true;
    if (state.category !== 'all') state.category = 'all';
    return;
  }

  if (state.category !== 'all' && !counts.has(state.category)) state.category = 'all';

  wrap.hidden = false;
  const total = state.groups.size;
  const btn = (q, label, n) =>
    `<button type="button" class="qbtn${state.category === q ? ' on' : ''}" data-cat="${esc(q)}">` +
    `${esc(label)} <span class="qbtn-count">${n}</span></button>`;
  wrap.innerHTML =
    btn('all', '全部', total) +
    present.map((c) => btn(c, CATEGORY_LABELS[c], counts.get(c))).join('');
}

// ---------- result filtering & sorting ----------
export function visibleResults() {
  const minSeed = parseInt($('#min-seeders').value, 10) || 0;
  const minSize = parseInt($('#min-size').value, 10) || 0;
  const name = $('#name-contains').value.trim().toLowerCase();

  let list = [...state.groups.values()].filter((it) => {
    if (it.seeders != null && it.seeders < minSeed) return false;
    if (it.size != null && it.size < minSize) return false;
    if (name && !it.name.toLowerCase().includes(name)) return false;
    if (!matchesQuality(it.name, state.quality)) return false;
    if (state.category !== 'all' && normalizeCategory(it.category, it.name) !== state.category) return false;
    if (state.safeMode && it.category === 'porn') return false;
    return true;
  });

  const dir = state.order === 'desc' ? -1 : 1;
  if (state.sort === 'relevance') {
    // Score into a temp Map to avoid mutating state.groups objects
    const scored = list.map((it) => ({ it, _score: relevanceScore(it.name, state.query) }));
    scored.sort((a, b) => (b._score - a._score) || ((b.it.seeders ?? -1) - (a.it.seeders ?? -1)));
    return scored.map((s) => s.it);
  } else {
    list.sort((a, b) => {
      let av, bv;
      if (state.sort === 'seeders') { av = a.seeders ?? -1; bv = b.seeders ?? -1; }
      else if (state.sort === 'size') { av = a.size ?? -1; bv = b.size ?? -1; }
      else { av = a.date ?? -1; bv = b.date ?? -1; }
      return (av - bv) * dir;
    });
  }
  return list;
}

// ---------- card rendering ----------
export function cardHTML(it) {
  if (it.sources && it.sources.length > 1) return stackedCardHTML(it);
  return singleCardHTML(it);
}

function singleCardHTML(it) {
  const seed = it.seeders != null ? it.seeders : '—';
  const leech = it.leechers != null ? it.leechers : '—';
  const size = it.sizeText || '—';
  const date = it.dateText || '—';
  const cat = it.category ? `<span class="badge">${esc(it.category)}</span>` : '';
  const magnetBtn = it.needsMagnet
    ? `<button class="btn" data-act="getmagnet" data-id="${esc(it.key)}">获取磁力</button>`
    : `<button class="btn primary" data-act="open" data-id="${esc(it.key)}">打开磁力</button>
       <button class="btn" data-act="copy" data-id="${esc(it.key)}">复制</button>`;
  const dlBtn = state.dl ? `<button class="btn qb" data-act="dl" data-id="${esc(it.key)}">推送到 ${esc(dlShort(state.dl))}</button>` : '';
  const provs = it.providers && it.providers.length ? it.providers : (it.sources || []).map((s) => s.provider);
  const sourceBadges = provs
    .map((pid) => `<span class="badge prov-${pid}">${esc(PROVIDER_LABEL[pid] || pid)}</span>`)
    .join('');
  const multi = provs.length > 1 ? `<span class="badge multi">${provs.length} 个来源</span>` : '';
  const faved = isFavorited(it.key);
  const favBtn = `<button class="fav-btn${faved ? ' on' : ''}" data-act="fav" data-id="${esc(it.key)}" title="${faved ? '取消收藏' : '收藏'}" aria-label="${faved ? '取消收藏' : '收藏'}">${faved ? '★' : '☆'}</button>`;
  const checked = state.checked.has(it.key);
  const viewed = state.viewed.has(it.infoHash);
  const checkBox = `<input type="checkbox" class="card-check" data-act="check" data-id="${esc(it.key)}"${checked ? ' checked' : ''} title="选择用于批量操作" aria-label="选择用于批量操作" />`;
  return `
  <div class="card${checked ? ' checked' : ''}${viewed ? ' viewed' : ''}" data-id="${esc(it.key)}">
    ${checkBox}
    ${favBtn}
    <div class="name" title="${esc(it.name)}">${highlight(it.name, state.query)}</div>
    <div class="badges">
      ${sourceBadges}
      ${multi}
      ${cat}
    </div>
    <div class="stats">
      <span class="stat"><span class="stat-k">做种</span><span class="stat-v seed">▲ ${seed}</span></span>
      <span class="stat"><span class="stat-k">下载</span><span class="stat-v leech">▼ ${leech}</span></span>
      <span class="stat"><span class="stat-k">大小</span><span class="stat-v">${size}</span></span>
      <span class="stat"><span class="stat-k">时间</span><span class="stat-v">${date}</span></span>
    </div>
    ${seedBarHTML(it)}
    <div class="actions">
      ${magnetBtn}
      ${dlBtn}
      <button class="btn ghost" data-act="detail" data-id="${esc(it.key)}">详情</button>
    </div>
  </div>`;
}

function stackedCardHTML(it) {
  const seed = it.seeders != null ? it.seeders : '—';
  const leech = it.leechers != null ? it.leechers : '—';
  const size = it.sizeText || '—';
  const date = it.dateText || '—';
  const cat = it.category ? `<span class="badge">${esc(it.category)}</span>` : '';
  const magnetBtn = it.needsMagnet
    ? `<button class="btn" data-act="getmagnet" data-id="${esc(it.key)}">获取磁力</button>`
    : `<button class="btn primary" data-act="open" data-id="${esc(it.key)}">打开磁力</button>
       <button class="btn" data-act="copy" data-id="${esc(it.key)}">复制</button>`;
  const dlBtn = state.dl ? `<button class="btn qb" data-act="dl" data-id="${esc(it.key)}">推送到 ${esc(dlShort(state.dl))}</button>` : '';
  const provs = it.providers && it.providers.length ? it.providers : (it.sources || []).map((s) => s.provider);
  const sourceBadges = provs
    .map((pid) => `<span class="badge prov-${pid}">${esc(PROVIDER_LABEL[pid] || pid)}</span>`)
    .join('');
  const faved = isFavorited(it.key);
  const favBtn = `<button class="fav-btn${faved ? ' on' : ''}" data-act="fav" data-id="${esc(it.key)}" title="${faved ? '取消收藏' : '收藏'}" aria-label="${faved ? '取消收藏' : '收藏'}">${faved ? '★' : '☆'}</button>`;
  const checked = state.checked.has(it.key);
  const viewed = state.viewed.has(it.infoHash);
  const checkBox = `<input type="checkbox" class="card-check" data-act="check" data-id="${esc(it.key)}"${checked ? ' checked' : ''} title="选择用于批量操作" aria-label="选择用于批量操作" />`;

  const rows = (it.sources || []).map((s, i) => {
    const label = esc(PROVIDER_LABEL[s.provider] || s.provider);
    const mag = s.magnet
      ? `<code class="src-magnet" title="${esc(s.magnet)}">${esc(shortMagnet(s.magnet))}</code>`
      : '<span class="src-muted">磁力未解析（点上方「获取磁力」）</span>';
    return `<div class="stacked-source" data-src="${i}">
      <span class="badge prov-${esc(s.provider)}">${label}</span>
      ${mag}
      <span class="src-actions">
        <button class="btn" data-act="copysrc" data-id="${esc(it.key)}" data-src="${i}">复制</button>
        <button class="btn ghost" data-act="detailsrc" data-id="${esc(it.key)}" data-src="${i}">详情</button>
      </span>
    </div>`;
  }).join('');

  return `
  <div class="card stacked-card${checked ? ' checked' : ''}${viewed ? ' viewed' : ''}" data-id="${esc(it.key)}">
    ${checkBox}
    ${favBtn}
    <div class="name" title="${esc(it.name)}">${highlight(it.name, state.query)}</div>
    <div class="badges">
      ${sourceBadges}
      <span class="badge multi">${provs.length} 个来源</span>
      ${cat}
    </div>
    <div class="stats">
      <span class="stat"><span class="stat-k">做种</span><span class="stat-v seed">▲ ${seed}</span></span>
      <span class="stat"><span class="stat-k">下载</span><span class="stat-v leech">▼ ${leech}</span></span>
      <span class="stat"><span class="stat-k">大小</span><span class="stat-v">${size}</span></span>
      <span class="stat"><span class="stat-k">时间</span><span class="stat-v">${date}</span></span>
    </div>
    ${seedBarHTML(it)}
    <div class="stacked-sources" hidden>
      <div class="stacked-sources-title">来源详情（同一种子，多站命中）</div>
      ${rows}
    </div>
    <div class="actions">
      ${magnetBtn}
      ${dlBtn}
      <button class="btn ghost" data-act="detail" data-id="${esc(it.key)}">详情</button>
      <button class="btn ghost" data-act="toggle-sources" data-id="${esc(it.key)}">来源详情 ▾</button>
    </div>
  </div>`;
}

// ---------- main render ----------
export function render() {
  const wrap = $('#results');
  renderCategoryFilters();
  const list = visibleResults();
  if (state.groups.size === 0) {
    wrap.innerHTML = '';
    $('#result-summary').hidden = true;
    $('#empty').hidden = state.loading;
    $('#empty').textContent = '没有结果。试试别的关键词，或检查引擎状态。';
    return;
  }
  $('#empty').hidden = list.length > 0;
  if (list.length === 0) {
    $('#empty').textContent = '没有符合筛选条件的结果，试着放宽筛选。';
  }
  // Result count summary
  const summary = $('#result-summary');
  if (summary) {
    const total = state.groups.size;
    const sortLabel = { relevance: '相关度', seeders: '做种数', size: '大小', date: '时间' }[state.sort] || state.sort;
    summary.innerHTML = `共 <span class="summary-num">${list.length}</span> 条` +
      `<span class="summary-meta">（去重后 ${total}，按${sortLabel}${state.order === 'desc' ? '降' : '升'}序）</span>` +
      (state.page > 1 ? `<span class="summary-meta">· 第 <span class="summary-num">${state.page}</span> 页</span>` : '');
    summary.hidden = false;
  }
  wrap.innerHTML = list.map(cardHTML).join('');
  // Back to top button
  const btt = $('#back-to-top');
  if (btt) btt.hidden = list.length < 10;
}

// ---------- favorites rendering ----------
function isFavorited(key) {
  return state.favorites.some((f) => f.key === key);
}

export function renderFavCount() {
  const el = $('#fav-count');
  if (!el) return;
  const n = state.favorites.length;
  el.textContent = n ? String(n) : '';
  el.hidden = !n;
}

export function renderFavorites() {
  const wrap = $('#favorites');
  const empty = $('#favorites-empty');
  const favQuery = ($('#fav-search') && $('#fav-search').value || '').trim().toLowerCase();

  let list = state.favorites;
  if (favQuery) {
    list = list.filter((it) => it.name.toLowerCase().includes(favQuery));
  }

  if (!list.length) {
    wrap.innerHTML = '';
    empty.hidden = false;
    empty.textContent = favQuery ? '没有匹配的收藏。' : '收藏夹还是空的。在搜索结果里点 ☆ 收藏，就会出现在这里。';
    return;
  }
  empty.hidden = true;
  wrap.innerHTML = list.map(cardHTML).join('');
}

// ---------- batch bar ----------
export function renderBatchBar() {
  const bar = $('#batch-bar');
  if (!bar) return;
  // Dynamic import to avoid circular dependency
  import('./actions.js').then(({ checkedItems }) => {
    const n = checkedItems().length;
    if (!n) { bar.hidden = true; return; }
    bar.hidden = false;
    const countEl = document.querySelector('#batch-count');
    if (countEl) countEl.textContent = n;
    const qbBtn = document.querySelector('#batch-qb');
    if (qbBtn) qbBtn.hidden = !(state.dl && state.dl.client);
  });
}

// ---------- view switch ----------
export function renderCurrentView() {
  if (state.view === 'favorites') renderFavorites();
  else render();
  renderBatchBar();
}