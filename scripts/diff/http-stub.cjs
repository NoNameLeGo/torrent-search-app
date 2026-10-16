'use strict';

// 把 `src/lib/http.js` 换成「读 fixture 的桩」，塞进 `require.cache`。
//
// ## 为什么能这么做（而不是用 loader hooks）
//
// `package.json` 是 `"type": "commonjs"`，`src/lib/http.js` 是 CJS（`require` /
// `module.exports`），provider 也是 `require('../lib/http')`。CJS 的模块解析走
// `require.cache` —— **预先塞一个桩进去，provider 就会拿到它**，不需要
// `--experimental-loader` / `module.register()` 那一套，也不受 Node 版本差异影响。
//
// ⚠️ 顺序很关键：**先 install、再 require provider**。provider 在模块顶层就
// `const { getJSON } = require('../lib/http')` 把函数**解构捕获**了，
// 之后再换 cache 已经晚了。
//
// ## 副作用（好的那种）
//
// 桩把 `axios` 完全挡在门外 ⇒ **本机能跑，不需要 `node_modules`**
// （用户明确要求不新增本地依赖）。11 个 HTML provider 仍需 `cheerio`，
// 那部分只能在 CI 跑。

const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..', '..');
const HTTP_MODULE = path.join(ROOT, 'src', 'lib', 'http.js');

/**
 * 装桩。
 *
 * @param spec   `fixtures.cjs` 里的一条（用它的 `rules`）
 * @param calls  收集本次实际请求过的 URL（差分报告里要打印，用来发现"多打了一次"）
 * @param log    收集 provider 拿到的返回值形状（调试用）
 */
function install(spec, calls = [], log = []) {
  const rules = spec.rules || [];

  function respond(url, wantJson) {
    calls.push(String(url));
    const rule = rules.find((r) => r.match.test(String(url)));
    if (!rule) {
      // ⚠️ 不静默：没配 fixture 说明**这份差分表漏了一个端点**，
      // 而 provider 会把它显示成 `unreachable (no_fixture_for:...)` —— 一眼能认出来。
      return {
        error: `no_fixture_for:${url}`,
        __missing: true,
      };
    }

    const file = path.join(ROOT, 'test', 'fixtures', rule.file);
    const text = fs.readFileSync(file, 'utf8');
    const asJson = rule.kind === 'json' || wantJson;

    if (asJson) {
      try {
        return { data: JSON.parse(text), status: 200, error: null };
      } catch (e) {
        return { data: null, status: 200, error: `bad_fixture_json:${rule.file}` };
      }
    }
    return { html: text, status: 200, error: null };
  }

  const stub = {
    // provider 里没人直接用 `http`，但导出形状要对齐，免得将来用了就炸
    http: { get: async () => ({ data: null, status: null }) },
    pickUA: () => 'diff-harness',
    getJSON: async (url) => {
      const r = respond(url, true);
      log.push({ url, kind: 'json', missing: Boolean(r.__missing) });
      return r;
    },
    getText: async (url) => {
      const r = respond(url, false);
      log.push({ url, kind: 'text', missing: Boolean(r.__missing) });
      return r;
    },
    postJSON: async (url, payload) => {
      const r = respond(url, true);
      log.push({ url, kind: 'postJSON', missing: Boolean(r.__missing), payload });
      return r;
    },
  };

  // 清掉 provider 的缓存不收 —— provider 之间互不引用，且每次 require 前都会
  // 重新装桩，所以缓存命中也没关系（桩是**按 provider** 换的，同一 provider
  // 只跑一次）。
  const resolved = require.resolve(HTTP_MODULE);
  require.cache[resolved] = {
    id: resolved,
    filename: resolved,
    loaded: true,
    exports: stub,
    children: [],
    paths: [],
  };

  return stub;
}

/** 把桩卸掉（跑完一个 provider 之后调用，避免污染下一个）。 */
function uninstall() {
  delete require.cache[require.resolve(HTTP_MODULE)];
}

module.exports = { install, uninstall, HTTP_MODULE };
