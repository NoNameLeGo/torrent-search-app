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
// `paginated: true` marks providers whose `search` actually consumes the `page`
// argument (page reaches the request URL/params). The other providers ignore
// `page` and return the same single-shot result set regardless — so from page 2
// onward we only re-query the paginated ones (see search()/searchStream()),
// avoiding wasted requests that just get deduped away client-side.
const REGISTRY = [
  { ...tpb, enabled: true },
  { ...x1337, enabled: true, paginated: true },
  { ...nyaa, enabled: true, paginated: true },
  { ...yts, enabled: true },
  { ...knaben, enabled: true },
  { ...torrentscsv, enabled: true },
  { ...anilibria, enabled: true },
  { ...anirena, enabled: true },
  { ...animetosho, enabled: true },
  { ...bangumimoe, enabled: true },
  { ...dmhy, enabled: true, paginated: true },
  { ...mikan, enabled: true },
  { ...subsplease, enabled: true },
  { ...sukebei, enabled: true },
  { ...tokyotoshokan, enabled: true },
  { ...nekobt, enabled: true },
  { ...bt4g, enabled: true, paginated: true },
  { ...btdigg, enabled: true },
  { ...eztv, enabled: true },
  { ...limetorrents, enabled: true, paginated: true },
  { ...therarbg, enabled: true },
  { ...rutor, enabled: true, paginated: true },
  { ...torrent9, enabled: true },
  { ...torrentdownload, enabled: true },
  { ...torrentdownloads, enabled: true },
  { ...torrentdatabase, enabled: true },
  { ...torrentkitty, enabled: true },
  { ...uindex, enabled: true },
  { ...zeromagnet, enabled: true },
  { ...bitsearch, enabled: true, paginated: true },
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
    ...REGISTRY.map((p) => ({ id: p.id, name: p.name, enabled: p.enabled, demo: !!p.demo, paginated: !!p.paginated })),
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

// Resolve the enabled provider objects for a request. From page 2 onward we
// drop the single-shot providers: they ignore `page` and would just return the
// same results to be deduped away — only paginated providers (and demo, which
// paginates its local pool) can yield genuinely new results deeper in.
function resolveTargets(providers, page) {
  const wanted = providers
    ? providers.split(',').map((s) => s.trim()).filter(Boolean)
    : null;

  return REGISTRY.concat(dynamicTorznabProviders()).filter((p) => {
    if (!p.enabled || (wanted && !wanted.includes(p.id))) return false;
    if (page > 1 && !p.paginated && !p.demo) return false;
    return true;
  });
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

// Honest aggregate hasMore: a paginated provider that returned results this page
// may have a next page; demo reports its own hasMore; single-shot providers are
// exhausted after page 1 and never claim more. Empty result set ⇒ no next page.
function aggregateHasMore(targets, perProvider) {
  return targets.some((p) => {
    const s = perProvider[p.id];
    if (!s || s.status !== 'ok' || s.count === 0) return false;
    if (p.demo) return !!s.hasMore;
    return !!p.paginated;
  });
}

// Run search across the requested (enabled) providers in parallel.
// Each provider is isolated: a failure in one never breaks the others.
async function search(query, { providers = null, page = 1 } = {}) {
  const targets = resolveTargets(providers, page);

  const perProvider = {};
  const settled = await Promise.all(targets.map(async (p) => {
    const { results, status } = await runProvider(p, query, page);
    perProvider[p.id] = status;
    return results;
  }));

  const results = settled.flat();
  const hasMore = aggregateHasMore(targets, perProvider);
  return { results, providers: perProvider, hasMore };
}

// Streaming variant: invoke onProvider({ id, name, results, status }) as soon
// as each provider settles, so the caller (SSE endpoint) can push incremental
// updates instead of waiting for the slowest provider. Resolves once every
// provider has reported, returning the same aggregate as search().
async function searchStream(query, { providers = null, page = 1 } = {}, onProvider) {
  const targets = resolveTargets(providers, page);

  const perProvider = {};
  await Promise.all(targets.map(async (p) => {
    const { results, status } = await runProvider(p, query, page);
    perProvider[p.id] = status;
    if (typeof onProvider === 'function') {
      onProvider({ id: p.id, name: p.name, results, status });
    }
  }));

  const hasMore = aggregateHasMore(targets, perProvider);
  return { providers: perProvider, hasMore };
}

module.exports = { list, getProvider, search, searchStream };
