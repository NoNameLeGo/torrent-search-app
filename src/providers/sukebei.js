'use strict';

// Sukebei (the "fansub" / non-anime half of nyaa) HTML scraper.
// Results table exposes the magnet link directly, so no detail fetch needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://sukebei.nyaa.si';

// Kotlin only lists a single domain; try it (mirror pattern kept for parity).
const DOMAINS = [BASE];

function parseRows($, base) {
  const rows = $('table.torrent-list > tbody > tr');
  if (rows.length === 0) return [];

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);

    const $name = $row.find('td:nth-child(2) > a:not(.comments)').first();
    const name = $name.text().trim();
    if (!name) return;

    const magnet = $row.find('td:nth-child(3) > a:nth-child(2)').first().attr('href');
    if (!magnet) return;

    const infoHashMatch = magnet.match(/xt=urn:btih:([a-f0-9]+)/i);
    const infoHash = infoHashMatch ? infoHashMatch[1] : null;

    const href = $name.attr('href') || '';
    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;

    results.push(normalize({
      provider: 'sukebei',
      name,
      size: $row.find('td:nth-child(4)').text(),
      date: $row.find('td:nth-child(5)').attr('data-timestamp'),
      seeders: $row.find('td:nth-child(6)').text(),
      leechers: $row.find('td:nth-child(7)').text(),
      infoHash,
      magnet,
      detailUrl,
      category: 'Porn',
    }));
  });
  return results;
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/?f=0&c=0_0&q=${encodeURIComponent(query)}`;
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
  return { results: [], error: `Sukebei unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'sukebei', name: 'Sukebei', search };
