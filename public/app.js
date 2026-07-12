'use strict';

const $ = (sel) => document.querySelector(sel);

const state = {
  query: '',
  page: 1,
  selected: new Set(),     // provider ids (empty = all enabled)
  all: [],                 // accumulated normalized results (raw, pre-merge)
  seen: new Set(),         // dedupe ids (同一 provider 同一结果跨页去重)
  groups: new Map(),       // 跨站合并：dedupeKey → 合并后的持久化结果对象
  hasMore: false,
  loading: false,
  searchId: 0,             // 递增令牌：每次新搜索 +1，用于作废在途的旧请求
  abort: null,             // 当前在途请求的 AbortController
  order: 'desc',
  sort: 'relevance',
  status: {},              // provider id → { status, count, error, ms } (streamed)
  es: null,                // active EventSource, so a new search can cancel it
  qb: JSON.parse(localStorage.getItem('qb') || 'null'),
};

// Map of provider id → display name, populated from /api/providers.
// Used by the status bar and result cards; falls back to the raw id.
const PROVIDER_LABEL = {};

// ---------- providers ----------
async function loadProviders() {
  try {
    const r = await fetch('/api/providers');
    const { providers } = await r.json();
    const chips = $('#provider-chips');
    chips.innerHTML = '';
    providers.forEach((p) => {
      PROVIDER_LABEL[p.id] = p.name;
      const el = document.createElement('div');
      el.className = `chip${p.enabled ? ' on' : ''}${p.demo ? ' demo' : ''}`;
      el.dataset.id = p.id;
      el.innerHTML = `<span class="dot"></span>${p.name}`;
      el.onclick = () => {
        el.classList.toggle('on');
        if (el.classList.contains('on')) state.selected.add(p.id);
        else state.selected.delete(p.id);
        if (state.query) doSearch();
      };
      if (p.enabled) state.selected.add(p.id);
      chips.appendChild(el);
    });
  } catch (e) {
    toast('无法加载引擎列表');
  }
}

// ---------- search ----------
async function doSearch() {
  state.query = $('#search-input').value.trim();
  if (!state.query) return;
  // 开启一次全新搜索：作废所有在途请求（快速改词/连点引擎时，晚到的旧响应
  // 不能覆盖新结果），重置分页与累积状态。
  state.searchId++;
  if (state.abort) { state.abort.abort(); state.abort = null; }
  state.loading = false;
  state.page = 1;
  state.all = [];
  state.seen = new Set();
  state.groups = new Map();
  await loadPage();
}

// Stream a page of results over SSE. Each provider's results and status arrive
// incrementally (one `provider` event apiece), so the list and status bar light
// up as engines return rather than waiting for the slowest one.
function loadPage() {
  if (state.loading || (!state.hasMore && state.page > 1)) return;
  state.loading = true;
  $('#loading').hidden = false;

  // Reset the per-provider status map at the start of a fresh search (page 1).
  if (state.page === 1) state.status = {};

  // 绑定本次流所属的搜索令牌：SSE 没有 fetch 可 abort，改用令牌 + es.close()
  // 作废旧流。晚到的旧流事件若发现 searchId 已变，直接丢弃，消除竞态覆盖。
  const myId = state.searchId;

  const params = new URLSearchParams({
    q: state.query,
    page: state.page,
  });
  if (state.selected.size) params.set('providers', [...state.selected].join(','));

  // Tear down any previous stream before opening a new one.
  if (state.es) { state.es.close(); state.es = null; }
  const es = new EventSource(`/api/search/stream?${params}`);
  state.es = es;

  const finish = () => {
    if (state.es === es) state.es = null;
    es.close();
    if (myId !== state.searchId) return; // 已被新搜索取代，别动新搜索的加载态
    state.loading = false;
    $('#loading').hidden = true;
    if (state.hasMore) state.page++;
  };

  es.addEventListener('provider', (ev) => {
    if (myId !== state.searchId) { es.close(); return; } // 旧流，结果作废
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
    // EventSource fires `error` both on server-sent error events and on
    // transport failure; either way we stop this page's stream.
    if (myId === state.searchId) {
      if (state.all.length === 0 && Object.keys(state.status).length === 0) {
        toast('搜索请求失败');
      }
      state.hasMore = false;
    }
    finish();
  });
}

// 把一条归一化结果并入其所属分组：带 infoHash 的按 hash 合并成一张卡（同一资源
// 多站命中只显示一次），无 hash 的（如 1337x 需懒解析磁力）以自身 id 独立成组。
// 做种/下载取各来源最大值，磁力优先保留已就绪的，来源站去重累积。
function mergeResult(it) {
  const key = it.infoHash ? `hash:${String(it.infoHash).toLowerCase()}` : it.id;
  let g = state.groups.get(key);
  if (!g) {
    g = {
      key,
      name: it.name,
      size: it.size,
      sizeText: it.sizeText,
      seeders: it.seeders,
      leechers: it.leechers,
      date: it.date,
      dateText: it.dateText,
      category: it.category,
      infoHash: it.infoHash || null,
      magnet: it.magnet || null,
      needsMagnet: !it.magnet && !!it.detailUrl,
      providers: [],
      sources: [],
    };
    state.groups.set(key, g);
  }
  g.sources.push({
    provider: it.provider,
    magnet: it.magnet || null,
    detailUrl: it.detailUrl || null,
    id: it.id,
  });
  if (!g.providers.includes(it.provider)) g.providers.push(it.provider);
  // 聚合：取最强信号。
  if (it.seeders != null && (g.seeders == null || it.seeders > g.seeders)) g.seeders = it.seeders;
  if (it.leechers != null && (g.leechers == null || it.leechers > g.leechers)) g.leechers = it.leechers;
  if (g.size == null && it.size != null) { g.size = it.size; g.sizeText = it.sizeText; }
  if (g.date == null && it.date != null) { g.date = it.date; g.dateText = it.dateText; }
  // 已就绪的磁力优先于无磁力。
  if (!g.magnet && it.magnet) { g.magnet = it.magnet; g.needsMagnet = false; }
  if (!g.magnet && it.detailUrl) g.needsMagnet = true;
  return g;
}

function renderStatus(providers) {
  const bar = $('#status-bar');
  const entries = Object.entries(providers);
  if (!entries.length) { bar.hidden = true; return; }
  bar.hidden = false;
  bar.innerHTML = entries.map(([id, s]) => {
    const cls = s.status === 'ok' ? 'ok' : 'err';
    const mark = s.status === 'ok' ? '✓' : '✕';
    const label = PROVIDER_LABEL[id] || id;
    const ms = s.ms != null ? ` ${s.ms}ms` : '';
    const detail = s.error ? ` (${s.error})` : ` · ${s.count} 条${ms}`;
    return `<span class="status-pill"><b>${label}</b> <span class="${cls}">${mark}${detail}</span></span>`;
  }).join('');
}

// ---------- client-side filter + sort ----------
// 面向分组（去重后的资源），而非原始逐条结果。
function visibleResults() {
  const minSeed = parseInt($('#min-seeders').value, 10) || 0;
  const minSize = parseInt($('#min-size').value, 10) || 0;
  const name = $('#name-contains').value.trim().toLowerCase();

  let list = [...state.groups.values()].filter((it) => {
    if (it.seeders != null && it.seeders < minSeed) return false;
    if (it.size != null && it.size < minSize) return false;
    if (name && !it.name.toLowerCase().includes(name)) return false;
    return true;
  });

  const dir = state.order === 'desc' ? -1 : 1;
  if (state.sort !== 'relevance') {
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

function render() {
  const wrap = $('#results');
  const list = visibleResults();
  if (state.groups.size === 0) {
    wrap.innerHTML = '';
    // While a stream is still in flight, let the "加载中…" indicator speak;
    // only declare "no results" once every provider has reported.
    $('#empty').hidden = state.loading;
    $('#empty').textContent = '没有结果。试试别的关键词，或检查引擎状态。';
    return;
  }
  $('#empty').hidden = list.length > 0;
  if (list.length === 0) {
    $('#empty').textContent = '没有符合筛选条件的结果，试着放宽筛选。';
  }
  wrap.innerHTML = list.map(cardHTML).join('');
}

// it 是一个分组对象（见 mergeResult）。data-id 用分组 key，来源徽章列出所有命中站，
// 多站命中时额外标注“N 个来源”。
function cardHTML(it) {
  const seed = it.seeders != null ? it.seeders : '—';
  const leech = it.leechers != null ? it.leechers : '—';
  const size = it.sizeText || '—';
  const date = it.dateText || '—';
  const cat = it.category ? `<span class="badge">${esc(it.category)}</span>` : '';
  const magnetBtn = it.needsMagnet
    ? `<button class="btn" data-act="getmagnet" data-id="${esc(it.key)}">获取磁力</button>`
    : `<button class="btn primary" data-act="open" data-id="${esc(it.key)}">打开磁力</button>
       <button class="btn" data-act="copy" data-id="${esc(it.key)}">复制</button>`;
  const qbBtn = state.qb ? `<button class="btn qb" data-act="qb" data-id="${esc(it.key)}">推送到 qB</button>` : '';
  const provs = it.providers && it.providers.length ? it.providers : (it.sources || []).map((s) => s.provider);
  const sourceBadges = provs
    .map((pid) => `<span class="badge prov-${pid}">${esc(PROVIDER_LABEL[pid] || pid)}</span>`)
    .join('');
  const multi = provs.length > 1 ? `<span class="badge multi">${provs.length} 个来源</span>` : '';
  return `
  <div class="card" data-id="${esc(it.key)}">
    <div class="name">${esc(it.name)}</div>
    <div class="badges">
      ${sourceBadges}
      ${multi}
      ${cat}
    </div>
    <div class="stats">
      <span class="seed">▲ 做种 <b>${seed}</b></span>
      <span class="leech">▼ 下载 <b>${leech}</b></span>
      <span>大小 <b>${size}</b></span>
      <span>时间 <b>${date}</b></span>
    </div>
    <div class="actions">
      ${magnetBtn}
      ${qbBtn}
    </div>
  </div>`;
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// ---------- actions ----------
// 卡片 data-id 用的是分组 key，因此按 key 从 groups 里取。
async function getItem(key) { return state.groups.get(key); }

// 确保分组拿到磁力：已有则直接返回；否则挑一个带 detailUrl 的来源做懒解析。
async function ensureMagnet(g) {
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

$('#results').addEventListener('click', async (e) => {
  const btn = e.target.closest('[data-act]');
  if (!btn) return;
  const id = btn.dataset.id;
  const it = await getItem(id);
  if (!it) return;
  const act = btn.dataset.act;

  if (act === 'getmagnet') {
    btn.textContent = '获取中…'; btn.disabled = true;
    const m = await ensureMagnet(it);
    if (m) { render(); toast('已获取磁力链接'); }
    return;
  }
  if (act === 'copy') {
    const m = await ensureMagnet(it);
    if (m) copyText(m);
    return;
  }
  if (act === 'open') {
    const m = await ensureMagnet(it);
    if (m) window.location.href = m;
    return;
  }
  if (act === 'qb') {
    const m = await ensureMagnet(it);
    if (m) sendToQB(m);
    return;
  }
});

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast('磁力链接已复制');
  } catch {
    const ta = document.createElement('textarea');
    ta.value = text; document.body.appendChild(ta); ta.select();
    try { document.execCommand('copy'); toast('磁力链接已复制'); }
    catch { toast('复制失败，请手动复制'); }
    document.body.removeChild(ta);
  }
}

async function sendToQB(magnet) {
  if (!state.qb) { openSettings(); toast('请先配置 qBittorrent'); return; }
  toast('正在推送到 qBittorrent…');
  try {
    const r = await fetch('/api/download/qbittorrent', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ...state.qb, magnet }),
    });
    const data = await r.json();
    if (data.ok) toast('已添加到 qBittorrent');
    else toast('推送失败：' + (data.error || '未知错误'));
  } catch (e) {
    toast('推送失败：网络错误');
  }
}

// ---------- settings ----------
function openSettings() {
  $('#qb-url').value = state.qb?.url || '';
  $('#qb-user').value = state.qb?.user || '';
  $('#qb-pass').value = state.qb?.pass || '';
  $('#settings-modal').hidden = false;
}
function closeSettings() { $('#settings-modal').hidden = true; }

$('#settings-btn').onclick = () => { openSettings(); loadTorznab(); };
$('#settings-cancel').onclick = closeSettings;
$('#settings-save').onclick = () => {
  state.qb = {
    url: $('#qb-url').value.trim(),
    user: $('#qb-user').value.trim(),
    pass: $('#qb-pass').value,
  };
  localStorage.setItem('qb', JSON.stringify(state.qb));
  closeSettings();
  render();
  toast('已保存下载工具设置');
};
$('#qb-test').onclick = async () => {
  const cfg = { url: $('#qb-url').value.trim(), user: $('#qb-user').value.trim(), pass: $('#qb-pass').value };
  if (!cfg.url) return toast('请先填写地址');
  const r = await fetch('/api/download/qbittorrent', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...cfg, magnet: 'magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678' }),
  });
  const data = await r.json();
  toast(data.ok ? '连接成功' : ('连接失败：' + (data.error || '')));
};

// Auto-detect a local qBittorrent WebUI (stock installs need no config).
async function autoDetectQB() {
  if (state.qb) return; // already configured manually
  try {
    const r = await fetch('/api/download/qbittorrent/detect');
    const d = await r.json();
    if (d.ok) {
      state.qb = { url: d.url, user: d.user, pass: d.pass };
      localStorage.setItem('qb', JSON.stringify(state.qb));
      render();
    }
  } catch (e) { /* detection is best-effort */ }
}

$('#qb-detect').onclick = async () => {
  toast('正在探测本机 qBittorrent…');
  const r = await fetch('/api/download/qbittorrent/detect');
  const d = await r.json();
  if (d.ok) {
    state.qb = { url: d.url, user: d.user, pass: d.pass };
    localStorage.setItem('qb', JSON.stringify(state.qb));
    $('#qb-url').value = d.url; $('#qb-user').value = d.user; $('#qb-pass').value = d.pass;
    render();
    toast('已自动发现并启用 qBittorrent');
  } else {
    toast('未在本机发现 qBittorrent WebUI（请手动填写）');
  }
};

// ---------- controls ----------
$('#search-form').addEventListener('submit', (e) => { e.preventDefault(); doSearch(); });
$('#sort-select').onchange = (e) => { state.sort = e.target.value; render(); };
$('#order-btn').onclick = () => {
  state.order = state.order === 'desc' ? 'asc' : 'desc';
  $('#order-btn').textContent = state.order === 'desc' ? '↓' : '↑';
  render();
};
['min-seeders', 'min-size', 'name-contains'].forEach((id) => {
  $(`#${id}`).addEventListener('input', () => render());
});
$('#reset-filters').onclick = () => {
  $('#min-seeders').value = ''; $('#min-size').value = '0'; $('#name-contains').value = '';
  render();
};

// ---------- infinite scroll ----------
const io = new IntersectionObserver((entries) => {
  if (entries[0].isIntersecting && state.hasMore && !state.loading) loadPage();
}, { rootMargin: '400px' });
io.observe($('#sentinel'));

// ---------- toast ----------
let toastTimer;
function toast(msg) {
  const t = $('#toast');
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 2200);
}

// ---------- torznab indexers ----------
async function loadTorznab() {
  const wrap = $('#torznab-list');
  if (!wrap) return;
  try {
    const r = await fetch('/api/torznab');
    const { indexers } = await r.json();
    wrap.innerHTML = '';
    (indexers || []).forEach((it) => {
      const row = document.createElement('div');
      row.className = 'tn-item';
      row.innerHTML =
        `<span class="tn-name">${esc(it.name)}</span>` +
        `<span class="tn-url">${esc(it.url)}</span>` +
        `<button class="btn ghost tn-del" data-id="${esc(it.id)}">删除</button>`;
      wrap.appendChild(row);
    });
  } catch (e) { /* ignore */ }
}

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

// ---------- init ----------
loadProviders();
autoDetectQB();
