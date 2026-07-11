'use strict';

// Bt4g HTML scraper. The results list shows name/size/seeders but NOT the
// infoHash; that lives on each torrent's detail page (embedded in a
// //downloadtorrentfile.com/hash/<hash> link). So we set needsMagnet + detailUrl
// and lazily resolve the magnet via resolveMagnet() — mirroring the Kotlin
// flow where getInfoHash() fetches the detail page.
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://bt4gprx.com';
const DOMAINS = [BASE];

function categoryFromRaw(raw) {
  if (raw === 'Video' || raw === 'Movie') return 'Movies';
  if (raw === 'Audio') return 'Music';
  if (raw === 'Doc') return 'Books';
  if (raw === 'App' || raw === 'Application') return 'Apps';
  return 'Other';
}

function parseRows($, base) {
  const items = $('div.notion-list-item');
  if (items.length === 0) return [];

  const results = [];
  items.each((_, el) => {
    const $item = $(el);

    const $title = $item.find('div.notion-list-item-title > a').first();
    const name = $title.text().trim();
    if (!name) return;

    const href = $title.attr('href') || '';
    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
    if (!detailUrl) return;

    const size = $item.find('div.notion-list-item-meta > span:nth-child(5) > b').text();
    const seeders = $item.find('div.notion-list-item-meta span#seeders').text();
    const leechers = $item.find('div.notion-list-item-meta span#leechers').text();

    let date = $item.find('div.notion-list-item-meta > span:nth-child(2)').text();
    if (date) date = date.replace(/^Creation Time:\s*/, '').trim();

    const rawCat = $item.find('div.notion-list-item-meta span.notion-tag').first().text().trim();
    const category = rawCat ? categoryFromRaw(rawCat) : null;

    results.push(normalize({
      provider: 'bt4g',
      name,
      size,
      date,
      seeders,
      leechers,
      detailUrl,
      category,
      needsMagnet: true,
    }));
  });
  return results;
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search?q=${encodeURIComponent(query)}&category=all&orderby=seeders&p=${page}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };
      const $ = cheerio.load(html);
      const results = parseRows($, base);
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
  return { results: [], error: `Bt4g unreachable (${errs.join('; ')})` };
}

// Fetch the detail page and pull the 40-char btih out of the
// //downloadtorrentfile.com/hash/<hash>?... link, then build the magnet.
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };

  const $ = cheerio.load(html);
  const href = $('a[href^="//downloadtorrentfile.com/hash/"]').first().attr('href');
  if (!href) return { magnet: null, error: 'no_hash_on_page' };

  const hash = href
    .replace(/^\/\/downloadtorrentfile\.com\/hash\//, '')
    .split('?')[0];
  if (!hash) return { magnet: null, error: 'no_hash_on_page' };

  return { magnet: `magnet:?xt=urn:btih:${hash}`, error: null };
}

module.exports = { id: 'bt4g', name: 'Bt4g', search, resolveMagnet };
