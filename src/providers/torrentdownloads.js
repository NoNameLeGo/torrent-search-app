'use strict';

// TorrentDownloads HTML scraper. The results list exposes name/size/seeders/
// peers/category directly, but the magnet + infoHash live on each torrent's
// detail page, so we expose a resolveMagnet() used lazily on click.
const cheerio = require('cheerio');
const { getText, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = [
  'https://torrentdownloads.pro',
  'https://torrentdownloads.me',
  'https://torrentdownloads.org',
];

function categoryFromIcon(src) {
  if (!src) return null;
  const file = src.replace(/^.*\/templates\/new\/images\/icons\//, '');
  const map = {
    'menu_icon0.png': 'All',
    'menu_icon1.png': 'Anime',
    'menu_icon2.png': 'Books',
    'menu_icon3.png': 'Games',
    'menu_icon4.png': 'Movies',
    'menu_icon5.png': 'Music',
    'menu_icon7.png': 'Apps',
    'menu_icon8.png': 'Series',
    'menu_icon9.png': 'Other',
  };
  return map[file] || 'Other';
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/search/?s_cat=0&search=${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { base, results: [], error };

      const $ = cheerio.load(html);
      const container = $('div.inner_container').last();
      if (!container.length) return { base, results: [], error: 'no_container' };

      const items = container.find('div.grey_bar3').slice(2);
      if (!items.length) return { base, results: [], error: 'no_results' };

      const results = [];
      items.each((_, el) => {
        const $item = $(el);
        const $link = $item.find('p:nth-child(1) > a:nth-child(2)').first();
        const name = $link.text().trim();
        const href = $link.attr('href');
        if (!name || !href) return;

        const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
        const size = $item.find('span:nth-child(5)').first().text().trim();
        const seeders = $item.find('span:nth-child(4)').first().text().trim();
        const leechers = $item.find('span:nth-child(3)').first().text().trim();
        const catImg = $item.find('p:nth-child(1) > img:nth-child(1)').first().attr('src');

        results.push(normalize({
          provider: 'torrentdownloads',
          name,
          size,
          seeders,
          leechers,
          category: categoryFromIcon(catImg),
          detailUrl,
          needsMagnet: true,
        }));
      });
      return { base, results, error: null };
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
  return { results: [], error: `TorrentDownloads unreachable (${errs.join('; ')})` };
}

// Lazily fetch the magnet + infoHash from a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };
  const $ = cheerio.load(html);
  const href = $('a[href^="magnet:?"]').first().attr('href');
  if (!href) return { magnet: null, error: 'no_magnet_on_page' };
  return { magnet: href, error: null };
}

module.exports = { id: 'torrentdownloads', name: 'TorrentDownloads', search, resolveMagnet };
