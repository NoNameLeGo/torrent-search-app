'use strict';

// LimeTorrents HTML scraper. Uses an infoHash (not a magnet) on the results
// page; the infoHash is extracted from the itorrents.net .torrent link.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://limetorrents.fun',
  'https://limetorrents.lol',
  'https://limetorrents.pro',
];

function categoryFromRaw(raw) {
  switch ((raw || '').trim()) {
    case 'TV': return 'Series';
    case 'Movie': return 'Movies';
    case 'Music': return 'Music';
    case 'App': return 'Apps';
    case 'E-book': return 'Books';
    case 'Anime': return 'Anime';
    case 'Games': return 'Games';
    default: return 'Other';
  }
}

async function searchOn(base, query, page) {
  // /search/<category>/<query>/date/<page>/
  const url = `${base}/search/all/${encodeURIComponent(query)}/date/${page}/`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('.table2 > tbody > tr');
  if (rows.length === 0) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);
    const $name = $row.find('td:nth-child(1) > div.tt-name > a:nth-child(2)').first();
    const name = $name.text().trim();
    const fileLink = $row.find('td:nth-child(1) > div.tt-name > a:nth-child(1)').first().attr('href');
    if (!name || !fileLink) return;

    // http://itorrents.net/torrent/<HASH>.torrent?title=...
    const m = fileLink.match(/itorrents\.net\/torrent\/([0-9A-Fa-f]+)\./);
    const infoHash = m ? m[1].toLowerCase() : null;
    if (!infoHash) return;

    const detailHref = $name.attr('href');
    const detailUrl = detailHref
      ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`)
      : null;

    // "date - in Category"
    const dcText = $row.find('td:nth-child(2)').text().trim();
    let date = null;
    let category = null;
    if (dcText.includes('- in')) {
      const idx = dcText.indexOf('-');
      date = dcText.slice(0, idx).trim();
      category = categoryFromRaw(
        dcText.slice(idx + 1).trim().replace(/^in /, '').replace(/\.$/, ''),
      );
    } else {
      date = dcText;
    }

    results.push(normalize({
      provider: 'limetorrents',
      name,
      size: $row.find('td:nth-child(3)').text(),
      seeders: $row.find('td.tdseed').text(),
      leechers: $row.find('td.tdleech').text(),
      date,
      infoHash,
      detailUrl,
      category,
    }));
  });

  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  const p = Math.max(1, page | 0);
  const attempts = DOMAINS.map((base) => searchOn(base, query, p));
  const settled = await Promise.allSettled(attempts);
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `LimeTorrents unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'limetorrents', name: 'LimeTorrents', search };
