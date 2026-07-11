'use strict';

// BitSearch HTML scraper. The results grid exposes the magnet link directly in
// each card, so no second request is needed. (Note: this Kotlin source uses
// HTML scraping, not a JSON API.)
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://bitsearch.to',
  'https://bitsearch.am',
];

function categoryFromRaw(raw) {
  if (!raw) return null;
  const r = raw.trim();
  if (r === 'TV') return 'Series';
  if (r === 'XXX') return 'Porn';
  if (r.startsWith('Movies')) return 'Movies';
  if (r.startsWith('Anime')) return 'Anime';
  if (r.startsWith('Softwares')) return 'Apps';
  if (r.startsWith('Games')) return 'Games';
  if (r.startsWith('Music')) return 'Music';
  if (r.startsWith('AudioBook') || r.startsWith('Ebook')) return 'Books';
  if (r.startsWith('Other')) return 'Other';
  return 'Other';
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search?q=${encodeURIComponent(query)}&page=${page}&sortBy=seeders`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const items = $('div.space-y-4 > div > div:nth-child(1)');
      if (!items.length) return { base, results: [], error: 'no_results' };

      const results = [];
      items.each((_, el) => {
        const $item = $(el);
        const name = $item.find('h3').first().text().trim();
        if (!name) return;

        const magnetEl = $item.find('div:nth-child(2) > a:nth-child(2)').first();
        const magnet = magnetEl.attr('href') || null;
        let infoHash = null;
        if (magnet) {
          const m = magnet.match(/btih:([a-f0-9]+)/i);
          if (m) infoHash = m[1];
        }

        const size = $item
          .find('div:nth-child(1) > div:nth-last-child(2) > span:nth-child(2) > span')
          .first().text().trim();
        const seeders = $item
          .find('div:nth-child(1) > div:nth-last-child(1) > span:nth-child(1) > span:nth-child(2)')
          .first().text().replace(/,/g, '').trim();
        const leechers = $item
          .find('div:nth-child(1) > div:nth-last-child(1) > span:nth-child(2) > span:nth-child(2)')
          .first().text().replace(/,/g, '').trim();
        const date = $item
          .find('div:nth-child(1) > div:nth-last-child(2) > span:nth-child(3) > span')
          .first().text().trim();
        const category = categoryFromRaw(
          $item.find('div:nth-child(1) > div:nth-last-child(2) > span:nth-child(1) > span')
            .first().text()
        );
        const detailHref = $item.find('h3 > a').first().attr('href');
        const detailUrl = detailHref
          ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
          : null;

        results.push(normalize({
          provider: 'bitsearch',
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
  return { results: [], error: `BitSearch unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'bitsearch', name: 'BitSearch', search };
