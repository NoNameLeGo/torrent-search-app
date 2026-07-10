'use strict';

// NekoBT HTML scraper (anime torrents). The results table carries the magnet
// link directly, so no detail-page fetch is required for search.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://nekobt.to';
const DOMAINS = [BASE];

function parseRows($, base) {
  const rows = $('table.table > tbody > tr');
  if (rows.length === 0) return [];

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);

    const $nameLink = $row.find('td:nth-child(3) > div:nth-child(1) > div > a').first();
    const name = $nameLink.find('span > span:nth-child(1)').first().text().trim();
    if (!name) return;

    const magnet = $row.find('td:nth-child(4) > div > a:nth-child(1)').first().attr('href');
    if (!magnet) return;

    const infoHashMatch = magnet.match(/xt=urn:btih:([a-f0-9]+)/i);
    const infoHash = infoHashMatch ? infoHashMatch[1] : null;

    const href = $nameLink.attr('href') || '';
    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;

    results.push(normalize({
      provider: 'nekobt',
      name,
      size: $row.find('td:nth-child(5) > span').text(),
      date: $row.find('td:nth-child(6) > span').text(),
      seeders: $row.find('td:nth-child(7) > span').text(),
      leechers: $row.find('td:nth-child(8) > span').text(),
      infoHash,
      magnet,
      detailUrl,
      category: 'Anime',
    }));
  });
  return results;
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search?query=${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };
      const $ = cheerio.load(html);
      const results = parseRows($, base);
      return { base, results, error: null };
    })()
  );

  const settled = await Promise.allSettled(attempts);
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `NekoBT unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'nekobt', name: 'NekoBT', search };
