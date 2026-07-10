'use strict';

// TheRarBg HTML scraper. The results list already carries name, size,
// seeders, peers, upload date and category. The magnet/infoHash only lives on
// the detail page, so we expose needsMagnet + resolveMagnet (lazily resolved by
// the UI, mirroring the Kotlin detail-page fetch).
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://therarbg.com',
  'https://therarbg.to',
  'https://therarbg.org',
];

function categoryFromRaw(raw) {
  switch ((raw || '').trim()) {
    case 'Anime': return 'Anime';
    case 'Apps': return 'Apps';
    case 'Books': return 'Books';
    case 'Games': return 'Games';
    case 'Movies': return 'Movies';
    case 'Music': return 'Music';
    case 'XXX': return 'Porn';
    case 'Tv': return 'Series';
    default: return 'Other';
  }
}

async function searchOn(base, query) {
  const url = `${base}/get-posts/keywords:${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('table > tbody > tr.list-entry');
  if (rows.length === 0) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);
    const $link = $row.find('td.cellName > div > a').first();
    const name = $link.text().trim();
    if (!name) return;

    const href = $link.attr('href');
    const detailUrl = href
      ? (href.startsWith('http') ? href : `${base}${href}`)
      : null;
    if (!detailUrl) return;

    const sizeOrder = $row.find('td.sizeCell').attr('data-order');
    const dateOrder = $row.find('td:nth-child(4)').attr('data-order');
    const uploadDate = dateOrder ? parseInt(dateOrder, 10) * 1000 : null;

    results.push(normalize({
      provider: 'therarbg',
      name,
      size: sizeOrder ? parseInt(sizeOrder, 10) : null,
      seeders: $row.find('td:nth-child(7)').text(),
      leechers: $row.find('td:nth-child(8)').text(),
      date: uploadDate,
      detailUrl,
      category: categoryFromRaw($row.find('td:nth-child(3) > a').text()),
      needsMagnet: true,
    }));
  });

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
  return { results: [], error: `TheRarBg unreachable (${errs.join('; ')})` };
}

// Lazily fetch the magnet link (and infoHash) from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };
  const $ = cheerio.load(html);
  const href = $('a[href^="magnet:?"]').first().attr('href');
  if (!href) return { magnet: null, error: 'no_magnet_on_page' };
  return { magnet: href, error: null };
}

module.exports = { id: 'therarbg', name: 'TheRarBg', search, resolveMagnet };
