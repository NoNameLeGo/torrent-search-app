'use strict';

// MegaPeer HTML scraper. The magnet URI lives on each torrent's detail page,
// so we defer the fetch to resolveMagnet() (lazily on click) instead of N+1
// inside search().
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, ruDate, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = ['https://megapeer.vip'];

// Site is served as UTF-8; getText decodes it correctly. Never throws.
async function getWin1251(url) {
  return getText(url);
}

// Pull the magnet URI and infoHash from a torrent's detail page.
// Used lazily by resolveMagnet().
async function fetchDetails(detailUrl) {
  const { html, error } = await getWin1251(detailUrl);
  if (error || !html) return null;
  const $ = cheerio.load(html);
  const magnet = $('a[href^="magnet:?xt="]').first().attr('href');
  if (!magnet) return null;
  return { magnet, infoHash: extractInfoHash(magnet) };
}

function parseListItem($, listItem, base) {
  const $li = $(listItem);
  const $name = $li.find('td:nth-child(2) > a:nth-child(2)').first();
  const nameHref = $name.attr('href');
  if (!nameHref) return null;
  const detailUrl = nameHref.startsWith('http') ? nameHref : `${base}${nameHref}`;

  const name = $name.text().trim();
  if (!name) return null;

  const size = $li.find('td:nth-child(3)').first().text().trim();
  const seeders = $li.find('td:nth-child(4) > font:nth-child(2)').first().text().trim();
  const leechers = $li.find('td:nth-child(4) > font:nth-child(4)').first().text().trim();
  const dateRaw = $li.find('td:nth-child(1)').first().text().trim();

  // No magnet fetch here → normalize() sets needsMagnet: true automatically.
  return normalize({
    provider: 'megapeer',
    name,
    size,
    seeders,
    leechers,
    date: ruDate(dateRaw),
    magnet: null,
    detailUrl,
  });
}

async function searchOne(base, query, page) {
  const url =
    `${base}/browse.php?search=${encodeURIComponent(query)}` +
    `&age=&cat=0&stype=0&sort=0&ascdesc=0`;
  const { html, error } = await getWin1251(url);
  if (error || !html) return { results: [], error };

  const $ = cheerio.load(html);
  const items = $('div#index > table > tbody > tr.table_fon').toArray();
  if (items.length === 0) return { results: [], error: 'no_results_parsed' };

  // parseListItem is now synchronous — no per-item HTTP round-trips.
  const results = items.map((li) => parseListItem($, li, base)).filter(Boolean);
  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'megapeer'
  );
}

// Lazily fetch magnet + infoHash from a detail page when the user clicks.
async function resolveMagnet(detailUrl) {
  const det = await fetchDetails(detailUrl);
  if (!det || !det.magnet) return { magnet: null, error: 'no_magnet' };
  return { magnet: det.magnet, infoHash: det.infoHash, error: null };
}

module.exports = { id: 'megapeer', name: 'MegaPeer', search, resolveMagnet };
