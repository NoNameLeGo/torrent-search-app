'use strict';

// OxTorrent HTML scraper (French site, UTF-8). The results table only links to
// each torrent's detail page, where the magnet + infoHash live, so we expose a
// resolveMagnet() used lazily on click. No iconv-lite needed (UTF-8).
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = [
  'https://oxtorrent.co',
  'https://oxtorrent.so',
];

async function searchOne(base, query, page) {
  const url = `${base}/recherche/${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('table > tbody > tr');
  if (!rows.length) return { base, results: [], error: 'no_results' };

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);
    const $link = $row.find('td:nth-child(1) > a').first();
    const name = $link.text().trim();
    const href = $link.attr('href');
    if (!name || !href) return;

    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
    results.push(normalize({
      provider: 'oxtorrent',
      name,
      detailUrl,
      needsMagnet: true,
    }));
  });
  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'OxTorrent'
  );
}

// Lazily fetch the magnet + infoHash from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };
  const $ = cheerio.load(html);
  const href = $('div.btn-magnet > a').first().attr('href');
  if (!href) return { magnet: null, error: 'no_magnet_on_page' };
  return { magnet: href, error: null };
}

module.exports = { id: 'oxtorrent', name: 'OxTorrent', search, resolveMagnet };
