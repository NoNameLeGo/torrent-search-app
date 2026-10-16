// Torznab 的 URL 规则真值 —— 把 JS 的两个函数**抄一份**在这里跑（不是 require 那个模块）。
//
// ⚠️ 为什么抄：`src/providers/torznab.js` 顶部 `require('cheerio')`，而本机没装 cheerio
// ⇒ require 它直接 MODULE_NOT_FOUND，整个检查跑不起来。抄这几行反而更可靠。
// ⚠️ 用法：node scripts/torznab-url-truth.cjs
// 抄的是 src/providers/torznab.js L19-39（改那边记得改这里）。
'use strict';

function normalizeApiUrl(url) {
  if (!url) return '';
  let u = String(url).trim();
  const q = u.indexOf('?');
  if (q >= 0) u = u.slice(0, q);
  u = u.replace(/\/+$/, '');
  if (!/\/api$/i.test(u)) u += '/api';
  return u;
}

function buildSearchUrl(config, query) {
  const api = normalizeApiUrl(config.url);
  const params = new URLSearchParams({
    apikey: config.apiKey || '',
    extended: '1',
    t: 'search',
    q: query,
  });
  return `${api}?${params.toString()}`;
}

const J = JSON.stringify;
console.log('=== normalizeApiUrl ===');
for (const u of [
  'http://host:9117/api/v2.0/indexers/all/results/torznab/api?apikey=abc',
  'http://host:9117/',
  'http://host:9117///',
  'http://host:9117',
  'http://host/API',
  'http://host/Api',
  'http://host/api/v2',
  '',
  '   ',
]) {
  console.log('  ' + J(u).padEnd(72) + ' -> ' + J(normalizeApiUrl(u)));
}

console.log('\n=== buildSearchUrl ===');
for (const [url, key, q] of [
  ['http://host', 'KEY', 'ubuntu'],
  ['http://h/api', 'k', 'ubuntu server'],
  ['http://h/api', 'k', '中文'],
  ['http://h/api', 'k', 'a&b=c'],
  ['http://h/api', '', 'q'],
  ['http://h/api', 'k', 'a*b~c-d.e_f'],
]) {
  console.log('  ' + J(`${url} | ${key} | ${q}`).padEnd(46) + ' -> ' + J(buildSearchUrl({ url, apiKey: key }, q)));
}
