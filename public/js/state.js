'use strict';

// ---------- DOM helpers (re-exported for all modules) ----------
export const $ = (sel) => document.querySelector(sel);
export const $$ = (sel) => document.querySelectorAll(sel);

// ---------- localStorage helpers ----------
export function loadJSON(key, fallback) {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return JSON.parse(raw);
  } catch { return fallback; }
}

export function saveJSON(key, value) {
  localStorage.setItem(key, JSON.stringify(value));
}

// ---------- download client config ----------
export const DL_CLIENTS = {
  qbittorrent: { label: 'qBittorrent', auth: 'userpass', defaultUrl: 'http://localhost:8080' },
  transmission: { label: 'Transmission', auth: 'userpass', defaultUrl: 'http://localhost:9091' },
  aria2: { label: 'aria2 / Motrix', auth: 'token', defaultUrl: 'http://localhost:16800/jsonrpc' },
  gopeed: { label: 'Gopeed', auth: 'token', defaultUrl: 'http://localhost:9999' },
};

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

function loadSafeMode() {
  try { return localStorage.getItem('safeMode') === 'true'; }
  catch { return false; }
}

export function saveSafeMode(val) {
  localStorage.setItem('safeMode', val ? 'true' : 'false');
}

function loadViewed() {
  try {
    const arr = JSON.parse(localStorage.getItem('viewed') || '[]');
    return Array.isArray(arr) ? new Set(arr) : new Set();
  } catch { return new Set(); }
}

export function saveViewed(viewed) {
  localStorage.setItem('viewed', JSON.stringify([...viewed]));
}

export function dlLabel(dl) {
  const c = dl && dl.client;
  return (c && DL_CLIENTS[c] && DL_CLIENTS[c].label) || '下载器';
}

export function dlShort(dl) {
  const c = dl && dl.client;
  if (!c) return '下载器';
  const s = { qbittorrent: 'qB', transmission: 'TR', aria2: 'Aria2', gopeed: 'Gopeed' };
  return s[c] || c;
}

// ---------- quality patterns ----------
export const QUALITY_PATTERNS = {
  '2160p': /\b(2160p|4k|uhd)\b/i,
  '1080p': /\b1080p\b/i,
  '720p': /\b720p\b/i,
  hdr: /\b(hdr|hdr10|dolby\s*vision|dovi|dv)\b/i,
};

export function matchesQuality(name, q) {
  if (q === 'all') return true;
  const pat = QUALITY_PATTERNS[q];
  return pat ? pat.test(String(name || '')) : true;
}

// ---------- category helpers ----------
export const CATEGORY_LABELS = {
  movies: '电影', series: '剧集', anime: '动漫', games: '游戏',
  apps: '软件', books: '书籍 / 有声书', music: '音乐', porn: '成人', other: '其他',
};
export const CATEGORY_ORDER = Object.keys(CATEGORY_LABELS);

function categoryFromRaw(raw) {
  const s = String(raw || '').toLowerCase();
  if (!s) return null;
  if (/\b(movie|film|hd\s*movie)/.test(s)) return 'movies';
  if (/\b(tv|series|show|episode)/.test(s)) return 'series';
  if (/\b(anime|cartoon)/.test(s)) return 'anime';
  if (/\b(game|rom|nintendo|playstation|xbox|switch)/.test(s)) return 'games';
  if (/\b(app|software|program|pc\b|application)/.test(s)) return 'apps';
  if (/\b(book|audiobook|ebook|comic|magazine|manga|literature)/.test(s)) return 'books';
  if (/\b(music|audio|album|flac|mp3|song)/.test(s)) return 'music';
  if (/\b(porn|xxx|adult|hentai|sex)/.test(s)) return 'porn';
  return null;
}

function categoryFromTitle(name) {
  const s = String(name || '').toLowerCase();
  if (!s) return null;
  if (/\bs\d{1,2}(e\d{1,3}|\b)|\b\d{1,2}x\d{2}\b|\bseason\s?\d|\bcomplete series\b/.test(s)) return 'series';
  if (/\b(xxx|porn|hentai|jav|onlyfans|brazzers)\b/.test(s)) return 'porn';
  if (/\b(fitgirl|repack|codex|plaza|skidrow|reloaded|razor1911|empress|dodi|goldberg|nsw|\.nsp|\.xci)\b/.test(s)) return 'games';
  if (/\b(x64|x86|win(?:32|64)?|keygen|cracked|activator|portable|multilingual)\b|\bv\d+\.\d+/.test(s)) return 'apps';
  if (/\b(epub|mobi|azw3|pdf|retail|audiobook|m4b)\b/.test(s)) return 'books';
  if (/\b(flac|mp3|320\s?kbps|discography|\bost\b|album)\b/.test(s)) return 'music';
  if (/\b(19|20)\d{2}\b.*\b(1080p|2160p|720p|bluray|blu-ray|web-?dl|webrip|bdrip|hdrip|x264|x265|hevc)\b/.test(s)) return 'movies';
  return 'other';
}

export function normalizeCategory(raw, name) {
  return categoryFromRaw(raw) || categoryFromTitle(name);
}

// ---------- provider groups ----------
export const GROUP_ORDER = ['anime', 'video', 'general', 'adult', 'other'];
export const GROUP_LABELS = {
  anime: '动漫', video: '影视', general: '综合', adult: '成人', other: '其他', custom: '自定义',
};
export const PROVIDER_GROUP = {
  nyaa: 'anime', anilibria: 'anime', animetosho: 'anime', anirena: 'anime',
  bangumimoe: 'anime', dmhy: 'anime', mikan: 'anime', subsplease: 'anime',
  tokyotoshokan: 'anime', nekobt: 'anime',
  eztv: 'video', yts: 'video', therarbg: 'video', torrent9: 'video',
  oxtorrent: 'video', rutor: 'video', megapeer: 'video',
  '1337x': 'general', tpb: 'general', knaben: 'general', torrentscsv: 'general',
  bt4g: 'general', btdigg: 'general', limetorrents: 'general',
  torrentdownload: 'general', torrentdownloads: 'general', torrentdatabase: 'general',
  torrentkitty: 'general', uindex: 'general', zeromagnet: 'general',
  bitsearch: 'general', internetarchive: 'general', filemood: 'general', demo: 'general',
  sukebei: 'adult', mypornclub: 'adult', xxxclub: 'adult', xxxtracker: 'adult',
  audiobookbay: 'other', blueroms: 'other', linuxtracker: 'other',
};

export function providerGroupOf(p) {
  if (p.id && p.id.startsWith('torznab:')) return 'custom';
  return PROVIDER_GROUP[p.id] || 'general';
}

export function saveSelectedProviders(selected) {
  localStorage.setItem('providers-selected', JSON.stringify([...selected]));
}

export function loadSelectedProviders() {
  try {
    const arr = JSON.parse(localStorage.getItem('providers-selected') || 'null');
    return Array.isArray(arr) ? arr : null;
  } catch { return null; }
}

// ---------- state ----------
// Pre-populate with all known provider names so badges/status show proper labels
// even before loadProviders() completes. The keys are provider ids; the values
// are display names. loadProviders() will override with server-provided names.
// Providers not listed here (e.g. Torznab dynamic) fall back to their id.
export const PROVIDER_LABEL = {
  '1337x': '1337x', tpb: 'The Pirate Bay', nyaa: 'NYAA', demo: 'Demo',
  yts: 'YTS', knaben: 'Knaben', torrentscsv: 'TorrentsCSV',
  anilibria: 'AniLibria', anirena: 'AniRena', animetosho: 'AnimeTosho',
  bangumimoe: 'Bangumi Moe', dmhy: '动漫花园', mikan: 'Mikan',
  subsplease: 'SubsPlease', sukebei: 'Sukebei',
  tokyotoshokan: '東京図書館', nekobt: 'NekoBT',
  bt4g: 'BT4G', btdigg: 'BTDigg',
  eztv: 'EZTV', limetorrents: 'LimeTorrents',
  therarbg: 'TheRarBg', rutor: 'Rutor', torrent9: 'Torrent9',
  torrentdownload: 'TorrentDownload', torrentdownloads: 'TorrentDownloads',
  torrentdatabase: 'TorrentDB', torrentkitty: 'TorrentKitty',
  uindex: 'uindex', zeromagnet: 'ZeroMagnet',
  bitsearch: 'BitSearch', oxtorrent: 'OxTorrent',
  audiobookbay: 'AudiobookBay', blueroms: 'BlueROMs',
  filemood: 'FileMood', internetarchive: 'Internet Archive',
  linuxtracker: 'LinuxTracker', megapeer: 'MegaPeer',
  mypornclub: 'MyPornClub', xxxclub: 'XXXClub', xxxtracker: 'XXXTracker',
};

export const HISTORY_MAX = 12;

export const state = {
  query: '',
  page: 1,
  browse: false,
  selected: new Set(),
  all: [],
  seen: new Set(),
  groups: new Map(),
  hasMore: false,
  loading: false,
  searchId: 0,
  abort: null,
  order: 'desc',
  sort: 'relevance',
  status: {},
  es: null,
  dl: loadDownloader(),
  quality: 'all',
  category: 'all',
  safeMode: loadSafeMode(),
  viewed: loadViewed(),
  view: 'search',
  history: loadJSON('history', []),
  favorites: loadJSON('favorites', []),
  checked: new Set(),
};

// ---------- provider list (loaded asynchronously) ----------
export let allProviders = [];

export async function loadProviders() {
  // Dynamic import to avoid circular dependency with render.js
  const { renderGroupChips, renderProviderChips } = await import('./render.js');
  const r = await fetch('/api/providers');
  const { providers } = await r.json();
  allProviders = providers;
  providers.forEach((p) => { PROVIDER_LABEL[p.id] = p.name; });
  const saved = loadSelectedProviders();
  state.selected = new Set(
    saved
      ? providers.filter((p) => saved.includes(p.id)).map((p) => p.id)
      : providers.filter((p) => p.enabled).map((p) => p.id)
  );
  renderGroupChips();
  renderProviderChips();
}
