'use strict';

// Mikan (mikanani.me) — HTML scraper. Each result row carries the magnet in a
// `data-clipboard-text` attribute, so no second request is needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const BASE = 'https://mikanani.me';
const DOMAINS = [BASE];

function ownText($el) {
  if (!$el || !$el.length) return '';
  let t = '';
  $el.contents().each((_, n) => {
    if (n.type === 'text') t += n.data;
  });
  return t.trim();
}

function parseListItem($, row, base) {
  const $row = $(row);
  const $nameLink = $row.find('td:nth-child(2) > a:nth-child(1)').first();
  const name = ownText($nameLink);
  if (!name) return null;

  const magnet =
    $row.find('td:nth-child(2) > a[data-clipboard-text]').first().attr('data-clipboard-text') ||
    null;
  if (!magnet) return null;

  const size = ownText($row.find('td:nth-child(3)'));
  const dateText = ownText($row.find('td:nth-child(4)'));
  const fileHref = $row.find('td:nth-child(5) > a').first().attr('href');
  const fileDownloadLink = fileHref
    ? fileHref.startsWith('http')
      ? fileHref
      : `${base}${fileHref}`
    : null;
  const href = $nameLink.attr('href');
  const detailUrl = href ? (href.startsWith('http') ? href : `${base}${href}`) : null;
  const infoHash = extractInfoHash(magnet);

  return {
    provider: 'mikan',
    name,
    size: size || null,
    date: dateText || null,
    infoHash,
    magnet,
    detailUrl,
    fileDownloadLink,
    category: 'Anime',
  };
}

async function searchOn(base, query) {
  const url = `${base}/Home/Search?searchstr=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error: error || 'no_html' };

  const $ = cheerio.load(html);
  const rows = $('tr.js-search-results-row');
  if (rows.length === 0) return { results: [], error: null };

  const results = [];
  rows.each((_, row) => {
    const raw = parseListItem($, row, base);
    if (raw) results.push(normalize(raw));
  });
  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOn(base, query)),
    'mikan'
  );
}

module.exports = { id: 'mikan', name: 'Mikan', search };
