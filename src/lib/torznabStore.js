'use strict';

// Persistent store for user-added Torznab indexers (Jackett / Prowlarr / *arr).
// Each entry: { id, name, url, apiKey, enabled }
// The file lives under <project>/data/torznab.json and MUST stay git-ignored
// because it contains API keys.

const fs = require('fs');
const path = require('path');

const DATA_DIR = path.join(__dirname, '..', '..', 'data');
const STORE_FILE = path.join(DATA_DIR, 'torznab.json');

function ensureDir() {
  try {
    if (!fs.existsSync(DATA_DIR)) fs.mkdirSync(DATA_DIR, { recursive: true });
  } catch (e) {
    /* best-effort */
  }
}

function loadAll() {
  ensureDir();
  try {
    if (!fs.existsSync(STORE_FILE)) return [];
    const raw = fs.readFileSync(STORE_FILE, 'utf8');
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? arr : [];
  } catch (e) {
    return [];
  }
}

function saveAll(list) {
  ensureDir();
  fs.writeFileSync(STORE_FILE, JSON.stringify(list, null, 2), 'utf8');
}

function slug(s) {
  return (
    String(s || '')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'indexer'
  );
}

function add(cfg) {
  const list = loadAll();
  const id = cfg.id || `torznab:${slug(cfg.name)}-${Date.now().toString(36)}`;
  const entry = {
    id,
    name: cfg.name || 'Torznab',
    url: (cfg.url || '').trim(),
    apiKey: cfg.apiKey || '',
    enabled: cfg.enabled !== false,
  };
  list.push(entry);
  saveAll(list);
  return entry;
}

function remove(id) {
  const list = loadAll().filter((c) => c.id !== id);
  saveAll(list);
  return list;
}

function get(id) {
  return loadAll().find((c) => c.id === id) || null;
}

// Safed copy for the client: never expose the raw apiKey.
function listPublic() {
  return loadAll().map((c) => ({
    id: c.id,
    name: c.name,
    url: c.url,
    enabled: c.enabled,
    hasKey: !!c.apiKey,
    apiKeyMasked: c.apiKey ? '••••' + c.apiKey.slice(-4) : '',
  }));
}

module.exports = { loadAll, saveAll, add, remove, get, listPublic };
