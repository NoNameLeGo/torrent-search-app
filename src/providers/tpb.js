'use strict';

// The Pirate Bay via the public apibay JSON API.
// Endpoint returns an array of results (or [{error:"no results"}] when empty).
const { getJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const API = 'https://apibay.org/q.php';

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
    category: it.category,
    files: it.num_files,
    detailUrl: it.id ? `https://thepiratebay.org/description.php?id=${it.id}` : null,
  }));

  return { results, error: null };
}

module.exports = { id: 'tpb', name: 'The Pirate Bay', search, testable: true };
