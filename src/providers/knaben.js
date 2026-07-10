'use strict';

// Knaben multi-category torrents via the official JSON API (POST).
// Mirrors upstream Knaben.kt: https://api.knaben.org/v1
const { postJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const API = 'https://api.knaben.org/v1';

// Map Knaben's numeric categoryId ranges back to a category label.
function mapCategory(categoryId) {
  if (categoryId == null) return null;
  const id = Number(categoryId);
  if (isNaN(id)) return null;
  if (id >= 1000000 && id < 2000000) return 'Music';
  if (id >= 2000000 && id < 3000000) return 'Series';
  if (id >= 3000000 && id < 4000000) return 'Movies';
  if (id >= 4000000 && id < 5000000) return 'Apps';
  if (id >= 5000000 && id < 6000000) return 'Porn';
  if (id >= 6000000 && id < 7000000) return 'Anime';
  if (id >= 7000000 && id < 8000000) return 'Games';
  if (id >= 9000000 && id < 10000000) return 'Books';
  if (id >= 10000000 && id < 11000000) return 'Other';
  return null;
}

async function search(query, { page = 1 } = {}) {
  const payload = {
    query,
    size: 300,
    order_by: 'seeders',
    order_direction: 'desc',
    hide_unsafe: true,
    hide_xxx: false,
  };

  const { data, error } = await postJSON(API, payload);
  if (error) return { results: [], error: `Knaben unreachable (${error})` };

  const hits = data && Array.isArray(data.hits) ? data.hits : [];
  if (hits.length === 0) return { results: [] };

  const results = hits.map((it) => {
    const magnet = it.magnetUrl || null;
    let infoHash = it.hash || null;
    if (!infoHash && magnet) {
      const m = magnet.match(/btih:([a-f0-9]+)/i);
      if (m) infoHash = m[1];
    }

    let catId = null;
    if (Array.isArray(it.categoryId) && it.categoryId.length) {
      const nums = it.categoryId.map(Number).filter((n) => !isNaN(n));
      if (nums.length) catId = Math.min(...nums);
    }

    return normalize({
      provider: 'knaben',
      name: it.title,
      infoHash,
      magnet,
      size: it.bytes,
      seeders: it.seeders,
      leechers: it.peers,
      date: it.date, // ISO 8601 string
      category: mapCategory(catId),
      detailUrl: it.details || null,
    });
  });

  return { results, error: null };
}

module.exports = { id: 'knaben', name: 'Knaben', search, testable: true };
