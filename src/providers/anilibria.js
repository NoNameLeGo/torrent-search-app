'use strict';

// AniLibria — JSON API provider.
// Search returns matching releases; the torrents for each release are fetched
// from a separate endpoint and mapped to magnet-based results.
const { getJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://anilibria.top', 'https://www.anilibria.top'];
const MAX_RELEASES = 15;

function parseTorrentObject(obj, base) {
  if (!obj) return null;
  const infoHash = obj.hash || null;
  if (!infoHash) return null;

  const release = obj.release || {};
  const releaseName =
    (release.name && (release.name.english || release.name.main)) || null;
  const name = obj.label || releaseName;
  if (!name) return null;

  const magnet = obj.magnet || null;
  const alias = release.alias || null;
  const detailUrl = alias ? `${base}/anime/releases/release/${alias}` : null;

  return {
    provider: 'anilibria',
    name,
    size: typeof obj.size === 'number' ? obj.size : null,
    seeders: obj.seeders,
    leechers: obj.leechers,
    date: obj.created_at || null,
    infoHash,
    magnet,
    detailUrl,
    category: 'Anime',
  };
}

async function searchOn(base, query) {
  const api = base.replace(/\/$/, '');
  const { data, error } = await getJSON(
    `${api}/api/v1/app/search/releases?query=${encodeURIComponent(query)}`
  );
  if (error || !Array.isArray(data)) return { results: [], error: error || 'bad_response' };

  const ids = data
    .map((r) => (r && r.id != null ? r.id : null))
    .filter((x) => x != null)
    .slice(0, MAX_RELEASES);

  const results = [];
  for (const id of ids) {
    const res = await getJSON(`${api}/api/v1/anime/torrents/release/${id}`);
    if (res.error || !Array.isArray(res.data)) continue;
    for (const obj of res.data) {
      const raw = parseTorrentObject(obj, base);
      if (raw) results.push(normalize(raw));
    }
  }
  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) => searchOn(base, query));
  const settled = await Promise.allSettled(attempts);
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const ok = settled.find(
    (s) => s.status === 'fulfilled' && s.value && !s.value.error && s.value.results.length === 0
  );
  if (ok) return { results: [], error: null };
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `anilibria unavailable (${errs.join('; ')})` };
}

module.exports = { id: 'anilibria', name: 'AniLibria', search };
