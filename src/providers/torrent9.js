'use strict';

// Torrent9 HTML scraper. The results list only links to a detail page; the
// name, size, seeders, peers, date, category and magnet all live on the detail
// page, so we fetch each detail page (in parallel) during search, mirroring the
// Kotlin parser. Never throws.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://www6.torrent9.to',
  'https://www.torrent9.to',
  'https://torrent9.so',
  'https://ww1.torrent9.to',
];

function categoryFromRaw(raw) {
  switch ((raw || '').trim()) {
    case 'ebook': return 'Books';
    case 'films': return 'Movies';
    case 'jeux-consoles': return 'Games';
    case 'jeux-pc': return 'Games';
    case 'logiciels': return 'Apps';
    case 'musique': return 'Music';
    case 'series': return 'Series';
    default: return 'Other';
  }
}

// Parse a Torrent9 detail page. Returns null if the essentials are missing.
function parseDetail($, base) {
  const name = $('div.movie-section h1').text().trim();
  const magnet = $('a[href^="magnet:?"]').first().attr('href');
  if (!name || !magnet) return null;

  let infoHash = null;
  const im = magnet.match(/btih:([a-f0-9]+)/i);
  if (im) infoHash = im[1];

  // Size: near a <strong> containing "Poids du torrent" (French units).
  let size = null;
  const $size = $('strong').filter((_, el) => $(el).text().includes('Poids du torrent')).first();
  if ($size.length) {
    const txt = $size.parent().text();
    const m = txt.match(/([\d.,]+\s*(?:Go|Mo|Ko|To|GB|MB|KB|TB|B))/i);
    if (m) {
      size = m[1]
        .replace(/Go/i, 'GB').replace(/Mo/i, 'MB')
        .replace(/Ko/i, 'KB').replace(/To/i, 'TB');
    }
  }

  const seeders = $('li[style="color:green"]').text();
  const leechers = $('li[style="color:red"]').text();

  // Date: dd/MM/yyyy near "Date d'ajout".
  let date = null;
  const $date = $('strong').filter((_, el) => $(el).text().includes("Date d")).first();
  if ($date.length) {
    const m = $date.parent().text().match(/(\d{2})\/(\d{2})\/(\d{4})/);
    if (m) date = Date.UTC(+m[3], +m[2] - 1, +m[1]); // ms timestamp
  }

  // Category: href like /torrents_films.html
  let category = null;
  const $cat = $('strong').filter((_, el) => $(el).text().includes('Cat')).first();
  if ($cat.length) {
    const href = $cat.parent().find('a').last().attr('href') || '';
    const slug = href.replace(/^.*\/torrents_/, '').replace(/\.html$/, '');
    category = categoryFromRaw(slug);
  }

  return { name, size, seeders, leechers, date, infoHash, magnet, category };
}

async function searchOn(base, query) {
  const url = `${base}/search_torrent/${encodeURIComponent(query)}.html`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('table > tbody > tr');
  if (rows.length === 0) return { base, results: [], error: 'no_results_parsed' };

  const detailUrls = [];
  rows.each((_, row) => {
    const href = $(row).find('td:nth-child(1) > a').first().attr('href');
    if (href) detailUrls.push(href.startsWith('http') ? href : `${base}${href}`);
  });
  if (detailUrls.length === 0) return { base, results: [], error: 'no_detail_links' };

  const parsed = await Promise.all(detailUrls.map(async (u) => {
    try {
      const r = await getText(u);
      if (r.error || !r.html) return null;
      return parseDetail(cheerio.load(r.html), base);
    } catch {
      return null;
    }
  }));

  const results = parsed
    .filter(Boolean)
    .map((d) => normalize({
      provider: 'torrent9',
      name: d.name,
      size: d.size,
      seeders: d.seeders,
      leechers: d.leechers,
      date: d.date,
      infoHash: d.infoHash,
      magnet: d.magnet,
      category: d.category,
    }));

  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) => searchOn(base, query));
  const settled = await Promise.allSettled(attempts);
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `Torrent9 unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'torrent9', name: 'Torrent9', search };
