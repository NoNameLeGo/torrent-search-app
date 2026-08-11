'use strict';

// TorrentKitty HTML scraper. The results table exposes the magnet link
// directly in each row, so no second request is needed.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = [
  'https://torrentkitty.tv',
  'https://torrentkitty.to',
  'https://torrentkitty.is',
];

async function searchOne(base, query, page) {
  const url = `${base}/search/${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { base, results: [], error };

  const $ = cheerio.load(html);
  const rows = $('table#archiveResult > tbody > tr').slice(1);
  if (!rows.length) return { base, results: [], error: 'no_results' };

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);
    const name = $row.find('td.name').first().text().trim();
    if (!name) return;

    const magnet = $row.find('td.action > a:nth-child(2)').first().attr('href') || null;
    const infoHash = extractInfoHash(magnet);

    const size = $row.find('td.size').first().text().trim().toUpperCase();
    const date = $row.find('td.date').first().text().trim();
    const detailHref = $row.find('td.action > a:nth-child(1)').first().attr('href');
    const detailUrl = detailHref
      ? detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`
      : null;

    results.push(normalize({
      provider: 'torrentkitty',
      name,
      size,
      date,
      category: null,
      infoHash,
      magnet,
      detailUrl,
    }));
  });
  return { base, results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'TorrentKitty'
  );
}

module.exports = { id: 'torrentkitty', name: 'TorrentKitty', search };
