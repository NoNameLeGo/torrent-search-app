'use strict';

// EZTV HTML scraper. The results table exposes the magnet link directly in
// each row, so no second request is needed. A `layout=def_wlinks` cookie is
// required, otherwise the rows come back without magnet links (per Kotlin src).
const { getText } = require('../lib/http');
const { extractInfoHash } = require('../lib/normalize');
const { createProvider, scrapeRows } = require('../lib/scraper');

const DOMAINS = [
  'https://eztvx.to',
  'https://eztv.re',
  'https://eztv.tf',
  'https://eztv.wtf',
];

module.exports = createProvider({
  id: 'eztv', name: 'EZTV',
  mirrors: DOMAINS,
  searchOn: (base, query, page) => scrapeRows(base, query, page, {
    id: 'eztv',
    url: ({ base, query }) => `${base}/search/${encodeURIComponent(query)}`,
    headers: { Cookie: 'layout=def_wlinks' },
    rowSelector: 'table:last-of-type > tbody > tr',
    skipRows: 2,
    extract($, $row, base) {
      const $name = $row.find('td:nth-child(2) > a.epinfo').first();
      const name = $name.text().trim();
      const magnet = $row.find('td:nth-child(3) > a.magnet').first().attr('href');
      if (!name || !magnet) return null;
      const detailHref = $name.attr('href');
      return {
        name,
        size: $row.find('td:nth-child(4)').text(),
        seeders: $row.find('td:nth-child(6)').text(),
        leechers: 0,
        infoHash: extractInfoHash(magnet),
        magnet,
        detailUrl: detailHref ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`) : null,
        category: 'Series',
      };
    },
  }),
});