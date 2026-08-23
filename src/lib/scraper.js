'use strict';

const cheerio = require('cheerio');
const { getText } = require('./http');
const { normalize, coercePage } = require('./normalize');
const { runMirrors } = require('./mirrors');

/**
 * Factory that eliminates the `search()` + `module.exports` boilerplate
 * repeated in every provider. Returns a provider object ready for export.
 *
 * @param {object} opts
 * @param {string} opts.id
 * @param {string} opts.name
 * @param {string[]} opts.mirrors
 * @param {function} opts.searchOn - (base, query, page) => { results, error }
 * @param {function} [opts.resolveMagnet]
 * @param {number} [opts.timeout] - per-request timeout in ms (default: 10000)
 */
function createProvider({ id, name, mirrors, searchOn, resolveMagnet, timeout }) {
  async function search(query, { page = 1 } = {}) {
    const p = coercePage(page);
    return runMirrors(
      mirrors.map((base) => () => searchOn(base, query, p)),
      name
    );
  }
  const out = { id, name, search };
  if (resolveMagnet) out.resolveMagnet = resolveMagnet;
  return out;
}

/**
 * Simple HTML scraper for the common row-iteration pattern:
 * fetch → cheerio → find rows → extract fields → normalize.
 *
 * `opts.extract` is called per row with ($, $row, base) and should return
 * an object with fields for normalize() (name, size, seeders, leechers, etc.)
 * or null to skip the row.
 */
async function scrapeRows(base, query, page, opts) {
  const { id, url, headers, rowSelector, skipRows, extract, timeout } = opts;
  const fullUrl = typeof url === 'function' ? url({ base, query, page }) : url;
  const { html, error } = await getText(fullUrl, { headers, ...(timeout ? { timeout } : {}) });
  if (error || !html) return { base, results: [], error };
  const $ = cheerio.load(html);
  const rows = $(rowSelector);
  if (!rows.length) return { base, results: [], error: 'no_results_parsed' };
  const results = [];
  rows.each((i, row) => {
    if (skipRows && i < skipRows) return;
    const extracted = extract($, $(row), base);
    if (extracted) results.push(normalize({ provider: id, ...extracted }));
  });
  return { base, results, error: null };
}

module.exports = { createProvider, scrapeRows };