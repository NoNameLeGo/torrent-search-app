'use strict';

// EZTV HTML scraper. The results table exposes the magnet link directly in
// each row, so no second request is needed. A `layout=def_wlinks` cookie is
// required, otherwise the rows come back without magnet links (per Kotlin src).
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = [
  'https://eztvx.to',
  'https://eztv.re',
  'https://eztv.tf',
  'https://eztv.wtf',
];

// Without the cookie the page returns rows without magnet links.
const EZTV_HEADERS = { Cookie: 'layout=def_wlinks' };

async function searchOn(base, query) {
  const url = `${base}/search/${encodeURIComponent(query)}`;
  const { html, error } = await getText(url, { headers: EZTV_HEADERS });
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  // table:last-of-type > tbody > tr, drop the first two header-ish rows.
  const rows = $('table:last-of-type > tbody > tr');
  if (rows.length <= 2) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
  rows.slice(2).each((_, row) => {
    const $row = $(row);
    const $name = $row.find('td:nth-child(2) > a.epinfo').first();
    const name = $name.text().trim();
    const magnet = $row.find('td:nth-child(3) > a.magnet').first().attr('href');
    if (!name || !magnet) return;

    const detailHref = $name.attr('href');
    const detailUrl = detailHref
      ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`)
      : null;

    const infoHash = extractInfoHash(magnet);

    results.push(normalize({
      provider: 'eztv',
      name,
      size: $row.find('td:nth-child(4)').text(),
      seeders: $row.find('td:nth-child(6)').text(),
      leechers: 0,
      infoHash,
      magnet,
      detailUrl,
      category: 'Series',
    }));
  });

  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOn(base, query)),
    'EZTV'
  );
}

module.exports = { id: 'eztv', name: 'EZTV', search };
