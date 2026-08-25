'use strict';

import { state, $, $$, PROVIDER_LABEL, dlLabel, dlShort, saveViewed } from './state.js';
import { esc, toast, copyText, csvCell, highlight } from './utils.js';
import { render, renderFavorites, renderBatchBar, renderCurrentView, renderFavCount } from './render.js';

// ---------- item lookup ----------
export async function getItem(key) {
  return state.groups.get(key) || state.favorites.find((f) => f.key === key) || null;
}

// ---------- magnet resolution ----------
export async function ensureMagnet(g) {
  if (!g.needsMagnet && g.magnet) return g.magnet;
  const src = g.sources.find((s) => s.magnet) || g.sources.find((s) => s.detailUrl);
  if (src && src.magnet) { g.magnet = src.magnet; g.needsMagnet = false; return src.magnet; }
  if (!src || !src.detailUrl) return null;
  const r = await fetch(`/api/magnet?provider=${src.provider}&url=${encodeURIComponent(src.detailUrl)}`);
  const data = await r.json();
  if (data.magnet) {
    g.magnet = data.magnet;
    g.needsMagnet = false;
    src.magnet = data.magnet;
    return data.magnet;
  }
  toast(data.error || '获取磁力失败');
  return null;
}

export function markViewed(it) {
  if (!it || !it.infoHash) return;
  state.viewed.add(it.infoHash);
  saveViewed(state.viewed);
}

// ---------- card click handler ----------
export async function onCardClick(e) {
  const btn = e.target.closest('[data-act]');
  if (!btn) return;
  const id = btn.dataset.id;
  const it = await getItem(id);
  if (!it) return;
  const act = btn.dataset.act;

  if (act === 'toggle-sources') {
    const card = btn.closest('.stacked-card');
    const body = card && card.querySelector('.stacked-sources');
    if (body) {
      body.hidden = !body.hidden;
      btn.textContent = body.hidden ? '来源详情 ▾' : '来源详情 ▴';
    }
    return;
  }

  if (act === 'copysrc') {
    markViewed(it);
    const src = (it.sources || [])[Number(btn.dataset.src)];
    let m = src && src.magnet;
    if (!m) m = await ensureMagnet(it);
    if (m) copyText(m);
    return;
  }

  if (act === 'detailsrc') {
    const src = (it.sources || [])[Number(btn.dataset.src)];
    if (src && src.detailUrl) {
      markViewed(it);
      window.open(src.detailUrl, '_blank', 'noopener noreferrer');
    } else {
      markViewed(it);
      openDetail(it);
    }
    return;
  }

  if (act === 'detail') { markViewed(it); openDetail(it); return; }
  if (act === 'check') { toggleChecked(it.key); return; }
  if (act === 'fav') { toggleFavorite(it); return; }
  if (act === 'getmagnet') {
    btn.textContent = '获取中…'; btn.disabled = true;
    const m = await ensureMagnet(it);
    if (m) { renderCurrentView(); toast('已获取磁力链接'); }
    return;
  }
  if (act === 'copy') { markViewed(it); const m = await ensureMagnet(it); if (m) copyText(m); return; }
  if (act === 'open') { markViewed(it); const m = await ensureMagnet(it); if (m) window.open(m, '_blank', 'noopener'); return; }
  if (act === 'dl') { markViewed(it); const m = await ensureMagnet(it); if (m) sendToClient(m); return; }
}

// ---------- detail modal ----------
export async function openDetail(it) {
  const modal = $('#detail-modal');
  const body = $('#detail-body');
  if (!modal || !body) return;

  const row = (label, value) =>
    `<div class="detail-row"><span class="detail-k">${esc(label)}</span>` +
    `<span class="detail-v">${value}</span></div>`;

  const provs = it.providers && it.providers.length
    ? it.providers
    : (it.sources || []).map((s) => s.provider);
  const sourceBadges = provs
    .map((pid) => `<span class="badge prov-${pid}">${esc(PROVIDER_LABEL[pid] || pid)}</span>`)
    .join(' ');

  const sources = (it.sources || []);
  const sourceLinks = sources.length
    ? sources.map((s) => {
        const label = esc(PROVIDER_LABEL[s.provider] || s.provider);
        if (s.detailUrl) {
          return `<a class="detail-link" href="${esc(s.detailUrl)}" target="_blank" rel="noopener noreferrer">${label} ↗</a>`;
        }
        return `<span class="detail-link disabled">${label}</span>`;
      }).join('')
    : '<span class="detail-muted">无来源信息</span>';

  const magnetLine = it.magnet
    ? `<code class="detail-magnet">${esc(it.magnet)}</code>`
    : (it.needsMagnet
        ? '<span class="detail-muted">尚未解析，点下方「获取磁力」</span>'
        : '<span class="detail-muted">无磁力</span>');

  const filesLine = it.files != null ? String(it.files) : '—';

  $('#detail-title').innerHTML = highlight(it.name, state.query);
  body.innerHTML =
    row('来源', sourceBadges || '—') +
    row('做种', it.seeders != null ? it.seeders : '—') +
    row('下载', it.leechers != null ? it.leechers : '—') +
    row('大小', esc(it.sizeText || '—')) +
    row('时间', esc(it.dateText || '—')) +
    row('分类', it.category ? esc(it.category) : '—') +
    row('文件数', filesLine) +
    row('infoHash', it.infoHash ? `<code>${esc(it.infoHash)}</code>` : '—') +
    row('磁力', magnetLine) +
    row('去站点看文件列表', `<div class="detail-links">${sourceLinks}</div>`);

  const actions = $('#detail-actions');
  const key = esc(it.key);
  const dlBtn = state.dl ? `<button class="btn qb" data-act="dl" data-id="${key}">推送到 ${esc(dlShort(state.dl))}</button>` : '';
  actions.innerHTML = (it.needsMagnet && !it.magnet)
    ? `<button class="btn" data-act="getmagnet" data-id="${key}">获取磁力</button>${dlBtn}`
    : `<button class="btn primary" data-act="open" data-id="${key}">打开磁力</button>` +
      `<button class="btn" data-act="copy" data-id="${key}">复制磁力</button>${dlBtn}`;

  modal.hidden = false;

  // Rich details: best-effort poster + description from the first source's
  // detail page (generic OpenGraph extraction). Hidden if nothing is found.
  const rich = $('#detail-rich');
  if (rich) {
    rich.hidden = true;
    const src = (it.sources || []).find((s) => s.detailUrl);
    if (src && src.detailUrl) {
      fetch(`/api/details?url=${encodeURIComponent(src.detailUrl)}`)
        .then((r) => r.json())
        .then((d) => {
          if (d.poster || d.description) {
            rich.innerHTML =
              (d.poster ? `<img class="detail-poster" src="${esc(d.poster)}" alt="" loading="lazy" referrerpolicy="no-referrer" />` : '') +
              (d.description ? `<p class="detail-desc">${esc(d.description)}</p>` : '');
            rich.hidden = false;
          }
        })
        .catch(() => { /* best-effort */ });
    }
  }
}

export function closeDetail() { $('#detail-modal').hidden = true; }

// ---------- favorites ----------
function saveFavorites() {
  localStorage.setItem('favorites', JSON.stringify(state.favorites));
  renderFavCount();
}

function isFavorited(key) {
  return state.favorites.some((f) => f.key === key);
}

export function toggleFavorite(it) {
  const i = state.favorites.findIndex((f) => f.key === it.key);
  if (i >= 0) {
    state.favorites.splice(i, 1);
    saveFavorites();
    toast('已取消收藏');
  } else {
    state.favorites.unshift({
      key: it.key,
      name: it.name,
      size: it.size, sizeText: it.sizeText,
      seeders: it.seeders, leechers: it.leechers,
      date: it.date, dateText: it.dateText,
      category: it.category,
      files: it.files != null ? it.files : null,
      infoHash: it.infoHash || null,
      magnet: it.magnet || null,
      needsMagnet: !!it.needsMagnet,
      providers: [...(it.providers || [])],
      sources: (it.sources || []).map((s) => ({ ...s })),
      savedAt: Date.now(),
    });
    saveFavorites();
    toast('已加入收藏');
  }
  renderCurrentView();
}

// ---------- batch operations ----------
export function toggleChecked(key) {
  if (state.checked.has(key)) state.checked.delete(key);
  else state.checked.add(key);
  renderCurrentView();
  renderBatchBar();
}

export function clearChecked() {
  state.checked.clear();
  renderCurrentView();
  renderBatchBar();
}

export function checkedItems() {
  const pool = state.view === 'favorites' ? state.favorites : [...state.groups.values()];
  const byKey = new Map(pool.map((it) => [it.key, it]));
  const items = [];
  for (const key of state.checked) {
    if (byKey.has(key)) items.push(byKey.get(key));
    else state.checked.delete(key);
  }
  return items;
}

// Count unresolved magnets in checked items. Returns 0 if all resolved.
function countUnresolved(items) {
  return items.filter((it) => it.needsMagnet && !it.magnet).length;
}

export async function batchCopyMagnets() {
  const items = checkedItems();
  if (!items.length) return;
  const unresolved = countUnresolved(items);
  if (unresolved) {
    toast(`⚠ 有 ${unresolved} 条磁力未解析，正在获取…`);
  }
  toast(`正在准备 ${items.length} 条磁力…`);
  const magnets = [];
  for (const it of items) {
    const m = await ensureMagnet(it).catch(() => null);
    if (m) magnets.push(m);
  }
  if (!magnets.length) { toast('没有可复制的磁力链接'); return; }
  await copyText(magnets.join('\n'));
  toast(`已复制 ${magnets.length}/${items.length} 条磁力`);
}

export async function batchSendToClient() {
  if (!state.dl || !state.dl.client) { openSettings(); toast('请先配置下载客户端'); return; }
  const items = checkedItems();
  if (!items.length) return;
  const unresolved = countUnresolved(items);
  if (unresolved) {
    toast(`⚠ 有 ${unresolved} 条磁力未解析，正在获取…`);
  }
  const label = dlLabel(state.dl);
  toast(`正在推送 ${items.length} 条到 ${label}…`);
  let ok = 0;
  // Batch push in chunks of 5 concurrent requests
  const BATCH = 5;
  for (let i = 0; i < items.length; i += BATCH) {
    const chunk = items.slice(i, i + BATCH);
    const results = await Promise.allSettled(chunk.map(async (it) => {
      const m = await ensureMagnet(it).catch(() => null);
      if (!m) return;
      const r = await fetch('/api/download/push', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ kind: state.dl.client, url: state.dl.url, user: state.dl.user || '', pass: state.dl.pass || '', token: state.dl.token || '', magnet: m }),
      });
      const data = await r.json();
      if (data.ok) ok++;
    }));
  }
  toast(`已推送 ${ok}/${items.length} 条到 ${label}`);
}

export function batchExportCsv() {
  const items = checkedItems();
  if (!items.length) return;
  const unresolved = countUnresolved(items);
  if (unresolved) {
    toast(`⚠ 有 ${unresolved} 条磁力未解析，CSV 中对应列将留空`);
  }
  const cols = ['name', 'seeders', 'leechers', 'sizeText', 'dateText', 'category', 'providers', 'infoHash', 'magnet'];
  const head = ['名称', '做种', '下载', '大小', '时间', '分类', '来源', 'infoHash', '磁力'];
  const rows = items.map((it) => cols.map((c) => {
    if (c === 'providers') return csvCell((it.providers || []).join(' | '));
    return csvCell(it[c]);
  }).join(','));
  const csv = '\uFEFF' + [head.join(','), ...rows].join('\r\n');
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `torrents-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}.csv`;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
  toast(`已导出 ${items.length} 条为 CSV`);
}

// ---------- download client ----------
export async function sendToClient(magnet) {
  if (!state.dl || !state.dl.client) { openSettings(); toast('请先配置下载客户端'); return; }
  const dl = state.dl, label = dlLabel(state.dl);
  toast(`正在推送到 ${label}…`);
  try {
    const r = await fetch('/api/download/push', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind: dl.client, url: dl.url, user: dl.user || '', pass: dl.pass || '', token: dl.token || '', magnet }),
    });
    const data = await r.json();
    if (data.ok) toast(`已添加到 ${label}`);
    else toast('推送失败：' + (data.error || '未知错误'));
  } catch (e) { toast('推送失败：网络错误'); }
}

export async function autoDetectDownloader() {
  if (state.dl) return;
  try {
    const r = await fetch('/api/download/detect');
    const d = await r.json();
    if (d.ok) { applyDetected(d); render(); }
  } catch (e) { /* best-effort */ }
}

export function applyDetected(d) {
  state.dl = { client: d.kind, url: d.url, user: d.user || '', pass: d.pass || '', token: d.token || '' };
  localStorage.setItem('dl', JSON.stringify(state.dl));
}

// Need openSettings - import from settings.js dynamically to avoid circular dep
async function openSettings() {
  const { openSettings: os } = await import('./settings.js');
  os();
}