'use strict';

// MyPornClub HTML scraper. Mirrors the Kotlin provider: the results list gives
// name/size/seeders/peers/date, but the infoHash (and magnet) live on each
// torrent's detail page, so we fetch every detail page in parallel to fill them.
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://myporn.club'];

// Fetch + parse a torrent detail page for its infoHash and magnet link.
async function fetchDetails(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return null;
  const $ = cheerio.load(html);
  let infoHash = $('div.torrent_info_div > div:nth-child(1)').text().trim();
  infoHash = infoHash.replace(/^\[hash_info\]:/i, '').trim().toLowerCase() || null;
  const magnet = $('a.md_btn').first().attr('href') || null;
  return { infoHash, magnet };
}

async function search(query, { page = 1 } = {}) {
  // Try all mirror domains in parallel; take the first that yields results.
  const attempts = DOMAINS.map((base) =>
    (async () => {
      // Kotlin: query.trim().replace("%20", "-") then /s/<q>/seeders
      let q = encodeURIComponent(query.trim());
      q = q.replace(/%20/g, '-');
      const url = `${base}/s/${q}/seeders`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const items = $('div.torrents_list > div.torrent_element');
      if (items.length === 0) return { base, results: [], error: 'no_results_parsed' };

      const partials = [];
      items.each((_, el) => {
        const $el = $(el);
        const $a = $el.find('div.torrent_element_text_div > a:nth-child(2)');
        const detailHref = $a.attr('href');
        if (!detailHref) return;
        const detailUrl = detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`;
        const name = $el
          .find('div.torrent_element_text_div > a:nth-child(2) > span.torrent_element_text_span')
          .text()
          .trim();
        if (!name) return;
        const size = $el
          .find('div.torrent_element_info > span.teiv:nth-child(4)')
          .text()
          .trim();
        const seeders = $el
          .find('div.torrent_element_info > span.teiv.teiv_seeders')
          .text()
          .trim();
        const leechers = $el
          .find('div.torrent_element_info > span.teiv.teiv_leechers')
          .text()
          .trim();
        const date = $el
          .find('div.torrent_element_info > span.teiv:nth-child(2)')
          .text()
          .trim();
        partials.push({ name, size, seeders, leechers, date, detailUrl });
      });

      if (partials.length === 0) return { base, results: [], error: 'no_results_parsed' };

      const results = await Promise.all(
        partials.map(async (p) => {
          const det = await fetchDetails(p.detailUrl);
          const infoHash = det && det.infoHash;
          let magnet = det && det.magnet;
          if (!magnet && infoHash) magnet = `magnet:?xt=urn:btih:${infoHash}`;
          return normalize({
            provider: 'mypornclub',
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
  return { results: [], error: `mypornclub unreachable (${errs.join('; ')})` };
}

// Lazily fetch the magnet link from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const det = await fetchDetails(detailUrl);
  if (!det) return { magnet: null, error: 'no_details' };
  let magnet = det.magnet;
  if (!magnet && det.infoHash) magnet = `magnet:?xt=urn:btih:${det.infoHash}`;
  return magnet ? { magnet, error: null } : { magnet: null, error: 'no_magnet' };
}

module.exports = { id: 'mypornclub', name: 'MyPornClub', search, resolveMagnet };
