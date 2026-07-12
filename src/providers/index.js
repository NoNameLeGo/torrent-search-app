'use strict';

// Original engines already present in this project.
const tpb = require('./tpb');
const x1337 = require('./1337x');
const nyaa = require('./nyaa');
const yts = require('./yts');
const knaben = require('./knaben');
const torrentscsv = require('./torrentscsv');
const demo = require('./demo');

// Torznab support (user-added Jackett / Prowlarr indexers).
const torznabStore = require('../lib/torznabStore');
const { makeProvider: makeTorznab } = require('./torznab');

// Build dynamic provider objects from stored, enabled Torznab indexers.
function dynamicTorznabProviders() {
  try {
    return torznabStore.loadAll()
      .filter((c) => c.enabled !== false && c.url)
      .map((c) => makeTorznab(c));
  } catch (e) {
    return [];
  }
}

// Batch 1 — anime / asian.
const anilibria = require('./anilibria');
const anirena = require('./anirena');
const animetosho = require('./animetosho');
const bangumimoe = require('./bangumimoe');
const dmhy = require('./dmhy');
const mikan = require('./mikan');

// Batch 2 — asian / indexers.
const subsplease = require('./subsplease');
const sukebei = require('./sukebei');
const tokyotoshokan = require('./tokyotoshokan');
const nekobt = require('./nekobt');
const bt4g = require('./bt4g');
const btdigg = require('./btdigg');

// Batch 3 — general / tv.
const eztv = require('./eztv');
const limetorrents = require('./limetorrents');
const therarbg = require('./therarbg');
const rutor = require('./rutor');
const torrent9 = require('./torrent9');
const torrentdownload = require('./torrentdownload');

// Batch 4 — general.
const torrentdownloads = require('./torrentdownloads');
const torrentdatabase = require('./torrentdatabase');
const torrentkitty = require('./torrentkitty');
const uindex = require('./uindex');
const zeromagnet = require('./zeromagnet');
const bitsearch = require('./bitsearch');
const oxtorrent = require('./oxtorrent');

// Batch 5 — specialized.
const audiobookbay = require('./audiobookbay');
const blueroms = require('./blueroms');
const filemood = require('./filemood');
const internetarchive = require('./internetarchive');
const linuxtracker = require('./linuxtracker');
const megapeer = require('./megapeer');

// Batch 6 — adult.
const mypornclub = require('./mypornclub');
const xxxclub = require('./xxxclub');
const xxxtracker = require('./xxxtracker');

// Order here is the default enable order shown in the UI.
const REGISTRY = [
  { ...tpb, enabled: true },
  { ...x1337, enabled: true },
  { ...nyaa, enabled: true },
  { ...yts, enabled: true },
  { ...knaben, enabled: true },
  { ...torrentscsv, enabled: true },
  { ...anilibria, enabled: true },
  { ...anirena, enabled: true },
  { ...animetosho, enabled: true },
  { ...bangumimoe, enabled: true },
  { ...dmhy, enabled: true },
  { ...mikan, enabled: true },
  { ...subsplease, enabled: true },
  { ...sukebei, enabled: true },
  { ...tokyotoshokan, enabled: true },
  { ...nekobt, enabled: true },
  { ...bt4g, enabled: true },
  { ...btdigg, enabled: true },
  { ...eztv, enabled: true },
  { ...limetorrents, enabled: true },
  { ...therarbg, enabled: true },
  { ...rutor, enabled: true },
  { ...torrent9, enabled: true },
  { ...torrentdownload, enabled: true },
  { ...torrentdownloads, enabled: true },
  { ...torrentdatabase, enabled: true },
  { ...torrentkitty, enabled: true },
  { ...uindex, enabled: true },
  { ...zeromagnet, enabled: true },
  { ...bitsearch, enabled: true },
  { ...oxtorrent, enabled: true },
  { ...audiobookbay, enabled: true },
  { ...blueroms, enabled: true },
  { ...filemood, enabled: true },
  { ...internetarchive, enabled: true },
  { ...linuxtracker, enabled: true },
  { ...megapeer, enabled: true },
  { ...mypornclub, enabled: true },
  { ...xxxclub, enabled: true },
  { ...xxxtracker, enabled: true },
  { ...demo, enabled: true },
];

function list() {
  const dyn = dynamicTorznabProviders().map((p) => ({
    id: p.id, name: p.name, enabled: true,
  }));
  return [
    ...REGISTRY.map((p) => ({ id: p.id, name: p.name, enabled: p.enabled, demo: !!p.demo })),
    ...dyn,
  ];
}

function getProvider(id) {
  if (id && id.startsWith('torznab:')) {
    const cfg = torznabStore.get(id);
    return cfg ? makeTorznab(cfg) : null;
  }
  return REGISTRY.find((p) => p.id === id);
}

// Resolve the enabled provider objects matching an optional comma-separated
// id filter (static registry + dynamic Torznab indexers).
function resolveTargets(providers) {
  const wanted = providers
    ? providers.split(',').map((s) => s.trim()).filter(Boolean)
    : null;
  return REGISTRY.concat(dynamicTorznabProviders()).filter((p) =>
    p.enabled && (!wanted || wanted.includes(p.id)));
}

// Run a single provider in isolation, timing it and never throwing.
// Returns { results, status } where status is the per-provider summary the
// client renders in the status bar.
async function runProvider(p, query, page) {
  const startedAt = Date.now();
  try {
    const out = await p.search(query, { page });
    return {
      results: out.results,
      status: {
        status: out.error ? 'error' : 'ok',
        count: out.results.length,
        error: out.error || null,
        hasMore: out.hasMore,
        ms: Date.now() - startedAt,
      },
    };
  } catch (e) {
    return {
      results: [],
      status: { status: 'error', count: 0, error: e.message || 'crash', ms: Date.now() - startedAt },
    };
  }
}

// Aggregate the per-provider hasMore flags into a single boolean.
// Demo paginates locally; a real provider signalling hasMore also counts.
function aggregateHasMore(targets, perProvider, totalResults) {
  if (totalResults === 0) return false;
  const demoProv = targets.find((p) => p.demo);
  if (demoProv && perProvider[demoProv.id] && perProvider[demoProv.id].hasMore) return true;
  return Object.values(perProvider).some((s) => s.status === 'ok' && !demoProv);
}

// Run search across the requested (enabled) providers in parallel.
// Each provider is isolated: a failure in one never breaks the others.
async function search(query, { providers = null, page = 1 } = {}) {
  const targets = resolveTargets(providers);

  const perProvider = {};
  const settled = await Promise.all(targets.map(async (p) => {
    const { results, status } = await runProvider(p, query, page);
    perProvider[p.id] = status;
    return results;
  }));

  const results = settled.flat();
  const hasMore = aggregateHasMore(targets, perProvider, results.length);
  return { results, providers: perProvider, hasMore };
}

// Streaming variant: invoke onProvider({ id, name, results, status }) as soon
// as each provider settles, so the caller (SSE endpoint) can push incremental
// updates instead of waiting for the slowest provider. Resolves once every
// provider has reported, returning the same aggregate as search().
async function searchStream(query, { providers = null, page = 1 } = {}, onProvider) {
  const targets = resolveTargets(providers);

  const perProvider = {};
  let total = 0;
  await Promise.all(targets.map(async (p) => {
    const { results, status } = await runProvider(p, query, page);
    perProvider[p.id] = status;
    total += results.length;
    if (typeof onProvider === 'function') {
      onProvider({ id: p.id, name: p.name, results, status });
    }
  }));

  const hasMore = aggregateHasMore(targets, perProvider, total);
  return { providers: perProvider, hasMore, count: total };
}

module.exports = { list, getProvider, search, searchStream };
