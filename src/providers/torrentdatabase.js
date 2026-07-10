'use strict';

// TorrentDatabase HTML scraper. The results table exposes the infoHash in a
// "/track/magnet/<hash>" link; we build the magnet (with the site's tracker)
// directly, so no second request is needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://developify.ca'];

const TRACKER = 'https%3A%2F%2Fdevelopify.ca%2Fannounce';

function categoryFromRaw(raw) {
  if (!raw) return null;
  const map = {
    Software: 'Apps',
    'E-Books': 'Books',
    AudioBooks: 'Books',
    Games: 'Games',
    Movies: 'Movies',
    Music: 'Music',
    Porn: 'Porn',
    TV: 'Series',
  };
  return map[raw.trim()] || 'Other';
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/newest?q=${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const rows = $('table.torrent-table > tbody > tr');
      if (!rows.length) return { base, results: [], error: 'no_results' };

      const results = [];
      rows.each((_, row) => {
        const $row = $(row);
        const $nameEl = $row.find('td:nth-child(1) > a:nth-child(2)').first();
        const name = $nameEl.text().trim();
        const magnetHref = $nameEl.attr('href') || '';
        if (!name) return;

        // infoHash is the part of the path after "/track/magnet/" up to '?'.
        const infoHash = magnetHref
          .replace(/^\/track\/magnet\//, '')
          .split('?')[0]
          .trim();
        if (!infoHash) return;
        const magnet = `magnet:?xt=urn:btih:${infoHash}&tr=${TRACKER}`;

        const $detail = $row.find('td:nth-child(1) > a:nth-child(1)').first();
        const detailHref = $detail.attr('href');
        const detailUrl = detailHref
          ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
          : null;

        const category = categoryFromRaw(
          $row.find('td:nth-child(2) > span.category-bubble').first().text()
        );
        const size = $row.find('td.size-cell').first().text().trim();
        const date = $row.find('td.date-cell').first().text().trim();
        const seeders = $row.find('td:nth-child(5) > div > span:nth-child(1)').first().text().trim();
        const leechers = $row.find('td:nth-child(5) > div > span:nth-child(2)').first().text().trim();

        results.push(normalize({
          provider: 'torrentdatabase',
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
  return { results: [], error: `TorrentDatabase unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'torrentdatabase', name: 'TorrentDatabase', search };
