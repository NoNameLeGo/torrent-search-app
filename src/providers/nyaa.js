'use strict';

// NYAA (anime/asian torrents) HTML scraper. Results table exposes the magnet
// link directly in each row, so no second request is needed in the common case.
const cheerio = require('cheerio');
const { getText } = require('../lib/http');
const { normalize, extractInfoHash } = require('../lib/normalize');

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

    // 第一列是分类单元格：<a href="/?c=1_2" title="Anime - English-translated">。
    // title 给出站点原文分类（Anime / Audio / Literature / Live Action / Software / Pictures 等），
    // 交给前端 normalizeCategory 归一到标准桶。取不到就置 null（前端归为「其他」）。
    const category =
      tds.eq(0).find('a[href*="c="]').first().attr('title') ||
      tds.eq(0).find('a').first().attr('title') ||
      null;

    // ⚠️ 2026-10-23 修：磁力在 **Link 格**（第 3 个 td），不在名字格。
    // 原来在 `nameCell` 里找 ⇒ 恒 null ⇒ needsMagnet 恒 true，而本站又没有
    // resolveMagnet ⇒ 前端「获取磁力」会一路失败。两版（JS + Rust）同轮一起改。
    const $magnet = $row.find('a[href^="magnet:"]').first();
    const magnet = $magnet.attr('href') || null;
    const viewHref = $view.attr('href');
    const detailUrl = viewHref ? `${BASE}${viewHref}` : null;

    const infoHash = extractInfoHash(magnet);

    // ⚠️ 2026-10-23 修列错位：名字格带 `colspan="2"`，一行只有 **8 个 td**
    // （分类 / 名字 / Link / Size / Date / Seeders / Leechers / Completed），
    // 而原来按 9 列读 `eq(2..5)` ⇒ size 读到空的 Link 格、date 读到 "624.0 MiB"、
    // seeders 读到日期串（parseInt 后变成 2009）。正确索引是 3/4/5/6。
    results.push(normalize({
      provider: 'nyaa',
      name,
      size: tds.eq(3).text(),
      date: tds.eq(4).text(),
      seeders: tds.eq(5).text(),
      leechers: tds.eq(6).text(),
      magnet,
      infoHash,
      detailUrl,
      category,
      needsMagnet: !magnet,
    }));
  });

  return { results, error: null };
}

module.exports = { id: 'nyaa', name: 'NYAA', search };
