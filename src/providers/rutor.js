'use strict';

// Rutor HTML scraper. The results list already carries the magnet link and all
// metadata. The site is served as UTF-8, so plain getText decoding is correct.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://rutor.info',
  'https://rutor.is',
  'https://rutor.ru',
];

// Russian genitive month abbreviations -> English, for date parsing.
const RU_MONTHS = {
  янв: 'Jan', фев: 'Feb', мар: 'Mar', апр: 'Apr', мая: 'May', июн: 'Jun',
  июл: 'Jul', авг: 'Aug', сен: 'Sep', окт: 'Oct', ноя: 'Nov', дек: 'Dec',
};

function ruDate(s) {
  if (!s) return s;
  s = s.replace(/Сегодня/i, 'Today').replace(/Вчера/i, 'Yesterday');
  return s.replace(/[а-яё]+/gi, (m) => RU_MONTHS[m.toLowerCase()] || m);
}

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

    let infoHash = null;
    const m = magnet.match(/btih:([a-f0-9]+)/i);
    if (m) infoHash = m[1];

    results.push(normalize({
      provider: 'rutor',
      name,
      size: $row.find('td:nth-child(3)').text(),
      seeders: $row.find('td:nth-child(4) > span:nth-child(1)').text(),
      leechers: $row.find('td:nth-child(4) > span:nth-child(3)').text(),
      date: ruDate($row.find('td:nth-child(1)').text()),
      infoHash,
      magnet,
      detailUrl,
      category: 'Other',
    }));
  });

  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  const p = Math.max(0, (page | 0) - 1); // rutor page is 0-based
  const attempts = DOMAINS.map((base) => searchOn(base, query, p));
  const settled = await Promise.allSettled(attempts);
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `Rutor unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'rutor', name: 'Rutor', search };
