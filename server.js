'use strict';

const path = require('path');
const express = require('express');
const axios = require('axios');
const providers = require('./src/providers');

const app = express();
const PORT = process.env.PORT || 3000;

app.use(express.json());
app.use(express.static(path.join(__dirname, 'public')));

// List available search engines.
app.get('/api/providers', (req, res) => {
  res.json({ providers: providers.list() });
});

// Aggregate search across enabled providers.
app.get('/api/search', async (req, res) => {
  const q = String(req.query.q || '').trim();
  const page = Math.max(1, parseInt(req.query.page, 10) || 1);
  const prov = req.query.providers || null;
  if (!q) return res.status(400).json({ error: 'missing query' });

  try {
    const out = await providers.search(q, { providers: prov, page });
    res.json({ query: q, page, ...out });
  } catch (e) {
    res.status(500).json({ error: e.message || 'search_failed' });
  }
});

// Lazily resolve a magnet link from a detail page (e.g. 1337x).
app.get('/api/magnet', async (req, res) => {
  const { provider, url } = req.query;
  if (!url) return res.status(400).json({ error: 'missing url' });
  const p = providers.getProvider(provider);
  if (p && typeof p.resolveMagnet === 'function') {
    const r = await p.resolveMagnet(url);
    return res.json(r);
  }
  // Fallback: try 1337x resolver regardless of provider.
  const x = providers.getProvider('1337x');
  if (x && typeof x.resolveMagnet === 'function') {
    const r = await x.resolveMagnet(url);
    return res.json(r);
  }
  res.status(404).json({ error: 'no resolver' });
});

// Login to a qBittorrent WebUI; returns the session cookie or throws.
async function qbLogin(base, user, pass) {
  const login = await axios.post(
    `${base}/api/v2/auth/login`,
    `username=${encodeURIComponent(user || '')}&password=${encodeURIComponent(pass || '')}`,
    { headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, timeout: 6000 }
  );
  const cookie = login.headers['set-cookie'] && login.headers['set-cookie'][0];
  if (!cookie || /failed/i.test(String(login.data))) throw new Error('login_failed');
  return cookie;
}

// Proxy: add a magnet to a qBittorrent WebUI instance (server-side, avoids CORS).
app.post('/api/download/qbittorrent', async (req, res) => {
  const { url, user, pass, magnet } = req.body || {};
  if (!url || !magnet) return res.status(400).json({ error: 'missing url or magnet' });
  try {
    const base = url.replace(/\/+$/, '');
    const cookie = await qbLogin(base, user, pass);
    const add = await axios.post(
      `${base}/api/v2/torrents/add`,
      `urls=${encodeURIComponent(magnet)}`,
      { headers: { 'Content-Type': 'application/x-www-form-urlencoded', Cookie: cookie }, timeout: 10000 }
    );
    res.json({ ok: true, status: add.status });
  } catch (e) {
    res.status(502).json({ error: e.code || e.message || 'qbittorrent_error' });
  }
});

// Auto-detect a local qBittorrent WebUI using common ports + default credentials,
// so the user doesn't have to configure anything for a stock install.
const QB_CANDIDATES = [
  { url: 'http://localhost:8080', user: 'admin', pass: 'adminadmin' },
  { url: 'http://localhost:8080', user: 'admin', pass: '' },
  { url: 'http://127.0.0.1:8080', user: 'admin', pass: 'adminadmin' },
  { url: 'http://localhost:8081', user: 'admin', pass: 'adminadmin' },
  { url: 'http://localhost:9090', user: 'admin', pass: 'adminadmin' },
  { url: 'http://localhost:8080', user: '', pass: '' },
];

app.get('/api/download/qbittorrent/detect', async (req, res) => {
  for (const c of QB_CANDIDATES) {
    try {
      const base = c.url.replace(/\/+$/, '');
      await qbLogin(base, c.user, c.pass);
      return res.json({ ok: true, url: c.url, user: c.user, pass: c.pass });
    } catch (e) { /* try next candidate */ }
  }
  res.json({ ok: false, error: 'no_qbittorrent_found' });
});

app.get('/api/health', (req, res) => res.json({ ok: true }));

app.listen(PORT, () => {
  console.log(`Torrent search app listening on http://localhost:${PORT}`);
});
