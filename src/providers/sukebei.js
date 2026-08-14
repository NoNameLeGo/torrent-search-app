'use strict';

// Sukebei (the "fansub" / non-anime half of nyaa) HTML scraper.
// Results table exposes the magnet link directly, so no detail fetch needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const BASE = 'https://sukebei.nyaa.si';

// Kotlin only lists a single domain; try it (mirror pattern kept for parity).
const DOMAINS = [BASE];

// Nyaa/Sukebei numeric category codes → standard bucket.
// Sukebei uses the same category scheme as nyaa.si (2-byte codes).
// https://github.com/nyaadevs/nyaa/wiki/FAQ#what-does-the-category-mean
const CATEGORY_MAP = {
  '1': 'Anime',   // 1_0 = Anime
  '2': 'Anime',   // 2_0 = Anime Alternate
  '3': 'Movies',  // 3_0 = Live Action
  '4': 'Porn',    // 4_0 = Hentai
  '5': 'Other',   // 5_0 = Irregular
  '6': 'Other',   // 6_0 = Photos
};

function mapSukebeiCategory(raw) {
  if (!raw) return null;
  const code = String(raw).trim().split('_')[0];
  return CATEGORY_MAP[code] || null;
}

function parseRows($, base) {
  const rows = $('table.torrent-list > tbody > tr');
  if (rows.length === 0) return [];

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);

    const $name = $row.find('td:nth-child(2) > a:not(.comments)').first();
    const name = $name.text().trim();
    if (!name) return;

    const magnet = $row.find('td:nth-child(3) > a:nth-child(2)').first().attr('href');
    if (!magnet) return;

    const infoHash = extractInfoHash(magnet);

    const href = $name.attr('href') || '';
    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;

    // Extract category from the first column's link title (e.g., "Hentai - English Translated")
    const rawCat = $row.find('td:nth-child(1) a').first().attr('title');
    const category = mapSukebeiCategory(rawCat);

    results.push(normalize({
      provider: 'sukebei',
      name,
      size: $row.find('td:nth-child(4)').text(),
      date: $row.find('td:nth-child(5)').attr('data-timestamp'),
      seeders: $row.find('td:nth-child(6)').text(),
      leechers: $row.find('td:nth-child(7)').text(),
      infoHash,
      magnet,
      detailUrl,
      category,
    }));
  });
  return results;
}

async function searchOne(base, query, page) {
  const url = `${base}/?f=0&c=0_0&q=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };
  const $ = cheerio.load(html);
  const results = parseRows($, base);
  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'Sukebei'
  );
}

module.exports = { id: 'sukebei', name: 'Sukebei', search };
