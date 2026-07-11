'use strict';

// AniRena — HTML scraper. The search result rows expose the magnet link as a
// redirect; we defer the redirect-follow to resolveMagnet() (lazily on click)
// so the search stays fast and resilient. We also mirror the Kotlin's detail
// page parser for resolveMagnet.
const cheerio = require('cheerio');
const { getText, pickUA, http } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://anirena.com';
const DOMAINS = [BASE];

const CATEGORY_MAP = {
  Anime: 'Anime',
  Manga: 'Books',
  Audio: 'Music',
  Literature: 'Books',
  'Live Action': 'Series',
  Software: 'Apps',
  Hentai: 'Porn',
  Other: 'Other',
};

function categoryFromRaw(raw) {
  if (!raw) return null;
  const key = raw.split('/')[0].trim();
  return CATEGORY_MAP[key] || 'Other';
}

// The magnet link on AniRena issues an HTTP redirect; follow it (without
// letting axios auto-redirect) to read the `Location` header holding the magnet.
async function followToMagnet(sourceUrl) {
  try {
    const res = await http.get(sourceUrl, {
      maxRedirects: 0,
      validateStatus: () => true,
      headers: { 'User-Agent': pickUA() },
    });
    const loc = res.headers && (res.headers.location || res.headers.Location);
    if (typeof loc === 'string' && loc.startsWith('magnet:')) return loc;
    return null;
  } catch (e) {
    return null;
  }
}

async function searchOn(base, query) {
  const url = `${base}/?q=${encodeURIComponent(query)}&page=1`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error: error || 'no_html' };

  const $ = cheerio.load(html);
  const rows = $('table.tl-table > tbody > tr');
  if (rows.length === 0) return { results: [], error: null };

  const results = [];
  rows.each((_, row) => {
    const $row = $(row);
    const $nameLink = $row.find('td.col-name > div.tl-name-wrap > a.tl-torrent-name').first();
    const name = $nameLink.text().trim();
    if (!name) return;

    const sourceHref = $row
      .find('td.col-actions > div.tl-actions > a:nth-child(1)')
      .first()
      .attr('href');
    if (!sourceHref) return;

    const size = $row.find('td.col-size').first().text().trim();
    const seeders = $row.find('td.col-se > span.tl-se').first().text().trim();
    const leechers = $row.find('td.col-le > span.tl-le').first().text().trim();
    const ts = $row.attr('data-created-ts');
    const date = ts && /^\d+$/.test(ts) ? parseInt(ts, 10) : null;
    const category = categoryFromRaw($row.find('td.col-cat').first().attr('title'));
    const href = $nameLink.attr('href');
    const detailUrl = href ? (href.startsWith('http') ? href : `${base}${href}`) : null;
    const fileDownloadLink = (() => {
      const h = $row.find('td.col-actions > div.tl-actions > a:nth-child(2)').first().attr('href');
      return h ? (h.startsWith('http') ? h : `${base}${h}`) : null;
    })();

    results.push(
      normalize({
        provider: 'anirena',
        name,
        size: size || null,
        seeders: seeders || null,
        leechers: leechers || null,
        date,
        category,
        detailUrl,
        fileDownloadLink,
        needsMagnet: true,
      })
    );
  });

  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  const attempts = DOMAINS.map((base) => searchOn(base, query));
  const settled = await Promise.allSettled(attempts);
  for (const s of settled) {
    const v = s.status === 'fulfilled' ? s.value : null;
    if (v && v.results && v.results.length) return { results: v.results, error: null };
  }
  const ok = settled.find(
    (s) => s.status === 'fulfilled' && s.value && !s.value.error && s.value.results.length === 0
  );
  if (ok) return { results: [], error: null };
  const errs = settled
    .map((s) => (s.status === 'fulfilled' ? s.value.error : 'crash'))
    .filter(Boolean);
  return { results: [], error: `anirena unavailable (${errs.join('; ')})` };
}

// Lazily resolve the magnet from the torrent's detail page (the magnet anchor
// there also redirects, so we follow it the same way).
async function resolveMagnet(detailUrl) {
  const { html, error } = await getText(detailUrl, { headers: { 'User-Agent': pickUA() } });
  if (error || !html) return { magnet: null, error: error || 'no_html' };
  const $ = cheerio.load(html);
  const href = $('div.td-actions > a:nth-child(1)').first().attr('href');
  if (!href) return { magnet: null, error: 'no_magnet_link' };
  const abs = href.startsWith('http') ? href : `${BASE}${href}`;
  const magnet = await followToMagnet(abs);
  return magnet ? { magnet, error: null } : { magnet: null, error: 'no_magnet' };
}

module.exports = { id: 'anirena', name: 'AniRena', search, resolveMagnet };
