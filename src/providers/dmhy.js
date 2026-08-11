'use strict';

// DMHY (share.dmhy.org) — HTML scraper. Results table exposes the magnet link
// directly in each row, so no second request is needed in the common case.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const BASE = 'https://share.dmhy.org';
const DOMAINS = [BASE];

const CATEGORY_MAP = {
  '2': 'Anime', '7': 'Anime', '31': 'Anime',
  '3': 'Books',
  '41': 'Series', '42': 'Series', '6': 'Series',
  '4': 'Music', '43': 'Music', '44': 'Music', '15': 'Music',
  '9': 'Games', '17': 'Games', '18': 'Games', '19': 'Games', '20': 'Games', '21': 'Games',
};

function categoryFromId(id) {
  return CATEGORY_MAP[id] || 'Other';
}

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
  const $nameLink = $row.find('td.title > a').first();
  const name = ownText($nameLink);
  if (!name) return null;

  const magnet = $row.find('td:nth-child(4) > a:nth-child(1)').first().attr('href') || null;
  if (!magnet) return null;

  const size = ownText($row.find('td:nth-child(5)'));
  const seeders = $row.find('td:nth-child(6)').first().text().trim();
  const leechers = $row.find('td:nth-child(7)').first().text().trim();
  const dateText = ownText($row.find('td:nth-child(1) > span'));
  const category = categoryFromId(
    ($row.find('td:nth-child(2) > a').first().attr('class') || '').replace('sort-', '').trim()
  );
  const href = $nameLink.attr('href');
  const detailUrl = href ? (href.startsWith('http') ? href : `${base}${href}`) : null;
  const infoHash = extractInfoHash(magnet);

  return {
    provider: 'dmhy',
    name,
    size: size || null,
    seeders: seeders || null,
    leechers: leechers || null,
    date: dateText || null,
    category,
    infoHash,
    magnet,
    detailUrl,
  };
}

async function searchOn(base, query, page) {
  const url = `${base}/topics/list?keyword=${encodeURIComponent(query)}&sort_id=0&team_id=0&order=date-desc&page=${page}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error: error || 'no_html' };

  const $ = cheerio.load(html);
  const rows = $('table#topic_list > tbody > tr');
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
    DOMAINS.map((base) => () => searchOn(base, query, page)),
    'dmhy'
  );
}

module.exports = { id: 'dmhy', name: 'DMHY', search };
