'use strict';

// Bangumi.Moe — JSON API provider. Search hits a POST endpoint that returns a
// JSON document with a `torrents` array; each entry carries the magnet directly.
const { postJSON } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const BASE = 'https://bangumi.moe';
const DOMAINS = [BASE];

function parseTorrentObject(obj, base) {
  if (!obj) return null;
  const name = obj.title || null;
  if (!name) return null;
  const magnet = obj.magnet || null;
  if (!magnet) return null;

  const infoHash = obj.infohash || extractInfoHash(magnet);
  const detailUrl = obj._id ? `${base}/torrent/${obj._id}` : null;

  return {
    provider: 'bangumimoe',
    name,
    size: typeof obj.size === 'string' ? obj.size.trim() : obj.size || null,
    seeders: obj.seeders,
    leechers: obj.leechers,
    date: obj.publish_time || null,
    infoHash,
    magnet,
    detailUrl,
    category: 'Anime',
  };
}

async function searchOn(base, query) {
  const { data, error } = await postJSON(`${base}/api/v2/torrent/search`, { query });
  if (error || !data || !Array.isArray(data.torrents)) {
    return { results: [], error: error || 'bad_response' };
  }
  const results = [];
  for (const obj of data.torrents) {
    const raw = parseTorrentObject(obj, base);
    if (raw) results.push(normalize(raw));
  }
  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOn(base, query)),
    'bangumimoe'
  );
}

module.exports = { id: 'bangumimoe', name: 'Bangumi.Moe', search };
