'use strict';

const tpb = require('./tpb');
const x1337 = require('./1337x');
const nyaa = require('./nyaa');
const yts = require('./yts');
const knaben = require('./knaben');
const torrentscsv = require('./torrentscsv');
const demo = require('./demo');

// Order here is the default enable order shown in the UI.
const REGISTRY = [
  { ...tpb, enabled: true },
  { ...x1337, enabled: true },
  { ...nyaa, enabled: true },
  { ...yts, enabled: true },
  { ...knaben, enabled: true },
  { ...torrentscsv, enabled: true },
  { ...demo, enabled: true },
];

function list() {
  return REGISTRY.map((p) => ({
    id: p.id, name: p.name, enabled: p.enabled, demo: !!p.demo,
  }));
}

function getProvider(id) {
  return REGISTRY.find((p) => p.id === id);
}

// Run search across the requested (enabled) providers in parallel.
// Each provider is isolated: a failure in one never breaks the others.
async function search(query, { providers = null, page = 1 } = {}) {
  const wanted = providers
    ? providers.split(',').map((s) => s.trim()).filter(Boolean)
    : null;

  const targets = REGISTRY.filter((p) =>
    p.enabled && (!wanted || wanted.includes(p.id)));

  const perProvider = {};
  const settled = await Promise.all(targets.map(async (p) => {
    try {
      const out = await p.search(query, { page });
      perProvider[p.id] = {
        status: out.error ? 'error' : 'ok',
        count: out.results.length,
        error: out.error || null,
        hasMore: out.hasMore,
      };
      return out.results;
    } catch (e) {
      perProvider[p.id] = { status: 'error', count: 0, error: e.message || 'crash' };
      return [];
    }
  }));

  const results = settled.flat();
  const demoProv = targets.find((p) => p.demo);
  const nonDemoOk = Object.values(perProvider).some((s) => s.status === 'ok' && !demoProv);
  let hasMore = false;
  if (demoProv && perProvider[demoProv.id] && perProvider[demoProv.id].hasMore) hasMore = true;
  if (nonDemoOk) hasMore = true;
  if (results.length === 0) hasMore = false;

  return { results, providers: perProvider, hasMore };
}

module.exports = { list, getProvider, search };
