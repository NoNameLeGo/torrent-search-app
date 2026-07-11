'use strict';

// TokyoToshokan HTML scraper. Each torrent spans TWO consecutive <tr> rows:
//   tr1 = title row (name, magnet, category, detail link)
//   tr2 = meta row (size | date, seeders, leechers)
// Kotlin's search uses no pagination, so the `page` arg is ignored.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://tokyotosho.info';
const DOMAINS = [BASE];

function categoryFromId(id) {
  if (['1', '7', '8', '10', '11'].includes(id)) return 'Anime';
  if (id === '3') return 'Books';
  if (['2', '9'].includes(id)) return 'Music';
  if (['4', '12', '13', '14', '15'].includes(id)) return 'Porn';
  return 'Other';
}

// Replicate Kotlin's split('|').drop(1).map { drop leading non-space, trim }.
function splitSizeDate(text) {
  const parts = String(text).split('|').slice(1).map((s) => s.trim().replace(/^\S*\s*/, '').trim());
  return [parts[0] || null, parts[1] || null];
}

function parseRows($, base) {
  const rows = $('table.listing > tbody > tr:nth-child(n+2)');
  if (rows.length === 0) return [];

  const results = [];
  for (let i = 0; i + 1 < rows.length; i += 2) {
    const $tr1 = $(rows[i]);
    const $tr2 = $(rows[i + 1]);

    const $name = $tr1.find('td.desc-top > a:nth-child(2)').first();
    const name = $name.text().trim();
    if (!name) continue;

    const magnet = $tr1.find('td.desc-top > a:nth-child(1)').first().attr('href');
    if (!magnet) continue;

    const infoHashMatch = magnet.match(/xt=urn:btih:([a-f0-9]+)/i);
    const infoHash = infoHashMatch ? infoHashMatch[1] : null;

    const detailHref = $tr1.find('td.web > a:last-child').first().attr('href');
    const detailUrl = detailHref ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`) : null;

    const catHref = $tr1.find('td:nth-child(1) > a').first().attr('href');
    const category = catHref ? categoryFromId(catHref.replace('/?cat=', '')) : null;

    const [size, rawDate] = splitSizeDate($tr2.find('td.desc-bot').text());

    results.push(normalize({
      provider: 'tokyotoshokan',
      name,
      size,
      date: rawDate,
      seeders: $tr2.find('td.stats > span:nth-child(1)').text(),
      leechers: $tr2.find('td.stats > span:nth-child(2)').text(),
      infoHash,
      magnet,
      detailUrl,
      category,
    }));
  }
  return results;
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      // type=0 -> All categories (Kotlin default; search context category unused here)
      const url = `${base}/search.php?terms=${encodeURIComponent(query)}&type=0&searchName=true`;
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
  return { results: [], error: `TokyoToshokan unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'tokyotoshokan', name: 'TokyoToshokan', search };
