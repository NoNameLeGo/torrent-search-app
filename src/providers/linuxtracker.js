'use strict';

// LinuxTracker HTML scraper.
// The site uses a flat <table> where each torrent occupies one <tr> with
// <td class="lista"> cells.  The identifying element is the <a> pointing to
// torrent-details inside a td.lista.
//
// Column layout (indexed by td position within the <tr>):
//   0: category icon    1: name (colspan=2)     2: download (.torrent)
//   3: date (DD/MM/YYYY) 4: size                 5: uploader
//   6: seeders           7: leechers             8: recommended by
//
// The detail URL carries the infoHash as its id query param (40 hex chars),
// so magnets can be built without a secondary request.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize } = require('../lib/normalize');
const { runMirrors } = require('../lib/mirrors');

const DOMAINS = ['https://linuxtracker.org'];

/** Parse a DD/MM/YYYY European date string into a Unix-ms timestamp. */
function parseEuDate(s) {
  if (!s) return s;
  const m = s.trim().match(/^(\d{1,2})\/(\d{1,2})\/(\d{4})$/);
  if (!m) return s; // fall through → let normalize try Date.parse
  return new Date(+m[3], +m[2] - 1, +m[1]).getTime();
}

async function searchOne(base, query) {
  const url =
    `${base}/index.php?page=torrents&search=${encodeURIComponent(query)}` +
    `&category=0&active=0`;
  const { html, error } = await getText(url);
  if (error || !html) return { results: [], error };

  const $ = cheerio.load(html);

  // Identify torrent rows by the detail link inside a td.lista.
  const nameLinks = $('td.lista a[href*="torrent-details"]');
  if (!nameLinks.length) return { results: [], error: 'no_results_parsed' };

  const results = [];
  nameLinks.each((_, a) => {
    const $a = $(a);
    const name = $a.text().trim();
    if (!name) return;

    const detailHref = $a.attr('href');
    const detailUrl = detailHref
      ? (detailHref.startsWith('http') ? detailHref : `${base}${detailHref}`)
      : null;

    // The detail URL's id query param is the SHA1 infoHash (40 hex chars).
    let infoHash = null;
    const idMatch = detailHref
      ? detailHref.match(/[?&]id=([a-f0-9]{40})/i)
      : null;
    if (idMatch) infoHash = idMatch[1].toLowerCase();

    // Walk up to the parent <tr> and pick columns by index.
    // Skip sidebar rows (Top 10 etc.) which only have 2 tds;
    // the main torrents table rows have 9 tds.
    const $tr = $a.closest('tr');
    const tds = $tr.find('td').toArray();
    if (tds.length < 6) return; // not a main-table row

    // ⚠️ 2026-10-23 修：「展开描述行」有 **19 个 td**，列含义与主表不同，而且值带文字前缀
    // （cheerio 真值见 html-probes 的 expanded.all_rows）：
    //   [2] "Added On: 09/08/2026"   [4] "Size: 16.13 MB"
    //   [6] "Seeds 7"                [8] "Leechers 0"
    // 主表行（9 个 td）是：[3] 日期 [4] 大小 [6] 种子 [7] 下载。
    // 原来一律按 9 格读 ⇒ 这类行的 size/date/peers 全是空。
    const expanded = tds.length >= 15;
    const cellText = (i) => (tds[i] ? $(tds[i]).text().trim() : '');
    const afterLabel = (s, label) => (s.startsWith(label) ? s.slice(label.length).trim() : s);

    const dateText     = expanded ? afterLabel(cellText(2), 'Added On:') : cellText(3);
    const sizeText     = expanded ? afterLabel(cellText(4), 'Size:')     : cellText(4);
    const seedersText  = expanded ? afterLabel(cellText(6), 'Seeds')     : cellText(6);
    const leechersText = expanded ? afterLabel(cellText(8), 'Leechers')  : cellText(7);

    results.push(normalize({
      provider: 'linuxtracker',
      name,
      size: sizeText,
      seeders: seedersText,
      leechers: leechersText,
      date: parseEuDate(dateText),
      infoHash,
      category: 'Apps',
      detailUrl,
    }));
  });
  return { results, error: null };
}

async function search(query, { page = 1 } = {}) {
  return runMirrors(
    DOMAINS.map((base) => () => searchOne(base, query)),
    'linuxtracker'
  );
}

module.exports = { id: 'linuxtracker', name: 'LinuxTracker', search };
