'use strict';

// MegaPeer HTML scraper. The magnet URI lives on each torrent's detail page,
// so we fetch it per item. The site is served as UTF-8, so plain getText
// decoding is correct.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://megapeer.vip'];

// Site is served as UTF-8; getText decodes it correctly. Never throws.
async function getWin1251(url) {
  return getText(url);
}

// Pull the magnet URI from a torrent's detail page.
async function getMagnetUri(detailUrl) {
  const { html, error } = await getWin1251(detailUrl);
  if (error || !html) return null;
  const $ = cheerio.load(html);
  const href = $('a[href^="magnet:?xt="]').first().attr('href');
  return href || null;
}

// Best-effort Russian "d MMM yy" date -> English form Date.parse can handle.
const RU_MONTHS = {
  'янв': 'Jan', 'фев': 'Feb', 'мар': 'Mar', 'апр': 'Apr',
  'май': 'May', 'июн': 'Jun', 'июл': 'Jul', 'авг': 'Aug',
  'сен': 'Sep', 'окт': 'Oct', 'ноя': 'Nov', 'дек': 'Dec',
};
function parseRuDate(s) {
  if (!s) return null;
  let out = s.replace(/Мая/gi, 'Май');
  for (const [ru, en] of Object.entries(RU_MONTHS)) {
    out = out.replace(new RegExp(ru, 'ig'), en);
  }
  return out.trim();
}

async function parseListItem($, listItem, base) {
  const $li = $(listItem);
  const $name = $li.find('td:nth-child(2) > a:nth-child(2)').first();
  const nameHref = $name.attr('href');
  if (!nameHref) return null;
  const detailUrl = nameHref.startsWith('http') ? nameHref : `${base}${nameHref}`;

  const magnetUri = await getMagnetUri(detailUrl);
  if (!magnetUri) return null;

  const name = $name.text().trim();
  if (!name) return null;

  const size = $li.find('td:nth-child(3)').first().text().trim();
  const seeders = $li.find('td:nth-child(4) > font:nth-child(2)').first().text().trim();
  const leechers = $li.find('td:nth-child(4) > font:nth-child(4)').first().text().trim();
  const dateRaw = $li.find('td:nth-child(1)').first().text().trim();
  const date = parseRuDate(dateRaw);

  let infoHash = null;
  const m = magnetUri.match(/btih:([a-f0-9]+)/i);
  if (m) infoHash = m[1];

  return normalize({
    provider: 'megapeer',
    name,
    size,
    seeders,
    leechers,
    date,
    magnet: magnetUri,
    infoHash,
    detailUrl,
  });
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url =
        `${base}/browse.php?search=${encodeURIComponent(query)}` +
        `&age=&cat=0&stype=0&sort=0&ascdesc=0`;
      const { html, error } = await getWin1251(url);
      if (error || !html) return { results: [], error };

      const $ = cheerio.load(html);
      const items = $('div#index > table > tbody > tr.table_fon').toArray();
      if (items.length === 0) return { results: [], error: 'no_results_parsed' };

      const parsed = await Promise.all(
        items.map((li) => parseListItem($, li, base).catch(() => null))
      );
      const results = parsed.filter(Boolean);
      return { results, error: null };
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
  return { results: [], error: `megapeer unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'megapeer', name: 'MegaPeer', search };
