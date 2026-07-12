'use strict';

const path = require('path');
const express = require('express');
const axios = require('axios');
const providers = require('./src/providers');
const torznabStore = require('./src/lib/torznabStore');
const { normalizeApiUrl } = require('./src/providers/torznab');
const { getText } = require('./src/lib/http');

const app = express();

// Resolve listen port from --port <n> (used by the Tauri sidecar) or PORT env, default 3000.
function resolvePort() {
  const i = process.argv.indexOf('--port');
  if (i !== -1 && process.argv[i + 1]) {
    const p = parseInt(process.argv[i + 1], 10);
    if (Number.isFinite(p) && p > 0) return p;
  }
  return process.env.PORT || 3000;
}
const PORT = resolvePort();

// Static frontend dir: --public-dir <path> (Tauri ships `public/` as a resource),
// else fall back to ./public (normal `node server.js` dev run).
function resolvePublicDir() {
  const i = process.argv.indexOf('--public-dir');
  if (i !== -1 && process.argv[i + 1]) return process.argv[i + 1];
  return path.join(__dirname, 'public');
}
const PUBLIC_DIR = resolvePublicDir();

// 仅允许 http/https 协议的外部 URL。qBittorrent / Jackett / Prowlarr 本身
// 通常跑在 localhost 或内网，这里不做主机白名单，只挡掉 file:// / ftp:// 等
// 非预期协议，避免把后端当成任意协议的代理。返回规范化后的 URL 字符串或 null。
function safeHttpUrl(raw) {
  if (!raw) return null;
  let u;
  try {
    u = new URL(String(raw).trim());
  } catch (e) {
    return null;
  }
  if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
  return u.toString();
}

app.use(express.json());
app.use(express.static(PUBLIC_DIR));

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

// Streaming aggregate search over Server-Sent Events. Emits one `provider`
// event per engine the moment it returns (results + status), so the frontend
// can render incrementally instead of waiting for the slowest engine. A final
// `done` event carries the overall hasMore flag; then the stream closes.
app.get('/api/search/stream', async (req, res) => {
  const q = String(req.query.q || '').trim();
  const page = Math.max(1, parseInt(req.query.page, 10) || 1);
  const prov = req.query.providers || null;
  if (!q) return res.status(400).json({ error: 'missing query' });

  res.writeHead(200, {
    'Content-Type': 'text/event-stream',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
  });
  // Disable proxy buffering so events flush immediately.
  res.flushHeaders && res.flushHeaders();

  const send = (event, data) => {
    res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
  };

  // Stop work if the client disconnects mid-stream.
  let closed = false;
  req.on('close', () => { closed = true; });

  try {
    const summary = await providers.searchStream(
      q,
      { providers: prov, page },
      ({ id, name, results, status }) => {
        if (closed) return;
        send('provider', { id, name, results, status });
      }
    );
    if (!closed) send('done', { hasMore: summary.hasMore });
  } catch (e) {
    if (!closed) send('error', { error: e.message || 'search_failed' });
  } finally {
    if (!closed) res.end();
  }
});

// Lazily resolve a magnet link from a detail page.
// Prefer the named provider's own resolver. If it has none, fall back to a
// generic "grab the first magnet: link on the page" scrape (which happens to be
// what 1337x's resolver does). The fallback is best-effort: sites whose detail
// page doesn't expose a plain magnet anchor will return no_magnet_on_page rather
// than failing silently, and the response carries `fallback: true` so the caller
// can tell the magnet came from the generic path, not a provider-specific one.
app.get('/api/magnet', async (req, res) => {
  const { provider, url } = req.query;
  if (!url) return res.status(400).json({ error: 'missing url' });
  if (!safeHttpUrl(url)) return res.status(400).json({ error: 'invalid_url_scheme' });
  const p = providers.getProvider(provider);
  if (p && typeof p.resolveMagnet === 'function') {
    const r = await p.resolveMagnet(url);
    return res.json(r);
  }
  // Generic fallback: reuse 1337x's page scraper as a provider-agnostic resolver.
  const generic = providers.getProvider('1337x');
  if (generic && typeof generic.resolveMagnet === 'function') {
    const r = await generic.resolveMagnet(url);
    return res.json({ ...r, fallback: true });
  }
  res.status(404).json({ error: 'no resolver' });
});

// Validate that a user-supplied URL uses http/https only, rejecting other
// schemes (file:, ftp:, etc.). Returns the trimmed URL or null if invalid.
// The backend fetches these URLs server-side, so an unvalidated scheme would
// let the client point us at arbitrary local resources.
function safeHttpUrl(url) {
  const s = String(url || '').trim();
  if (!s) return null;
  try {
    const u = new URL(s);
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
    return s;
  } catch (e) {
    return null;
  }
}

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
  const safeUrl = safeHttpUrl(url);
  if (!safeUrl) return res.status(400).json({ error: 'invalid_url_scheme' });
  try {
    const base = safeUrl.replace(/\/+$/, '');
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

// ---- Torznab indexer management (Jackett / Prowlarr / *arr) ----
// List configured indexers (api keys are masked for the client).
app.get('/api/torznab', (req, res) => {
  res.json({ indexers: torznabStore.listPublic() });
});

// Add a new Torznab indexer.
app.post('/api/torznab', (req, res) => {
  const { name, url, apiKey, enabled } = req.body || {};
  if (!name || !url) return res.status(400).json({ error: 'name and url are required' });
  if (!safeHttpUrl(url)) return res.status(400).json({ error: 'invalid_url_scheme' });
  const entry = torznabStore.add({ name, url, apiKey, enabled });
  const pub = torznabStore.listPublic().find((c) => c.id === entry.id);
  res.json({ indexer: pub });
});

// Delete a Torznab indexer.
app.delete('/api/torznab/:id', (req, res) => {
  torznabStore.remove(req.params.id);
  res.json({ ok: true });
});

// Validate an indexer endpoint via a `t=caps` request (no query needed).
app.post('/api/torznab/test', async (req, res) => {
  const { url, apiKey } = req.body || {};
  if (!url) return res.status(400).json({ error: 'url required' });
  if (!safeHttpUrl(url)) return res.json({ ok: false, error: 'invalid_url_scheme' });
  const api = normalizeApiUrl(url);
  const params = new URLSearchParams({ apikey: apiKey || '', t: 'caps' });
  const { html, error } = await getText(`${api}?${params.toString()}`, { timeout: 8000 });
  if (error) return res.json({ ok: false, error });
  res.json({ ok: true, length: (html || '').length });
});

function start(port = PORT) {
  // 绑定回环地址：桌面单机应用无需对局域网暴露服务。这可避免同网段其他机器
  // 访问搜索接口，尤其是 /api/download/qbittorrent 与 /api/torznab/test
  // 这类会代为向外发起请求的端点被当作开放代理探测内网。
  return app.listen(port, '127.0.0.1', () => {
    console.log(`Torrent search app listening on http://localhost:${port}`);
  });
}

// 直接 `node server.js` 才自启；被 Electron 主进程 require 时不自动监听
if (require.main === module) {
  start();
}

module.exports = { app, start };
