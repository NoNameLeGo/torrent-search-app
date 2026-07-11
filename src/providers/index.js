'use strict';

// Original engines already present in this project.
const tpb = require('./tpb');
const x1337 = require('./1337x');
const nyaa = require('./nyaa');
const yts = require('./yts');
const knaben = require('./knaben');
const torrentscsv = require('./torrentscsv');
const demo = require('./demo');

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
