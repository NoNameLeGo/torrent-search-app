'use strict';

// Rutor HTML scraper. The results list already carries the magnet link and all
// metadata. The site is served as UTF-8, so plain getText decoding is correct.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, ruDate, extractInfoHash, coercePage } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = [
  'https://rutor.info',
  'https://rutor.is',
  'https://rutor.ru',
];

// Site is served as UTF-8; getText decodes it correctly. Never throws.
async function getWin1251(url) {
  return getText(url);
}

async function searchOn(base, query, page) {
  // /search/<page>/<category>/<match>/<sort>/<query>
  const url = `${base}/search/${page}/0/010/2/${encodeURIComponent(query)}`;
  const { html, error } = await getWin1251(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  // div#index > table > tbody > tr, drop the first (header) row.
  const rows = $('div#index > table > tbody > tr');
  if (rows.length <= 1) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
  rows.slice(1).each((_, row) => {
    const $row = $(row);
    const $name = $row.find('td:nth-child(2) > a:nth-child(3)').first();
    const name = $name.text().trim();
    const magnet = $row.find('td:nth-child(2) > a:nth-child(2)').first().attr('href');
    if (!name || !magnet) return;

    const detailHref = $name.attr('href');
    const detailUrl = detailHref
      ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`)
      : null;

    const infoHash = extractInfoHash(magnet);

    // rutor's column layout has drifted from the reference Kotlin source (an
    // extra "comments" column was inserted), so match cells by content instead
    // of a fixed index: the size cell looks like "1.44 GB"; seeders/leechers
    // live in the td that carries two <span>s (green up / red down).
    let sizeText = '';
    let seedersText = '';
    let leechersText = '';
    $row.find('td').each((_, td) => {
      const $td = $(td);
      const t = $td.text().trim();
      // Russian site may render sizes with Cyrillic units (ГБ/МБ/КБ).
      // Note: no \b boundary (JS \b only matches ASCII words, so it would
      // never anchor a Cyrillic unit), and NO bare "B"/"Б" — a name like
      // "Black Box (2026)" contains a digit AND a "B" and would false-match.
      if (!sizeText && /\d/.test(t) && /(GB|MB|KB|TB|ГБ|МБ|КБ|ТБ)/i.test(t)) sizeText = t;
      const spans = $td.find('span');
      if (spans.length >= 2) {
        seedersText = $(spans[0]).text().trim();
        leechersText = $(spans[spans.length - 1]).text().trim();
      }
    });
    // Normalize Cyrillic units + comma decimals so downstream size parsing works.
    sizeText = sizeText
      .replace(/ГБ/gi, 'GB').replace(/МБ/gi, 'MB').replace(/КБ/gi, 'KB')
      .replace(',', '.');

    results.push(normalize({
      provider: 'rutor',
      name,
      size: sizeText,
      seeders: seedersText,
      leechers: leechersText,
      date: ruDate($row.find('td:nth-child(1)').text()),
      infoHash,
      magnet,
      detailUrl,
      category: null, // rutor search-list has no per-result category; let frontend infer
    }));
  });

  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  const p = coercePage(page) - 1; // rutor page is 0-based
  return runMirrors(
    DOMAINS.map((base) => () => searchOn(base, query, p)),
    'Rutor'
  );
}

module.exports = { id: 'rutor', name: 'Rutor', search };
