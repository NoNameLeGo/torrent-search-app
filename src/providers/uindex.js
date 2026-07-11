'use strict';

// UIndex HTML scraper. The results table exposes the magnet link directly in
// each row, so no second request is needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://uindex.org',
  'https://uindex.to',
];

function categoryFromRaw(raw) {
  if (!raw) return null;
  const map = {
    Anime: 'Anime',
    Apps: 'Apps',
    Games: 'Games',
    Movies: 'Movies',
    Music: 'Music',
    XXX: 'Porn',
    TV: 'Series',
    Other: 'Other',
  };
  return map[raw.trim()] || 'Other';
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search.php?search=${encodeURIComponent(query)}&c=0`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const container = $('table.sr-table, table.top-table').first();
      if (!container.length) return { base, results: [], error: 'no_container' };
      const rows = container.find('tbody > tr');
      if (!rows.length) return { base, results: [], error: 'no_results' };

      const results = [];
      rows.each((_, row) => {
        const $row = $(row);
        const $name = $row.find('td.sr-col-name > a.sr-torrent-link').first();
        const name = $name.text().trim();
        if (!name) return;

        const magnet = $row.find('td.sr-col-name > a.sr-magnet').first().attr('href') || null;
        let infoHash = null;
        if (magnet) {
          const m = magnet.match(/btih:([a-f0-9]+)/i);
          if (m) infoHash = m[1];
        }

        const size = $row.find('td.sr-col-size').first().text().trim();
        const seeders = $row
          .find('td.sr-col-seeders > span.sr-seed').first().text().replace(/,/g, '').trim();
        const leechers = $row
          .find('td.sr-col-leechers > span.sr-leech').first().text().replace(/,/g, '').trim();
        const date = $row.find('td.sr-col-uploaded').first().text().trim();
        const category = categoryFromRaw(
          $row.find('td.sr-col-cat > a.sr-cat-badge').first().text()
        );
        const detailHref = $name.attr('href');
        const detailUrl = detailHref
          ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
          : null;

        results.push(normalize({
          provider: 'uindex',
          name,
          size,
          seeders,
          leechers,
          date,
          category,
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
  return { results: [], error: `UIndex unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'uindex', name: 'UIndex', search };
