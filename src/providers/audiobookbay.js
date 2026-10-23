'use strict';

// AudioBookBay HTML scraper. The results page lists torrents with name, size,
// and date. The info hash lives on each torrent's detail page and is deferred
// to resolveMagnet() to avoid N+1 fetches during search().
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

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

// Lazily build a magnet link from the infoHash on a torrent's detail page.
async function resolveMagnet(detailUrl) {
  const infoHash = await getInfoHash(detailUrl);
  if (!infoHash) return { magnet: null, error: 'no_info_hash' };
  const magnet = `magnet:?xt=urn:btih:${infoHash}`;
  return { magnet, infoHash, error: null };
}

function parseListItem($, listItem, base) {
  const $li = $(listItem);
  const $link = $li.find('div.postTitle > h2 > a').first();
  const href = $link.attr('href');
  if (!href) return null;
  const detailUrl = href.startsWith('http') ? href : `${base}${href}`;

  const name = $link.text().trim();
  if (!name) return null;

  // ⚠️ 2026-10-23 修：页面用 `<br />` 分隔字段，而 cheerio 的 `.text()` **不给 `<br>` 补换行**
  // ⇒ 整段挤成一行：`Posted: 1 Sep 2026Format: MP3 / Bitrate: 128 KbpsFile Size: 19.24 GBs`。
  // 原来按 '\n' 切只剩一条 ⇒ `File Size:` 分支永不命中（size 恒 null），
  // 而 `Posted:` 分支命中后把整段垃圾塞进 date（`Date.parse` → NaN → null）。
  // 改成按**字段名**切：取标签之后、下一个已知标签之前的那一段。
  const infoText = $li.find('div.postContent > p:nth-child(3)').first().text();
  const LABELS = ['Posted:', 'Format:', 'Bitrate:', 'File Size:'];
  const fieldAfter = (text, label) => {
    const i = text.indexOf(label);
    if (i < 0) return null;
    const rest = text.slice(i + label.length);
    let end = rest.length;
    for (const other of LABELS) {
      if (other === label) continue;
      const j = rest.indexOf(other);
      if (j >= 0 && j < end) end = j;
    }
    const v = rest.slice(0, end).trim();
    return v || null;
  };

  let size = fieldAfter(infoText, 'File Size:');
  // 站上写 `19.24 GBs` —— 去掉结尾那个 s
  if (size) size = size.replace(/s$/, '');
  const date = fieldAfter(infoText, 'Posted:');

  // InfoHash deferred to resolveMagnet() — normalize() sets needsMagnet automatically.
  return normalize({
    provider: 'audiobookbay',
    name,
    size,
    date,
    magnet: null,
    category: 'Books',
    detailUrl,
  });
}

async function searchOne(base, query, page) {
  const url = `${base}/?s=${encodeURIComponent(query)}`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error };
  const $ = cheerio.load(html);
  const items = $('div.post').toArray();
  if (items.length === 0) return { results: [], error: 'no_results_parsed' };

  // parseListItem is synchronous — no per-item HTTP round-trips.
  const results = items.map((li) => parseListItem($, li, base)).filter(Boolean);
  return { results, error: results.length === 0 ? 'no_results_parsed' : null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query, page)),
    'audiobookbay'
  );
}

module.exports = { id: 'audiobookbay', name: 'AudioBookBay', search, resolveMagnet };
