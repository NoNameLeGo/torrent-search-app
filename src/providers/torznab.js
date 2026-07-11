'use strict';

// Torznab provider factory. Torznab is the de-facto API spec implemented by
// Jackett, Prowlarr and most *arr indexers. A stored "config" (user-added
// indexer) produces a standard provider object that plugs into the aggregate
// search layer.
//
// Request : {apiBase}?apikey={key}&extended=1&t=search&q={query}
// Response: RSS XML; each <item> exposes title, magnet (enclosure),
//           size, and seeders/peers/infohash via <torznab:attr>.

const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

// Normalize a user-supplied Torznab URL into the API endpoint (ends with /api).
// Accepts both the full "Copy Torznab Feed" URL from Jackett/Prowlarr and a
// bare base host.
function normalizeApiUrl(url) {
  if (!url) return '';
  let u = String(url).trim();
  const q = u.indexOf('?');
  if (q >= 0) u = u.slice(0, q); // drop any query string
  u = u.replace(/\/+$/, ''); // drop trailing slashes
  if (!/\/api$/i.test(u)) u += '/api'; // ensure it ends at /api
  return u;
}

// Build the search request URL for a config + query.
function buildSearchUrl(config, query) {
  const api = normalizeApiUrl(config.url);
  const params = new URLSearchParams({
    apikey: config.apiKey || '',
    extended: '1',
    t: 'search',
    q: query,
  });
  return `${api}?${params.toString()}`;
}

// Parse a torznab RSS XML string into normalized result objects.
function parseItems(xml, providerId) {
  const $ = cheerio.load(xml, { xmlMode: true });
  const items = [];
  $('item').each((_, el) => {
    const $el = $(el);
    const name = $el.find('title').first().text().trim();
    if (!name) return;

    // Magnet link: enclosure url if it's a magnet, else <link>, else none.
    const enc = $el.find('enclosure').first();
    const encUrl = enc.attr('url') || '';
    let magnet = encUrl.startsWith('magnet:') ? encUrl : '';
    if (!magnet) {
      const link = $el.find('link').first().text().trim();
      if (link.startsWith('magnet:')) magnet = link;
    }

    // Size: enclosure length attr, fallback to <size> element.
    let size = enc.attr('length') || '';
    if (!size || isNaN(parseInt(size, 10))) {
      size = $el.find('size').first().text().trim();
    }

    // torznab:attr helpers.
    const attr = (name) => {
      let v = null;
      $el.find('torznab\\:attr').each((_, a) => {
        const $a = $(a);
        if (($a.attr('name') || '').toLowerCase() === name.toLowerCase()) {
          v = $a.attr('value');
        }
      });
      return v;
    };

    const infoHashFromMagnet = magnet.match(/btih:([a-f0-9]+)/i);
    const infoHash =
      attr('infohash') || (infoHashFromMagnet ? infoHashFromMagnet[1] : null);
    const seeders = attr('seeders');
    const leechers = attr('peers');
    const category = attr('category');

    const detailUrl =
      $el.find('comments').first().text().trim() ||
      $el.find('link').first().text().trim() ||
      $el.find('guid').first().text().trim() ||
      null;

    const pubDate = $el.find('pubDate').first().text().trim();

    items.push(
      normalize({
        provider: providerId,
        name,
        size,
        seeders,
        leechers,
        date: pubDate,
        infoHash,
        magnet,
        detailUrl,
        category: category || null,
      })
    );
  });
  return items;
}

async function searchOn(config, query) {
  const url = buildSearchUrl(config, query);
  const { html, error } = await getText(url, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { results: [], error: error || 'empty_response' };
  try {
    const results = parseItems(html, config.id);
    return { results, error: null };
  } catch (e) {
    return { results: [], error: e.message || 'parse_failed' };
  }
}

// Factory: turn a stored config into a standard provider object.
function makeProvider(config) {
  const id = config.id || `torznab:${config.name}`;
  return {
    id,
    name: config.name || 'Torznab',
    enabled: config.enabled !== false,
    search: async (query, _opts = {}) => searchOn(config, query),
  };
}

module.exports = { normalizeApiUrl, buildSearchUrl, parseItems, makeProvider };
