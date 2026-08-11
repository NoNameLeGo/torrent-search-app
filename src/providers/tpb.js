'use strict';

// The Pirate Bay via the public apibay JSON API.
// Endpoint returns an array of results (or [{error:"no results"}] when empty).
const { getJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const API = 'https://apibay.org/q.php';

// TPB uses 3-digit numeric category codes; map to our standard buckets.
//   1xx Audio → music/books   2xx Video → movies/series
//   3xx Apps                  4xx Games
//   5xx Porn                  6xx Other → books/other
function tpbCategory(code) {
  const n = String(code || '').trim();
  const prefix = n.charAt(0);
  switch (prefix) {
    case '1': return n === '102' ? 'Books' : 'Music';          // Audio
    case '2': return n === '205' || n === '208' ? 'Series' : 'Movies'; // TV shows
    case '3': return 'Apps';
    case '4': return 'Games';
    case '5': return 'Porn';
    case '6': return n === '601' || n === '602' ? 'Books' : 'Other';
    default:  return null;
  }
}

async function search(query, { page = 1 } = {}) {
  const url = `${API}?q=${encodeURIComponent(query)}`;
  const { data, error } = await getJSON(url);
  if (error) return { results: [], error: `TPB unreachable (${error})` };
  if (!Array.isArray(data)) return { results: [], error: 'unexpected response' };

  // apibay signals "no results" with a single object carrying an error field.
  if (data.length === 1 && data[0] && data[0].error) return { results: [] };

  const results = data.map((it) => normalize({
    provider: 'tpb',
    name: it.name,
    infoHash: it.info_hash,
    size: it.size, // bytes (string)
    seeders: it.seeders,
    leechers: it.leechers,
    date: it.added ? Number(it.added) * 1000 : null,
    category: tpbCategory(it.category),
    files: it.num_files,
    detailUrl: it.id ? `https://thepiratebay.org/description.php?id=${it.id}` : null,
  }));

  return { results, error: null };
}

module.exports = { id: 'tpb', name: 'The Pirate Bay', search, testable: true };
