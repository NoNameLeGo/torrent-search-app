# AGENTS.md — Torrent Search App

## Quick start

```bash
npm install   # express, axios, cheerio (runtime) + electron, electron-builder (dev)
npm start     # node server.js → http://localhost:3000
```

Electron mode: `npm run electron` (picks a free port automatically, no collision with a running `npm start`).

## Testing

This repo uses **Node.js built-in `assert` module** for golden-file tests. No external test framework required.

```bash
npm test          # run all tests
npm run test:watch  # watch mode (requires nodemon)
```

### Test structure

```
test/
  run.js              ← main test runner (uses node:test or just assert)
  fixtures/           ← saved HTML/JSON from real provider responses
    tpb-ubuntu.json   ← TPB API response for "ubuntu" query
```

### How to add a new provider test

1. Save a real response to `test/fixtures/<provider>-<query>.json` or `.html`
2. Add a test block in `test/run.js` using `createMockHTTP()` to intercept requests
3. Run `npm test` to verify

**No network calls during tests** — all fixtures are local files.

### Current coverage

- ✅ `tpb.js` — search parsing, category mapping, empty results, HTTP errors
- ✅ `linuxtracker.js` — HTML parsing, infoHash extraction from URL, date/size/seeds parsing
- ✅ `normalize.js` — size/date parsing, magnet building, infoHash extraction, ruDate, edge cases

### Platform note

Tests use `node:test` (built into Node.js v16+) — **zero dependencies**. No jest/mocha required.

### Agent Call Instructions

See [test/README.md](test/README.md) for detailed guide on when and how to run tests, plus instructions for adding new provider tests.

**Quick reference for agents:**
- Modify provider → run `npm test`
- Modify normalize.js → run `node test/normalize.test.js`
- Add new provider → save fixture to `test/fixtures/`, add test block to `test/run.js`, run `npm test`

## Architecture (one screen)

```
public/               ← static frontend (index.html, app.js, styles.css)
server.js             ← Express entry point; exports { app, start }
electron/main.js      ← Electron wrapper; requires server.js and calls start(port)
src/providers/        ← one file per search engine
src/lib/http.js       ← shared axios instance (UA rotation, 10 s timeout, never throws)
src/lib/normalize.js  ← size/date/magnet parsing → canonical TorrentResult shape
```

- `server.js` only auto-listens when run directly (`require.main === module`). When required by Electron it returns the `app` without binding.
- All providers export `search(query, { page }) → { results, error, hasMore }`. Add new engines here.
- `src/lib/http.js` wrappers (`getText`, `getJSON`, `postJSON`) never throw; they return `{ data|html, error }`. Match this pattern in new providers.
- The `demo` provider is offline-only and always enabled — useful for testing the UI without network.

## Adding a provider

1. Create `src/providers/<name>.js` exporting `{ id, name, search }`.
2. Add `resolveMagnet(url)` if magnets require a detail-page fetch (see `1337x.js` for the pattern).
3. Register in `src/providers/index.js` — the array order is the UI display order.
4. Results should pass through `normalize()` from `src/lib/normalize.js`.

## Electron packaging

```bash
npm run dist          # NSIS installer → dist/BT-Search-Electron-Setup-<ver>.exe
npm run build:portable  # manual portable build → dist/portable/
```

产物名必须是纯 ASCII 且带 `-Electron-` / `-Tauri-` 标识；改名需四处同步，见下方
「Code review findings → ✅ 已修复 — 发布配置」。

Build caches are redirected to `.cache/` (project-local, gitignored) to avoid polluting `%LOCALAPPDATA%`.

## Platform note

This is a **Windows-first** project. `start.bat`/`stop.bat` are the primary dev launchers. `npm run electron` and `npm run dist` use `set` (not `export`) for env vars — they are Windows-only scripts.

## Candidate features (borrowed from upstream `prajwalch/TorrentSearch`)

Compared against the upstream Android app. Our search aggregation, multi-client
download push (qB/TR/aria2/Gopeed), batch ops, and CSV export already exceed it.

**Done:** Category system — every result is normalized into one of a few standard
buckets (`movies`/`series`/`anime`/`games`/`apps`/`books`/`music`/`porn`/`other`) by
`normalizeCategory()` in `public/app.js`: provider-supplied `category` (透传 via
`normalize.js`) wins, else a high-confidence title-based inference (`categoryFromTitle`)
fills the gap. The UI renders a category-filter chip row (`renderCategoryFilters`) built
from the buckets actually present in the current results. This is *orthogonal* to engine
grouping (source dimension) — category is the *content* dimension.

The remaining gaps worth closing, ranked by ROI:

1. **Safe Mode** — one toggle that auto-disables NSFW providers (we already have an
   "adult" group: Sukebei/XXXClub/…) and hides NSFW results. Cheap: a localStorage
   flag wired into engine selection + result filtering. High value for demo/家用.
   The `porn` category bucket is already in place to feed the result-hiding half.
2. **Viewed / dead-torrent filtering** — dead-torrent (seeders=0) filtering mostly
   exists via the min-seeders filter. Missing: mark "already viewed" results (opened
   details / copied magnet) as dimmed. Store viewed `infoHash` set in localStorage.
3. **Browse (top/latest)** — browse trending/latest without a query. Needs providers
   to support query-less top/latest fetching (not every site has this) — higher cost,
   mid-term, start with the few engines that support it.
4. **Bookmarks export/import** — we already persist favorites in localStorage; upstream
   adds export-to-file / import. Natural for a desktop app; guards against cache clears.
5. **Richer details (poster/screenshots/description)** — upstream detail screen has
   media poster, screenshot previews, Markdown description. Depends on each site's
   detail-page structure — lower ROI, nice-to-have.

Suggested order: **Safe Mode + viewed filtering** next — both pure frontend, low risk,
and Safe Mode is interrelated (the `porn` category feeds its result-hiding half).

## Code review findings (coder-facing, 2026-07 full-codebase audit)

Concrete, file-referenced technical debt from a full pass over `server.js`, `src/lib/*`, all 42 providers, `public/app.js`, `electron/main.js`. Each item carries a priority (高/中/低). User-facing counterparts live in README「可能的功能 / 可能的优化方向」.

### ✅ 已修复（2026-08-10）— `public/app.js` 引用断裂：曾导致 `main` 整体不可用

提交 `73d1bdd`「下载推送支持多客户端」把后端（`server.js`、`src/lib/downloaders.js`）与
`public/index.html` 都改完了，但 `public/app.js` 的改造是**半成品**，留下 6 处只被调用、
从未定义的标识符。**6 处已全部收口**：统一到 `dlLabel()` / `DL_CLIENTS` /
`autoDetectDownloader()` / `state.dl`，新增 `sendToClient(magnet)` 与
`dlPushBody(magnet)`（POST `/api/download/push`，body 形状 `{kind,url,user,pass,token,magnet}`），
`batchSendToQB` 更名 `batchSendToClient`，localStorage 键统一为 `'dl'`
（`loadDownloader()` 从旧 `'qb'` 键迁移的逻辑保留未动）。同时把 `PROVIDER_LABEL`
改成空对象 + `loadProviders()` 动态填充，与 `feat/tauri` 对齐，顺带消掉那处常年合并冲突。

以下为当初的故障记录，保留作为回归测试的清单（改动下载推送相关代码后照此复验）：

1. **搜索结果完全白屏（最严重，此前审计漏记）** — `dlShort()` 调用于 `cardHTML` L626
   与详情弹窗 L809，真实存在的是 `dlLabel()`（L42）。只要 `state.dl` 有值，每次
   `render()` 就抛 `ReferenceError`。实测搜 `ubuntu`：状态栏 41 个引擎全部 ✓ 返回，
   结果区 **0 张卡片**，且 `#empty` 提示也被隐藏（异常发生在 `$('#empty').hidden = ...`
   之后、`wrap.innerHTML = ...` 之前）——用户看到的是**完全静默的白屏**，无任何报错提示。
   控制台：`dlShort is not defined [41 times]`。
2. **⚙ 设置入口整个失效** — `DL_META` 用于 `syncDlAuthFields()` L990 与探测 toast L1071，
   真实的表叫 `DL_CLIENTS`（L35）。`openSettings()` 在把 modal 的 `hidden` 置 false
   **之前**就抛错，故面板打不开，连带 `loadTorznab()` 也不执行。实测点击齿轮后
   `#settings-modal.hidden` 仍为 `true`。后果：无法配置下载器、无法逐引擎勾选、
   无法添加 Torznab —— 这三个功能的唯一入口都在这个面板里。
3. **设置永不持久化（此前审计漏记）** — `loadDownloader()` 读 `localStorage['dl']`（L51），
   但保存写的是 `localStorage['downloader']`（L1027、L1049）。键名不匹配。

另外三处：
4. `sendToClient(m)` 调用于 L745（`onCardClick`）与 L838（详情弹窗）——**从未定义**，
   只有 `sendToQB(magnet)`（L969）。每次「推送到 X」点击都抛 `ReferenceError`。
5. `autoDetectQB()` 在文件末尾 L1379 调用——真实函数是 `autoDetectDownloader()`（L1053）。
   首屏自动探测从不运行（此时监听器已绑定完，故应用其余部分还能带伤跑）。
6. `state.qb` 读于 `renderBatchBar` L880、`batchSendToQB` L901/913、`sendToQB` L970/976
   —— state 里只有 `state.dl`（L21）。批量推送按钮永久隐藏；推送路径一律跳回设置。
   且 POST body 直接展开 `{...state.qb}`，而非 `/api/download/push` 期望的
   `{kind, url, user, pass, token}`。

**组合出的用户故事**：全新用户能搜但推送按钮永不出现（`state.dl` 为 null）；老用户升级后
`loadDownloader()` 从旧 `qb` 键迁移出 `state.dl` → **一搜就白屏**；任何人保存设置 →
当前会话立刻白屏，刷新后配置丢失。

⚠️ **`feat/tauri` 上没有修好的版本可摘。** 此前本文档写着 "feat/tauri may already carry a
fixed variant"，这是**错的**：`feat/tauri` 上是**改造前的 qB-only 旧版**，自洽且可用
（用 `state.qb`、有 `autoDetectQB()` 定义、DOM 是 `#qb-url`/`#qb-user`/`#qb-pass` 系列，
且不存在 `sendToClient`/`DL_META`/`dlShort`）。修复是在 `main` 上重写的，
反过来同步到 `feat/tauri` 时要把整套多客户端改造一起带过去。

### ✅ 已修复（2026-08-10）— 发布配置：`main` 对齐 `feat/tauri` 的既定约定

产物文件名的约定是**纯 ASCII 且带 `-Electron-` / `-Tauri-` 标识**（中文名下载 URL 会被
percent-encode，个别老旧下载工具会拿到编码串或问号名，这是当初改名的原因）。已改：

- `package.json`：`artifactName` → `BT-Search-Electron-Setup-${version}.${ext}`
- `package.json`：`dist` 脚本去掉硬编码的 `--publish=onTag`，改由 workflow 显式传
  （曾因脚本里写死导致 `--publish=onTag --publish=never` 拼接、后者未生效而发布失败）
- `.github/workflows/build.yml`：两个 upload-artifact 的 `name` → `BT-Search-Electron-Setup` /
  `BT-Search-Electron-Portable`；构建步显式传 `-- --publish=onTag` 保住打 tag 发布的原行为
- `README.md`：安装包示例名跟着更新

**`scripts/build-portable.js` 故意不改**：便携版目录名与其内部的 `BT聚合搜索.exe` 是
解压后给用户看的名字，不参与下载 URL，`feat/tauri` 上同样保留中文。build.yml 里
`working-directory` 与 `path` 仍指向 `dist/portable/BT聚合搜索`，与之保持一致。

另注：`feat/tauri` 有 3 个 workflow（`build.yml` / `release.yml` / `tauri-build.yml`），
`main` 只有 `build.yml`；正式发布（含 Tauri 产物）走 `feat/tauri` 的 `release.yml`。

### 高 — provider layer

- ~~`src/providers/linuxtracker.js` ~L52-54: passes Russian dates straight to `normalize.parseDate` → date is always `null`.~~ **✅ 已修复（2026-08-10）：** 完整重写了解析器以匹配实际 HTML 结构（扁平表格，每列独立 `<td>`，日期为 `DD/MM/YYYY` 格式）。原代码找的是 `table.lista[width="100%"] > tbody > tr` 但实际表格的 `td` 才有 `class="lista"`；且列表页根本没有磁力链接（每个结果都因 `!magnetUri` 被 `continue` 跳过）。修复后从 `td.lista a[href*="torrent-details"]` 识别行，通过 `td` 索引提取各列，从 URL 的 `id` 参数直接提取 infoHash 构造磁力。
- ~~`src/providers/bitsearch.js` ~L55-70: extremely deep `div:nth-child(1) > div:nth-last-child(2) > span:nth-child(2)` selector chain — any layout tweak kills it.~~ **✅ 已修复（2026-08-14）：** 改用内容启发式匹配（按文本模式识别 size/seeders/leechers/date，按 class 识别 category），不再依赖深层位置选择器。⚠️ bitsearch.to/.am/.eu 仍不可达，无法实测。

### 中 — architecture / correctness

- ~~**`hasMore` is a guess**~~ **✅ 已修复（2026-08-10）：** 两处改动——(a) 前端 `loadPage()` 记录本页开始前的 `state.all.length`，只有本页新增了唯一结果才翻页，否则即便服务端说 `hasMore` 也停住（防重复数据虚报翻页）；(b) 服务端 `aggregateHasMore()` 尊重 provider 明确返回的 `hasMore`（`typeof s.hasMore === 'boolean'`），未明确返回时才回退到 `!!p.paginated` 启发式。
- ~~**Duplicate mirror-retry skeleton**: **33 个** provider 逐字重复同一个 `Promise.allSettled → first non-empty` 块。~~ **✅ 已修复（2026-08-10）：** 创建 `src/lib/mirrors.js` 导出 `runMirrors(attempts, name)`，全部 34 个使用 allSettled 的 provider 已迁移。
- ~~**Duplicate RU_MONTHS maps** in `rutor.js`, `megapeer.js`; `btih:` 提取正则散在 26 个文件里。~~ **✅ 已修复（2026-08-11）：** `extractInfoHash(str)` + `ruDate(s)` + `RU_MONTHS` 归入 `src/lib/normalize.js`；26 个 provider 的内联 btih 提取全部替换为 `extractInfoHash()`；rutor/megapeer 的本地 RU_MONTHS 已移除。
- ~~**N+1 detail fetches inside `search()`**: `mypornclub.js`, `xxxclub.js`, `torrent9.js`, `audiobookbay.js`, `blueroms.js`, `megapeer.js` fetch every result's detail page during search — rate-limit/ban risk and latency.~~ **✅ 已修复（2026-08-11）：** 6 个 provider 全部转为 lazy `resolveMagnet`；`mypornclub`/`xxxclub` 已有 resolver 只需移除 N+1；`torrent9`/`audiobookbay`/`blueroms`/`megapeer` 新增 `resolveMagnet` 导出。项目 resolveMagnet 从 9 个增至 13 个。
- ~~**Category data quality**: `sukebei.js` hardcodes `'Porn'` (site also hosts non-adult), `rutor.js` hardcodes `'Other'` though the site exposes categories, `tpb.js` passes raw numeric category strings ("200") unmapped.~~ **部分修复（2026-08-14）：** `tpb.js` 新增 `tpbCategory()` 将 3 位数字码映射为标准分类；`sukebei.js` 新增 `mapSukebeiCategory()` 解析 Nyaa/Sukebei 分类 title 属性（如 `"Hentai - English Translated"` → `'Porn'`，`"Anime - English translated"` → `'Anime'`）；`rutor.js` 改为返回 `null`（搜索结果列表不含分类信息，交由前端启发式归类）。
- **Single-domain providers with no mirror fallback**: bt4g, knaben, torrentdatabase, blueroms, filemood, linuxtracker, megapeer, xxxclub, xxxtracker, zeromagnet. `torrentdatabase.js` points at `developify.ca` — name/domain mismatch, likely stale. ⚠️ 这些 provider 已使用 `runMirrors()` 基础设施，只需补充备用域名数组即可启用回退；目前因无已知可用镜像暂维持单域名。

  **母项目参照**：`prajwalch/TorrentSearch`（Android 版）同样存在此问题，未实现多域名回退。
  
  **建议方案**：参考 SearXNG 和 Jackett 的做法，为每个单域名 provider 添加备用域名数组，利用已有 `runMirrors()` 基础设施自动重试。
  
  **域名状态参考**（2025-08 调研，⚠️ 表中「主域名」与代码实际不一致——以代码为准）：
  | Provider | 代码实际主域名 | 备用域名（待验证） | 状态 |
  |----------|--------------|------------------|------|
  | bt4g | bt4gprx.com（代码） | bt4g.org, bt4gapp.com | ⚠️ 需验证 |
  | knaben | api.knaben.org/v1（JSON API，无 DOMAINS/runMirrors） | vicetemple.io 等 | ⚠️ 需验证 |
  | torrentdatabase | developify.ca | — | 🔴 可能已失效 |
  | blueroms | www.blueroms.ws | — | 需查证 |
  | filemood | filemood.com（代码） | — | ⚠️ 有可疑网站警告 |
  | linuxtracker | linuxtracker.org | — | ✅ 活跃 |
  | megapeer | megapeer.vip | — | 需查证 |
  | xxxclub | xxxclub.to | xxxclub.club | ⚠️ 需验证 |
  | xxxtracker | xxxtor.com（代码） | — | 需查证 |
  | zeromagnet | 9mag.net（代码） | — | 需查证 |
  
  **待办**：逐个验证备用域名可用性，更新对应 provider 文件的 `DOMAINS` 数组。
  
  ⚠️ **验证前勿盲目添加**：`runMirrors()` 用 `Promise.allSettled` 等全部镜像返回，
  一个失联域名会让每次搜索白白多等最多 10s（`http.js` timeout）。必须先确认域名可达
  再加入 `DOMAINS` 数组，否则是在给用户添堵。
- ~~**`PROVIDER_LABEL` on `main`** has only 4 entries — badges/status show raw ids.~~ ✅ 已修复（2026-08-10）

### 中 — server / security hygiene

- `server.js` `/api/magnet` (~L102) + `/api/torznab/test` (~L205) are SSRF-ish proxies: `safeHttpUrl` only checks scheme, deliberately no host allowlist. Binding to 127.0.0.1 (L220) is the real mitigation — **keep it**; if remote access is ever added, add host checks first. Consider also capping `/api/magnet` to domains known to providers.
  
  **母项目参照**：`prajwalch/TorrentSearch`（Android 应用）无此问题，因为直接在设备上进行网络请求，不涉及服务端代理。
  
  **性质**：设计决策，非 bug。当前 127.0.0.1 绑定已提供足够保护。
- `data/torznab.json` stores API keys in plaintext (`src/lib/torznabStore.js`); `listPublic()` masks correctly. **✅ 已修复（2026-08-14）：** `.gitignore` 已忽略 `data/`，并在 README「说明与边界」段新增安全警告。
- ~~`torznabStore.saveAll()` does a bare `fs.writeFileSync` — no try/catch (crashes the request on EACCES/ENOSPC) and read-modify-write is racy.~~ **✅ 已修复（2026-08-11）：** 改为 write-to-temp-then-rename，失败时保持原文件不变。
- ~~qBittorrent login failure detection in `src/lib/downloaders.js` string-matches `/fails|failed/i` on the response body — fragile across qB versions; also only the first `set-cookie` entry is used.~~ **✅ 已修复（2026-08-11）：** 改为检查 HTTP 403（新版 qB 返回） + 遍历所有 set-cookie entries 找 SID=，不再依赖响应体字符串匹配。

### 低 — polish

- ~~`normalize.js` `parseDate` treats "1 month ago" as fixed 30 d and misses "a minute ago"/"last month" phrasings~~ ✅ 已修复（2026-08-11）：新增 5 种短语支持，month 改为 30.44 天
- ~~Page param coercion inconsistent across paginated providers~~ ✅ 已修复（2026-08-11）：新增 `coercePage()` 共享 helper
- ~~`mypornclub.js` ~L28-30 encodes-then-replaces `%20` → `-`~~ ✅ 已修复（2026-08-11）：先 replace 空格再 encodeURIComponent
- ~~`electron/main.js` `before-quit` (~L89) closes server without destroying keep-alive sockets~~ ✅ 已修复（2026-08-12）：新增 `closeAllConnections()`
- ~~`src/lib/http.js` `getText/getJSON/postJSON` 展开顺序 bug — `...opts` 在 headers 合并之后展开，若调用方传 headers 会整体覆盖合并结果~~ ✅ 已修复（2026-08-12）：先解构 headers，再展开 rest
- ~~No tests at all~~ **✅ 已修复（2026-08-14）：** 新增 golden-file 测试框架，使用 Node.js 内置 `assert` 模块，零依赖。当前覆盖 `tpb.js` 和 `normalize.js`，后续可扩展到其他 provider。

## Syncing features between `main` and `feat/tauri`

**铁律：所有修复和功能必须同时在 `main` 和 `feat/tauri` 两个分支上完成。** 不允许先修一个再同步另一个。每轮工作结束后，两个分支的代码（除分支固有限外）必须一致。

The two branches are maintained in parallel: same commit messages, different hashes. Do **not** bulk cherry-pick the whole `feat/tauri..main` range — most of those commits are the parallel twins and would apply duplicate changes. Cherry-pick only the genuinely new commit(s).

**Workflow that avoids losing commits:** do the cherry-pick in a temporary worktree, then **push to the remote *before* removing the worktree**. If you delete the worktree first, the cherry-picked commit is unreachable (the branch ref never pointed at it) and gets garbage-collected — the sync silently vanishes.

```bash
git worktree add <tmp> feat/tauri
# cd into <tmp>, cherry-pick, resolve conflicts, commit
git push origin feat/tauri     # push FIRST
git worktree remove <tmp>      # clean up AFTER push confirmed
```

⚠️ **`feat/tauri` 的 `public/app.js` 整体落后于 `main`，不是「另一个修好的版本」。**
它是多客户端改造**之前**的 qB-only 版本（`state.qb` / `autoDetectQB()` / `#qb-url` 系列 DOM）。
往那边找 bug 修复会白跑一趟；反过来，从 `main` 同步下载相关功能到 `feat/tauri` 时，
要连带把整套多客户端改造（含下面的 6 处修名）一起带过去，不能只摘单个提交。

**~~Known recurring conflict — `PROVIDER_LABEL` / `loadProviders`~~ 已消解（2026-08-10）:**
两分支曾在这里分叉——`main` 是只含 ~4 项的静态字面量，`feat/tauri` 是 `PROVIDER_LABEL = {}`
加 `loadProviders` 里 `providers.forEach((p) => { PROVIDER_LABEL[p.id] = p.name; });` 动态填充。
现在 `main` 已采用与 `feat/tauri` 相同的动态填充写法，这段代码两边一致，不再产生冲突。

若日后再在此处分叉：解冲突时取 incoming（`main`）逻辑，**但务必保留那行动态填充**，
丢了它会让所有徽章/状态栏显示名退化成 provider 原始 id。

## Next steps（下一步）

### ✅ 已完成：多客户端下载器同步到 `feat/tauri`

`main` 和 `feat/tauri` 两分支的 `public/app.js` 现已完全一致，均支持：
- **四种下载器**：qBittorrent / Transmission / aria2·Motrix / Gopeed
- **统一状态**：`state.dl`（单键 `'dl'`，body 形状 `{kind,url,user,pass,token,magnet}`）
- **核心函数**：`DL_CLIENTS`、`dlLabel()`、`sendToClient()`、`autoDetectDownloader()`、`batchSendToClient()`
- **后端路由**：`/api/download/push`、`/api/download/test`、`/api/download/detect`、`/api/download/clients`
- **共享模块**：`src/lib/downloaders.js`（196 行，两分支一致）

**Tauri 专属适配**（`feat/tauri` 独有）：
- `server.js` 新增 `resolvePort()` 和 `resolvePublicDir()` 函数，支持 `--port` 和 `--public-dir` 参数
- `src-tauri/` 目录：Rust 主进程、sidecar 启动逻辑、构建配置
- `scripts/prepare-sidecar.mjs`：复制当前 Node.js 可执行文件作为 sidecar
- `.github/workflows/release.yml`：双版本构建（Electron + Tauri）

**已知小差异**（不影响功能）：
- `src/providers/nyaa.js`：`main` 多了 `category` 字段提取（9 行），`feat/tauri` 暂无
- `server.js`：`feat/tauri` 比 `main` 多 17 行（Tauri CLI 参数支持）

### ✅ 已完成：Safe Mode + 已浏览置灰（2026-08-15）

**Safe Mode**（纯前端，localStorage 持久化）：
- 新增 `state.safeMode` + `loadSafeMode()`/`saveSafeMode()` 工具函数
- 开关位于主界面分组切换条右侧（`#safe-mode-toggle`）
- 启用后：成人分组（`adult`）从顶部 chip 隐藏；`toggleGroup('adult')` 被拦截并提示
- 结果过滤：`visibleResults()` 中 `state.safeMode && it.category === 'porn'` 的条目被过滤
- localStorage 键：`safeMode`（`'true'`/`'false'`）

**已浏览置灰**（纯前端，localStorage 持久化）：
- 新增 `state.viewed`（Set of infoHash）+ `loadViewed()`/`saveViewed()` 工具函数
- `markViewed(it)` 在用户打开详情、复制磁力、推送下载器时调用
- 卡片渲染：`cardHTML()` 添加 `viewed` class（`opacity: .55; filter: grayscale(.4)`）
- hover 恢复透明度（`.card.viewed:hover { opacity: .8 }`）
- localStorage 键：`viewed`（JSON array of strings）

**受影响文件**：`public/app.js`（+81 行）、`public/index.html`（+6 行）、`public/styles.css`（+33 行）

### 新功能（候选）
1. ~~Safe Mode~~ ✅ 已完成
2. ~~已浏览置灰~~ ✅ 已完成
3. 收藏导出/导入、Browse 浏览、详情海报等（ROI 递减）

---

## ✅ 已完成：种子结果堆叠分组显示（2026-08-17）

**来源**：[prajwalch/TorrentSearch#99](https://github.com/prajwalch/TorrentSearch/issues/99)

### 实现总结

同一种子（相同 infoHash）多站命中时，卡片渲染为带边框的「堆叠分组容器」：
- **单来源结果**：走 `singleCardHTML()`，渲染与历史完全一致（兼容性零影响）
- **多来源结果**：走新增的 `stackedCardHTML()`，顶部主信息（名称/做种/大小/时间/分类）+「N 个来源」徽章；「来源详情」区默认折叠，可展开/收起
- **来源行**：每行 = 站名徽章 + 截断显示的磁力（title 存全文）+ [复制] [详情] 按钮；磁力未就绪的来源显示占位文案，走整卡的「获取磁力」统一解析
- **交互**：复用 `onCardClick` 事件委托，新增 `toggle-sources`（展开/折叠）、`copysrc`（复制指定来源磁力，未就绪回退整卡磁力）、`detailsrc`（有详情页外链新标签打开，否则打开聚合详情弹窗）
- **样式**：`.stacked-card` / `.stacked-sources` / `.stacked-source` / `.src-magnet` 等；`[hidden]` 显式压过 `display:flex` 保证折叠生效

**受影响文件**：`public/app.js`（新增 `stackedCardHTML()`/`shortMagnet()`，`cardHTML()` 改为分派）、`public/styles.css`（+33 行）

### 验收核对（2026-08-17 完成）

- ✅ 单来源结果：渲染方式不变（`singleCardHTML` 即原 `cardHTML` 本体）
- ✅ 多来源结果：带边框分组容器 + 顶部主信息 + 可展开来源列表（probe 全绿）
- ✅ 展开/折叠交互：`toggle-sources` 事件委托，流畅无重渲染
- ✅ 批量操作/收藏/CSV 导出不受影响（`data-id` 仍是分组 key，`onCardClick` 分派前置）
- ✅ 顺带修复 3 处「已修复」声明与实际不符的残留 bug：`DL_META` 未定义（2 处 → `DL_CLIENTS`）、设置保存写 `localStorage['downloader']` 而读取 `'dl'`（2 处，设置永不持久化）、重复的 `loadViewed`/`saveViewed` 定义（已删冗余副本）

---

## 仓库结构说明（2026-08-15 修复）

### 嵌套 Git 仓库结构

```
D:\Vibe-Coding\          ← 父目录 Git 仓库（本地，无 remote）
├── .gitignore           ← 已忽略所有子项目和工具链配置
├── torrent-search-app\  ← 本项目的独立 Git 仓库
│   └── .git             ← 指向 https://github.com/NoNameLeGo/torrent-search-app.git
├── ai-berkshire\        ← 其他子项目（各自有独立 .git）
├── my-novel\
└── ...
```

**关键约束：**
1. 父目录 `D:\Vibe-Coding` 是个人 AI Agent 工作区根目录，**不应推送到任何 remote**
2. 本项目的 remote 是 `origin: https://github.com/NoNameLeGo/torrent-search-app.git`
3. 父仓库的 remote 已于 2026-08-15 移除（之前错误指向本项目）
4. 子项目目录（`torrent-search-app/` 等）在父仓库中被 `.gitignore` 忽略，避免 git status 混乱

**对 AI Agent 的影响：**
- 当 Agent 在 `torrent-search-app` 目录工作时，应只操作本项目文件
- 读到父目录的 git status 时，应理解这是"工作区根目录"而非项目本身
- 不要尝试在父目录执行 `git push` 或修改 remote

**相关文件：**
- 父目录 `.gitignore`: `D:\Vibe-Coding\.gitignore`
- 本项目 `.gitignore`: `D:\Vibe-Coding\torrent-search-app\.gitignore`
