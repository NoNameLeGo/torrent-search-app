'use strict';

// InternetArchive search via the advancedsearch JSON API. No HTML scraping —
// we request `output=json` and parse the `docs` array directly.
const { getJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://archive.org';

function categoryFromMediaType(mediaType) {
  switch (mediaType) {
    case 'software': return 'Apps';
    case 'texts': return 'Books';
    case 'movies': return 'Movies';
    default: return 'Other';
  }
}

async function search(query, { page = 1 } = {}) {
  const url =
    `${BASE}/advancedsearch.php` +
    `?q=title:${encodeURIComponent(query)}` +
    `&fl[]=title,item_size,publicdate,mediatype,identifier,btih` +
    `&rows=100&page=1&output=json`;

  const { data, error } = await getJSON(url);
  if (error || !data) {
    return { results: [], error: `internetarchive unreachable (${error || 'no_data'})` };
  }

  const docs = data && data.response && data.response.docs;
  if (!Array.isArray(docs)) return { results: [], error: 'no_docs' };

  const results = [];
  for (const d of docs) {
    const name = d.title;
    const infoHash = d.btih ? String(d.btih).toLowerCase().trim() : null;
    if (!name || !infoHash) continue;

    const size = d.item_size != null ? Number(d.item_size) : null;
    const category = d.mediatype ? categoryFromMediaType(d.mediatype) : 'Other';
    const detailUrl = d.identifier ? `${BASE}/details/${d.identifier}` : null;

    results.push(normalize({
      provider: 'internetarchive',
      name,
      size,
      date: d.publicdate,
      infoHash,
      category,
      detailUrl,
    }));
  }

  return { results, error: null };
}

module.exports = { id: 'internetarchive', name: 'InternetArchive', search };
