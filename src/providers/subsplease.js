'use strict';

// SubsPlease — JSON API scraper. The search endpoint returns a JSON object
// keyed by show name; each value holds a "downloads" array, one per release.
// The full magnet URI (with btih + xl size) is present in each download
// object, so no detail-page fetch is needed for the common case.
const { getJSON, pickUA } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const BASE = 'https://subsplease.org';

// Extract 40-char btih hex from a magnet URI.
function infoHashFromMagnet(magnet) {
  const m = String(magnet).match(/xt=urn:btih:([a-f0-9]+)/i);
  return m ? m[1] : null;
}

// Extract the byte size from the magnet's xl= param (Kotlin formatBytes).
function sizeFromMagnet(magnet) {
  const q = String(magnet).split('?')[1] || '';
  const xl = q.split('&').find((p) => p.startsWith('xl='));
  if (!xl) return null;
  const bytes = parseInt(xl.slice(3), 10);
  return isNaN(bytes) ? null : bytes;
}

async function search(query, { page = 1 } = {}) {
  // Kotlin builds: /api?f=search&tz=$&s=<query>  (tz=$ is a literal sentinel).
  const url = `${BASE}/api?f=search&tz=$&s=${encodeURIComponent(query)}`;

  const { data, error } = await getJSON(url, {
    headers: { 'User-Agent': pickUA(), Accept: 'application/json' },
  });
  if (error || !data) return { results: [], error: `SubsPlease unreachable (${error || 'no_data'})` };

  const results = [];
  // Top level object: showName -> { release_date, page, episode, downloads[] }
  for (const [showName, animeObj] of Object.entries(data)) {
    if (!animeObj || typeof animeObj !== 'object') continue;

    const releaseDate = animeObj.release_date; // RFC1123 string
    const episode = animeObj.episode;
    const pagePath = animeObj.page;
    const detailsBase = pagePath ? `${BASE}/${pagePath}` : null;

    const downloads = Array.isArray(animeObj.downloads) ? animeObj.downloads : [];
    for (const d of downloads) {
      const magnet = d && d.magnet;
      if (!magnet) continue;
      const res = d.res;
      const name = `${showName} [${res}p]`;
      const infoHash = infoHashFromMagnet(magnet);
      const size = sizeFromMagnet(magnet);
      const detailUrl = detailsBase
        ? `${detailsBase}?ep=${encodeURIComponent(episode)}&res=${encodeURIComponent(res)}`
        : null;

      results.push(normalize({
        provider: 'subsplease',
        name,
        size,
        date: releaseDate,
        infoHash,
        magnet,
        detailUrl,
        category: 'Anime',
      }));
    }
  }

  return { results, error: null };
}

module.exports = { id: 'subsplease', name: 'SubsPlease', search };
