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
  { ...yts, enabled: true, browseable: true },
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
  { ...demo, enabled: true, browseable: true },
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
// In browse mode (`browse=true`, empty query) we only query providers that
// actually support query-less latest/trending (`browseable: true`).
function resolveTargets(providers, page, browse) {
  const wanted = providers
    ? providers.split(',').map((s) => s.trim()).filter(Boolean)
    : null;

  return REGISTRY.concat(dynamicTorznabProviders()).filter((p) => {
    if (!p.enabled || (wanted && !wanted.includes(p.id))) return false;
    if (browse && !p.browseable) return false;
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
// may have a next page. If a provider explicitly reports hasMore (demo.js does),
// respect that; otherwise assume paginated providers have more pages as long as
// they returned results. The frontend will additionally stop if a page produces
// no new unique results (deduplication guard).
function aggregateHasMore(targets, perProvider) {
  return targets.some((p) => {
    const s = perProvider[p.id];
    if (!s || s.status !== 'ok' || s.count === 0) return false;
    // Respect explicit hasMore (demo) vs fallback for plain paginated engines.
    if (typeof s.hasMore === 'boolean') return s.hasMore;
    return !!p.paginated;
  });
}

// Concurrency-limited Promise.all: runs at most `limit` async tasks at once.
// Each result is passed to `onSettle(result)` as soon as it resolves, before
// the next task starts — preserving the streaming behavior of searchStream.
// ponytail: simple pool, replace with p-limit if per-task prioritization matters.
async function asyncPool(tasks, limit, onSettle) {
  const results = [];
  const executing = new Set();
  for (const task of tasks) {
    const p = Promise.resolve(task()).then((r) => {
      onSettle && onSettle(r);
      return r;
    });
    results.push(p);
    executing.add(p);
    const clean = () => executing.delete(p);
    p.then(clean, clean);
    if (executing.size >= limit) await Promise.race(executing);
  }
  return Promise.all(results);
}

// ponytail: 8 concurrent requests to third-party sites — enough to keep the
// pipe full without looking like a DDoS. Bump if providers add more mirrors.
const MAX_CONCURRENT = 8;

// Run search across the requested (enabled) providers with concurrency control.
// Each provider is isolated: a failure in one never breaks the others.
async function search(query, { providers = null, page = 1, browse = false } = {}) {
  const targets = resolveTargets(providers, page, browse);

  const perProvider = {};
  const allResults = [];
  await asyncPool(
    targets.map((p) => () =>
      runProvider(p, query, page).then(({ results, status }) => {
        perProvider[p.id] = status;
        allResults.push(...results);
      })
    ),
    MAX_CONCURRENT
  );

  const hasMore = aggregateHasMore(targets, perProvider);
  return { results: allResults, providers: perProvider, hasMore };
}

// Streaming variant: invoke onProvider({ id, name, results, status }) as soon
// as each provider settles, so the caller (SSE endpoint) can push incremental
// updates instead of waiting for the slowest provider. Concurrency-limited to
// MAX_CONCURRENT to avoid flooding third-party sites. Resolves once every
// provider has reported, returning the same aggregate as search().
async function searchStream(query, { providers = null, page = 1, browse = false } = {}, onProvider) {
  const targets = resolveTargets(providers, page, browse);

  const perProvider = {};
  await asyncPool(
    targets.map((p) => () =>
      runProvider(p, query, page).then(({ results, status }) => {
        perProvider[p.id] = status;
        if (typeof onProvider === 'function') {
          onProvider({ id: p.id, name: p.name, results, status });
        }
      })
    ),
    MAX_CONCURRENT
  );

  const hasMore = aggregateHasMore(targets, perProvider);
  return { providers: perProvider, hasMore };
}

module.exports = { list, getProvider, search, searchStream };
