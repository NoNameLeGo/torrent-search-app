'use strict';

// XXXTracker HTML scraper. Mirrors the Kotlin provider: results come from a
// table; the magnet (and infoHash) are present directly in each row, so no
// per-result detail fetch is required. Upload dates use Russian month
// abbreviations, normalized to English before parsing (as the Kotlin does).
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = ['https://xxxtor.com'];

// Russian month abbreviations -> English (provider emits ru-RU month names).
const MONTH_MAP = {
  янв: 'Jan', фев: 'Feb', мар: 'Mar', апр: 'Apr',
  май: 'May', июн: 'Jun', июл: 'Jul', авг: 'Aug',
  сен: 'Sep', окт: 'Oct', ноя: 'Nov', дек: 'Dec',
};

function normalizeUploadDate(s) {
  if (!s) return s;
  const parts = s.trim().split(/\s+/);
  if (parts.length >= 3) {
    const eng = MONTH_MAP[parts[1].toLowerCase()] || parts[1];
    return `${parts[0]} ${eng} ${parts[2]}`;
  }
  return s;
}

async function searchOne(base, query, page) {
  const url = `${base}/b.php?search=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('table > tbody > tr');
  if (rows.length <= 1) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
  rows.each((i, el) => {
    if (i === 0) return;
    const $el = $(el);
    const name = $el.find('td:nth-child(2) > a:nth-child(3)').text().trim();
    if (!name) return;
    const magnet = $el.find('td:nth-child(2) > a:nth-child(1)').attr('href') || null;
    if (!magnet || !magnet.startsWith('magnet:')) return;

    const infoHash = extractInfoHash(magnet);

    const size = $el.find('td:nth-child(3)').text().trim();
    const seeders = $el
      .find('td:nth-child(4) > span:nth-child(1)')
      .text()
      .trim();
    const leechers = $el
      .find('td:nth-child(4) > span:nth-child(2)')
      .text()
      .trim();
    const date = normalizeUploadDate(
      $el.find('td:nth-child(1)').text().trim()
    );

    const detailHref = $el.find('td:nth-child(2) > a:nth-child(3)').attr('href');
    const detailUrl = detailHref
      ? detailHref.startsWith('http')
        ? detailHref
        : `${base}${detailHref}`
      : null;

    results.push(
      normalize({
        provider: 'xxxtracker',
        name,
        size,
        seeders,
        leechers,
        date,
        infoHash,
        magnet,
        detailUrl,
        category: 'Porn',
      })
    );
  });
  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'xxxtracker'
  );
}

module.exports = { id: 'xxxtracker', name: 'XXXTracker', search };
