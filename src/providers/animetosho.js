'use strict';

// AnimeTosho — HTML scraper. The results list exposes the magnet link directly
// in each row, so no second request is needed in the common case.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const BASE = 'https://animetosho.org';
const DOMAINS = [BASE];

// "Today" / "Yesterday" / "d/M/yyyy ..." -> timestamp.
function parseDate(raw) {
  const s = (raw || '').trim();
  if (!s) return null;
  if (/^today/i.test(s)) return Date.now();
  if (/^yesterday/i.test(s)) return Date.now() - 86400000;
  const first = s.split(/\s+/)[0];
  const parts = first.split('/');
  if (parts.length === 3) {
    const d = parseInt(parts[0], 10);
    const m = parseInt(parts[1], 10);
    const y = parseInt(parts[2], 10);
    if (!isNaN(d) && !isNaN(m) && !isNaN(y)) {
      const dt = new Date(y, m - 1, d);
      if (!isNaN(dt.getTime())) return dt.getTime();
    }
  }
  const t = Date.parse(s);
  return isNaN(t) ? null : t;
}

function parseEntry($, entry, base) {
  const $entry = $(entry);
  const $anchor = $entry.find('div.link > a').first();
  const name = $anchor.text().trim();
  if (!name) return null;

  const href = $anchor.attr('href');
  const detailUrl = href ? (href.startsWith('http') ? href : `${base}${href}`) : null;

  const size = $entry.find('div.size').first().text().trim();
  if (!size) return null;

  const $links = $entry.find('div.links').first();
  const $span = $links.find('span').filter((_, el) => $(el).attr('title')).first();
  let seeders = null;
  let leechers = null;
  if ($span.length) {
    const m = $span.text().match(/(\d+)\D+(\d+)/);
    if (m) {
      seeders = m[1];
      leechers = m[2];
    }
  }

  const dateAttr = $entry.find('div.date').first().attr('title');
  const date = parseDate(dateAttr ? dateAttr.replace('Date/time submitted:', '').trim() : null);
  if (!date) return null;

  const magnet = $links.find('a[href^="magnet:"]').first().attr('href') || null;
  if (!magnet) return null;
  const fileDownloadLink = $links.find('a.dllink').first().attr('href') || null;
  const infoHash = extractInfoHash(magnet);

  return {
    provider: 'animetosho',
    name,
    size,
    seeders,
    leechers,
    date,
    infoHash,
    magnet,
    detailUrl,
    fileDownloadLink,
    category: 'Anime',
  };
}

async function searchOn(base, query) {
  const url = `${base}/search?q=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error: error || 'no_html' };

  const $ = cheerio.load(html);
  const entries = $('div.home_list_entry');
  if (entries.length === 0) return { results: [], error: null };

  const results = [];
  entries.each((_, entry) => {
    const raw = parseEntry($, entry, base);
    if (raw) results.push(normalize(raw));
  });
  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOn(base, query)),
    'animetosho'
  );
}

module.exports = { id: 'animetosho', name: 'AnimeTosho', search };
