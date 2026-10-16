// 用 JS 的 demo.js 产出真值 —— Rust 的断言照抄这些数，不靠心算。
//
// ⚠️ 写成脚本文件：内联脚本会被 PowerShell 吃掉 $ 和反引号（这一路栽过三次）。
// 用法：node scripts/tmp-demo-truth.cjs
const path = require('path');

// demo.js 用 Date.now() 算日期 —— 把"现在"钉死，两边才对得上
const FIXED_NOW = 1800000000000; // 2027-01-15T08:00:00Z 附近，任取一个固定值
const realNow = Date.now;
Date.now = () => FIXED_NOW;

const demo = require(path.join(__dirname, '..', 'src', 'providers', 'demo.js'));

const show = (r) => ({
  name: r.name,
  size: r.size,
  size_text: r.size_text,
  seeders: r.seeders,
  leechers: r.leechers,
  date: r.date,
  date_text: r.date_text,
  category: r.category,
  info_hash: r.info_hash,
  magnet: r.magnet,
  id: r.id,
});

(async () => {
  for (const q of ['ubuntu', 'demo', '中文关键词']) {
    console.log(`\n===== query = ${JSON.stringify(q)} =====`);
    // page 1
    const p1 = await demo.search(q, { page: 1 });
    console.log(`page1: results=${p1.results.length} error=${p1.error} hasMore=${p1.hasMore}`);
    console.log('  [0]', JSON.stringify(show(p1.results[0])));
    console.log('  [1]', JSON.stringify(show(p1.results[1])));
    console.log('  [19]', JSON.stringify(show(p1.results[19])));

    const p2 = await demo.search(q, { page: 2 });
    console.log(`page2: results=${p2.results.length} hasMore=${p2.hasMore}`);
    console.log('  [0]', JSON.stringify(show(p2.results[0])));

    const p6 = await demo.search(q, { page: 6 });
    console.log(`page6: results=${p6.results.length} hasMore=${p6.hasMore}`);
    const p7 = await demo.search(q, { page: 7 });
    console.log(`page7: results=${p7.results.length} hasMore=${p7.hasMore}`);

    // 确定性：同一个 query 两次必须一模一样
    const again = await demo.search(q, { page: 1 });
    console.log(`确定性(名字与顺序): ${JSON.stringify(again.results.map((r) => r.name)) === JSON.stringify(p1.results.map((r) => r.name))}`);

    // 空 query（browseable 路径）
    const empty = await demo.search('', { page: 1 });
    console.log(`空 query: results=${empty.results.length} 名字=${JSON.stringify(empty.results[0].name)}`);
  }
})();

Date.now = realNow;
