'use strict';

// MyPornClub HTML scraper. The results list gives name/size/seeders/peers/date.
// The infoHash and magnet live on each torrent's detail page and are deferred
// to resolveMagnet() to avoid N+1 fetches during search().
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

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

async function searchOne(base, query, page) {
  let q = encodeURIComponent(query.trim());
  q = q.replace(/%20/g, '-');
  const url = `${base}/s/${q}/seeders`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const items = $('div.torrents_list > div.torrent_element');
  if (items.length === 0) return { base, results: [], error: 'no_results_parsed' };

  const results = [];
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
    // Magnet deferred to resolveMagnet() — normalize() sets needsMagnet automatically.
    results.push(normalize({
      provider: 'mypornclub',
      name,
      size,
      seeders,
      leechers,
      date,
      magnet: null,
      detailUrl,
      category: 'Porn',
    }));
  });

  return { base, results, error: results.length === 0 ? 'no_results_parsed' : null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'mypornclub'
  );
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
