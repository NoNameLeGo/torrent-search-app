'use strict';

// Torrent9 HTML scraper. The search results page only links to torrent
// detail pages where all metadata (name, size, seeders, date, category, magnet)
// lives. The magnet is deferred to resolveMagnet() to avoid N+1 fetches during
// search().
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

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

  const infoHash = extractInfoHash(magnet);

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

// Fetch + parse a single detail page. Used lazily by resolveMagnet().
async function fetchDetails(detailUrl) {
  const { html, error } = await getText(detailUrl);
  if (error || !html) return null;
  return parseDetail(cheerio.load(html), '');
}

// Lazily fetch magnet + infoHash from a detail page when the user clicks.
async function resolveMagnet(detailUrl) {
  const det = await fetchDetails(detailUrl);
  if (!det || !det.magnet) return { magnet: null, error: 'no_magnet' };
  return { magnet: det.magnet, infoHash: det.infoHash, error: null };
}

async function searchOn(base, query) {
  const url = `${base}/search_torrent/${encodeURIComponent(query)}.html`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('table > tbody > tr');
  if (rows.length === 0) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
  rows.each((_, row) => {
    const $a = $(row).find('td:nth-child(1) > a').first();
    const href = $a.attr('href');
    if (!href) return;
    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
    const name = $a.text().trim() || '(untitled)';
    // All metadata is on the detail page; magnet deferred to resolveMagnet().
    // normalize() auto-sets needsMagnet when magnet=null and detailUrl is present.
    results.push(normalize({
      provider: 'torrent9',
      name,
      magnet: null,
      detailUrl,
    }));
  });

  return { base, results, error: results.length === 0 ? 'no_results_parsed' : null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOn(base, query)),
    'Torrent9'
  );
}

module.exports = { id: 'torrent9', name: 'Torrent9', search, resolveMagnet };
