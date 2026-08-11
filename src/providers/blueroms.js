'use strict';

// BlueRoms HTML scraper. The results page links to download pages where the
// magnet URI is base64-encoded. The magnet fetch is deferred to resolveMagnet()
// to avoid N+1 fetches during search().
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = ['https://www.blueroms.ws'];

// Fetch the download page and decode the base64 magnet URI from the button.
async function getMagnetUri(downloadPageUrl) {
  const { html, error } = await getText(downloadPageUrl);
  if (error || !html) return null;
  const $ = cheerio.load(html);
  const enc = $('#magnet-button').first().attr('data-link');
  if (!enc) return null;
  try {
    return Buffer.from(enc, 'base64').toString('utf8');
  } catch (e) {
    return null;
  }
}

// Lazily decode the base64 magnet from a download page when the user clicks.
async function resolveMagnet(downloadPageUrl) {
  const magnetUri = await getMagnetUri(downloadPageUrl);
  if (!magnetUri) return { magnet: null, error: 'no_magnet' };
  const infoHash = extractInfoHash(magnetUri);
  return { magnet: magnetUri, infoHash, error: null };
}

function parseListItem($, listItem, base) {
  const $li = $(listItem);
  const $dl = $li.find('div.card-footer > a').first();
  const dlHref = $dl.attr('href');
  if (!dlHref) return null;
  const downloadPageUrl = dlHref.startsWith('http') ? dlHref : `${base}${dlHref}`;

  const $name = $li.find('h4.card-title > a').first();
  const gameName = $name.text().trim();
  if (!gameName) return null;

  // Platform label: <strong>Platform:</strong> <text>
  let platform = null;
  $li.find('strong').each((_, el) => {
    if ($(el).text().trim().startsWith('Platform:') && platform === null) {
      platform = $(el).parent().text().replace(/Platform:/i, '').trim();
    }
  });
  const name = platform ? `${gameName} - ${platform}` : gameName;

  // Size label: <strong>Size:</strong> <text>
  let size = null;
  $li.find('strong').each((_, el) => {
    if ($(el).text().trim().startsWith('Size:') && size === null) {
      size = $(el).parent().text().replace(/Size:/i, '').trim();
    }
  });

  // Magnet deferred to resolveMagnet() — normalize() sets needsMagnet automatically.
  return normalize({
    provider: 'blueroms',
    name,
    size,
    magnet: null,
    category: 'Games',
    detailUrl: downloadPageUrl,
  });
}

async function searchOne(base, query, page) {
  const url = `${base}/search?g=0&p=0&q=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error };
  const $ = cheerio.load(html);
  const items = $('div.row > div.col-xs-12 > div.card').toArray();
  if (items.length === 0) return { results: [], error: 'no_results_parsed' };

  // parseListItem is synchronous — no per-item HTTP round-trips.
  const results = items.map((li) => parseListItem($, li, base)).filter(Boolean);
  return { results, error: results.length === 0 ? 'no_results_parsed' : null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'blueroms'
  );
}

module.exports = { id: 'blueroms', name: 'BlueRoms', search, resolveMagnet };
