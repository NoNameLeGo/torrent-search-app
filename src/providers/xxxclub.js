'use strict';

// XXXClub HTML scraper. The results list gives name/size/seeders/peers/date.
// The magnet and infoHash live on each torrent's detail page and are deferred
// to resolveMagnet() to avoid N+1 fetches during search().
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = ['https://xxxclub.to'];

// Fetch + parse a torrent detail page for its magnet link and infoHash.
async function fetchDetails(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return null;
  const $ = cheerio.load(html);
  const magnet = $('a[href^="magnet:?"]').first().attr('href') || null;
  const infoHash = extractInfoHash(magnet);
  return { magnet, infoHash };
}

async function searchOne(base, query, page) {
  const url = `${base}/torrents/search/all/${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error };

  const $ = cheerio.load(html);
  const container = $('div.browsetableinside, div.divtableinside').first();
  if (!container.length) return { results: [], error: 'no_results_parsed' };
  const items = container.find('ul > li');
  if (items.length === 0) return { results: [], error: 'no_results_parsed' };

  const results = [];
  items.each((_, el) => {
    const $el = $(el);
    const $a = $el.find('span:nth-child(2) > a[href^="/torrents/details"]').first();
    const detailHref = $a.attr('href');
    if (!detailHref) return;
    const detailUrl = detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`;
    const name = $a.text().trim();
    if (!name) return;
    const size = $el.find('span.siz').text().trim();
    const seeders = $el.find('span.see').text().trim();
    const leechers = $el.find('span.lee').text().trim();
    const date = $el.find('span.adde').text().trim();
    // Magnet deferred to resolveMagnet() — normalize() sets needsMagnet automatically.
    results.push(normalize({
      provider: 'xxxclub',
      name,
      size,
      seeders,
      leechers,
      date,
      magnet: null,
      detailUrl,
      category: 'Porn',
    }));
  });

  return { results, error: results.length === 0 ? 'no_results_parsed' : null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'xxxclub'
  );
}

// Lazily fetch the magnet link from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const det = await fetchDetails(detailUrl);
  if (!det || !det.magnet) return { magnet: null, error: 'no_magnet' };
  return { magnet: det.magnet, error: null };
}

module.exports = { id: 'xxxclub', name: 'XXXClub', search, resolveMagnet };
