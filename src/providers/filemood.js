'use strict';

// FileMood HTML scraper. The results table exposes the name, size and
// seeders/peers directly; the 40-char info hash is embedded in the detail
// page URL (e.g. .../something-<hash>.html), so we parse it from there.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://filemood.com'];

function parseInfoHash(detailUrl) {
  if (!detailUrl) return null;
  let h = detailUrl;
  if (h.endsWith('.html')) h = h.slice(0, -'.html'.length);
  const idx = h.lastIndexOf('-');
  if (idx < 0) return null;
  h = h.slice(idx + 1).toLowerCase().trim();
  return /^[a-f0-9]{40}$/.test(h) ? h : null;
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/result?q=${encodeURIComponent(query)}+in%3Atitle`;
      const { html, error } = await getText(url);
      if (error || !html) return { results: [], error };
      const $ = cheerio.load(html);
      const rows = $('table > tbody > tr').toArray().filter((tr) => {
        return $(tr).find('a.btn-success').length > 0;
      });
      if (rows.length === 0) return { results: [], error: 'no_results_parsed' };

      const results = [];
      for (const tr of rows) {
        const $tr = $(tr);
        const name = $tr.find('td.dn-title').first().text().trim();
        if (!name) continue;

        const detailHref = $tr.find('td.dn-btn > div > a').first().attr('href');
        const detailUrl = detailHref
          ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
          : null;
        const infoHash = parseInfoHash(detailUrl);
        if (!infoHash) continue;

        const size = $tr.find('td.dn-size').first().text().trim();
        const statusText = $tr.find('td.dn-status').first().text().trim();
        const parts = statusText.split('/').map((s) => s.trim());
        const seeders = parts[0] || null;
        const leechers = parts[1] || null;

        results.push(normalize({
          provider: 'filemood',
          name,
          size,
          seeders,
          leechers,
          infoHash,
          category: 'Other',
          detailUrl,
        }));
      }
      return { results, error: null };
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
  return { results: [], error: `filemood unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'filemood', name: 'FileMood', search };
