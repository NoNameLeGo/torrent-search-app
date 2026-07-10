'use strict';

// 1337x HTML scraper. The search results page lists torrents in a table;
// the magnet link lives on each torrent's detail page, so we expose a
// resolveMagnet() used lazily when the user clicks "Get magnet".
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://1337x.to',
  'https://1337x.st',
  'https://1377x.to',
  'https://x1337x.ws',
];

async function search(query, { page = 1 } = {}) {
  // Try all mirror domains in parallel; take the first that returns results.
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search/${encodeURIComponent(query)}/${page}/`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };
      const $ = cheerio.load(html);
      const rows = $('table tbody tr');
      if (rows.length === 0) return { base, results: [], error: 'no_results_parsed' };

      const results = [];
      rows.each((_, row) => {
        const $row = $(row);
        const $link = $row.find('td.name a').first();
        const name = $link.text().trim();
        const href = $link.attr('href') || '';
        if (!name || !href.includes('/torrent/')) return;

        const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
        results.push(normalize({
          provider: '1337x',
          name,
          size: $row.find('td.size').text(),
          seeders: $row.find('td.seeds').text(),
          leechers: $row.find('td.leeches').text(),
          date: $row.find('td.date').text(),
          detailUrl,
          needsMagnet: true,
        }));
      });
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
  return { results: [], error: `1337x unreachable (${errs.join('; ')})` };
}

// Lazily fetch the magnet link from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };
  const $ = cheerio.load(html);
  const href = $('a[href^="magnet:"]').first().attr('href');
  if (!href) return { magnet: null, error: 'no_magnet_on_page' };
  return { magnet: href, error: null };
}

module.exports = { id: '1337x', name: '1337x', search, resolveMagnet };
