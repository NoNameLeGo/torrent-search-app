'use strict';

// 把 **Node 侧**的搜索结果导成规范化 JSON。
//
// 用法：
//   node scripts/diff/dump-node.cjs                 # 全部（当前 = 5 个 JSON provider）
//   node scripts/diff/dump-node.cjs tpb torrentscsv # 只跑这几个
//   node scripts/diff/dump-node.cjs -o node.json    # 写文件（给 compare 用）
//
// 不需要 `node_modules`（http-stub 把 axios 挡住了），不需要网络。

const fs = require('node:fs');
const path = require('node:path');

const { allProviders, missingFixtures } = require('./fixtures.cjs');
const { install, uninstall } = require('./http-stub.cjs');
const { canonOutcome } = require('./canonical.cjs');

function parseArgs(argv) {
  const out = { only: [], output: null, jsonOnly: false };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '-o' || argv[i] === '--out') out.output = argv[++i];
    else if (argv[i] === '--json-only') out.jsonOnly = true;
    else out.only.push(argv[i]);
  }
  return out;
}

async function dumpOne(spec) {
  // ① 先查 fixture 在不在 —— 缺了要当场说清楚，别让 provider 拿到 network error
  const missing = missingFixtures(spec);
  if (missing.length) {
    return { provider: spec.id, fatal: `fixture 不存在: ${missing.join(', ')}` };
  }

  // ② 装桩 → ③ require provider → ④ 跑
  const calls = [];
  const log = [];
  install(spec, calls, log);

  let mod;
  try {
    mod = require(path.resolve(__dirname, spec.module));
  } catch (e) {
    return { provider: spec.id, fatal: `require 失败: ${e.message}` };
  }

  let raw;
  try {
    raw = await mod.search(spec.query, { page: spec.page || 1 });
  } catch (e) {
    return { provider: spec.id, fatal: `search 抛异常: ${e.message}` };
  }

  const canon = canonOutcome(spec.id, spec.query, spec.page || 1, raw);

  // ⑤ 把"实际打过哪些 URL"也带上：多打一次/打错端点这种事，只看结果看不出来
  canon.fixture_calls = calls;
  // 全项目的约定是 provider **永不抛**（失败落在 error 里）—— 这里顺带钉一下
  canon.node_names = { exported: Object.keys(mod).sort(), log: log.length };

  uninstall();
  return canon;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));

  // 默认跑**全部 16 家** —— CI 走的就是这条，所以「cheerio 没装」会在 CI 里**响**
  // （`require 失败: Cannot find module 'cheerio'` → fatal → 非零退出）。
  // `--json-only` 只给本机用：本机不装 node_modules，11 家 HTML provider 跑不了。
  const pool = args.jsonOnly
    ? allProviders().filter((s) => !s.rules.some((r) => r.kind === 'html'))
    : allProviders();
  const specs = pool.filter((s) => !args.only.length || args.only.includes(s.id));

  if (!specs.length) {
    console.error(`没有匹配的 provider（可选：${allProviders().map((s) => s.id).join(' ')}）`);
    process.exit(2);
  }

  const out = [];
  for (const spec of specs) out.push(await dumpOne(spec));

  const json = JSON.stringify(out, null, 2);
  if (args.output) {
    fs.writeFileSync(args.output, json + '\n', 'utf8');
    console.error(`已写入 ${args.output}（${out.length} 个 provider）`);
    // 摘要打到 stderr，stdout 留给 JSON，方便管道
    for (const o of out) {
      const n = o.results ? o.results.length : 0;
      console.error(
        `  ${o.provider.padEnd(16)} ${o.fatal ? 'FATAL ' + o.fatal : `${n} 条` + (o.error ? `  error=${o.error}` : '')}`,
      );
    }
  } else {
    process.stdout.write(json + '\n');
  }

  // 有 fatal 就是 harness 自己的问题（fixture 缺失/require 失败），必须非零退出
  if (out.some((o) => o.fatal)) process.exit(1);
  if (args.jsonOnly) {
    console.error('提示：--json-only 只跑了 5 个 JSON provider；CI 里跑的是全部 16 家');
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
