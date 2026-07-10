'use strict';

// ZeroMagnet (0Magnet) HTML scraper. The results list only links to each
// torrent's detail page, where the magnet + infoHash live, so we expose a
// resolveMagnet() used lazily on click (the engine is Porn-category only).
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://9mag.net'];

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search?q=${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const rows = $('table.file-list > tbody > tr');
      if (!rows.length) return { base, results: [], error: 'no_results' };

      const results = [];
      rows.each((_, row) => {
        const $row = $(row);
        const $link = $row.find('td:nth-child(1) > a').first();
        const name = $link.text().trim();
        const href = $link.attr('href');
        if (!name || !href) return;

        const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
        results.push(normalize({
          provider: 'zeromagnet',
          name,
          category: 'Porn',
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
  return { results: [], error: `ZeroMagnet unreachable (${errs.join('; ')})` };
}

// Lazily fetch the magnet + infoHash from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };
  const $ = cheerio.load(html);
  const magnet = $('input#input-magnet').first().attr('value');
  if (!magnet) return { magnet: null, error: 'no_magnet_on_page' };
  return { magnet, error: null };
}

module.exports = { id: 'zeromagnet', name: 'ZeroMagnet', search, resolveMagnet };
