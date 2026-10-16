'use strict';

// M6 差分比较器：拿两侧的规范化 JSON，**逐字段**比，差异要么命中白名单，要么算失败。
//
// 用法：
//   node scripts/diff/compare.mjs rust.json node.json
//   node scripts/diff/compare.mjs rust.json node.json --whitelist scripts/diff/whitelist.json
//
// 退出码：0 = 全部一致或命中白名单；1 = 有未命中白名单的差异（CI 硬门）。
//
// ## 设计要点（这些才是 M6 的价值所在）
//
// 1. **默认比全部 15 个字段**，不是"挑几个关键的比" —— 漏比一个字段的差分等于没差分。
// 2. **两侧的 provider 集合必须完全一致**：一边多一个、少一个都算失败。
//    （"Rust 少移植了一家"是真差异，不能静默。）
// 3. **结果条数必须一致**，且**逐行按下标对齐**（顺序也是语义的一部分：
//    归一化/去重/排序若有分歧，条数相同但内容会错位 ⇒ 必须报出来）。
// 4. **白名单是显式的、带理由的、按 (provider, field) 粒度**的。
//    它只能"豁免一处已知的有意分歧"，**不能**用来豁免"整家 provider 不参与比对" ——
//    后者会让差分悄悄失去覆盖面。要整家豁免就得在 `fixtures.cjs` 里不列它，
//    那样至少是**显式**的。
// 5. 差异报告带**下标**和**两侧原值**，能直接定位到第几条的哪个字段。

const fs = require('node:fs');
const path = require('node:path');

const { FIELDS } = require('./canonical.cjs');

const DEFAULT_WHITELIST = path.join(__dirname, 'whitelist.json');

function parseArgs(argv) {
  const out = { files: [], whitelist: DEFAULT_WHITELIST, only: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--whitelist' || argv[i] === '-w') out.whitelist = argv[++i];
    else if (argv[i] === '--only') {
      // 「**本次比哪些**」——逗号分隔。
      // ⚠️ 这是**显式作用域**，不是放宽检查：列表**内**的 provider 仍要求两侧都在
      //（少一边 = 结构性失败）。需要它的原因是 triage 期间两侧覆盖面可以不同
      //（Rust 已 16 家、CI 里的 Node 只跑 5 家 JSON）。
      out.only = String(argv[++i] || '')
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean);
    } else out.files.push(argv[i]);
  }
  return out;
}

function loadWhitelist(file) {
  if (!fs.existsSync(file)) return [];
  const raw = JSON.parse(fs.readFileSync(file, 'utf8'));
  const entries = Array.isArray(raw.entries) ? raw.entries : [];
  for (const e of entries) {
    // 白名单条目**必须有理由**：没有理由的豁免，三个月后没人敢删
    if (!e.provider || !e.field || !e.reason) {
      throw new Error(`whitelist 条目缺 provider/field/reason: ${JSON.stringify(e)}`);
    }
    if (!FIELDS.includes(e.field)) {
      throw new Error(`whitelist 里的 field 不在 canonical 字段表里: ${e.field}`);
    }
  }
  return entries;
}

const key = (provider, field) => `${provider}\u0000${field}`;

/** 逐字段比一条结果。返回差异列表。 */
function diffResult(provider, index, a, b, whitelisted) {
  const diffs = [];
  for (const f of FIELDS) {
    const va = a[f];
    const vb = b[f];
    // JSON 已归一化，直接严格比：NaN/Infinity 在导出时就变成 null 了
    if (Object.is(va, vb)) continue;
    const wl = whitelisted.has(key(provider, f));
    diffs.push({ provider, index, field: f, rust: va, node: vb, whitelisted: wl });
  }
  return diffs;
}

function compare(rust, node, whitelist) {
  const whitelisted = new Set(whitelist.map((e) => key(e.provider, e.field)));
  const report = { providers: [], unexpected: [], whitelistedHits: [], fatal: [] };

  const rmap = new Map((rust || []).map((o) => [o.provider, o]));
  const nmap = new Map((node || []).map((o) => [o.provider, o]));

  for (const p of new Set([...rmap.keys(), ...nmap.keys()])) {
    const a = rmap.get(p);
    const b = nmap.get(p);
    if (!a) {
      report.fatal.push(`provider 只在 Node 侧存在：${p}（Rust 漏了/没导出）`);
      continue;
    }
    if (!b) {
      report.fatal.push(`provider 只在 Rust 侧存在：${p}`);
      continue;
    }
    if (a.fatal || b.fatal) {
      report.fatal.push(`provider ${p} 导出失败：rust=${a.fatal || '-'} node=${b.fatal || '-'}`);
      continue;
    }

    const rows = { provider: p, rust: a.results.length, node: b.results.length, diffs: 0 };
    report.providers.push(rows);

    // 顶层：provider 级错误串与 has_more 也必须是语义一致的
    // （⚠️ error 串**允许**不同：Rust 的网络层文案是有意 divergence，见 M3）
    if ((a.has_more === null) !== (b.has_more === null) || a.has_more !== b.has_more) {
      report.unexpected.push({
        provider: p,
        field: 'has_more',
        rust: a.has_more,
        node: b.has_more,
        whitelisted: false,
      });
    }

    if (a.results.length !== b.results.length) {
      report.unexpected.push({
        provider: p,
        field: '<条数>',
        rust: a.results.length,
        node: b.results.length,
        whitelisted: false,
      });
      // 条数都不同了，逐行比没有意义（会刷屏）—— 报出条数就够定位了
      continue;
    }

    for (let i = 0; i < a.results.length; i++) {
      for (const d of diffResult(p, i, a.results[i], b.results[i], whitelisted)) {
        rows.diffs++;
        if (d.whitelisted) report.whitelistedHits.push(d);
        else report.unexpected.push(d);
      }
    }

    // 白名单里列了、但这次**没触发**的条目要提示出来 ——
    // 命中不了的白名单就是过期文档，越早发现越好
    for (const e of whitelist) {
      if (e.provider !== p) continue;
      const hit = report.whitelistedHits.some(
        (h) => h.provider === p && h.field === e.field,
      );
      if (!hit) rows.unusedWhitelist = (rows.unusedWhitelist || []).concat(e.field);
    }
  }

  return report;
}

function printReport(report, whitelist) {
  console.log(`canonical 字段数 = ${FIELDS.length}：${FIELDS.join(', ')}`);
  console.log('');
  for (const p of report.providers) {
    const tag = p.diffs === 0 ? '✅ 完全一致' : `↔ ${p.diffs} 处差异`;
    console.log(
      `  ${p.provider.padEnd(16)} rust=${String(p.rust).padStart(4)} node=${String(p.node).padStart(4)}  ${tag}`,
    );
    if (p.unusedWhitelist && p.unusedWhitelist.length) {
      console.log(`      ⚠️ 白名单里这些字段这次没触发（可能已过期）：${p.unusedWhitelist.join(', ')}`);
    }
  }

  if (report.whitelistedHits.length) {
    console.log(`\n命中白名单 ${report.whitelistedHits.length} 处（有意分歧，不算失败）：`);
    const byField = new Map();
    for (const h of report.whitelistedHits) {
      const k = `${h.provider}.${h.field}`;
      byField.set(k, (byField.get(k) || 0) + 1);
    }
    for (const [k, n] of byField) console.log(`  ${k} × ${n}`);
  }

  if (report.fatal.length) {
    console.log('\n🔴 结构性失败：');
    for (const f of report.fatal) console.log(`  ${f}`);
  }

  if (report.unexpected.length) {
    console.log(`\n🔴 未命中白名单的差异 ${report.unexpected.length} 处：`);
    for (const d of report.unexpected.slice(0, 60)) {
      const at = d.index === undefined ? '' : `[${d.index}]`;
      console.log(
        `  ${d.provider}${at}.${d.field}\n      rust = ${JSON.stringify(d.rust)}\n      node = ${JSON.stringify(d.node)}`,
      );
    }
    if (report.unexpected.length > 60) {
      console.log(`  …还有 ${report.unexpected.length - 60} 处`);
    }
  }

  const ok = report.fatal.length === 0 && report.unexpected.length === 0;
  console.log(
    `\n${ok ? '✅ 差分通过' : '🔴 差分失败'} —— ${report.providers.length} 个 provider，` +
      `${report.unexpected.length} 处未豁免差异，${report.whitelistedHits.length} 处命中白名单` +
      `（白名单共 ${whitelist.length} 条）`,
  );
  return ok;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.files.length !== 2) {
    console.error(
      '用法：node scripts/diff/compare.cjs rust.json node.json [--whitelist file] [--only a,b,c]',
    );
    process.exit(2);
  }

  const read = (f) => JSON.parse(fs.readFileSync(f, 'utf8'));
  let rust = read(args.files[0]);
  let node = read(args.files[1]);

  // `--only`：把**两侧**都裁到同一个显式作用域。
  // ⚠️ 作用域**必须显式**（不能靠"取交集"）—— 取交集的话，某家在一边掉了会静默
  // 退出比对，而那正是 M6 最不能出的问题（覆盖面静默缩水）。
  if (args.only.length) {
    const keep = (arr) => arr.filter((o) => args.only.includes(o.provider));
    const droppedN = node.length - keep(node).length;
    const droppedR = rust.length - keep(rust).length;
    console.log(
      `⚠️ 本次只比 ${args.only.length} 家（--only）：${args.only.join(', ')}\n` +
        `   导出里还有 ${droppedR} 家 Rust / ${droppedN} 家 Node **没进本次比对** —— 这是 triage 期间的临时作用域，\n` +
        `   清完必须去掉 --only，让它比全部（见 AGENTS.md「M6」）。`,
    );
    rust = keep(rust);
    node = keep(node);
  }

  const whitelist = loadWhitelist(args.whitelist);
  const report = compare(rust, node, whitelist);
  const ok = printReport(report, whitelist);
  process.exit(ok ? 0 : 1);
}

// 让 `compare()` 能被自测直接 require（M6 的比较器本身也要有测试）
if (require.main === module) main();

module.exports = { compare, printReport, loadWhitelist, FIELDS };
