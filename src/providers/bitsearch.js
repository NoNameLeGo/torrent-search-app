'use strict';

// BitSearch HTML scraper. The results grid exposes the magnet link directly in
// each card, so no second request is needed. (Note: this Kotlin source uses
// HTML scraping, not a JSON API.)
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

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

async function searchOne(base, query, page) {
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

    const magnetEl = $item.find('a[href^="magnet:"]').first();
    const magnet = magnetEl.attr('href') || null;
    const infoHash = extractInfoHash(magnet);

    // Extract metadata by scanning children for labeled spans.
    // Layout: text label + value span (pair); we look for known labels.
    let size = '';
    let seeders = '';
    let leechers = '';
    let date = '';
    let rawCategory = '';
    $item.find('div').each((_, el) => {
      const $el = $(el);
      const text = $el.text().trim();
      if (!text || text === 'magnet:') return;
      // Heuristics: match by content pattern rather than deep position.
      if (!size && /\d[\d,.]+\s*(TB|GB|MB|KB)/i.test(text)) size = text;
      else if (!seeders && /^[\d,]+$/.test(text)) seeders = text.replace(/,/g, '');
      else if (!leechers && /^[\d,]+$/.test(text)) leechers = text.replace(/,/g, '');
      else if (!date && /Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec/.test(text)) date = text;
    });
    // Category: find the icon/label element near the top of the card.
    const catEl = $item.find('[class*="category"], [class*="tag"], [class*="badge"]')
      .addBack().filter((_, el) => {
        const c = $(el).attr('class') || '';
        return /category|tag|badge|icon/i.test(c);
      }).first();
    rawCategory = catEl.text().trim() || '';
    const category = rawCategory ? categoryFromRaw(rawCategory) : null;
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
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'BitSearch'
  );
}

module.exports = { id: 'bitsearch', name: 'BitSearch', search };
