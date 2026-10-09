#!/usr/bin/env node
/**
 * provider 探活：对每个引擎的**真实搜索 URL** 发一次请求，报告
 * `HTTP 状态 / 响应字节数 / 结果行数（粗判）`。
 *
 * ## 它回答什么
 *
 * 「这个引擎现在还能用吗」—— 这是离线 fixture 测试**证明不了**的那件事
 * （`test/fixtures/` 是快照，全绿只说明解析逻辑没退化，不说明站点还活着）。
 * 也是 Q5「站点改版怎么发现」的手动那一半：`live_smoke.rs` 覆盖的是 5 个
 * JSON provider，这个脚本覆盖**全部** 40 个。
 *
 * ## 用法
 *
 *   node scripts/provider-health.mjs            # 默认查 ubuntu
 *   node scripts/provider-health.mjs matrix     # 换关键词
 *   node scripts/provider-health.mjs --json     # 机器可读输出
 *
 * ## 零依赖
 *
 * 只用 Node 内置的 `fetch`（需要 Node 18+）。
 *
 * ## ⚠️ 本机跑的两个坑（2026-10-09 实测）
 *
 * 1. **Node 的 `fetch` 默认不认 `HTTP_PROXY`**（undici 不读环境变量代理）。
 *    要经代理走：Node 24+ 加 `--use-env-proxy`。
 * 2. **本机那层代理是 MITM 的**，Node 不信任它的证书 → 会报
 *    `DEPTH_ZERO_SELF_SIGNED_CERT` / `UNABLE_TO_GET_ISSUER_CERT_LOCALLY`。
 *    探活场景可以 `NODE_TLS_REJECT_UNAUTHORIZED=0`（**仅限探活，别进生产代码**）。
 *
 * ⚠️⚠️ **即便如此，本机结果仍然不可全信**：同一域名前后两次探活可能给出不同结论
 * （实测 `linuxtracker` 一次 200、几分钟后 000；`filemood` 报 ECONNRESET，
 * 但我们手里有它真实的结果页快照）。本机还已知对 `archive.org` 等域名有 DNS 污染。
 *
 * → **要权威结论，在 GitHub Actions（干净出口）上跑**（`.github/workflows/provider-health.yml`），
 *   或者在你自己机器上跑一次。
 *
 * 本机推荐命令：
 *
 *   NODE_TLS_REJECT_UNAUTHORIZED=0 node --use-env-proxy scripts/provider-health.mjs
 *
 * ## 怎么读结果
 *
 * | 状态 | 含义 |
 * |---|---|
 * | `200` + rows > 0 | ✅ 正常 |
 * | `200` + rows = 0 | ⚠️ 页面在，但**结果行数为 0** —— 要么关键词真没结果，要么选择器失效 |
 * | `403` / `503` | 被拦（Cloudflare 挑战页 / WAF / 机房 IP 限流） |
 * | `404` / `502` | 路径变了 / 镜像死了 |
 * | `timeout` / `error` | 连不上（DNS 被污染 / 网络被挡 / 站点挂了）—— **要区分是本机网络还是站点** |
 *
 * ⚠️ rows 是**粗判**（数 `<tr` 之类），只用来区分「页面正常」和「页面根本不是结果页」。
 * 精确的字段级验证仍然靠离线 fixture 测试。
 */

const UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ' +
  '(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36';

const TIMEOUT_MS = Number(process.env.PROBE_TIMEOUT_MS || 12000);

/**
 * 每个引擎一条。`path` 里的 `{q}` 会替换成 URL 编码后的关键词。
 * `mark` 是正则源串，用来粗数结果行。
 * `bases` 按 provider 里的 `DOMAINS` 顺序，前一个失败才试下一个（对齐 runMirrors 的意图）。
 */
const PROVIDERS = [
  // ── 已移植（7）─────────────────────────────────────────────
  { id: 'tpb', ported: true, bases: ['https://apibay.org'], path: '/q.php?q={q}', mark: 'info_hash' },
  { id: 'knaben', ported: true, bases: ['https://api.knaben.org/v1'], method: 'POST',
    body: JSON.stringify({ search_type: 'score', search_field: 'title', query: '{Q}', order_by: 'seeders', order_direction: 'desc', size: 50 }),
    headers: { 'content-type': 'application/json' }, mark: '"hits"' },
  { id: 'torrentscsv', ported: true, bases: ['https://torrents-csv.com'], path: '/service/search?q={q}', mark: '"infohash"' },
  { id: 'yts', ported: true, bases: ['https://movies-api.accel.li'],
    path: '/api/v2/list_movies.json?query_term={q}&limit=50', mark: '"movie_count"' },
  { id: 'internetarchive', ported: true, bases: ['https://archive.org'],
    path: '/advancedsearch.php?q={q}&fl[]=identifier&fl[]=btih&fl[]=title&rows=10&page=1&output=json', mark: '"numFound"' },
  { id: 'linuxtracker', ported: true, bases: ['https://linuxtracker.org'],
    path: '/index.php?page=torrents&search={q}&category=0&active=0', mark: '<tr' },
  { id: 'filemood', ported: true, bases: ['https://filemood.com'], path: '/result?q={q}+in%3Atitle', mark: '<tr' },

  // ── 待移植：HTML 抓取（B 组剩下的）──────────────────────────
  { id: '1337x', bases: ['https://1337x.to', 'https://1337x.st', 'https://1377x.to', 'https://x1337x.ws'],
    path: '/search/{q}/1/', mark: '<tr' },
  { id: 'animetosho', bases: ['https://animetosho.org'], path: '/search?q={q}', mark: '<tr' },
  { id: 'anirena', bases: ['https://anirena.com'], path: '/?q={q}&page=1', mark: '<tr' },
  { id: 'audiobookbay', bases: ['https://audiobookbay.lu'], path: '/?s={q}', mark: 'class="post' },
  { id: 'bitsearch', bases: ['https://bitsearch.to', 'https://bitsearch.am'],
    path: '/search?q={q}&page=1&sortBy=seeders', mark: '<tr' },
  { id: 'blueroms', bases: ['https://www.blueroms.ws'], path: '/search?g=0&p=0&q={q}', mark: '<tr' },
  { id: 'bt4g', bases: ['https://bt4gprx.com'],
    path: '/search?q={q}&category=all&orderby=seeders&p=1', mark: '<tr' },
  { id: 'btdigg', bases: ['https://btdig.com'], path: '/search?q={q}', mark: '<tr' },
  { id: 'dmhy', bases: ['https://share.dmhy.org'],
    path: '/topics/list?keyword={q}&sort_id=0&team_id=0&order=date-desc&page=1', mark: '<tr' },
  { id: 'eztv', bases: ['https://eztvx.to', 'https://eztv.re', 'https://eztv.tf', 'https://eztv.wtf'],
    path: '/search/{q}', mark: '<tr' },
  { id: 'limetorrents', bases: ['https://limetorrents.fun', 'https://limetorrents.lol', 'https://limetorrents.pro'],
    path: '/search/all/{q}/date/1/', mark: '<tr' },
  { id: 'megapeer', bases: ['https://megapeer.vip'],
    path: '/browse.php?search={q}&age=&cat=0&stype=0&sort=0&ascdesc=0', mark: '<tr', note: 'win1251 编码，需专门验' },
  { id: 'mikan', bases: ['https://mikanani.me'], path: '/Home/Search?searchstr={q}', mark: '<tr' },
  { id: 'mypornclub', bases: ['https://myporn.club'], path: '/s/{q}/seeders', mark: '<tr' },
  { id: 'nekobt', bases: ['https://nekobt.to'], path: '/search?query={q}', mark: '<tr' },
  { id: 'nyaa', bases: ['https://nyaa.si'], path: '/?f=0&c=0_0&q={q}&page=1', mark: '<tr' },
  { id: 'oxtorrent', bases: ['https://oxtorrent.co', 'https://oxtorrent.so'], path: '/recherche/{q}', mark: '<tr' },
  { id: 'rutor', bases: ['https://rutor.info', 'https://rutor.is', 'https://rutor.ru'],
    path: '/search/1/0/010/2/{q}', mark: '<tr', note: '俄站，编码需验' },
  { id: 'sukebei', bases: ['https://sukebei.nyaa.si'], path: '/?f=0&c=0_0&q={q}', mark: '<tr' },
  { id: 'therarbg', bases: ['https://therarbg.com', 'https://therarbg.to', 'https://therarbg.org'],
    path: '/get-posts/keywords:{q}', mark: '<tr' },
  { id: 'tokyotoshokan', bases: ['https://tokyotosho.info'],
    path: '/search.php?terms={q}&type=0&searchName=true', mark: '<tr' },
  { id: 'torrent9', bases: ['https://www6.torrent9.to', 'https://www.torrent9.to', 'https://torrent9.so'],
    path: '/search_torrent/{q}.html', mark: '<tr' },
  { id: 'torrentdatabase', bases: ['https://developify.ca'], path: '/newest?q={q}', mark: '<tr' },
  { id: 'torrentdownload', bases: ['https://torrentdownload.info', 'https://torrentdownload.me'],
    path: '/search?q={q}', mark: '<tr' },
  { id: 'torrentdownloads', bases: ['https://torrentdownloads.pro', 'https://torrentdownloads.me', 'https://torrentdownloads.org'],
    path: '/search/?s_cat=0&search={q}', mark: '<tr' },
  { id: 'torrentkitty', bases: ['https://torrentkitty.tv', 'https://torrentkitty.to', 'https://torrentkitty.is'],
    path: '/search/{q}', mark: '<tr' },
  { id: 'uindex', bases: ['https://uindex.org', 'https://uindex.to'], path: '/search.php?search={q}&c=0', mark: '<tr' },
  { id: 'xxxclub', bases: ['https://xxxclub.to'], path: '/torrents/search/all/{q}', mark: '<tr' },
  { id: 'xxxtracker', bases: ['https://xxxtor.com'], path: '/b.php?search={q}', mark: '<tr' },
  { id: 'zeromagnet', bases: ['https://9mag.net'], path: '/search?q={q}', mark: '<tr' },
  // ── 待移植：JSON API（B 组之后可能更值得优先）──────────────
  { id: 'anilibria', bases: ['https://anilibria.top'],
    path: '/api/v1/app/search/releases?query={q}', mark: '"id"' },
  { id: 'bangumimoe', bases: ['https://bangumi.moe'], method: 'POST', path: '/api/v2/torrent/search',
    body: JSON.stringify({ query: '{Q}' }), headers: { 'content-type': 'application/json' }, mark: '_id' },
  { id: 'subsplease', bases: ['https://subsplease.org'], path: '/api?f=search&tz=$&s={q}', mark: '"title"' },
];

const Q = process.argv.find((a) => !a.startsWith('-') && a !== process.argv[0] && a !== process.argv[1]) || 'ubuntu';
const AS_JSON = process.argv.includes('--json');
const q = encodeURIComponent(Q);

async function probeOne(p) {
  let last = { base: p.bases[0], status: 0, bytes: 0, rows: 0, ms: 0, err: 'no-attempt' };

  for (const base of p.bases) {
    const url = base + (p.path || '').replace(/\{q\}/g, q);
    const t0 = Date.now();
    try {
      const res = await fetch(url, {
        method: p.method || 'GET',
        headers: {
          'user-agent': UA,
          accept: 'text/html,application/json,application/xml;q=0.9,*/*;q=0.8',
          'accept-language': 'en-US,en;q=0.9',
          ...(p.headers || {}),
        },
        body: p.body ? p.body.replace(/\{Q\}/g, Q) : undefined,
        redirect: 'follow',
        signal: AbortSignal.timeout(TIMEOUT_MS),
      });
      const text = await res.text();
      const rows = (text.match(new RegExp(p.mark, 'gi')) || []).length;
      return { base, status: res.status, bytes: Buffer.byteLength(text), rows, ms: Date.now() - t0 };
    } catch (e) {
      // fetch 的失败一律是 TypeError("fetch failed")，真正的原因在 e.cause 里
      // （ECONNREFUSED / ENOTFOUND / UND_ERR_CONNECT_TIMEOUT / 证书错误 …）
      const cause = e.cause || {};
      const why =
        e.name === 'TimeoutError'
          ? 'timeout'
          : cause.code || cause.message || e.name || String(e);
      last = { base, status: 0, bytes: 0, rows: 0, ms: Date.now() - t0, err: String(why).slice(0, 48) };
    }
  }
  return last;
}

function verdict(r) {
  if (r.status === 0) return `❌ ${r.err}`;
  if (r.status === 200 || r.status === 201) return r.rows > 0 ? '✅' : '⚠️ 0 行';
  if (r.status === 403 || r.status === 451) return '⛔ 被拦';
  if (r.status === 404) return '❓ 404';
  if (r.status >= 500) return '❓ 5xx';
  return `↪ ${r.status}`;
}

const results = [];
for (const p of PROVIDERS) {
  const r = await probeOne(p);
  results.push({ ...r, id: p.id, ported: !!p.ported, note: p.note });
  if (!AS_JSON) {
    const tag = p.ported ? '（已移植）' : '';
    process.stderr.write(
      `. ${p.id.padEnd(16)} ${String(r.status).padEnd(5)} ${String(r.bytes).padStart(7)}B ` +
        `${String(r.rows).padStart(4)} 行  ${verdict(r)} ${tag}\n`
    );
  }
}

if (AS_JSON) {
  console.log(JSON.stringify({ query: Q, results }, null, 2));
} else {
  const okCount = results.filter((r) => r.status === 200 && r.rows > 0).length;
  console.log(`\n关键词「${Q}」：${results.length} 个引擎，${okCount} 个有结果。\n`);
  console.log('| 引擎 | HTTP | 字节 | 行 | 判断 | 备注 |');
  console.log('|---|---|---|---|---|---|');
  for (const r of results) {
    console.log(
      `| \`${r.id}\` | ${r.status || '—'} | ${r.bytes} | ${r.rows} | ${verdict(r)} | ${r.note || ''} |`
    );
  }
}
