'use strict';

// TorrentKitty HTML scraper. The results table exposes the magnet link
// directly in each row, so no second request is needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://torrentkitty.tv',
  'https://torrentkitty.to',
  'https://torrentkitty.is',
];

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search/${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const rows = $('table#archiveResult > tbody > tr').slice(1);
      if (!rows.length) return { base, results: [], error: 'no_results' };

      const results = [];
      rows.each((_, row) => {
        const $row = $(row);
        const name = $row.find('td.name').first().text().trim();
        if (!name) return;

        const magnet = $row.find('td.action > a:nth-child(2)').first().attr('href') || null;
        let infoHash = null;
        if (magnet) {
          const m = magnet.match(/btih:([a-f0-9]+)/i);
          if (m) infoHash = m[1];
        }

        const size = $row.find('td.size').first().text().trim().toUpperCase();
        const date = $row.find('td.date').first().text().trim();
        const detailHref = $row.find('td.action > a:nth-child(1)').first().attr('href');
        const detailUrl = detailHref
          ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
          : null;

        results.push(normalize({
          provider: 'torrentkitty',
          name,
          size,
          date,
          category: null,
          infoHash,
          magnet,
          detailUrl,
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
  return { results: [], error: `TorrentKitty unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'torrentkitty', name: 'TorrentKitty', search };
