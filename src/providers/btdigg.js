'use strict';

// BTDigg HTML scraper. The results list exposes the magnet link directly,
// so no detail-page fetch is needed for search.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

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

    const infoHash = extractInfoHash(magnet);

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

async function searchOne(base, query, page) {
  const url = `${base}/search?q=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };
  const $ = cheerio.load(html);
  const results = parseRows($, base);
  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'BTDigg'
  );
}

module.exports = { id: 'btdigg', name: 'BTDigg', search };
