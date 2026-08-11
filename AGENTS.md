# AGENTS.md — Torrent Search App

## Quick start

```bash
npm install   # express, axios, cheerio (runtime) + electron, electron-builder (dev)
npm start     # node server.js → http://localhost:3000
```

Electron mode: `npm run electron` (picks a free port automatically, no collision with a running `npm start`).

## No test / lint / typecheck

This repo has **no tests, linter, or formatter** configured. There is nothing to run before shipping. If you add tooling, document it here.

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
- `src/providers/bitsearch.js` ~L55-70: extremely deep `div:nth-child(1) > div:nth-last-child(2) > span:nth-child(2)` selector chain — any layout tweak kills it. Rewrite content-based, following the documented pattern in `rutor.js` ~L67-80. ⚠️ bitsearch.to/.am/.eu 当前全部不可达，无法获取实际 HTML 验证。

### 中 — architecture / correctness

- ~~**`hasMore` is a guess**~~ **✅ 已修复（2026-08-10）：** 两处改动——(a) 前端 `loadPage()` 记录本页开始前的 `state.all.length`，只有本页新增了唯一结果才翻页，否则即便服务端说 `hasMore` 也停住（防重复数据虚报翻页）；(b) 服务端 `aggregateHasMore()` 尊重 provider 明确返回的 `hasMore`（`typeof s.hasMore === 'boolean'`），未明确返回时才回退到 `!!p.paginated` 启发式。
- ~~**Duplicate mirror-retry skeleton**: **33 个** provider 逐字重复同一个 `Promise.allSettled → first non-empty` 块。~~ **✅ 已修复（2026-08-10）：** 创建 `src/lib/mirrors.js` 导出 `runMirrors(attempts, name)`，全部 34 个使用 allSettled 的 provider 已迁移。
- ~~**Duplicate RU_MONTHS maps** in `rutor.js`, `megapeer.js`; `btih:` 提取正则散在 26 个文件里。~~ **✅ 已修复（2026-08-11）：** `extractInfoHash(str)` + `ruDate(s)` + `RU_MONTHS` 归入 `src/lib/normalize.js`；26 个 provider 的内联 btih 提取全部替换为 `extractInfoHash()`；rutor/megapeer 的本地 RU_MONTHS 已移除。
- ~~**N+1 detail fetches inside `search()`**: `mypornclub.js`, `xxxclub.js`, `torrent9.js`, `audiobookbay.js`, `blueroms.js`, `megapeer.js` fetch every result's detail page during search — rate-limit/ban risk and latency.~~ **✅ 已修复（2026-08-11）：** 6 个 provider 全部转为 lazy `resolveMagnet`；`mypornclub`/`xxxclub` 已有 resolver 只需移除 N+1；`torrent9`/`audiobookbay`/`blueroms`/`megapeer` 新增 `resolveMagnet` 导出。项目 resolveMagnet 从 9 个增至 13 个。
- ~~**Category data quality**: `sukebei.js` hardcodes `'Porn'` (site also hosts non-adult), `rutor.js` hardcodes `'Other'` though the site exposes categories, `tpb.js` passes raw numeric category strings ("200") unmapped.~~ **部分修复（2026-08-11）：** `tpb.js` 新增 `tpbCategory()` 将 3 位数字码映射为标准分类（1xx→Music/Books, 2xx→Movies/Series, 3xx→Apps, 4xx→Games, 5xx→Porn）；sukebei/rutor 需访问站点提取分类，当前不可达暂缓。
- **Single-domain providers with no mirror fallback**: bt4g, knaben, torrentdatabase, blueroms, filemood, linuxtracker, megapeer, xxxclub, xxxtracker, zeromagnet. `torrentdatabase.js` points at `developify.ca` — name/domain mismatch, likely stale.
- ~~**`PROVIDER_LABEL` on `main`** has only 4 entries — badges/status show raw ids.~~ ✅ 已修复（2026-08-10）

### 中 — server / security hygiene

- `server.js` `/api/magnet` (~L102) + `/api/torznab/test` (~L205) are SSRF-ish proxies: `safeHttpUrl` only checks scheme, deliberately no host allowlist. Binding to 127.0.0.1 (L220) is the real mitigation — **keep it**; if remote access is ever added, add host checks first. Consider also capping `/api/magnet` to domains known to providers.
- `data/torznab.json` stores API keys in plaintext (`src/lib/torznabStore.js`); `listPublic()` masks correctly, but confirm `data/` stays gitignored and consider warning in README.
- ~~`torznabStore.saveAll()` does a bare `fs.writeFileSync` — no try/catch (crashes the request on EACCES/ENOSPC) and read-modify-write is racy.~~ **✅ 已修复（2026-08-11）：** 改为 write-to-temp-then-rename，失败时保持原文件不变。
- ~~qBittorrent login failure detection in `src/lib/downloaders.js` string-matches `/fails|failed/i` on the response body — fragile across qB versions; also only the first `set-cookie` entry is used.~~ **✅ 已修复（2026-08-11）：** 改为检查 HTTP 403（新版 qB 返回） + 遍历所有 set-cookie entries 找 SID=，不再依赖响应体字符串匹配。

### 低 — polish

- `normalize.js` `parseDate` treats "1 month ago" as fixed 30 d and misses "a minute ago"/"last month" phrasings; `parseSize` silently returns `null` for unknown units (e.g. "Гб" — currently pre-translated by providers, keep it that way).
- Page param coercion inconsistent across paginated providers: only rutor/limetorrents apply `|0`; harmless today (index.js passes ints) but normalize in a shared helper.
- `mypornclub.js` ~L28-30 encodes-then-replaces `%20` → `-`, inverting the upstream Kotlin order; special chars still end up percent-encoded in path (occasional misses).
- `electron/main.js` `before-quit` (~L89) closes the HTTP server but doesn't destroy keep-alive sockets — fine for desktop exit, but if graceful restart is ever added use `server.closeAllConnections()`.
- No tests at all (see top of this file). Highest-value first target: golden-file tests for each provider's parser against saved HTML fixtures — they double as change detectors when sites redesign.

## Syncing features between `main` and `feat/tauri`

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

### 多客户端下载器同步到 `feat/tauri`

`main` 的 `public/app.js` 支持 qBittorrent / Transmission / aria2 / Gopeed 四种下载器，
通过 `state.dl`（单键 `'dl'`，body 形状 `{kind,url,user,pass,token,magnet}`）与后端
`src/lib/downloaders.js` 通信。`feat/tauri` 仍是改造前的 qB-only 版本（`state.qb` /
`#qb-url` 系列 DOM、无 `downloaders.js`）。

从 `main` 同步多客户端到 `feat/tauri` 时需连带移植：
- `public/app.js`：`DL_CLIENTS`、`dlLabel()`、`sendToClient()`、`dlPushBody()`、
  `autoDetectDownloader()`、`state.dl`、`batchSendToClient()`、settings modal 的 `#dl-client`
  select 及联动字段
- `public/index.html`：下载器设置面板的对应 DOM id
- `server.js`：`/api/download/push` 路由 + `downloaders` 模块引用
- `src/lib/downloaders.js`：**从 main 拷贝整个文件**（上次 cherry-pick 时 ta 被删了）
- conflict 处理：`package.json` 取 tauri 的（不含 electron 打包依赖）

### 新功能（候选）
1. **Safe Mode**（纯前端，低风险）— 开关禁用成人引擎 + 隐藏 NSFW 结果
2. **已浏览置灰**（纯前端）— 查看/复制过的卡片 dimmed，localStorage 持久化
3. 收藏导出/导入、Browse 浏览、详情海报等（ROI 递减）
