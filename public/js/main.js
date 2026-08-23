'use strict';

import { state, $, $$, allProviders, PROVIDER_LABEL, DL_CLIENTS, dlShort, dlLabel, saveSafeMode,
         saveSelectedProviders, loadProviders, providerGroupOf } from './state.js';
import { esc, toast, copyText } from './utils.js';
import { renderGroupChips, renderProviderChips, renderStatus, render, renderFavorites,
         renderFavCount, renderBatchBar, renderCurrentView } from './render.js';
import { onCardClick, openDetail, closeDetail, ensureMagnet, markViewed, getItem,
         toggleChecked, clearChecked, checkedItems, batchCopyMagnets, batchSendToClient,
         batchExportCsv, sendToClient, autoDetectDownloader, applyDetected } from './actions.js';
import { pushHistory, removeHistory, clearHistory, renderHistory, showHistory, hideHistory } from './history.js';
import { openSettings, closeSettings, syncDlAuthFields, readDlForm, loadTorznab } from './settings.js';

// ---------- provider toggling (called from renderProviderChips oncick) ----------
export function toggleProvider(id) {
  if (state.selected.has(id)) state.selected.delete(id);
  else state.selected.add(id);
  saveSelectedProviders(state.selected);
  renderGroupChips();
  renderProviderChips();
  if (state.query) doSearch();
}

export function toggleGroup(g) {
  if (g === 'adult' && state.safeMode) {
    toast('安全模式已启用，成人引擎已被禁用');
    return;
  }
  const list = allProviders.filter((p) => providerGroupOf(p) === g);
  const allOn = list.every((p) => state.selected.has(p.id));
  list.forEach((p) => {
    if (allOn) state.selected.delete(p.id);
    else state.selected.add(p.id);
  });
  saveSelectedProviders(state.selected);
  renderGroupChips();
  renderProviderChips();
  if (state.query) doSearch();
}

export function toggleAllProviders() {
  const allOn = allProviders.length > 0 && allProviders.every((p) => state.selected.has(p.id));
  if (allOn) state.selected.clear();
  else allProviders.forEach((p) => state.selected.add(p.id));
  saveSelectedProviders(state.selected);
  renderGroupChips();
  renderProviderChips();
  if (state.query) doSearch();
}

// Recommended engines: high-availability, broadly useful, non-adult.
const RECOMMENDED = new Set([
  '1337x', 'tpb', 'nyaa', 'yts', 'eztv', 'therarbg', 'torrentscsv', 'demo',
]);
// Standard: recommended + more popular engines, still non-adult.
const STANDARD = new Set([
  ...RECOMMENDED,
  'bt4g', 'btdigg', 'limetorrents', 'torrentkitty', 'torrentdownload',
  'torrentdownloads', 'bitsearch', 'oxtorrent', 'knaben', 'uindex',
  'subsplease', 'tokyotoshokan', 'nekobt',
]);

export function applyPreset(name) {
  const ids = name === 'all' ? null : name === 'standard' ? STANDARD : RECOMMENDED;
  state.selected.clear();
  allProviders.forEach((p) => {
    if (!ids || ids.has(p.id)) state.selected.add(p.id);
  });
  saveSelectedProviders(state.selected);
  renderGroupChips();
  renderProviderChips();
  if (state.query) doSearch();
  toast(name === 'all' ? '已启用全部引擎' : `已应用「${name === 'recommended' ? '推荐' : '标准'}」预设`);
}

// ---------- search ----------
export async function doSearch() {
  state.query = $('#search-input').value.trim();
  if (!state.query) return;
  const welcome = $('#welcome');
  if (welcome) welcome.hidden = true;
  if (allProviders.length && state.selected.size === 0) {
    state.searchId++;
    if (state.es) { state.es.close(); state.es = null; }
    state.groups = new Map();
    state.status = {};
    state.hasMore = false;
    state.loading = false;
    renderStatus(state.status);
    render();
    $('#empty').hidden = false;
    $('#empty').textContent = '没有启用任何搜索引擎。点顶部分组或到 ⚙ 设置里勾选引擎。';
    return;
  }
  pushHistory(state.query);
  if (state.view !== 'search') switchView('search');
  hideHistory();
  state.searchId++;
  if (state.abort) { state.abort.abort(); state.abort = null; }
  state.loading = false;
  state.page = 1;
  state.all = [];
  state.seen = new Set();
  state.groups = new Map();
  state.category = 'all';
  state.checked.clear();
  renderBatchBar();
  await loadPage();
}

function loadPage() {
  if (state.loading) return;
  if (!state.hasMore && state.page > 1) return;
  state.loading = true;
  $('#loading').hidden = false;

  const prevCount = state.all.length;
  if (state.page === 1) state.status = {};

  const myId = state.searchId;
  const params = new URLSearchParams({ q: state.query, page: state.page });
  if (state.selected.size) params.set('providers', [...state.selected].join(','));

  if (state.es) { state.es.close(); state.es = null; }
  const es = new EventSource(`/api/search/stream?${params}`);
  state.es = es;

  const finish = () => {
    if (state.es === es) state.es = null;
    es.close();
    if (myId !== state.searchId) return;
    state.loading = false;
    $('#loading').hidden = true;
    if (state.hasMore && state.all.length > prevCount) state.page++;
    else if (state.page > 1) state.hasMore = false;
  };

  es.addEventListener('provider', (ev) => {
    if (myId !== state.searchId) { es.close(); return; }
    let msg;
    try { msg = JSON.parse(ev.data); } catch { return; }
    state.status[msg.id] = msg.status || {};
    (msg.results || []).forEach((it) => {
      if (state.seen.has(it.id)) return;
      state.seen.add(it.id);
      state.all.push(it);
      mergeResult(it);
    });
    renderStatus(state.status);
    render();
  });

  es.addEventListener('done', (ev) => {
    let msg = {};
    try { msg = JSON.parse(ev.data); } catch { /* ignore */ }
    if (myId === state.searchId) state.hasMore = !!msg.hasMore;
    finish();
  });

  es.addEventListener('error', () => {
    if (myId === state.searchId) {
      if (state.all.length === 0 && Object.keys(state.status).length === 0) {
        toast('搜索请求失败');
      }
      state.hasMore = false;
    }
    finish();
  });
}

function mergeResult(it) {
  const key = it.infoHash ? `hash:${String(it.infoHash).toLowerCase()}` : it.id;
  let g = state.groups.get(key);
  if (!g) {
    g = {
      key, name: it.name, size: it.size, sizeText: it.sizeText,
      seeders: it.seeders, leechers: it.leechers,
      date: it.date, dateText: it.dateText, category: it.category,
      files: it.files != null ? it.files : null,
      infoHash: it.infoHash || null, magnet: it.magnet || null,
      needsMagnet: !it.magnet && !!it.detailUrl,
      providers: [], sources: [],
    };
    state.groups.set(key, g);
  }
  g.sources.push({ provider: it.provider, magnet: it.magnet || null, detailUrl: it.detailUrl || null, id: it.id });
  if (!g.providers.includes(it.provider)) g.providers.push(it.provider);
  if (it.seeders != null && (g.seeders == null || it.seeders > g.seeders)) g.seeders = it.seeders;
  if (it.leechers != null && (g.leechers == null || it.leechers > g.leechers)) g.leechers = it.leechers;
  if (g.size == null && it.size != null) { g.size = it.size; g.sizeText = it.sizeText; }
  if (g.date == null && it.date != null) { g.date = it.date; g.dateText = it.dateText; }
  if (g.files == null && it.files != null) g.files = it.files;
  if (!g.magnet && it.magnet) { g.magnet = it.magnet; g.needsMagnet = false; }
  if (!g.magnet && it.detailUrl) g.needsMagnet = true;
  return g;
}

// ---------- view switch ----------
function switchView(view) {
  state.view = view;
  $$('.view-tab').forEach((t) => t.classList.toggle('on', t.dataset.view === view));
  $('#search-view').hidden = view !== 'search';
  $('#favorites-view').hidden = view !== 'favorites';
  $('.controls').hidden = view !== 'search';
  $('#status-bar').hidden = view !== 'search' || !Object.keys(state.status).length;
  state.checked.clear();
  renderCurrentView();
  renderBatchBar();
}

// ---------- event bindings ----------
function bindEvents() {
  // Search form
  $('#search-form').addEventListener('submit', (e) => { e.preventDefault(); doSearch(); });

  // Sort / order
  $('#sort-select').onchange = (e) => { state.sort = e.target.value; render(); };
  $('#order-btn').onclick = () => {
    state.order = state.order === 'desc' ? 'asc' : 'desc';
    $('#order-btn').textContent = state.order === 'desc' ? '↓' : '↑';
    render();
  };

  // Filters
  ['min-seeders', 'min-size', 'name-contains'].forEach((id) => {
    $(`#${id}`).addEventListener('input', () => render());
  });
  $('#reset-filters').onclick = () => {
    $('#min-seeders').value = ''; $('#min-size').value = '0'; $('#name-contains').value = '';
    state.category = 'all';
    render();
  };

  // Provider chips (settings panel)
  $('#provider-chips').addEventListener('click', (e) => {
    const btn = e.target.closest('.pgroup-toggle');
    if (!btn) return;
    toggleGroup(btn.dataset.group);
  });

  // Engine presets
  $('.preset-btns').addEventListener('click', (e) => {
    const btn = e.target.closest('[data-preset]');
    if (!btn) return;
    applyPreset(btn.dataset.preset);
  });

  // Group chips (top bar)
  $('#group-chips').addEventListener('click', (e) => {
    const chip = e.target.closest('.group-chip');
    if (!chip) return;
    const g = chip.dataset.group;
    if (g === '__all__') toggleAllProviders();
    else toggleGroup(g);
  });

  // Safe mode toggle
  $('#safe-mode-toggle').onchange = (e) => {
    state.safeMode = e.target.checked;
    saveSafeMode(state.safeMode);
    renderGroupChips();
    renderProviderChips();
    render();
    toast(state.safeMode ? '安全模式已启用（成人引擎禁用）' : '安全模式已关闭');
  };

  // Card clicks
  $('#results').addEventListener('click', onCardClick);
  $('#favorites').addEventListener('click', onCardClick);

  // Detail modal
  $('#detail-modal').addEventListener('click', async (e) => {
    if (e.target.closest('[data-close]')) { closeDetail(); return; }
    const btn = e.target.closest('[data-act]');
    if (!btn) return;
    const it = await getItem(btn.dataset.id);
    if (!it) return;
    const act = btn.dataset.act;
    if (act === 'getmagnet') {
      btn.textContent = '获取中…'; btn.disabled = true;
      const m = await ensureMagnet(it);
      if (m) { openDetail(it); renderCurrentView(); toast('已获取磁力链接'); }
      else { btn.textContent = '获取磁力'; btn.disabled = false; }
      return;
    }
    if (act === 'copy') { markViewed(it); const m = await ensureMagnet(it); if (m) copyText(m); return; }
    if (act === 'open') { markViewed(it); const m = await ensureMagnet(it); if (m) window.location.href = m; return; }
    if (act === 'dl') { markViewed(it); const m = await ensureMagnet(it); if (m) sendToClient(m); return; }
  });

  // Batch buttons
  $('#batch-copy').onclick = batchCopyMagnets;
  $('#batch-qb').onclick = batchSendToClient;
  $('#batch-csv').onclick = batchExportCsv;
  $('#batch-clear').onclick = clearChecked;

  // Settings
  $('#dl-client').onchange = syncDlAuthFields;
  $('#settings-btn').onclick = () => { openSettings(); loadTorznab(); };
  $('#settings-cancel').onclick = closeSettings;

  // Theme toggle
  $('#theme-btn').onclick = () => {
    document.documentElement.classList.toggle('light');
    const isLight = document.documentElement.classList.contains('light');
    localStorage.setItem('theme', isLight ? 'light' : 'dark');
    toast(isLight ? '已切换亮色主题' : '已切换暗色主题');
  };
  $('#settings-save').onclick = () => {
    state.dl = readDlForm();
    localStorage.setItem('dl', JSON.stringify(state.dl));
    closeSettings();
    render();
    toast('已保存下载工具设置');
  };
  $('#dl-test').onclick = async () => {
    const cfg = readDlForm();
    if (!cfg.url) return toast('请先填写地址');
    toast('测试连接中…');
    try {
      const r = await fetch('/api/download/test', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(cfg),
      });
      const data = await r.json();
      toast(data.ok ? '连接成功' : ('连接失败：' + (data.error || '')));
    } catch (e) { toast('测试失败：网络错误'); }
  };
  $('#dl-detect').onclick = async () => {
    toast('正在探测本机下载器…');
    try {
      const r = await fetch('/api/download/detect');
      const d = await r.json();
      if (d.ok) {
        applyDetected(d);
        openSettings();
        render();
        toast(`已自动发现并启用 ${DL_CLIENTS[d.kind]?.label || d.kind}`);
      } else {
        toast('未在本机发现受支持的下载器（请手动填写）');
      }
    } catch (e) { toast('探测失败：网络错误'); }
  };

  // Torznab
  $('#torznab-form').addEventListener('submit', (e) => e.preventDefault());
  $('#tn-add').onclick = async () => {
    const name = $('#tn-name').value.trim();
    const url = $('#tn-url').value.trim();
    const apiKey = $('#tn-key').value;
    const enabled = $('#tn-enabled').checked;
    if (!name || !url) return toast('请填写名称和 URL');
    try {
      const r = await fetch('/api/torznab', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name, url, apiKey, enabled }),
      });
      const d = await r.json();
      if (d.indexer) {
        $('#tn-name').value = ''; $('#tn-url').value = ''; $('#tn-key').value = '';
        await loadTorznab();
        loadProviders();
        toast('已添加 Torznab 索引器');
      } else {
        toast('添加失败：' + (d.error || ''));
      }
    } catch (e) { toast('添加失败：网络错误'); }
  };
  $('#torznab-list').addEventListener('click', async (e) => {
    const btn = e.target.closest('.tn-del');
    if (!btn) return;
    const id = btn.dataset.id;
    try {
      await fetch(`/api/torznab/${encodeURIComponent(id)}`, { method: 'DELETE' });
      await loadTorznab();
      loadProviders();
      toast('已删除');
    } catch (e) { toast('删除失败'); }
  });
  $('#tn-test').onclick = async () => {
    const url = $('#tn-url').value.trim();
    const apiKey = $('#tn-key').value;
    if (!url) return toast('请填写 URL');
    toast('测试连接中…');
    try {
      const r = await fetch('/api/torznab/test', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url, apiKey }),
      });
      const d = await r.json();
      toast(d.ok ? '连接成功' : ('连接失败：' + (d.error || '')));
    } catch (e) { toast('测试失败：网络错误'); }
  };

  // View tabs
  $('.views').addEventListener('click', (e) => {
    const tab = e.target.closest('.view-tab');
    if (!tab) return;
    switchView(tab.dataset.view);
  });

  // Favorites search
  const favSearch = $('#fav-search');
  if (favSearch) favSearch.addEventListener('input', () => renderFavorites());

  // Quality filters
  $('#quality-filters').addEventListener('click', (e) => {
    const btn = e.target.closest('.qbtn');
    if (!btn) return;
    state.quality = btn.dataset.q;
    $('#quality-filters').querySelectorAll('.qbtn').forEach((b) => b.classList.toggle('on', b === btn));
    render();
  });

  // Category filters
  $('#category-filters').addEventListener('click', (e) => {
    const btn = e.target.closest('.qbtn');
    if (!btn) return;
    state.category = btn.dataset.cat;
    render();
  });

  // Welcome samples
  const welcomeEl = $('#welcome');
  if (welcomeEl) welcomeEl.addEventListener('click', (e) => {
    const b = e.target.closest('[data-sample]');
    if (!b) return;
    $('#search-input').value = b.dataset.sample;
    doSearch();
  });

  // Search history
  $('#search-input').addEventListener('focus', showHistory);
  $('#search-input').addEventListener('blur', () => setTimeout(hideHistory, 120));
  $('#history-dropdown').addEventListener('mousedown', (e) => {
    const clear = e.target.closest('.history-clear');
    if (clear) { e.preventDefault(); clearHistory(); return; }
    const del = e.target.closest('.h-del');
    if (del) { e.preventDefault(); removeHistory(del.dataset.del); return; }
    const term = e.target.closest('.history-term');
    if (!term) return;
    e.preventDefault();
    $('#search-input').value = term.dataset.q;
    hideHistory();
    if (state.view !== 'search') switchView('search');
    doSearch();
  });

  // Keyboard shortcuts
  document.addEventListener('keydown', async (e) => {
    const mod = e.ctrlKey || e.metaKey;
    // ESC: close modals
    if (e.key === 'Escape') {
      if (!$('#detail-modal').hidden) { closeDetail(); return; }
      if (!$('#settings-modal').hidden) { closeSettings(); return; }
      return;
    }
    // Don't fire shortcuts when typing in inputs (except for global ones)
    const tag = document.activeElement && document.activeElement.tagName;
    const inInput = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    // Ctrl+K / / : focus search
    if ((mod && e.key === 'k') || (!inInput && e.key === '/')) {
      e.preventDefault();
      $('#search-input').focus();
      $('#search-input').select();
      return;
    }
    if (inInput) return;
    // Ctrl+D: push first visible result to downloader
    if (mod && e.key === 'd') {
      e.preventDefault();
      if (state.view === 'search') {
        const { visibleResults } = await import('./render.js');
        const list = visibleResults();
        if (list.length) {
          const { ensureMagnet, sendToClient } = await import('./actions.js');
          const m = await ensureMagnet(list[0]);
          if (m) sendToClient(m);
        } else toast('没有可推送的结果');
      }
      return;
    }
    // Ctrl+Shift+F: toggle favorites view
    if (mod && e.shiftKey && e.key === 'F') {
      e.preventDefault();
      switchView(state.view === 'favorites' ? 'search' : 'favorites');
      return;
    }
  });

  // Infinite scroll
  const io = new IntersectionObserver((entries) => {
    if (entries[0].isIntersecting && state.hasMore && !state.loading) loadPage();
  }, { rootMargin: '400px' });
  io.observe($('#sentinel'));
}

// ---------- init ----------
async function init() {
  // Restore theme preference
  try {
    if (localStorage.getItem('theme') === 'light') {
      document.documentElement.classList.add('light');
    }
  } catch { /* ignore */ }

  try {
    await loadProviders();
  } catch (e) {
    toast('无法加载引擎列表');
  }
  autoDetectDownloader();
  renderFavCount();

  const initQ = new URLSearchParams(location.search).get('q');
  if (initQ) {
    $('#search-input').value = initQ;
    doSearch();
  }
}

bindEvents();
init();