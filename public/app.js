'use strict';

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => document.querySelectorAll(sel);

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
  dl: loadDownloader(),    // 下载客户端配置 { client, url, user, pass, token }（含旧 qb 迁移）
  quality: 'all',          // 画质快捷筛选：all / 2160p / 1080p / 720p / hdr
  view: 'search',          // 当前视图：search / favorites
  history: JSON.parse(localStorage.getItem('history') || '[]'),   // 最近搜索词
  favorites: JSON.parse(localStorage.getItem('favorites') || '[]'), // 收藏的种子
  checked: new Set(),      // 批量操作：已勾选的卡片 key（跨重渲染按 key 保持）
};

const PROVIDER_LABEL = { '1337x': '1337x', tpb: 'The Pirate Bay', nyaa: 'NYAA', demo: 'Demo' };

// ---------- 下载客户端 ----------
// 支持把磁力推送到本机运行的下载器。auth 决定设置面板显示哪些字段：
// userpass（用户名/密码）或 token（RPC 密钥 / API Token）。与后端 downloaders.META 对应。
const DL_CLIENTS = {
  qbittorrent: { label: 'qBittorrent', auth: 'userpass', defaultUrl: 'http://localhost:8080' },
  transmission: { label: 'Transmission', auth: 'userpass', defaultUrl: 'http://localhost:9091' },
  aria2: { label: 'aria2 / Motrix', auth: 'token', defaultUrl: 'http://localhost:16800/jsonrpc' },
  gopeed: { label: 'Gopeed', auth: 'token', defaultUrl: 'http://localhost:9999' },
};

function dlLabel() {
  const c = state.dl && state.dl.client;
  return (c && DL_CLIENTS[c] && DL_CLIENTS[c].label) || '下载器';
}

// 读取下载器配置：优先新键 dl；若不存在但有旧 qb 配置，迁移为 { client:'qbittorrent', ... }，
// 让老用户升级后 qBittorrent 设置无缝延续。迁移后写回 dl 键，旧 qb 键留着不动（无害）。
function loadDownloader() {
  try {
    const dl = JSON.parse(localStorage.getItem('dl') || 'null');
    if (dl && dl.client) return dl;
    const qb = JSON.parse(localStorage.getItem('qb') || 'null');
    if (qb && qb.url) {
      const migrated = { client: 'qbittorrent', url: qb.url, user: qb.user || '', pass: qb.pass || '', token: '' };
      localStorage.setItem('dl', JSON.stringify(migrated));
      return migrated;
    }
  } catch { /* ignore */ }
  return null;
}

const HISTORY_MAX = 12;

// 画质匹配规则：把标题里常见的清晰度/HDR 写法归一到快捷标签。
// 用正则而非简单 includes，兼顾 4k/2160p、带分隔符的 h.265 之类写法。
const QUALITY_PATTERNS = {
  '2160p': /\b(2160p|4k|uhd)\b/i,
  '1080p': /\b1080p\b/i,
  '720p': /\b720p\b/i,
  hdr: /\b(hdr|hdr10|dolby\s*vision|dovi|dv)\b/i,
};

function matchesQuality(name, q) {
  if (q === 'all') return true;
  const pat = QUALITY_PATTERNS[q];
  return pat ? pat.test(String(name || '')) : true;
}

// ---------- provider groups ----------
// 引擎按站点性质预设分组，UI 里分区展示、支持整组一键全选/全不选。
// 维护提醒：新增 provider 时在此登记其组归属；未登记的内置引擎回退到「综合」，
// 动态 Torznab 引擎（id 以 torznab: 开头）归到「自定义」。
const GROUP_ORDER = ['anime', 'video', 'general', 'adult', 'other'];
const GROUP_LABELS = {
  anime: '动漫',
  video: '影视',
  general: '综合',
  adult: '成人',
  other: '其他',
  custom: '自定义',
};
const PROVIDER_GROUP = {
  // 动漫 / 亚洲
  nyaa: 'anime', anilibria: 'anime', animetosho: 'anime', anirena: 'anime',
  bangumimoe: 'anime', dmhy: 'anime', mikan: 'anime', subsplease: 'anime',
  tokyotoshokan: 'anime', nekobt: 'anime',
  // 影视 / 剧集
  eztv: 'video', yts: 'video', therarbg: 'video', torrent9: 'video',
  oxtorrent: 'video', rutor: 'video', megapeer: 'video',
  // 综合
  '1337x': 'general', tpb: 'general', knaben: 'general', torrentscsv: 'general',
  bt4g: 'general', btdigg: 'general', limetorrents: 'general',
  torrentdownload: 'general', torrentdownloads: 'general', torrentdatabase: 'general',
  torrentkitty: 'general', uindex: 'general', zeromagnet: 'general',
  bitsearch: 'general', internetarchive: 'general', filemood: 'general', demo: 'general',
  // 成人
  sukebei: 'adult', mypornclub: 'adult', xxxclub: 'adult', xxxtracker: 'adult',
  // 其他（有声书 / ROM / Linux 发行版）
  audiobookbay: 'other', blueroms: 'other', linuxtracker: 'other',
};

function providerGroupOf(p) {
  if (p.id && p.id.startsWith('torznab:')) return 'custom';
  return PROVIDER_GROUP[p.id] || 'general';
}

// 引擎选择持久化：记住用户勾选的引擎，下次打开恢复。
function saveSelectedProviders() {
  localStorage.setItem('providers-selected', JSON.stringify([...state.selected]));
}
function loadSelectedProviders() {
  try {
    const arr = JSON.parse(localStorage.getItem('providers-selected') || 'null');
    return Array.isArray(arr) ? arr : null;
  } catch { return null; }
}

// ---------- providers ----------
// 已知的全部 provider（loadProviders 填充），供分组渲染与全选/反选复用。
let allProviders = [];

async function loadProviders() {
  try {
    const r = await fetch('/api/providers');
    const { providers } = await r.json();
    allProviders = providers;

    // 恢复上次勾选：有持久化记录则用记录（与当前可用引擎取交集，避免残留已删除的
    // Torznab id），否则回退到各引擎的默认 enabled。
    const saved = loadSelectedProviders();
    state.selected = new Set(
      saved
        ? providers.filter((p) => saved.includes(p.id)).map((p) => p.id)
        : providers.filter((p) => p.enabled).map((p) => p.id)
    );

    renderProviderChips();
  } catch (e) {
    toast('无法加载引擎列表');
  }
}

// 按预设分组渲染引擎 chips：每组一个可折叠区块，组标题带「全选/全不选」，
// 并显示该组已选/总数。空组不渲染。
function renderProviderChips() {
  const wrap = $('#provider-chips');
  wrap.innerHTML = '';

  // 按组归拢。custom（Torznab）单列在末尾。
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
      const el = document.createElement('div');
      el.className = `chip${on ? ' on' : ''}${p.demo ? ' demo' : ''}`;
      el.dataset.id = p.id;
      el.innerHTML = `<span class="dot"></span>${esc(p.name)}`;
      el.onclick = () => toggleProvider(p.id);
      chips.appendChild(el);
    });
    section.appendChild(chips);
    wrap.appendChild(section);
  });
}

function toggleProvider(id) {
  if (state.selected.has(id)) state.selected.delete(id);
  else state.selected.add(id);
  saveSelectedProviders();
  renderProviderChips();
  if (state.query) doSearch();
}

// 整组全选 / 全不选：该组已全选则清空该组，否则补齐该组。
function toggleGroup(g) {
  const list = allProviders.filter((p) => providerGroupOf(p) === g);
  const allOn = list.every((p) => state.selected.has(p.id));
  list.forEach((p) => {
    if (allOn) state.selected.delete(p.id);
    else state.selected.add(p.id);
  });
  saveSelectedProviders();
  renderProviderChips();
  if (state.query) doSearch();
}

// ---------- search ----------
async function doSearch() {
  state.query = $('#search-input').value.trim();
  if (!state.query) return;
  // 搜索时切回搜索视图并记录历史。
  pushHistory(state.query);
  if (state.view !== 'search') switchView('search');
  hideHistory();
  // 开启一次全新搜索：作废所有在途请求（快速改词/连点引擎时，晚到的旧响应
  // 不能覆盖新结果），重置分页与累积状态。
  state.searchId++;
  if (state.abort) { state.abort.abort(); state.abort = null; }
  state.loading = false;
  state.page = 1;
  state.all = [];
  state.seen = new Set();
  state.groups = new Map();
  // 新搜索会重建 groups，旧勾选的 key 全部失效，清空避免残留计数。
  state.checked.clear();
  renderBatchBar();
  await loadPage();
}

// Stream a page of results over SSE. Each provider's results and status arrive
// incrementally (one `provider` event apiece), so the list and status bar light
// up as engines return rather than waiting for the slowest one.
function loadPage() {
  // 正在加载中，或已无更多结果且不是首页，直接跳过。
  if (state.loading) return;
  if (!state.hasMore && state.page > 1) return;
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
      files: it.files != null ? it.files : null,
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
  if (g.files == null && it.files != null) g.files = it.files;
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
    if (!matchesQuality(it.name, state.quality)) return false;
    return true;
  });

  const dir = state.order === 'desc' ? -1 : 1;
  if (state.sort === 'relevance') {
    // 相关度：按标题与关键词的匹配度打分降序，同分用做种数兜底。
    // 这是启发式打分（见 relevanceScore），不是严格检索模型，结果仅供参考。
    list.forEach((it) => { it._score = relevanceScore(it.name, state.query); });
    list.sort((a, b) => (b._score - a._score) || ((b.seeders ?? -1) - (a.seeders ?? -1)));
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

// 轻量标题相关度打分：把关键词分词后按命中强度累加。
// 常见启发式思路——完整短语 > 前缀 > 精确词 > 子串，再叠加词覆盖率；
// 不是 BM25/TF-IDF 那类严格模型，中文无分词时主要靠子串命中，故结果仅供参考。
function relevanceScore(name, query) {
  const n = String(name || '').toLowerCase();
  const q = String(query || '').toLowerCase().trim();
  if (!q) return 0;
  let score = 0;
  if (n.includes(q)) score += 5;      // 完整短语命中，最强信号
  if (n.startsWith(q)) score += 3;    // 出现在标题开头，额外加权
  const split = (s) => s.split(/[^a-z0-9一-鿿]+/).filter(Boolean);
  const nTokens = split(n);
  const qTokens = split(q);
  const nSet = new Set(nTokens);
  let hit = 0;
  for (const t of qTokens) {
    if (nSet.has(t)) { score += 2; hit++; }                            // 精确词命中
    else if (nTokens.some((w) => w.startsWith(t))) { score += 1; hit++; } // 前缀命中
    else if (n.includes(t)) { score += 0.5; hit++; }                  // 子串命中
  }
  if (qTokens.length) score += (hit / qTokens.length) * 3;            // 词覆盖率加权
  return score;
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
  const dlBtn = state.dl ? `<button class="btn qb" data-act="dl" data-id="${esc(it.key)}">推送到 ${esc(dlShort())}</button>` : '';
  const provs = it.providers && it.providers.length ? it.providers : (it.sources || []).map((s) => s.provider);
  const sourceBadges = provs
    .map((pid) => `<span class="badge prov-${pid}">${esc(PROVIDER_LABEL[pid] || pid)}</span>`)
    .join('');
  const multi = provs.length > 1 ? `<span class="badge multi">${provs.length} 个来源</span>` : '';
  const faved = isFavorited(it.key);
  const favBtn = `<button class="fav-btn${faved ? ' on' : ''}" data-act="fav" data-id="${esc(it.key)}" title="${faved ? '取消收藏' : '收藏'}">${faved ? '★' : '☆'}</button>`;
  const checked = state.checked.has(it.key);
  const checkBox = `<input type="checkbox" class="card-check" data-act="check" data-id="${esc(it.key)}"${checked ? ' checked' : ''} title="选择用于批量操作" />`;
  return `
  <div class="card${checked ? ' checked' : ''}" data-id="${esc(it.key)}">
    ${checkBox}
    ${favBtn}
    <div class="name">${highlight(it.name, state.query)}</div>
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
      ${dlBtn}
      <button class="btn ghost" data-act="detail" data-id="${esc(it.key)}">详情</button>
    </div>
  </div>`;
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// 把标题里命中搜索词的片段包上 <mark> 高亮。先按空白拆词，逐词在（已转义的）
// 标题上做不区分大小写的整体替换。先 esc 再高亮，避免 XSS；高亮标签本身可信。
function highlight(name, query) {
  const safe = esc(name);
  const q = String(query || '').trim();
  if (!q) return safe;
  const tokens = [...new Set(q.split(/\s+/).filter((t) => t.length >= 1))]
    .map(esc)
    .filter(Boolean)
    // 长词优先，避免短词先匹配把长词拆断。
    .sort((a, b) => b.length - a.length)
    .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')); // 转义正则元字符
  if (!tokens.length) return safe;
  const re = new RegExp(`(${tokens.join('|')})`, 'gi');
  return safe.replace(re, '<span class="hl">$1</span>');
}

// ---------- actions ----------
// 卡片 data-id 用的是分组 key。搜索视图从 groups 取；收藏视图的卡片不在 groups
// 里（可能来自上次会话），故回退到收藏数据。收藏对象存的就是分组快照，形状一致。
async function getItem(key) {
  return state.groups.get(key) || state.favorites.find((f) => f.key === key) || null;
}

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

// 卡片动作处理：搜索视图与收藏视图共用同一套按钮逻辑。
async function onCardClick(e) {
  const btn = e.target.closest('[data-act]');
  if (!btn) return;
  const id = btn.dataset.id;
  const it = await getItem(id);
  if (!it) return;
  const act = btn.dataset.act;

  if (act === 'detail') {
    openDetail(it);
    return;
  }
  if (act === 'check') {
    toggleChecked(it.key);
    return;
  }
  if (act === 'fav') {
    toggleFavorite(it);
    return;
  }
  if (act === 'getmagnet') {
    btn.textContent = '获取中…'; btn.disabled = true;
    const m = await ensureMagnet(it);
    if (m) { renderCurrentView(); toast('已获取磁力链接'); }
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
  if (act === 'dl') {
    const m = await ensureMagnet(it);
    if (m) sendToClient(m);
    return;
  }
}

$('#results').addEventListener('click', onCardClick);
$('#favorites').addEventListener('click', onCardClick);

// ---------- detail preview ----------
// 务实版详情：不为 40 个站逐个抓文件树，而是把已有的聚合信息集中展示——
// 标题、统计、分类、infoHash、磁力（可现取），以及各来源的详情页外链，
// 让用户跳到站点自行核对文件列表。所有值都经 esc 转义后注入弹窗。
async function openDetail(it) {
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

  // 各来源的详情页外链：有 detailUrl 的列成可点链接，供用户去站点看文件列表。
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

  // 弹窗内的动作按钮：随磁力是否就绪切换「获取磁力」/「打开·复制」。
  const actions = $('#detail-actions');
  const key = esc(it.key);
  const dlBtn = state.dl ? `<button class="btn qb" data-act="dl" data-id="${key}">推送到 ${esc(dlShort())}</button>` : '';
  actions.innerHTML = (it.needsMagnet && !it.magnet)
    ? `<button class="btn" data-act="getmagnet" data-id="${key}">获取磁力</button>${dlBtn}`
    : `<button class="btn primary" data-act="open" data-id="${key}">打开磁力</button>` +
      `<button class="btn" data-act="copy" data-id="${key}">复制磁力</button>${dlBtn}`;

  modal.hidden = false;
}

function closeDetail() { $('#detail-modal').hidden = true; }

// 详情弹窗内的动作复用 onCardClick 那套（data-act/data-id），但需在动作后刷新
// 弹窗内容（如获取磁力成功后，把「获取磁力」换成「打开/复制」）。
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
  if (act === 'copy') { const m = await ensureMagnet(it); if (m) copyText(m); return; }
  if (act === 'open') { const m = await ensureMagnet(it); if (m) window.location.href = m; return; }
  if (act === 'dl') { const m = await ensureMagnet(it); if (m) sendToClient(m); return; }
});

// ---------- batch operations ----------
// 勾选集按 key 保持，跨重渲染稳定。勾/取消勾后刷新当前视图（复选框态、卡片高亮）
// 与批量工具条（显隐、计数）。
function toggleChecked(key) {
  if (state.checked.has(key)) state.checked.delete(key);
  else state.checked.add(key);
  renderCurrentView();
  renderBatchBar();
}

function clearChecked() {
  state.checked.clear();
  renderCurrentView();
  renderBatchBar();
}

// 取当前视图内被勾选的条目对象（搜索视图从 groups，收藏视图从 favorites）。
// 只保留仍存在的 key，顺带清理已消失的残留勾选。
function checkedItems() {
  const pool = state.view === 'favorites' ? state.favorites : [...state.groups.values()];
  const byKey = new Map(pool.map((it) => [it.key, it]));
  const items = [];
  for (const key of state.checked) {
    if (byKey.has(key)) items.push(byKey.get(key));
    else state.checked.delete(key);
  }
  return items;
}

// 批量工具条：有勾选时浮出，显示选中数与三个批量动作。
function renderBatchBar() {
  const bar = $('#batch-bar');
  if (!bar) return;
  const n = checkedItems().length;
  if (!n) { bar.hidden = true; return; }
  bar.hidden = false;
  $('#batch-count').textContent = n;
  // qB 推送按钮仅在已配置 qBittorrent 时可用。
  const qbBtn = $('#batch-qb');
  if (qbBtn) qbBtn.hidden = !state.qb;
}

// 批量复制磁力：逐条 ensureMagnet（可能触发懒解析），拿到的磁力用换行拼接复制。
// 单条失败不阻断其余；全失败给出提示。
async function batchCopyMagnets() {
  const items = checkedItems();
  if (!items.length) return;
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

// 批量推送到 qBittorrent：串行推送，避免瞬时打爆 WebUI。逐条汇总成功数。
async function batchSendToQB() {
  if (!state.qb) { openSettings(); toast('请先配置 qBittorrent'); return; }
  const items = checkedItems();
  if (!items.length) return;
  toast(`正在推送 ${items.length} 条到 qBittorrent…`);
  let ok = 0;
  for (const it of items) {
    const m = await ensureMagnet(it).catch(() => null);
    if (!m) continue;
    try {
      const r = await fetch('/api/download/qbittorrent', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...state.qb, magnet: m }),
      });
      const data = await r.json();
      if (data.ok) ok++;
    } catch { /* 单条失败不阻断 */ }
  }
  toast(`已推送 ${ok}/${items.length} 条到 qBittorrent`);
}

// 导出 CSV：把勾选条目导出为 CSV 文件下载。字段按每个 CSV 单元格转义（含引号、
// 逗号、换行）。磁力若尚未解析则留空（不为导出触发大量懒解析请求）。
function batchExportCsv() {
  const items = checkedItems();
  if (!items.length) return;
  const cols = ['name', 'seeders', 'leechers', 'sizeText', 'dateText', 'category', 'providers', 'infoHash', 'magnet'];
  const head = ['名称', '做种', '下载', '大小', '时间', '分类', '来源', 'infoHash', '磁力'];
  const cell = (v) => {
    const s = v == null ? '' : String(v);
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const rows = items.map((it) => cols.map((c) => {
    if (c === 'providers') return cell((it.providers || []).join(' | '));
    return cell(it[c]);
  }).join(','));
  // 前置 BOM，Excel 直接双击不乱码。
  const csv = '﻿' + [head.join(','), ...rows].join('\r\n');
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

$('#batch-copy').onclick = batchCopyMagnets;
$('#batch-qb').onclick = batchSendToQB;
$('#batch-csv').onclick = batchExportCsv;
$('#batch-clear').onclick = clearChecked;

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
// 按当前选中的客户端切换认证字段的显隐：userpass 显示用户名/密码，token 显示 token。
function syncDlAuthFields() {
  const kind = $('#dl-client').value;
  const meta = DL_META[kind] || DL_META.qbittorrent;
  $('#dl-userpass').hidden = meta.auth !== 'userpass';
  $('#dl-tokenwrap').hidden = meta.auth !== 'token';
  // aria2 的 token 是 rpc-secret，Gopeed 的是 API Token，提示文案略作区分。
  $('#dl-token-label').textContent = kind === 'aria2'
    ? 'RPC 密钥（rpc-secret，无则留空）'
    : 'API Token（无则留空）';
}

function openSettings() {
  const dl = state.dl || {};
  $('#dl-client').value = dl.client || 'qbittorrent';
  $('#dl-url').value = dl.url || '';
  $('#dl-user').value = dl.user || '';
  $('#dl-pass').value = dl.pass || '';
  $('#dl-token').value = dl.token || '';
  syncDlAuthFields();
  $('#settings-modal').hidden = false;
}
function closeSettings() { $('#settings-modal').hidden = true; }

// 从设置面板收集当前配置。
function readDlForm() {
  return {
    client: $('#dl-client').value,
    url: $('#dl-url').value.trim(),
    user: $('#dl-user').value.trim(),
    pass: $('#dl-pass').value,
    token: $('#dl-token').value.trim(),
  };
}

$('#dl-client').onchange = syncDlAuthFields;
$('#settings-btn').onclick = () => { openSettings(); loadTorznab(); };
$('#settings-cancel').onclick = closeSettings;
$('#settings-save').onclick = () => {
  state.dl = readDlForm();
  localStorage.setItem('downloader', JSON.stringify(state.dl));
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

// 应用一份探测到的配置：写入 state + localStorage，回填表单，刷新卡片按钮。
function applyDetected(d) {
  state.dl = { client: d.kind, url: d.url, user: d.user || '', pass: d.pass || '', token: d.token || '' };
  localStorage.setItem('downloader', JSON.stringify(state.dl));
}

// 首屏静默探测本机下载器（已配置则跳过），零配置命中常见默认端口。
async function autoDetectDownloader() {
  if (state.dl) return; // already configured manually
  try {
    const r = await fetch('/api/download/detect');
    const d = await r.json();
    if (d.ok) { applyDetected(d); render(); }
  } catch (e) { /* detection is best-effort */ }
}

$('#dl-detect').onclick = async () => {
  toast('正在探测本机下载器…');
  try {
    const r = await fetch('/api/download/detect');
    const d = await r.json();
    if (d.ok) {
      applyDetected(d);
      openSettings();
      render();
      toast(`已自动发现并启用 ${DL_META[d.kind]?.label || d.kind}`);
    } else {
      toast('未在本机发现受支持的下载器（请手动填写）');
    }
  } catch (e) { toast('探测失败：网络错误'); }
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

// 组标题「全选/全不选」按钮：chips 是重渲染的，用事件委托而非逐按钮绑定。
$('#provider-chips').addEventListener('click', (e) => {
  const btn = e.target.closest('.pgroup-toggle');
  if (!btn) return;
  toggleGroup(btn.dataset.group);
});

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

// ---------- favorites ----------
// 收藏存的是分组快照（含 magnet / sources），这样即便跨会话、原搜索早已不在，
// 收藏视图也能独立打开磁力/复制/推送。仅存必要字段，避免 localStorage 膨胀。
function saveFavorites() {
  localStorage.setItem('favorites', JSON.stringify(state.favorites));
  renderFavCount();
}

function isFavorited(key) {
  return state.favorites.some((f) => f.key === key);
}

function toggleFavorite(it) {
  const i = state.favorites.findIndex((f) => f.key === it.key);
  if (i >= 0) {
    state.favorites.splice(i, 1);
    saveFavorites();
    toast('已取消收藏');
  } else {
    // 存一份精简快照：卡片渲染与磁力解析所需的字段。
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

function renderFavCount() {
  const el = $('#fav-count');
  if (!el) return;
  const n = state.favorites.length;
  el.textContent = n ? String(n) : '';
  el.hidden = !n;
}

function renderFavorites() {
  const wrap = $('#favorites');
  const empty = $('#favorites-empty');
  if (!state.favorites.length) {
    wrap.innerHTML = '';
    empty.hidden = false;
    return;
  }
  empty.hidden = true;
  // 收藏视图不套搜索筛选，但同样走关键词高亮（此时 query 可能为空，highlight 会原样返回）。
  wrap.innerHTML = state.favorites.map(cardHTML).join('');
}

// ---------- view switch ----------
function switchView(view) {
  state.view = view;
  $$('.view-tab').forEach((t) => t.classList.toggle('on', t.dataset.view === view));
  $('#search-view').hidden = view !== 'search';
  $('#favorites-view').hidden = view !== 'favorites';
  // 收藏视图下，搜索相关的筛选/引擎控件无意义，隐藏以免误导。
  $('.controls').hidden = view !== 'search';
  $('#status-bar').hidden = view !== 'search' || !Object.keys(state.status).length;
  // 两个视图的可选池不同，切换时清空勾选，避免跨池计数与批量操作混淆。
  state.checked.clear();
  renderCurrentView();
  renderBatchBar();
}

function renderCurrentView() {
  if (state.view === 'favorites') renderFavorites();
  else render();
  renderBatchBar();
}

// ---------- search history ----------
function pushHistory(q) {
  const v = q.trim();
  if (!v) return;
  // 去重（不区分大小写）后置顶，截断到上限。
  state.history = [v, ...state.history.filter((h) => h.toLowerCase() !== v.toLowerCase())]
    .slice(0, HISTORY_MAX);
  localStorage.setItem('history', JSON.stringify(state.history));
}

function removeHistory(q) {
  state.history = state.history.filter((h) => h !== q);
  localStorage.setItem('history', JSON.stringify(state.history));
  renderHistory();
}

function clearHistory() {
  state.history = [];
  localStorage.setItem('history', JSON.stringify(state.history));
  hideHistory();
}

function renderHistory() {
  const box = $('#history-dropdown');
  if (!state.history.length) { box.hidden = true; box.innerHTML = ''; return; }
  const head =
    `<div class="history-head"><span>最近搜索</span>` +
    `<button class="history-clear" data-clear="1">清空全部</button></div>`;
  const items = state.history.map((h) =>
    `<div class="history-item" data-q="${esc(h)}">` +
    `<span class="h-icon">🕘</span>` +
    `<span class="h-term">${esc(h)}</span>` +
    `<button class="h-del" data-del="${esc(h)}" title="删除">✕</button>` +
    `</div>`
  ).join('');
  box.innerHTML = head + items;
  box.hidden = false;
}

function showHistory() { if (state.history.length) renderHistory(); }
function hideHistory() { $('#history-dropdown').hidden = true; }

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

// ---------- view / quality / history bindings ----------
$('.views').addEventListener('click', (e) => {
  const tab = e.target.closest('.view-tab');
  if (!tab) return;
  switchView(tab.dataset.view);
});

$('#quality-filters').addEventListener('click', (e) => {
  const btn = e.target.closest('.qbtn');
  if (!btn) return;
  state.quality = btn.dataset.q;
  $$('.qbtn').forEach((b) => b.classList.toggle('on', b === btn));
  render();
});

// 历史下拉：聚焦展示，点选填入并搜索，✕ 删除单条。用 mousedown 抢在 blur 之前，
// 否则输入框 blur 先隐藏下拉，点击落空。
$('#search-input').addEventListener('focus', showHistory);
$('#search-input').addEventListener('blur', () => setTimeout(hideHistory, 120));

$('#history-dropdown').addEventListener('mousedown', (e) => {
  const clear = e.target.closest('.history-clear');
  if (clear) {
    e.preventDefault();
    state.history = [];
    localStorage.setItem('history', JSON.stringify(state.history));
    hideHistory();
    return;
  }
  const del = e.target.closest('.h-del');
  if (del) {
    e.preventDefault();
    removeHistory(del.dataset.del);
    return;
  }
  const item = e.target.closest('.history-item');
  if (!item) return;
  e.preventDefault();
  $('#search-input').value = item.dataset.q;
  hideHistory();
  if (state.view !== 'search') switchView('search');
  doSearch();
});

// ---------- init ----------
loadProviders();
autoDetectQB();
renderFavCount();
