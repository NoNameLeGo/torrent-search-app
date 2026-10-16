'use strict';

// provider → 「它会请求哪些 URL」→「用哪份 fixture 回」。
//
// ⚠️ **这份表必须和 Rust 侧 `crates/bt-providers/tests/<name>.rs` 喂的 fixture 一致。**
// 两边读的不是同一份快照，"差分"就毫无意义 —— 那是在比两份不同的输入。
//
// ⚠️ `match` 用真实 URL 的正则片段，**别写得太松**：`/knaben/` 这种会把
// 任何带 knaben 的 URL 都吃下，将来 provider 多打一个端点就被静默喂错 fixture。
//
// 目前只覆盖 **5 个 JSON provider** —— 它们不需要 `cheerio`，所以本机（无
// `node_modules`）就能跑。11 个 HTML provider 需要 cheerio，只能在 CI 里跑，
// 见 `htmlProviders()` 里那段 TODO。

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
 * HTML provider —— 需要 `cheerio`，所以**只能在 CI 跑**。
 *
 * TODO(M6)：每条规则要对着 `crates/bt-providers/tests/<name>.rs` 抄一遍 fixture 名。
 * 现在返回空数组是**有意**的：宁可显式地"还没覆盖"，也不要一份看起来覆盖了、
 * 实际喂错 fixture 的表。
 */
function htmlProviders() {
  return [];
}

/** fixture 是否真的存在 —— 缺了要当场报，别让 provider 拿到一个假的 network error。 */
function missingFixtures(spec) {
  return spec.rules
    .map((r) => r.file)
    .filter((f) => !fs.existsSync(path.join(ROOT, 'test', 'fixtures', f)));
}

module.exports = { JSON_PROVIDERS, htmlProviders, missingFixtures, ROOT };
