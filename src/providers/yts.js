'use strict';

// YTS (Yify) movie torrents via the public JSON API.
// Mirrors upstream Yts.kt: https://movies-api.accel.li/api/v2/list_movies.json
const { getJSON } = require('../lib/http');
const { normalize } = require('../lib/normalize');

const API = 'https://movies-api.accel.li/api/v2';

async function search(query, { page = 1 } = {}) {
  const url = `${API}/list_movies.json?query_term=${encodeURIComponent(query)}&limit=50`;
  const { data, error } = await getJSON(url);
  if (error) return { results: [], error: `YTS unreachable (${error})` };

  const movies = data && data.data && Array.isArray(data.data.movies) ? data.data.movies : [];
  if (movies.length === 0) return { results: [] };

  const results = [];
  for (const movie of movies) {
    const title = movie.title_long || movie.title;
    if (!title) continue;
    const movieId = movie.id;
    const detailBase = movie.url;
    const torrents = Array.isArray(movie.torrents) ? movie.torrents : [];

    for (const t of torrents) {
      const infoHash = (t.hash || '').toLowerCase();
      if (!infoHash) continue;

      const quality = t.quality || '-';
      const type = t.type || '-';
      const codec = t.video_codec || '-';
      const name = `${title} [${quality}] [${type}] [${codec}]`;
      const detailUrl = detailBase
        ? `${detailBase}?movieid=${movieId}&infohash=${infoHash}`
        : null;

      results.push(normalize({
        provider: 'yts',
        name,
        infoHash,
        size: t.size_bytes,
        seeders: t.seeds,
        leechers: t.peers,
        date: t.date_uploaded_unix ? Number(t.date_uploaded_unix) : null,
        category: 'Movies',
        detailUrl,
      }));
    }
  }

  return { results, error: null };
}

module.exports = { id: 'yts', name: 'YTS', search, testable: true };
