'use strict';

// LinuxTracker HTML scraper. The site is served as UTF-8, so plain getText
// decoding is correct.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://linuxtracker.org'];

// Site is served as UTF-8; getText decodes it correctly. Never throws.
async function getWin1251(url) {
  return getText(url);
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url =
        `${base}/index.php?page=torrents&search=${encodeURIComponent(query)}` +
        `&category=0&active=0`;
      const { html, error } = await getWin1251(url);
      if (error || !html) return { results: [], error };

      const $ = cheerio.load(html);
      const items = $('table.lista[width="100%"] > tbody > tr')
        .toArray()
        .filter((tr) =>
          $(tr).find('a[href^="index.php?page=torrent-details&id="][title]').length > 0
        );
      if (items.length === 0) return { results: [], error: 'no_results_parsed' };

      const results = [];
      for (const tr of items) {
        const $tr = $(tr);
        const $name = $tr
          .find('a[href^="index.php?page=torrent-details&id="][title]')
          .first();
        const name = $name.attr('title') || $name.text().trim();
        const magnetUri = $tr.find('a[href^="magnet:?"]').first().attr('href');
        if (!name || !magnetUri) continue;

        const size = $tr
          .find('td:nth-child(2) > table > tbody > tr:nth-child(2) > td')
          .first().text().trim();
        const seeders = $tr
          .find('td:nth-child(2) > table > tbody > tr:nth-child(3) > td')
          .first().text().trim();
        const leechers = $tr
          .find('td:nth-child(2) > table > tbody > tr:nth-child(4) > td')
          .first().text().trim();
        const date = $tr
          .find('td:nth-child(2) > table > tbody > tr > td')
          .first().text().trim();

        const detailHref = $name.attr('href');
        const detailUrl = detailHref
          ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
          : null;

        let infoHash = null;
        const m = magnetUri.match(/btih:([a-f0-9]+)/i);
        if (m) infoHash = m[1];

        results.push(normalize({
          provider: 'linuxtracker',
          name,
          size,
          seeders,
          leechers,
          date,
          magnet: magnetUri,
          infoHash,
          category: 'Apps',
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
  return { results: [], error: `linuxtracker unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'linuxtracker', name: 'LinuxTracker', search };
