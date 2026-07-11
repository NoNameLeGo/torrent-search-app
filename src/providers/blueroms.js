'use strict';

// BlueRoms HTML scraper. The results page links to a per-torrent download
// page whose `button#magnet-button` carries a base64-encoded magnet URI.
// We decode it to obtain the magnet + info hash for each result.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://www.blueroms.ws'];

// Fetch the download page and decode the base64 magnet URI from the button.
async function getMagnetUri(downloadPageUrl) {
  const { html, error } = await getText(downloadPageUrl);
  if (error || !html) return null;
  const $ = cheerio.load(html);
  const enc = $('#magnet-button').first().attr('data-link');
  if (!enc) return null;
  try {
    return Buffer.from(enc, 'base64').toString('utf8');
  } catch (e) {
    return null;
  }
}

async function parseListItem($, listItem, base) {
  const $li = $(listItem);
  const $dl = $li.find('div.card-footer > a').first();
  const dlHref = $dl.attr('href');
  if (!dlHref) return null;
  const downloadPageUrl = dlHref.startsWith('http') ? dlHref : `${base}${dlHref}`;

  const magnetUri = await getMagnetUri(downloadPageUrl);
  if (!magnetUri) return null;

  const $name = $li.find('h4.card-title > a').first();
  const gameName = $name.text().trim();
  if (!gameName) return null;

  // Platform label: <strong>Platform:</strong> <text>
  let platform = null;
  $li.find('strong').each((_, el) => {
    if ($(el).text().trim().startsWith('Platform:') && platform === null) {
      platform = $(el).parent().text().replace(/Platform:/i, '').trim();
    }
  });
  const name = platform ? `${gameName} - ${platform}` : gameName;

  // Size label: <strong>Size:</strong> <text>
  let size = null;
  $li.find('strong').each((_, el) => {
    if ($(el).text().trim().startsWith('Size:') && size === null) {
      size = $(el).parent().text().replace(/Size:/i, '').trim();
    }
  });

  const detailsHref = $name.attr('href');
  const detailUrl = detailsHref
    ? detailsHref.startsWith('http') ? detailsHref : `${base}${detailsHref}`
    : null;

  let infoHash = null;
  const m = magnetUri.match(/btih:([a-f0-9]+)/i);
  if (m) infoHash = m[1];

  return normalize({
    provider: 'blueroms',
    name,
    size,
    magnet: magnetUri,
    infoHash,
    category: 'Games',
    detailUrl,
  });
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search?g=0&p=0&q=${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { results: [], error };
      const $ = cheerio.load(html);
      const items = $('div.row > div.col-xs-12 > div.card').toArray();
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
  return { results: [], error: `blueroms unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'blueroms', name: 'BlueRoms', search };
