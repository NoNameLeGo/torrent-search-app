'use strict';

// AudioBookBay HTML scraper. The results page lists torrents but the info
// hash lives on each torrent's detail page, so we fetch the detail page per
// item (mirroring the Kotlin implementation) to obtain the btih hash.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const DOMAINS = ['https://audiobookbay.lu'];

// Extract the 40-char info hash from a torrent's detail page.
async function getInfoHash(detailUrl) {
  const { html, error } = await getText(detailUrl);
  if (error || !html) return null;
  const $ = cheerio.load(html);
  let hash = null;
  $('td').each((_, el) => {
    const own = $(el).text().trim();
    if (own === 'Info Hash:') {
      hash = $(el).next().text().trim();
      return false;
    }
  });
  return hash && /^[a-f0-9]{40}$/i.test(hash) ? hash : null;
}

async function parseListItem($, listItem, base) {
  const $li = $(listItem);
  const $link = $li.find('div.postTitle > h2 > a').first();
  const href = $link.attr('href');
  if (!href) return null;
  const detailUrl = href.startsWith('http') ? href : `${base}${href}`;
  const infoHash = await getInfoHash(detailUrl);
  if (!infoHash) return null;

  const name = $link.text().trim();
  if (!name) return null;

  const infoText = $li.find('div.postContent > p:nth-child(3)').first().text();
  const lines = infoText.split('\n').map((l) => l.trim()).filter(Boolean);

  let size = null;
  let date = null;
  for (const line of lines) {
    if (line.startsWith('File Size:')) {
      size = line.substring('File Size:'.length).trim().replace(/s$/, '');
    } else if (line.startsWith('Posted:')) {
      date = line.substring('Posted:'.length).trim();
    }
  }

  return normalize({
    provider: 'audiobookbay',
    name,
    size,
    date,
    infoHash,
    category: 'Books',
    detailUrl,
  });
}

async function search(query, { page = 1 } = {}) {
  // Try all mirror domains in parallel; take the first that returns results.
  const attempts = DOMAINS.map((base) =>
    (async () => {
      const url = `${base}/?s=${encodeURIComponent(query)}`;
      const { html, error } = await getText(url);
      if (error || !html) return { results: [], error };
      const $ = cheerio.load(html);
      const items = $('div.post').toArray();
      if (items.length === 0) return { results: [], error: 'no_results_parsed' };

      const parsed = await Promise.all(
        items.map((li) => parseListItem($, li, base).catch(() => null))
      );
      const results = parsed.filter(Boolean);
      return { results, error: null };
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
  return { results: [], error: `audiobookbay unreachable (${errs.join('; ')})` };
}

module.exports = { id: 'audiobookbay', name: 'AudioBookBay', search };
