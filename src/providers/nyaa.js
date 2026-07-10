'use strict';

// NYAA (anime/asian torrents) HTML scraper. Results table exposes the magnet
// link directly in each row, so no second request is needed in the common case.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://nyaa.si';

async function search(query, { page = 1 } = {}) {
  const url = `${BASE}/?f=0&c=0_0&q=${encodeURIComponent(query)}&page=${page}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error: `NYAA unreachable (${error})` };

  const $ = cheerio.load(html);
  const rows = $('table.torrent-list tbody tr');
  if (rows.length === 0) return { results: [] };

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);
    const tds = $row.find('td');
    if (tds.length < 6) return;

    const nameCell = tds.eq(1);
    const $view = nameCell.find('a[href^="/view/"]').first();
    const name = ($view.text() || nameCell.text()).trim();
    if (!name) return;

    const $magnet = nameCell.find('a[href^="magnet:"]').first();
    const magnet = $magnet.attr('href') || null;
    const viewHref = $view.attr('href');
    const detailUrl = viewHref ? `${BASE}${viewHref}` : null;

    let infoHash = null;
    if (magnet) {
      const m = magnet.match(/btih:([a-f0-9]+)/i);
      if (m) infoHash = m[1];
    }

    results.push(normalize({
      provider: 'nyaa',
      name,
      size: tds.eq(2).text(),
      date: tds.eq(3).text(),
      seeders: tds.eq(4).text(),
      leechers: tds.eq(5).text(),
      magnet,
      infoHash,
      detailUrl,
      needsMagnet: !magnet,
    }));
  });

  return { results, error: null };
}

module.exports = { id: 'nyaa', name: 'NYAA', search };
