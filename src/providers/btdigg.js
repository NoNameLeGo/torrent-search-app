'use strict';

// BTDigg HTML scraper. The results list exposes the magnet link directly,
// so no detail-page fetch is needed for search.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://btdig.com';
const DOMAINS = [BASE];

function parseRows($, base) {
  const items = $('div.one_result > div');
  if (items.length === 0) return [];

  const results = [];
  items.each((_, el) => {
    const $item = $(el);

    const $name = $item.find('div.torrent_name > a').first();
    const name = $name.text().trim();
    if (!name) return;

    const magnet = $item.find('div.torrent_magnet > div.fa-magnet > a').first().attr('href');
    if (!magnet) return;

    const infoHashMatch = magnet.match(/xt=urn:btih:([a-f0-9]+)/i);
    const infoHash = infoHashMatch ? infoHashMatch[1] : null;

    const href = $name.attr('href') || '';
    const detailUrl = href.startsWith('http') ? href : `${base}${href}`;

    let date = $item.find('span.torrent_age').text();
    if (date) date = date.replace(/^found\s+/, '').trim();

    results.push(normalize({
      provider: 'btdigg',
      name,
      size: $item.find('span.torrent_size').text(),
      date,
      infoHash,
      magnet,
      detailUrl,
      category: 'Other',
    }));
  });
  return results;
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search?q=${encodeURIComponent(query)}`;
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
  return { results: [], error: `BTDigg unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'btdigg', name: 'BTDigg', search };
