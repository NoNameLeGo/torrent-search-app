'use strict';

// XXXClub HTML scraper. Mirrors the Kotlin provider: the results list gives
// name/size/seeders/peers/date, but the magnet (and infoHash) live on each
// torrent's detail page, so we fetch every detail page in parallel to fill them.
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://xxxclub.to'];

// Fetch + parse a torrent detail page for its magnet link and infoHash.
async function fetchDetails(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return null;
  const $ = cheerio.load(html);
  const magnet = $('a[href^="magnet:?"]').first().attr('href') || null;
  let infoHash = null;
  if (magnet) {
    const m = magnet.match(/xt=urn:btih:([a-f0-9]+)/i);
    if (m) infoHash = m[1].toLowerCase();
  }
  return { magnet, infoHash };
}

async function search(query, { page = 1 } = {}) {
  // Try all mirror domains in parallel; take the first that yields results.
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/torrents/search/all/${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const container = $('div.browsetableinside, div.divtableinside').first();
      if (!container.length) return { base, results: [], error: 'no_results_parsed' };
      const items = container.find('ul > li');
      if (items.length === 0) return { base, results: [], error: 'no_results_parsed' };

      const partials = [];
      items.each((_, el) => {
        const $el = $(el);
        const $a = $el.find('span:nth-child(2) > a[href^="/torrents/details"]').first();
        const detailHref = $a.attr('href');
        if (!detailHref) return;
        const detailUrl = detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`;
        const name = $a.text().trim();
        if (!name) return;
        const size = $el.find('span.siz').text().trim();
        const seeders = $el.find('span.see').text().trim();
        const leechers = $el.find('span.lee').text().trim();
        const date = $el.find('span.adde').text().trim();
        partials.push({ name, size, seeders, leechers, date, detailUrl });
      });

      if (partials.length === 0) return { base, results: [], error: 'no_results_parsed' };

      const results = await Promise.all(
        partials.map(async (p) => {
          const det = await fetchDetails(p.detailUrl);
          const magnet = det && det.magnet;
          const infoHash = det && det.infoHash;
          return normalize({
            provider: 'xxxclub',
            name: p.name,
            size: p.size,
            seeders: p.seeders,
            leechers: p.leechers,
            date: p.date,
            infoHash,
            magnet,
            detailUrl: p.detailUrl,
            category: 'Porn',
            needsMagnet: !magnet,
          });
        })
      );
      return { base, results: results.filter(Boolean), error: null };
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
  return { results: [], error: `xxxclub unreachable (${errs.join('; ')})` };
}

// Lazily fetch the magnet link from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const det = await fetchDetails(detailUrl);
  if (!det || !det.magnet) return { magnet: null, error: 'no_magnet' };
  return { magnet: det.magnet, error: null };
}

module.exports = { id: 'xxxclub', name: 'XXXClub', search, resolveMagnet };
