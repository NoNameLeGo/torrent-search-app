'use strict';

// provider → 「它会请求哪些 URL」→「用哪份 fixture 回」。
//
// ⚠️ **这份表必须和 Rust 侧 `crates/bt-providers/tests/<name>.rs` 喂的 fixture 一致。**
// 两边读的不是同一份快照，"差分"就毫无意义 —— 那是在比两份不同的输入。
//
// ⚠️ `match` 用真实 URL 的正则片段，**别写得太松**：`/knaben/` 这种会把
// 任何带 knaben 的 URL 都吃下，将来 provider 多打一个端点就被静默喂错 fixture。
//
// JSON provider **不需要 `cheerio`**，所以本机（无 `node_modules`）就能跑；
// HTML provider 需要 cheerio，只能在 CI 里跑（`npm ci --omit=dev`）。

const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..', '..');

/** JSON provider：一次请求、无 cheerio 依赖。 */
const JSON_PROVIDERS = [
  {
    id: 'tpb',
    module: '../../src/providers/tpb.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /apibay\.org\/q\.php/, file: 'tpb-ubuntu.json', kind: 'json' }],
  },
  {
    id: 'knaben',
    module: '../../src/providers/knaben.js',
    query: 'ubuntu',
    page: 1,
    // POST —— URL 就是 API 本身，没有查询串
    rules: [{ match: /^https:\/\/api\.knaben\.org\/v1$/, file: 'knaben-ubuntu.json', kind: 'json' }],
  },
  {
    id: 'torrentscsv',
    module: '../../src/providers/torrentscsv.js',
    query: 'ubuntu',
    page: 1,
    rules: [
      { match: /torrents-csv\.com\/service\/search/, file: 'torrentscsv-ubuntu.json', kind: 'json' },
    ],
  },
  {
    id: 'yts',
    module: '../../src/providers/yts.js',
    query: 'matrix',
    page: 1,
    // ⚠️ yts 的"两层"是**数据结构**（movie → torrents），不是两次 HTTP ——
    // 它只打 `list_movies.json` 一次。
    rules: [
      {
        match: /movies-api\.accel\.li\/api\/v2\/list_movies\.json/,
        file: 'yts-matrix.json',
        kind: 'json',
      },
    ],
  },
  {
    id: 'subsplease',
    module: '../../src/providers/subsplease.js',
    query: 'one piece',
    page: 1,
    // ⚠️ `tz=$` 是**字面量哨兵**（上游 Kotlin 就是这么发的），不是变量
    rules: [{ match: /subsplease\.org\/api/, file: 'subsplease-one-piece.json', kind: 'json' }],
  },
  {
    id: 'bangumimoe',
    module: '../../src/providers/bangumimoe.js',
    query: 'one piece',
    page: 1,
    // POST —— URL 就是 API 本身，body 是 `{"query": …}`
    rules: [
      {
        // 只匹配主机 + 路径前缀（别用 `^https:` 那种要转义的写法 —— 之前被脚本吃坏过一次）
        match: /bangumi\.moe\/api\/v2\/torrent\/search/,
        file: 'bangumimoe-one-piece.json',
        kind: 'json',
      },
    ],
  },
  {
    id: 'internetarchive',
    module: '../../src/providers/internetarchive.js',
    query: 'ubuntu',
    page: 1,
    rules: [
      {
        match: /archive\.org\/advancedsearch\.php/,
        file: 'internetarchive-ubuntu.synthetic.json',
        kind: 'json',
      },
    ],
  },
];

/**
 * HTML provider —— 需要 `cheerio`，所以**只能在 CI 跑**（`npm ci --omit=dev` 装它）。
 *
 * `match` 用**主机名**而不是完整路径，这是有意的：
 *   · 每个 provider 只会打自己的域名，而每个 provider 又各自拿到**独立的桩实例**
 *     ⇒ 主机级匹配不会串台，也不用跟着路径改版（这些站路径改得很勤）；
 *   · 多镜像的（rutor / therarbg / limetorrents / torrent9 / oxtorrent）**几个域名
 *     都要列全** —— JS 侧是并行打所有镜像、取第一个非空结果，所以桩对**每个**镜像
 *     都得答得出来。
 *
 * ⚠️ `kind: 'html'` 表示桩返回 `{ html }`（而不是 `{ data }`）。
 * ⚠️ `megapeer` 的 fixture 是 **windows-1251 字节**：桩按 utf8 读文件会得到 U+FFFD —
 *    这**正是** axios 的行为（它无视 charset 头），所以是**忠实复现**，
 *    而 Rust 侧认 charset 头解码正确 ⇒ 两边的 name/size 等**本来就该不同**，
 *    那些差异在 `whitelist.json` 里（有意 divergence）。
 *
 * query 与 fixture 名必须与 `crates/bt-providers/tests/dump_rust.rs` 的 `SPECS` 一致。
 */
const HTML_PROVIDERS = [
  {
    id: 'linuxtracker',
    module: '../../src/providers/linuxtracker.js',
    query: 'linux',
    page: 1,
    rules: [{ match: /linuxtracker\.org/, file: 'linuxtracker-linux.html', kind: 'html' }],
  },
  {
    id: 'filemood',
    module: '../../src/providers/filemood.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /filemood\.com/, file: 'filemood-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'rutor',
    module: '../../src/providers/rutor.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /rutor\.(info|is|ru)/, file: 'rutor-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'audiobookbay',
    module: '../../src/providers/audiobookbay.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /audiobookbay\.lu/, file: 'audiobookbay-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'therarbg',
    module: '../../src/providers/therarbg.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /therarbg\.(com|to|org)/, file: 'therarbg-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'limetorrents',
    module: '../../src/providers/limetorrents.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /limetorrents\.(fun|lol|pro)/, file: 'limetorrents-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'torrent9',
    module: '../../src/providers/torrent9.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /torrent9\.(to|so)/, file: 'torrent9-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'oxtorrent',
    module: '../../src/providers/oxtorrent.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /oxtorrent\.(co|so)/, file: 'oxtorrent-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'nyaa',
    module: '../../src/providers/nyaa.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /nyaa\.si/, file: 'nyaa-ubuntu.html', kind: 'html' }],
  },
  {
    id: 'megapeer',
    module: '../../src/providers/megapeer.js',
    // ⚠️ 与 Rust 侧一致：megapeer 的 fixture 是 1080p 那批
    query: '1080p',
    page: 1,
    rules: [{ match: /megapeer\.vip/, file: 'megapeer-1080p.html', kind: 'html' }],
  },
  // ---- 2026-10-21/22 新搬的三家（fixture 都是 CI 抓的真实页）----
  {
    id: 'dmhy',
    module: '../../src/providers/dmhy.js',
    query: 'one piece',
    page: 1,
    rules: [{ match: /share\.dmhy\.org/, file: 'dmhy-one-piece.html', kind: 'html' }],
  },
  {
    id: 'mikan',
    module: '../../src/providers/mikan.js',
    query: 'one piece',
    page: 1,
    rules: [{ match: /mikanani\.me/, file: 'mikan-one-piece.html', kind: 'html' }],
  },
  {
    id: 'animetosho',
    module: '../../src/providers/animetosho.js',
    query: 'one piece',
    page: 1,
    rules: [{ match: /animetosho\.org/, file: 'animetosho-one-piece.html', kind: 'html' }],
  },
  {
    id: 'anirena',
    module: '../../src/providers/anirena.js',
    query: 'one piece',
    page: 1,
    rules: [{ match: /anirena\.com/, file: 'anirena-one-piece.html', kind: 'html' }],
  },
  {
    id: 'nekobt',
    module: '../../src/providers/nekobt.js',
    query: 'one piece',
    page: 1,
    rules: [{ match: /nekobt\.to/, file: 'nekobt-one-piece.html', kind: 'html' }],
  },
  {
    id: 'zeromagnet',
    module: '../../src/providers/zeromagnet.js',
    query: 'ubuntu',
    page: 1,
    rules: [{ match: /9mag\.net/, file: 'zeromagnet-ubuntu.html', kind: 'html' }],
  },
  {
    // 2026-10-24：第 24 家。fixture 由 CI 抓（本机对 xxxtor.com 不通）
    id: 'xxxtracker',
    module: '../../src/providers/xxxtracker.js',
    query: 'one piece',
    page: 1,
    rules: [{ match: /xxxtor\.com/, file: 'xxxtracker-one-piece.html', kind: 'html' }],
  },
];

/** 全部 24 家。⚠️ 这个数是「已移植」的家数 —— 改了 provider 集合这里要跟着改。 */
function allProviders() {
  return [...JSON_PROVIDERS, ...HTML_PROVIDERS];
}

/** 兼容旧调用点：返回 HTML 那 17 家。 */
function htmlProviders() {
  return HTML_PROVIDERS;
}

/** fixture 是否真的存在 —— 缺了要当场报，别让 provider 拿到一个假的 network error。 */
function missingFixtures(spec) {
  return spec.rules
    .map((r) => r.file)
    .filter((f) => !fs.existsSync(path.join(ROOT, 'test', 'fixtures', f)));
}

module.exports = { JSON_PROVIDERS, HTML_PROVIDERS, allProviders, htmlProviders, missingFixtures, ROOT };
