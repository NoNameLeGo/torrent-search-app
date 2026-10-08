#!/usr/bin/env node
'use strict';

// cheerio 侧的探针执行器 —— 选择器语义对照的「真值来源」。
//
// 与 Rust 侧的 crates/bt-core/tests/dom_probes.rs 读同一份 probe 清单
// （test/fixtures/html-probes.json），跑同样的 op，输出 JSON。
// 把它的输出存成 test/fixtures/html-probes.expected.json，
// Rust 测试就会逐条比对 —— 这就是「scraper 的选择器语义与 cheerio 一致」的证据。
//
// 用法：
//   node scripts/html-probes.cjs                  # 打到 stdout
//   node scripts/html-probes.cjs out.json         # 写文件
//
// 需要 cheerio。本机没装（node_modules 已删），所以走 CI：
// .github/workflows/html-probes.yml（手动触发，`npm install --no-save --omit=dev cheerio`）。

const fs = require('node:fs');
const path = require('node:path');

const cheerio = require('cheerio');

const ROOT = path.join(__dirname, '..');
const SPEC = path.join(ROOT, 'test', 'fixtures', 'html-probes.json');

/** 与 Rust 侧 run_probe() 一一对应。改一边必须改另一边。 */
function runProbe($, p) {
  const limit = p.limit == null ? 3 : p.limit;
  switch (p.op) {
    case 'count':
      return $(p.selector).length;

    case 'text_first':
      return $(p.selector).first().text().trim();

    case 'attr_first': {
      const v = $(p.selector).first().attr(p.attr);
      return v === undefined ? null : v;
    }

    case 'attrs':
      return $(p.selector)
        .slice(0, limit)
        .map((_, el) => {
          const v = $(el).attr(p.attr);
          return v === undefined ? null : v;
        })
        .get();

    case 'texts':
      return $(p.selector)
        .slice(0, limit)
        .map((_, el) => $(el).text().trim())
        .get();

    case 'row_cells':
      return $(p.selector)
        .slice(0, limit)
        .map((_, el) => {
          const row = $(el).closest(p.closest);
          if (!row.length) return [];
          return row
            .find(p.within)
            .map((__, cell) => $(cell).text().trim())
            .get();
        })
        .get();

    default:
      return { error: `unknown op ${p.op}` };
  }
}

const spec = JSON.parse(fs.readFileSync(SPEC, 'utf8'));
const out = {};

for (const fixture of spec.fixtures) {
  const file = path.join(ROOT, 'test', 'fixtures', fixture.file);
  const html = fs.readFileSync(file, 'utf8');
  const $ = cheerio.load(html);
  out[fixture.file] = {};
  for (const probe of fixture.probes) {
    out[fixture.file][probe.id] = runProbe($, probe);
  }
}

const json = `${JSON.stringify(out, null, 2)}\n`;
const target = process.argv[2];
if (target) {
  fs.writeFileSync(target, json);
  process.stderr.write(`wrote ${target}\n`);
} else {
  process.stdout.write(json);
}
