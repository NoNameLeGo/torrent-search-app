'use strict';

// Offline demo provider. Generates deterministic, realistic-looking results so
// the whole UI (sort / filter / pagination / infinite scroll) is exercisable
// without network access or reachable engines. Magnets use fake info hashes.
const { normalize } = require('../lib/normalize');

const CATEGORIES = ['Movies', 'TV', 'Anime', 'Music', 'Software', 'Games'];
const SUFFIXES = [
  '[1080p]', '[720p]', '[2160p 4K]', 'EXTENDED', 'REMASTERED',
  'COMPLETE', '[WEB-DL]', '[BluRay]', 'SEASON 1', 'COLLECTION',
];

function mulberry32(seed) {
  return function () {
    seed |= 0; seed = (seed + 0x6D2B79F5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function hashStr(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function randHex(rng, n) {
  let out = '';
  for (let i = 0; i < n; i++) out += Math.floor(rng() * 16).toString(16);
  return out;
}

const PER_PAGE = 20;

function buildPool(query) {
  const rng = mulberry32(hashStr(query || 'demo'));
  const total = 120;
  const pool = [];
  for (let i = 0; i < total; i++) {
    const cat = CATEGORIES[Math.floor(rng() * CATEGORIES.length)];
    const suffix = SUFFIXES[Math.floor(rng() * SUFFIXES.length)];
    const ep = String(1 + Math.floor(rng() * 24)).padStart(2, '0');
    const name = `${query} ${suffix} - S01E${ep} [${cat}]`;
    const size = Math.floor((rng() * 48 + 0.2) * 1024 ** 3); // 0.2GB - 48GB
    const seeders = Math.floor(rng() * 5000);
    const leechers = Math.floor(rng() * 1500);
    const ageDays = Math.floor(rng() * 730);
    const date = Date.now() - ageDays * 86400e3;
    const infoHash = randHex(rng, 40);
    pool.push(normalize({
      provider: 'demo',
      name,
      size,
      seeders,
      leechers,
      date,
      category: cat,
      infoHash,
      magnet: `magnet:?xt=urn:btih:${infoHash}&dn=${encodeURIComponent(name)}`,
    }));
  }
  return pool;
}

async function search(query, { page = 1 } = {}) {
  const pool = buildPool(query);
  const start = (page - 1) * PER_PAGE;
  const slice = pool.slice(start, start + PER_PAGE);
  return { results: slice, error: null, hasMore: start + PER_PAGE < pool.length };
}

module.exports = { id: 'demo', name: 'Demo (offline)', search, demo: true };
