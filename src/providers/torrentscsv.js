'use strict';

// TorrentsCSV via the public JSON search API.
// Mirrors upstream TorrentsCSV.kt: https://torrents-csv.com/service/search
const { getJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const API = 'https://torrents-csv.com/service/search';

async function search(query, { page = 1 } = {}) {
  const url = `${API}?q=${encodeURIComponent(query)}`;
  const { data, error } = await getJSON(url);
  if (error) return { results: [], error: `TorrentsCSV unreachable (${error})` };

  const torrents = data && Array.isArray(data.torrents) ? data.torrents : [];
  if (torrents.length === 0) return { results: [] };

  const results = torrents.map((it) => normalize({
    provider: 'torrentscsv',
    name: it.name,
    infoHash: it.infohash,
    size: it.size_bytes,
    seeders: it.seeders,
    leechers: it.leechers,
    date: it.created_unix ? Number(it.created_unix) : null,
    category: 'Other',
  }));

  return { results, error: null };
}

module.exports = { id: 'torrentscsv', name: 'TorrentsCSV', search, testable: true };
