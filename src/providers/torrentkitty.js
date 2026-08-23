'use strict';

// TorrentKitty HTML scraper. The results table exposes the magnet link
// directly in each row, so no second request is needed.
const { extractInfoHash } = require('../lib/normalize');
const { createProvider, scrapeRows } = require('../lib/scraper');

const DOMAINS = [
  'https://torrentkitty.tv',
  'https://torrentkitty.to',
  'https://torrentkitty.is',
];

module.exports = createProvider({
  id: 'torrentkitty', name: 'TorrentKitty',
  mirrors: DOMAINS,
  searchOn: (base, query, page) => scrapeRows(base, query, page, {
    id: 'torrentkitty',
    url: ({ base, query }) => `${base}/search/${encodeURIComponent(query)}`,
    rowSelector: 'table#archiveResult > tbody > tr',
    skipRows: 1,
    extract($, $row, base) {
      const name = $row.find('td.name').first().text().trim();
      if (!name) return null;
      const magnet = $row.find('td.action > a:nth-child(2)').first().attr('href') || null;
      const detailHref = $row.find('td.action > a:nth-child(1)').first().attr('href');
      return {
        name,
        size: $row.find('td.size').first().text().trim().toUpperCase(),
        date: $row.find('td.date').first().text().trim(),
        category: null,
        infoHash: extractInfoHash(magnet),
        magnet,
        detailUrl: detailHref ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`) : null,
      };
    },
  }),
});