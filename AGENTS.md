# AGENTS.md — Torrent Search App

## Quick start

```bash
npm install   # express, axios, cheerio (runtime) + electron, electron-builder, esbuild (dev)
npm start     # node server.js → http://localhost:3000
```

Electron mode: `npm run electron` (picks a free port automatically, no collision with a running `npm start`).

## 🏗️ 构建约束 — 先读这条（硬规则，2026-10-04 用户明令）

**不要在本机编译任何东西。** 用户原话：「不要在本地新增什么构建环境什么的，我电脑没空间了」。
实测磁盘（2026-10-07）：C: 301G / 剩 **31G**（90% 用），D: 653G / 剩 **41G**（94% 用）—— **两个盘都快满了，不只是 C**。

凡会产出 `target/` / `dist/` / `node_modules/` 或联网拉依赖的命令
（`cargo build`、`cargo test`、`npm install`、`npm run dist` …）：**先问用户，默认改走 GitHub Actions**。
不要因为「D 盘反正还有 41G」就自作主张开跑。

**注**：本机的 `CARGO_HOME`（`D:\Vibe-Coding\.cargo`）已被清理，`cargo` 已不在 PATH 上；
`RUSTUP_HOME`（`D:\Vibe-Coding\.rustup`）的 toolchain 还在但缺 shim。
**结论：本地跑不了 cargo（编译/测试一律走 CI）；但 toolchain 里的 `rustfmt.exe` 可以独立跑** ——
改完 `.rs` 后 `rustfmt --edition 2021 --check <files>` 一遍再推，能省掉 fmt 门的 CI 往返
（路径与用法见 `CLAUDE.md`「Rust tooling on this machine」；2026-10-11 起实战使用）。

| 场景 | workflow | 触发 |
|---|---|---|
| Rust workspace（编译 + 测试 + bt-app 全链路自检） | `rust.yml` | push `main`/`feat/rust`、PR 改 `crates/**`、手动 |
| Electron 安装包 + portable | `build.yml` | push `main`、`v*` tag |
| Electron + Tauri 统一发版 | `release.yml` | `v*` tag |
| Tauri 编译校验 | `tauri-build.yml` | push `feat/tauri` |
| Slint spike / M1 自检 | `slint-spike.yml` | push `spike/slint-ui`、PR 改 `spike/**` ⚠️ 要手动触发得先同步到 `main` |
| **Node 版测试** | `node.yml` | push `main`、PR 改 `src/**`/`test/**`/`server.js`/`package*.json`、手动 —— **2026-10-16 新建**：在此之前 CI 里**没有任何一步跑 Node 测试**（`npm test` 只存在于 `package.json`） |
| **产物体积门（25MB）** | `rust-size.yml` | push `main` 改 `Cargo.toml`/`crates/**`、手动 —— **2026-10-16 才从探针改成门** |
| **生成并提交 `Cargo.lock`** | `lockfile.yml` | 仅手动 —— 本机无 cargo，锁文件只能在这里生成 |
| workflow 文件体检 | `ci-guard.yml` | push/PR 改 `.github/workflows/**` 或 `scripts/check-workflows.py`、手动 |

**已知可回收空间**：`src-tauri/target` 约 1.1GB（`cargo clean` 即可，但本机已无 cargo）。

## 🦀 Rust 重写主线（`feat/rust` 分支，2026-10-08 起）

本项目**推倒重开、用 Rust 重写**，在 `feat/rust` 上进行，长期长成主分支。
`main` 上的 Node/Express + Electron/Tauri 实现继续可用；Rust 版每搬完一块就替代一块。

**📊 进度快照（2026-10-16 —— **`main` 已是 Rust 主线，M0–M7 全部完成**）**：**16 个 provider 已搬**（5 JSON + 11 HTML），
CI 全绿 + M4 全链路自检 12/12（clippy + fmt 均为硬门；**具体测试数看最近一次 `rust.yml` 日志** —— 本文件里出现的数字都是快照，别当现值）；
探针 **126 条 / 17 fixture**；
**聚合层 = 100%（M2）、下载器 = 100%（M3）、UI = 100%（M4）、引擎体检 = 100%（M5，结论「不砍」）、
差分 = 100%（M6，16 家全量 0 处未豁免差异）、打包 = 100%（M7，v0.3.6/v0.3.8-beta 已发）**
⇒ **没有"下一个里程碑"了** —— 往后是审计/加固（见 §「Rust 侧遗留清单」）。
Node 版整套仍在 `main` 上共存（D18：那道门（M6）已经开了，但"删"是按**函数粒度**的，还没开始）。

🔴 **2026-10-11 目标重订 —— 动手前先读 `docs/ROADMAP.md`。**
那一轮追问推翻了旧路线的隐含假设（「能搜到东西」就是目标），结论是：
**真正的目标 = 「小、快、原生的桌面成品」，搜索只是载体** →
**跳过 WebView 中间态，直接上 Slint；核心做成库，不写 HTTP/SSE。**
⇒ **旧 Q2（复用 `public/` 前端）与 Q6（WebView↔Rust 通信）双双作废。**
M1–M7 全部通过（见上面进度快照）—— ⚠️ **这一段是 2026-10-11 的目标重订记录，别当成"当前状态"读**。

### 范围决策

⚠️ **2026-10-11 已重订。canonical 登记在 `docs/ROADMAP.md` §5（用 D 编号）。**
下表是 2026-10-08 的**旧编号 Q**，保留只为翻历史时对得上 —— **其中 Q2 / Q6 已作废**。

| # | 问题 | 决定 |
|---|---|---|
| Q1 | 产品形态 | **只做桌面原生应用；浏览器 / 手机访问模式放弃**（沿用，见 D2） |
| ~~Q2~~ | ~~UI 方案~~ | ❌ **已作废（2026-10-11）**：原「先不动 UI、复用 `public/` 前端」被推翻 → **直接上 Slint**（`docs/ROADMAP.md` **D8**） |
| Q3 | 平台 | **暂时 Windows-only**（但代码里别写死平台相关的东西）—— 沿用 |
| Q4 | 切换时机 | **门 = 「聚合层跑通」**（口径 C）。**不做「40 个引擎齐」的硬要求**（5 个引擎已确认死亡，按字面执行这条分支永远合不回去），**也不把差分门当硬门** —— 差分（G1，见 `docs/AGENT_HANDBOOK.md` §6）降为**自选验证工具** |
| Q5 | 站点改版怎么发现 | **CI 默认只跑离线 fixture**（稳定的门）；另加一条 `workflow_dispatch` **手动触发的联网冒烟**，允许失败 —— 沿用 |
| ~~Q6~~ | ~~WebView ↔ Rust 怎么通信~~ | ❌ **已作废（2026-10-11）** —— **没有 WebView 就没有这个问题**（`docs/ROADMAP.md` **D9**：核心是库，Slint 进程内直调；不写 HTTP、不写 SSE） |

### 路线（2026-10-11 重订 —— 取代旧的「阶段一 / 阶段二」）

**canonical 在 `docs/ROADMAP.md` §3。** 一句话版：

```
M0 ✅ run_mirrors 提取（净 −73 行）
M1 ✅ 验 Slint 风险（#13548）—— **未复现，Slint 可用**（2026-10-16，见下面「Slint 探针结论」）
M2 ✅ 聚合层 + Provider trait（2026-10-16，347 tests；**D5「聚合层跑通」的门已满足**）
M3 ✅ bt-downloaders（2026-10-16，4 客户端 / 34 条离线测试）
M4 ✅ bt-app Slint MVP（2026-10-16，UI + 接线 + 映射层 + **全链路 --selftest 12/12**）
M5 ✅ 引擎体检（2026-10-16 —— **结论是「一个都不砍」**，见「provider 探活」；
单边 403 不足以判死）
M6 ✅ 差分验证（2026-10-16 —— **16 家全量、0 处未豁免差异**，门已放开到全量）
M7 ✅ 打包 + GitHub Release（2026-10-16 —— **v0.3.6-beta 首发**，四套产物；**v0.3.8-beta** 带上 windows 子系统修复（不再弹控制台）+ Torznab 界面 + 离线 demo 引擎）
```

### 🧹 2026-10-16 审计修复（50 条发现 → 三组已收口，15 HIGH 全部核验）

一次全仓审计（4 个只读 reviewer 分头：`bt-app` / `bt-providers+bt-core` / `downloaders+torznab` / 仓库卫生），
**共 50 条发现**（15 HIGH）。HIGH 逐条回到源码核验过，下面只列**已经改掉的**；
报告与全部发现的原文在 `.audit/*.md`（**未提交**，本地留存）。

**A. 三个 race（`bt-app` —— 都是「await 回来后写错行」）**

| 缺口 | 后果（改之前） | 怎么修的 |
|---|---|---|
| 第二次搜索没有代数围栏 | 搜 ubuntu 回车、立刻搜 debian ⇒ 旧结果被追加进新清空的列表、用**新**查询高亮，旧完成闭包还写 `status`/`has_more` | `UiState.search_gen`（单调递增）+ 两个闭包各一道 `if st.search_gen != my_gen { return }` |
| `refresh_row` 拿 rows 下标去写筛选后的视图 | 点分类 chip 后再推送 ⇒ 按钮文案不刷新、或**别的行**的数据画到那张卡上 | 新增 `RowData.key`（跨 await 一律用 key，不用 idx）+ `row_by_key`/`set_action_by_key`/`set_magnet_by_key`/`refresh_row(st, key)` |
| `push_row` 捕获 `idx` 在**网络往返之后**才写回 | 解析磁力期间点排序 ⇒ 磁力与「已推送 ✓」落到**别**的结果上，之后复制/推送用的是错磁力 | 同上（`Plan.key`）；`next_key` **只增不减**，不随 `begin_search` 归零 |

附带两处 UI 修正：`targets.is_empty()` 的早退回搬到 `begin_search` **之前**（否则「取消勾选全部 paginated 站 + 点下一页」会把第 1 页结果清光）；分类/排序 chip 搜索中置灰（`ChipRow` 加 `in property <bool> can-pick`）；状态行从搜索行里搬出来独立成整行（原来 `width: 230px` 正好截掉「16 家全部失败」那句诊断）。

**B. 两处「静默假成功」+ 一处 panic**

| 缺口 | 后果（改之前） | 怎么修的 |
|---|---|---|
| qB `/torrents/add` 在拒绝时回 **HTTP 200 + 正文 `Fails.`**（不是错误码） | 同一个磁力推两次 ⇒ UI 报「已推送到 qbittorrent（HTTP 200）」并标 Done，**实际什么都没加** | Rust 侧读 body（`eq_ignore_ascii_case("fails.")`）；**Node 侧 `src/lib/downloaders.js` 同缺陷、同轮修掉** |
| Torznab/Jackett 用 **HTTP 200 + `<error code="100">`** 报错 | 错 API key 与「真的没结果」在 UI 上**完全一样**（`✓ 0 结果`） | `parse_items` 探测 `<error>` 子节点（`code == "0"` 且描述为空时不算错 —— 部分索引器成功响应里也有） |
| `normalize_api_url` 用**裸字节切分**判 `/api` 结尾 | 用户给的 URL 以多字节结尾（`http://localhost:9117/中文`）⇒ 每次搜索都 **panic** | `u.get(u.len().saturating_sub(4)..).is_some_and(...)` |

附带四级 LOW/MED 一次收口：`safe_http_url` 的**归一化结果**不再被丢掉（原来只当谓词用，配置里存的是未校验原串）；`Kind::parse` 认不出时**报错**而不是默认成 qBittorrent（否则凭据被发到那台主机的 qB 登录端点、还误导为「登录失败」）；启动装载 `indexers.json` 也过 `safe_http_url`（此前**只有 UI 那条路过**，手改配置能让 `file://`/`ftp://` 进搜索路径）；`btih()` 不再用 `to_lowercase()` 的副本找下标回原串切（U+0130 → 3 字节会错位、可 panic）；`Box::leak` 的注释改成实话（`provider_set()` 每次搜索现造 ⇒ 每次搜索每索引器漏 2 个 String）。

**C. 门与文档（「文档写了✅、机器上没跑」才是本轮最值钱的一类）**

| 缺口 | 证据 | 怎么修 |
|---|---|---|
| **CI 里没有任何一步跑 Node 测试** | `grep -rn "npm test" .github/workflows/` = **0 命中**，而 README 把 Node 版当日常主力 ⇒ 解析回归能绿灯发布 | 新建 **`node.yml`**（push main / PR / 手动，`npm ci` + `npm test`） |
| 体积门**不是门** | `AGENTS.md` 写「体积回归门（25MB）✅」，而 `rust-size.yml` 只挂 `workflow_dispatch` | 加 `push: main`（限 `Cargo.toml`/`crates/**`） |
| `test/providers.test.js` 没人跑**而且早就坏了** | 不在 `npm test` 里、无 workflow 提及；`mockHTTP` 拿 RegExp 去 `startsWith` ⇒ 用例真的打外网 | 删除（覆盖在 `test/run.js` 里全有） |
| `check-workflows.py` 不查 workflow 引用的脚本是否存在 | 正是 `docs/CI_FAILURES.md` #1 的故障形态（workflow 在、脚本不在） | 加 `check_scripts_exist`，`python3 scripts/check-workflows.py` 实测 13 条全 OK + 负向用例会报 |
| 文档自相矛盾 | `AGENTS.md:660` 写「下次开工 = M2」而 `:190` 已记 M2 完成；`:1765` 还写「下一个里程碑是 M1」（相隔 1600 行）；`:223` 写「bt-torznab 还没做」而 crate 早在；`AGENT_HANDBOOK.md` 引用两个**不存在**的脚本 | 逐条改正（并把不存在的 `scripts/diff-providers.mjs` 换成真的 `scripts/diff/*.cjs` + `dump_rust`） |
| 版本两个真源不一致 | workspace `version = "0.0.1"` ⇒ 拿到 0.3.7-beta 的人**在 exe 属性里看到 0.0.1**；`Cargo.lock` 不在仓库而发版跑 `cargo build --release`（**无 `--locked`**） | `Cargo.toml` → `0.3.7-beta`（与 `package.json` 一致）+ `release-check.js` 加机器判据 + `Cargo.lock` **已入库**（`lockfile.yml` 生成，6781 行）且 `rust.yml` / `rust-size.yml` / `release.yml` 的 cargo 命令**都加了 `--locked`** |

其他：`package.json` 里 4 个指向**不存在**的 `src-tauri/` 的脚本 + `@tauri-apps/cli` 依赖已删（main 上根本没有那个目录 ⇒ 每次 `npm ci` 白装）；`server.js` 给所有 async 路由加 `wrap()` 转 `next(err)` + 加 4 参错误中间件（Express 4 不转发 rejected promise，而 node≥22 下未处理拒绝会**终止进程**；默认错误页还会回显栈与绝对路径）；`electron/main.js` 的 `shell.openExternal` 加 `^(https?|magnet):` 白名单（`esc()` 只挡 HTML、不挡 scheme）；`aggregate.rs` 的 `catch_unwind` 把 **`search()` 调用本身**包进闭包（旧写法只包住返回的 future ⇒ 同步段 panic 仍带走整轮聚合，而唯一那条用例把 panic 放在了 future 里所以全绿）。

⚠️ **两条有意留着没改**（不是遗忘）：`/api/magnet`、`/api/torznab/test`、`/api/download/*` 的 SSRF 面（只查 scheme、无 host 白名单）—— 属**已登记的设计决策**，缓解措施是绑 `127.0.0.1`；Node 版依赖树里的 `21 vulnerabilities`（1 critical）—— 属 Node 版，且本机不装依赖、改它需要 `npm audit fix` 与真跑一轮测试。

### 🔍 Rust 侧遗留清单（2026-10-16 审计；A 表 ①②③ ✅ / B 表 ④ ✅）



**结论：七个里程碑全部完成；`crates/` 里没有任何 TODO/FIXME 型欠账。**

⚠️ **本清单的进度在 2026-10-16 晚些时候又推进了一轮**（产品级改进 6 项）：

| # | 改进 | 状态 |
|---|---|---|
| 1 | 排序（做种/相关度/体积/日期）+ 分类筛选 | ✅ |
| 2 | 「一条都没有」说得清原因 + **失败分网络层/站点层** | ✅ 判据是 `bt_core::http::classify_error`（依据 providers 复刻的 axios 错误码，不是猜文案） |
| 3 | 体积回归门（25MB，`rust-size.yml`） | ✅ ⚠️ **2026-10-16 才真的挂上 `push main` 门** —— 在那之前它只挂 `workflow_dispatch`，等于「谁也不跑的探针」而文档已当它是门 |
| 4 | registry 动态化（`ProviderFactory` + `provider_set()`，每次现造一份） | ✅ **注入与追加都有**（`add_dynamic_factory` 就是 Torznab 用的追加路径） |
| 5 | Torznab | 🟡 **只差 UI** —— `crates/bt-torznab` 已完成**解析层**（`roxmltree`，10 条测试）+ **请求层/Provider**（`normalize_api_url` / `build_search_url` / `TorznabProvider`）+ **动态追加**（`add_dynamic_factory`）+ **索引器持久化**（`indexers.json`）+ 启动装载。⚠️ **还剩**："在 UI 里加/删索引器"的界面 —— 现在要用手编 `%APPDATA%\bt-search\indexers.json`（`save_indexers()` 已写好但**没有调用方**） |
| 6 | 引擎腐坏数据化（`health.rs` 落盘 + `--health` + 状态条 ⚠ 标记） | ✅ |


**A. 用户能直接感到的三个（产品级）—— ✅ 全部已修**

| # | 缺口 | 证据（修之前） | 怎么修的 |
|---|---|---|---|
| 1 | **分页完全没接** | `main.rs` 两处写死 `page: 1`；`has_more` 在 UI 层完全没用 | ✅ `start_search(ui, q, page)` + `next-page` 回调；`has_more` 来自 `StreamOutcome`；UI 加「第 N 页 · 下一页」 |
| 2 | **下载器配置不持久化** | `bt-app` 里没有 `fs::write`/`serde`/配置文件 | ✅ `src/config_store.rs`：`%APPDATA%\bt-search\config.json`，serde，失败一律不致命 |
| 3 | **没有用户级站点开关** | `SearchArgs { providers: None }` ⇒ 永远 16 家全打 | ✅ `AppState.sites` + `selected_providers()`；一个都没勾时 **UI 层拦住不搜**（不是搜全部） |

⚠️ 1 和 3 都是同一类：**"能力做完了、UI 没接"** —— 引擎层的分页语义与 `providers=`
过滤语义 M2 就建好并测过了，缺的只是 `bt-app` 那一层。

**B. 工程级 —— 4 已修，5/6 见下**

| # | 缺口 | 状态 |
|---|---|---|
| 4 | **`catch_unwind` 未做** | ✅ **已修**：`run_provider` 用 `AssertUnwindSafe(..).catch_unwind()`，配了 `a_panicking_provider_does_not_take_down_the_others`（去掉它这条会 red，而不是静默通过）。当时"本机没有 `cargo`"的理由**已失效**（M3/M4/M6 证明这条路上本机不编译也能交付） |
| 5 | **`demo` 引擎缺失** | ⬜ 未做。`Provider::demo()` 建模了、`ProviderInfo.demo` 导出了，但 registry 里没有 demo 引擎 ⇒ 该字段恒 `false`。JS 有 `demo.js`（离线确定性假数据）。**注意它与 6 共用同一堵墙**（见下） |
| 6 | **Torznab / registry 动态化** | 🟡 **registry 动态化已完成**（见上表 #4：工厂 + `provider_set()`）；**Torznab 的解析层也完成了**，剩请求层 / `Provider` 实现 / **动态追加** / UI 配置。墙已摸清（见下），追加路径是下一个具体动作 |

#### 🧱 #4 的墙：`Box<dyn Provider>` **不能 clone** —— ✅ **已按出路 ① 解决（2026-10-16）**

**先记结论**：`bt-app` 现在有 `ProviderFactory` + `INJECTED` + `provider_set()`，
**每次搜索现造一份** provider 集合，注入**可重复**（旧版 `OnceLock::set` 第二次静默失败）。
`--selftest` 的假 provider 已工厂化。CI 全绿（含走 push 路径的 M4 自检）。

⚠️ **下面这段墙的原始记录保留** —— 它是"为什么必须用工厂"的原因，
也是**改用 `&[&dyn Provider]`（出路②）之前**该读的东西。

----

想"静态 16 家 + 运行期动态的（Torznab / demo / 测试注入）并成一份集合"时，
撞到的是这一条 —— **不是 `Send`**（`pub trait Provider: Send + Sync` ✅、
`Vec<Box<dyn Provider>>` 可以 move 进后台任务 ✅），而是：

```
aggregate::search_stream(http, registry: &[Box<dyn Provider>], …)   ← 只认**一个**切片
registry() -> &'static [Box<dyn Provider>]                          ← 静态那份是"借来的"
```
两者要进同一个切片 ⇒ 必须**拥有一份拷贝** ⇒ 但 `Box<dyn Provider>` 拷不出来。
而且 `search_stream` 是在**后台任务里**调的（靠 `registry()` 的 `'static`），
所以"改成每次现造一份"会连带改那里。

**⚠️ 改动时新引入的一个风险（自查抓到，已修）**：`provider_set()` 读的是
**`thread_local`** 的 `INJECTED`，而 tokio 的后台线程是**另一个线程** ——
在 `spawn` 里调它会读到 `None` ⇒ 退回静态注册表 ⇒ `--selftest` 的假 provider 找不到。
旧代码用 `registry()`（`'static`）从哪个线程读都对，**所以这是新引入的**。
⇒ **规矩：`provider_set()` 只能在 UI 线程（spawn 之前）调，取到的东西 move 进去。**

**两条干净的出路（选一条，别发明第三条）：**

**① provider 工厂**（推荐，且与 Torznab 天然吻合）
```rust
pub type ProviderFactory = Box<dyn Fn() -> Box<dyn Provider> + Send + Sync>;
// provider_set() 每次搜索调一遍工厂 ⇒ 得到一份**自己拥有**的集合
```
Torznab 本来就是"从存好的索引器配置**现造**一个 provider"，不是长期持有一个。
⚠️ **代价**：`--selftest` 的 `install_registry(fake_registry())` 要改成返回工厂
（现在它传的是实例）—— 那 3 个假 provider 得能**重建**。

**② `&[&dyn Provider]`**（把 `aggregate` 的签名从 `&[Box<…>]` 改成 `&[&dyn …]`）
```rust
let refs: Vec<&dyn Provider> = statics.iter().chain(dyns.iter()).map(|b| b.as_ref()).collect();
```
动态那批由本地 `Vec<Box<…>>` 持有、静态那批是 `'static`，**都能借** ✅
⚠️ **代价**：`aggregate.rs` + 它的测试（那里到处传 `&[Box<dyn Provider>]`）都要改。

**③ 顺带记一个更小的（不解决动态化，但能立刻做）**：`install_registry` 现在用
`OnceLock::set` 并且**刻意忽略失败**（第二次静默无效）。要让"注入"可重复
（Torznab 与测试都需要），它得换成 `thread_local! { static INJECTED: RefCell<...> }` ——
**但这一步会连带改 `registry()` 的返回类型**（`thread_local` 给不出 `&'static`），
于是又回到上面那道墙。**所以 ③ 不能单独做**，要跟 ① 或 ② 一起。

**预计规模**：选 ① ⇒ 改 `main.rs`(4 处调用点) + `selftest.rs`(工厂化 3 个假 provider)；
选 ② ⇒ 再带上 `aggregate.rs` 与它的测试。**半天以内，但必须一次改完**（编译不过会卡住 main）。

#### ⑤ Torznab 的规格（2026-10-16 抄自 JS 参考实现，**已核对源码**）

端点：`{apiBase}?apikey={key}&extended=1&t=search&q={query}`
（`src/providers/torznab.js` L8 / L32-33）

**字段映射**（逐条对着 `parseItems` 抄的，别凭印象写）：

| TorrentResult | 来源 | 备注 |
|---|---|---|
| `name` | `<title>` 文本 | `trim()` |
| `magnet` | `<enclosure url>` **以 `magnet:` 开头**时用它；否则 `<link>` 以 `magnet:` 开头时用 `<link>`；都没有就是空 | **两级兜底**，不是"或" |
| `size` | `<enclosure length>` 属性；为空或非数字 ⇒ 退回 `<size>` 元素文本 | 注意是"解析不出**数字**"也算空 |
| `infoHash` | `<torznab:attr name="infohash" value>`；没有就从 magnet 里正则 `btih:([a-f0-9]+)` 兜底 | **两级兜底** |
| `seeders` | `<torznab:attr name="seeders" value>` | ⚠️ **`0` 是有效值**，别当 falsy 丢（JS 那边踩过这个坑） |
| `leechers` | `<torznab:attr name="peers" value>` | 名字是 **peers** 不是 leechers |
| `detailUrl` | `<comments>` → `<link>` → `<guid>` 三级兜底 | 与 magnet 那条**互相独立** |
| `date` | `<pubDate>` | 交给 `normalize()` 解析 |
| `category` | `<torznab:attr name="category" value>` | ⚠️ **第一版规格漏了这条**（核对源码时补上的）—— 而它正好喂给 #1 的分类筛选 |
| `provider` | 配置里的 id | 运行时注入，不是站点给的 |

**⚠️ 解析器：不能用 `bt_core::dom`**（决定性依据）：
JS 用的是 `cheerio.load(xml, { xmlMode: true })` —— **XML 模式**。
而 Rust 侧的 `bt_core::dom` 是 `scraper`/html5ever，**没有 XML 模式**：
HTML 解析器只在 foreign content（SVG/MathML）里认自闭合标签，
所以 `<torznab:attr name="seeders" value="128" />` 的 `/` 会被**忽略**，
该元素保持"打开"，**后续兄弟节点变成它的子节点** ⇒ 结构全错。
⇒ 必须用真正的 XML 解析器。

**两条路（选一条，别发明第三条）：**

1. **引 `quick-xml`**（正确性最优）。⚠️ 代价：本机完全无法预检，而它的
   **API 在 0.30 → 0.31 之间变过**（`read_event_into(&mut buf)` → `read_event()`、
   `trim_text` 挪进 `config_mut()`）⇒ 押版本等于押运气，但 CI 反馈只有 3 分钟，
   迭代得起。**建议钉住一个小版本并在注释里写明"针对该版本的 API"。**
2. **手写 item 扫描器**（零依赖）。输入是 **Jackett/Prowlarr 机器生成**的、
   结构固定的 RSS，不是任意 XML。⚠️ 但必须处理：**XML 实体解码**
   （`&amp;` `&lt;` `&quot;` `&#NN;` —— 标题里的 `&` 极常见）、自闭合标签、
   CDATA。手写就必须**自己写全**，漏一个就是静默错解。
   ⇒ 若选它，**把解析封在一个 `parse_items(xml) -> Vec<RawItem>` 里**，
   将来换 `quick-xml` 只动这一个函数。

**fixture 已经写好了**：`test/fixtures/torznab-search.xml`（3 个 item，
分别覆盖"标准形态 / 实体+两级兜底+seeders=0 / 全缺字段"）。
**⚠️ 它是手写的，不是抓来的** —— 每条都对应上面映射表里的一个分支；
真正的联网校验要靠 `live-smoke.yml` 那条路（或让用户填一个真索引器）。

**✅ 这条已经过期（2026-10-16 晚）：`crates/bt-torznab` 早已落地** —— `roxmltree` 解析层 +
`TorznabProvider`（请求层）+ `add_dynamic_factory` 动态追加 + `indexers.json` 持久化 + 启动装载，
**只差 UI 的"加/删索引器"界面**（现在要手编 `%APPDATA%\bt-search\indexers.json`，
`save_indexers()` 已写好但**没有调用方**）。下面的"顺序建议"已完成，留作历史。

**C. 2026-10-16 审计顺手修掉的一件（并且纠正了我自己上一轮的错误诊断）**

**`feat/tauri` 的提交没推上去。** 裸 `git push` + 该分支**没有 upstream 跟踪** ⇒
`fatal: The current branch feat/tauri has no upstream branch`，
而我用 `Select-Object -Last 1` 过滤输出时把它吃成了空行，**没看见**。
⇒ v0.3.6-beta 里那个 `BT-Search-Tauri-Setup-0.3.5-beta.exe` 版本号不对。

**⚠️ 2026-10-16 晚：这件事我诊断错过两次，最后靠读代码定案 —— 完整证据链如下**

1. **第一次诊断（"bump 错了文件：Tauri 取 `tauri.conf.json` 而非 `package.json`"）—— 是对的。**
   我后来把它写成了"那是错的"，**那句才是错的**（就是下面这段）。
2. **第二次诊断（"提交根本没到远端，CI 构建的是旧的 `feat/tauri`"）—— 也对，但只解释了一部分。**
3. **定案证据（v0.3.8-beta）**：这次我把 `feat/tauri` 的 `package.json` 改成了 `0.3.7-beta`
   并且**确认推送成功**（`git show feat/tauri:package.json` = 0.3.7-beta），
   可发出来的安装包**仍然叫 `BT-Search-Tauri-Setup-0.3.6-beta.exe`**。
   去读 `release.yml` 才看到真源：
   ```yaml
   ver=$(node -p "require('./src-tauri/tauri.conf.json').version")
   ```
   而 `tauri.conf.json` 当时是 `0.3.6-beta`。⇒ **第一次诊断成立。**
4. 已修：`feat/tauri` 的三处一律 `0.3.7-beta` ——
   `src-tauri/tauri.conf.json`（**真源**）、`src-tauri/Cargo.toml`（conf 缺 version 时的回退源，
   它一直停在 `0.3.4-beta`，落后三个版本）、`package.json`（Electron 用）。
   CLAUDE.md 里自相矛盾的两处说法（`:91` 说 package.json、`:260` 说 tauri.conf.json）已统一。

**⚠️ 教训（比这件事本身值钱）**：
- **同一件事在两份文档里说法不一致时，两份都不能信 —— 去读代码。**
  我在两个错误诊断之间来回，就是因为都在改文档、而不是去看 `release.yml` 那一行。
- **"改对了"要有产物证据**：`feat/tauri` 的 `package.json` 确实是 0.3.7-beta（那次是真的推上去了），
  但**产品名没变** —— 只有下载下来的文件名能证明改对了。

**已发布的 v0.3.8-beta 是既成事实**（规则：绝不重打同一个 tag，也**不重命名那个资产** ——
改名会让文件名说 0.3.7 而安装包内部仍写 0.3.6，那是造假）。
那个安装包的**内容**与 v0.3.6-beta 相同（Tauri 线这两版没改过代码），只是名字里的版本号旧了。

**⚠️ 教训（与 `gh` 的 403 同一类）**：**本仓库有分支没有 upstream 跟踪
（`main` / `feat/tauri`）—— 推它们必须显式 `git push origin <branch>`，
且不要用 `Select-Object -Last 1` 过滤推送输出**，否则失败会被静默吃掉。
（`main` 已在切主线时补上 upstream；`feat/tauri` 这次也推通了。）

**D. 不是遗留，是设计（别再当欠账）**：D17 Torznab 本期不做 · D18 Node 版留着
（M6 已完成 ⇒ 那道门开了，但"删"是按**函数粒度**的，还没开始）·
M5 那 5 个 403 是**机房 IP 被拦**不是死站 · Rust 版 **16 家 vs Node 40 家**已如实写明。

**🟡 M6 进行中（2026-10-16）—— 差分 harness 建成，5 个 JSON provider 首次真跑就零差异。**

`scripts/diff/` 四件套 + `crates/bt-providers/tests/dump_rust.rs` + `rust.yml` 两步。
实测（CI run `38846312012`）：

```
tpb              rust= 100 node= 100  ✅ 完全一致
knaben           rust=   5 node=   5  ✅ 完全一致
torrentscsv      rust=  25 node=  25  ✅ 完全一致
yts              rust=   7 node=   7  ✅ 完全一致
internetarchive  rust=   8 node=   8  ✅ 完全一致
✅ 差分通过 —— 5 个 provider，0 处未豁免差异（白名单共 8 条，全部未触发）
```

**145 条结果 × 15 个字段 = 2175 个字段逐一对齐、零差异** —— 这 5 家与 Node
**逐字段同构**，不再是"手抄期望值"的间接证据。

**🔑 让这件事可行的关键勘明**：`package.json` 是 `"type": "commonjs"`，
`src/lib/http.js` 是 CJS ⇒ **`require.cache` 预填一个桩**就能把 HTTP 层换掉，
不需要 loader hooks；而桩把 `axios` 挡在门外 ⇒ **本机就能跑，不需要 `node_modules`**
（符合「不新增本地依赖」的约束）。⚠️ 顺序是硬要求：**先 install 桩、再 require provider**
（provider 在模块顶层就 `const { getJSON } = require('../lib/http')` 解构捕获了）。

**设计上防「假通过」的五条**（都在 `compare.cjs` 里）：默认比**全部 15 个字段**；
**两侧 provider 集合必须完全一致**（少移植一家是真差异，不能静默）；**条数必须一致且逐行
按下标对齐**（顺序也是语义）；**白名单只能豁免单字段且必须有 reason，不允许整家豁免**；
**命中不了的白名单条目会被报成「可能已过期」**。
另外顶层 `has_more` 也比（M2 的发现：`undefined` 与显式 `false` 不是一回事），
但 `error` 串**不**比 —— Rust 的网络层文案是有意 divergence（M3 定的）。

**这一轮踩到两个坑，都不是"逻辑错"，是环境/约定错：**

1. 🔴 **windows-latest 上多行 `run:` 默认走 PowerShell，不是 bash。**
   所以 `set -euo pipefail` 会炸（`set` 是 `Set-Variable` 的别名，
   报 `A parameter cannot be found ... 'euo'`）。**多行 `run:` 必须显式写 `shell: bash`。**
   之前的步骤都是单行命令（`npm ci` / `cargo test`），所以这个坑一直没暴露。
2. 🔴 **cargo 跑测试时 CWD 是「包根」（`crates/bt-providers`），不是仓库根。**
   所以 `BT_DIFF_OUT=.cache/diff/rust.json` 会写到 `crates/bt-providers/.cache/` 去，
   而 CI 下一步在仓库根找它 ⇒ 文件不存在。修法是在 harness 里用
   `CARGO_MANIFEST_DIR` 上溯两级解析相对路径 —— **harness 不该依赖调用方的 CWD**。

**🔴 2026-10-16 首次全量跑（16 家）—— 抓到 111 处未命中白名单的差异。这正是 M6 的价值。**

| provider | rust | node | 结论 |
|---|---|---|---|
| tpb / knaben / torrentscsv / yts / filemood / audiobookbay / therarbg / torrent9 / oxtorrent / nyaa / zeromagnet | — | — | ✅ **11 家完全一致** |
| `internetarchive` | 8 | 8 | ↔ 7 |
| `linuxtracker` | 18 | 18 | ↔ 18 |
| `rutor` | 100 | 100 | ↔ 2 |
| `limetorrents` | 40 | 40 | ↔ 2 |
| `megapeer` | 50 | 50 | ↔ 228（白名单命中 146；`magnet` 那条**未触发**） |

**差异分两类，处理方式完全不同：**

**① harness 造的假差异（必须先消掉，否则会淹没真差异）**
```
rust = "https://archive.org/details/ubuntu-7.10-alpha3-desktop-i386"
node = "<BASE>/details/ubuntu-7.10-alpha3-desktop-i386"
```
`internetarchive`（可能还有别家）的 `detail_url` 用**硬编码域名**，不在"本侧 base"里
⇒ 我按"本侧实际用过的 origin"做的归一化**盖不住它**。
🔧 **正确做法：把归一化从"各侧提供 origin 列表"改成 compare 时的通用剥离** ——
`detail_url` 里任何 `^https?://[^/]+` 一律换成 `<BASE>`。
理由：各侧维护域名表就是**第三份会漂的表**；而 `detail_url` 的域名本来就是 harness
喂进去的（base），比**路径形状**才有意义。
⚠️ **代价要说清楚**：通用剥离会**盖掉"两侧选中的镜像域名不同"**。
JS 的多镜像 `runMirrors` 是**并行**的（谁先回谁赢），所以镜像选择本来就有随机性；
但这确实等于放弃了对它的比对 —— 一条**有意接受**的限制，别当成没看见。

**② 真差异（triage 完之前**不要**往白名单里塞）**
- `size_text` 舍入分歧：`"3.2 GB"` vs `"3.3 GB"`、`"4.2"` vs `"4.3"`、`"1.2"` vs `"1.3"`
- `date`：`rust = 1791676800000` / `"2026-10-11"` vs `node = null` / `"—"`
  ⇒ **Rust 产出了 Node 没有的日期**
- `date` 毫秒级差：`1768454947986` vs `1768454964886`
- `megapeer` 228 处里剩下那些（且 `magnet` 白名单条目这次**没触发** ⇒ 过期或判错）

**📊 triage 视图（2026-10-16，`m6-diff.yml` run 38849948966）—— 104 处其实只有 6 组、3 个根因。**

**12 家完全一致**：tpb / knaben / torrentscsv / yts / internetarchive / filemood /
audiobookbay / therarbg / torrent9 / oxtorrent / nyaa / zeromagnet。
`linuxtracker` 的 18 处**全部命中白名单**（detail_url 那条有意 divergence）⇒ 0 处未豁免。

未豁免的 104 处：

| (provider, field) | 处数 | 形态 |
|---|---|---|
| `megapeer.date` | **50** | rust `1791676800000` vs node `null` |
| `megapeer.date_text` | **50** | rust `"2026-10-11"` vs node `"—"` |
| `rutor.size_text` | 2 | `"3.2 GB"` vs `"3.3 GB"`、`"4.2"` vs `"4.3"` |
| `limetorrents.date` | 1 | `1768456238650` vs `1768456242896` |
| `limetorrents.size_text` | 1 | `"1.2 GB"` vs `"1.3 GB"` |

**🔎 三个根因（triage 进度：104 → 3）**

**A. `megapeer.date` / `date_text`（100 处）—— ✅ 已结案：不是移植 bug。**
两侧调的是**同一个函数**（`megapeer.js:51` 的 `ruDate` / `megapeer.rs:163` 的 `ru_date`），
差的是**输入**：megapeer 是 windows-1251 站点，日期格里的**俄语月份名**在 JS 侧被
axios 按 utf8 硬解成 U+FFFD ⇒ 月份匹配不上 ⇒ `null` / `"—"`；Rust 认 charset 头 ⇒
解析出 `2026-10-11`。
⇒ 这是**已登记**的「megapeer windows-1251」有意 divergence 的**连带损伤**，
我的白名单只是没收全。已补 `megapeer.date` / `megapeer.date_text` 两条；
同时**删掉**了 `megapeer.magnet`（被 compare 报「未触发」⇒ 过期条目，
这正是那条提示的用途）。

**B. `size_text` 舍入（3 处：`rutor` 2 + `limetorrents` 1）—— ⏳ 唯一剩下的真差异。**
形态：`"3.2 GB"` vs `"3.3 GB"`、`"4.2"` vs `"4.3"`、`"1.2"` vs `"1.3"` ——
**总是 Rust 比 Node 小 0.1**，而 **`size` 字段本身完全一致**（没进差异列表）
⇒ 纯粹是**格式化**的分歧，不是解析。

🔬 **精确假设（待验证）：Rust 的 `{:.1}` 用的是「就近取整、平局取偶」，JS 的 `Math.round`
是「平局向上」。** 以 `3.25` 为例：JS `Math.round(32.5)/10 = 3.3`；
Rust `format!("{:.1}", 3.25)` 因为 3.25 在二进制里是精确值 ⇒ 平局 ⇒ 取偶 ⇒ `"3.2"`。
**症状完全吻合**（差正好 0.1、且总是 Rust 小）。
🔧 若验证成立：Rust 侧应改成 `(x * 10.0).round() / 10.0`（`f64::round` 是**平局远离零**，
对正数与 `Math.round` 一致），**而不是**靠 `{:.1}`。
⚠️ 改的是 `bt-core` 的 `normalize`，影响**所有** provider 的 `size_text`
⇒ 改完必须重跑全量探针（`gh workflow run m6-diff.yml --ref main`）确认没有新的红。
⚠️ 验之前先确认那三行的**原始字节数**（`size` 已知，直接算一遍 3.25 那一档是不是真的平局）。

**C. `limetorrents.date`（1 处）—— ✅ 已结案：本质非确定性。**
证据：两次探针跑的值都不同
（`1768456238650`/`1768456242896` → `1768456467343`/`1768456468250`）。
页面给相对时间、provider 用当前时间换算 ⇒ 两侧各取一次 `Date.now()`，**必然**不同。
⇒ 既不修也不进白名单，而是进 **`compare.cjs` 的 `NONDETERMINISTIC` 表：只比形状**
（`(rust === null) === (node === null)`）—— 这样仍然抓得住「一边解析挂了」。
本机三条用例钉住语义：两个不同时间戳→放过；一边 null→**报**；形状同+别的字段不同→不受影响。

**⏳ 只剩一步**：修 B（3 处）→ 重跑探针确认 0 处 → 摘掉 `rust.yml` 的
`--json-only` 与 `--only`，16 家全部进门。

**🐛 这套 harness 自己的一个坑（值得记）**：`compare.cjs` 里有**两种键形式** ——
`key()` 生成 `provider\u0000field`（白名单 Set 用）、`provider.field`（非确定性表用）。
第一版给非确定性表也用了 `key()` 去查，**永远查不中 ⇒ 静默失效**
（两个不同的时间戳照样被判成差异）。本机三条用例当场抓出来了。
**教训**：键形式混用不报错、只静默失效 —— 而"静默失效"正好会**伪装成「差分通过」**，
是 M6 最不能出的问题。

**⏳ 顺序**：A（104→4）→ B（4→1）→ C（1→0）→ 摘掉 `rust.yml` 的 `--json-only` 与
`--only`，16 家全部进门。全量报告随时可用 `gh workflow run m6-diff.yml --ref main`
重新取（它是探针、不是门）。

**⚠️ 一个查不出来的风险**：两侧的 fixture 名是**两份手写表**，**写错不会报错**
（各自喂各自的 fixture、都"成功"）—— 只能靠人核对，已写进 `fixtures.cjs` 表头。

**三条硬约束，别调换**：**M1 必须最先**（Slint 挂了，M2 的架构就得重做）；**M6 必须早于删 Node**（否则「逐字段一致」永远无法再验）；**M5 早于补引擎**（现在没有理由盲搬）。

### 🌿 分支现状（2026-10-16 —— **`main` 已切换成 Rust 主线**）

> **2026-10-16 收尾核对（实测，不是推断）**：`feat/rust` 与 `legacy/node-electron`
> 相对 `main` 各有 **0 个独有提交**（`git rev-list --count main..<b>` = 0，
> `git diff --stat main...<b>` 空）⇒ 它们**已经没有任何独有内容**，
> 是纯指针。⚠️ **但本表明确写了"保留不删 / 存档"，所以这次只核对、不删除** ——
> 既定决策不该被一次"清理"顺手推翻。真要删，改这一行并说清理由再删。
>
> **远端**：`origin/{main, feat/rust, feat/tauri, legacy/node-electron, spike/slint-ui}` ——
> 与本地一一对应，`git remote prune` 无待清理项。
> **tag**：`v0.0.1` … `v0.3.6-beta` 共 10 个。⚠️ 其中 **`v0.3.3-beta` 有 tag 但从来
> 没有过 release**（`gh release view v0.3.3-beta` = not found）—— 历史遗留，
> **保留**（tag 是历史标记，删它比留着更容易让人困惑）。

| 分支 | 角色 | 状态 |
|---|---|---|
| `main` | **Rust 主线**（Node 版原地保留到 M6，D18） | ✅ **2026-10-16 已切**。切换点是 merge commit `7605e7a`；⚠️ 下表里的 tip / 测试数都是**切换当天**的快照，之后 main 一直在前进（当前状态看 `git log` / CI） |
| `legacy/node-electron` | Node/Electron 线的存档 | 停在 `9b9461c` = **切换前的最后一个纯 Node main**（就是存档该有的形态）。✅ 核对：相对 main 无独有提交 |
| `feat/rust` | 切换前的 Rust 线；合并后与 `main` 同源 | **保留不删**；合并点 = `082712a`。✅ 核对：相对 main 无独有提交 |
| `feat/tauri` | Tauri 壳（Node backend + node sidecar） | 旧线，仍可发版（`release.yml` 的 tauri 作业 checkout 它）。⚠️ 只有 3 条 workflow（build / release / tauri-build）—— 它**从没**同步过较新的那些（ci-guard / live-smoke / html-probes / provider-health / fetch-fixture / rust）。**这不是 bug**：实测这 3 条都不引用 `scripts/`，所以不存在 main 当年那种「workflow 在、脚本不在」的坏链。✅ 相对 main 有 69 个独有提交（Tauri 壳本体）⇒ **必须保留** |
| `spike/slint-ui` | M1 的 spike（`--selftest` + `slint-spike.yml`） | 一次性；**代码不在 `main` 上**（7239 行），别去那儿找。它的 workflow 只触发 `spike/slint-ui`（不挂 main）⇒ 不会因缺 `spike/` 目录而变红。M1 已通过、结论已留档（见「Slint 探针结论」），故**保留作证据** |

**✅ 切换记录（2026-10-16，全部实测，不是推断）**

**切之前**（三条独立核过）：
- `git merge-tree --write-tree main feat/rust` ⇒ **exit 0，零冲突**
- `main` **没有任何**顶层条目是 `feat/rust` 缺的 ⇒ **不丢文件**；
  `feat/rust` 只新增三个顶层条目：`.gitattributes`、`Cargo.toml`、`crates/`
- Node 家当在 `feat/rust` 上完整（`package.json` / `server.js` / `public/` /
  `electron/` / `build.yml` / `docs/RELEASE_ARTIFACTS.md` 全在）⇒ 合并后仍能出 Electron 包
- 版本号两边都是 `0.3.5-beta` ⇒ 不打乱发版

**切之后**（`main` = `7605e7a`，三个 workflow **全绿**）：
- **`Rust (test)` success** —— 触发已加 `main`（前一个提交 `082712a`）。
  这条是切换的**硬性配套**：不加的话切完 Rust 侧一条 CI 都不跑，
  而整套测试 + bt-app 全链路自检全靠它把关。
- **`Build Windows 发行包` success**，真产出 `BT-Search-Electron-Portable.zip`
  （151,416,455 字节）⇒ **根目录多了 `Cargo.toml` + `crates/` 之后 Electron 打包照旧**。
  这条之前只是"理论排除"（electron-builder 的 `files` 是白名单、
  `package.json` 无 `workspaces`、build.yml 不 glob 仓根），**现在是实测**。
- `CI guard (workflow files)` success

**⚠️ 维护规则改了**：`legacy/node-electron` **不再**跟随 `main` 的 tip ——
`main` 的含义已经变了，它就该停在 `9b9461c`。原规则（「始终指向 `main` 的当前 tip」）**作废**。

**⚠️ 换 `main` ≠ 可以删 Node 版**：D18 要求留到 M6 差分验证做完，
`server.js` / `public/` / `src/**` / `electron/` / `test/fixtures/` 一个都不能提前删
（§0.4 禁改清单）。main 现在是「**Rust 主线 + Node 版共存**」，这是**有意**的中间形态。

**2026-10-16 已修**：`main` 上 `provider-health.yml` / `html-probes.yml` 调用的
`scripts/provider-health.mjs` / `scripts/html-probes.cjs` / `scripts/gh-retry.sh`
**曾一直缺失**（两条手动触发的 workflow 在 `main` 上是坏的），已从 `feat/rust` 补齐；
`slint-spike.yml` 也从 `spike/slint-ui` 同步到了 `main`（手动触发要带 `--ref spike/slint-ui`）。
⚠️ 2026-10-16 之前 `rust.yml` **刻意不在 `main` 上**（那时 main 没有 `Cargo.toml`/`crates/`，
放过去只会是必失败的 workflow）—— **这条已经过期**：合并后它就在 main 上，且**必须**在。

**旧路线为什么被推翻**：它假设「能搜到东西」是目标，于是「复用 WebView 前端」看起来最省事。
但真实目标是**成品属性**（小 / 快 / 原生），而 WebView 路线**给不出那个成品**，还要多写一层
终局用不上的 HTTP + SSE（Slint 是进程内直调）。详见 `docs/ROADMAP.md` §0。

**Slint 探针结论（留档）** —— `spike/slint-ui` 分支与 `slint-spike.yml` 已于 2026-10-09 删除，
结论必须留着，否则 M1 要重新踩一遍。**重建成本约 500 行** —— 步骤、及格线、Plan B 见 `docs/AGENT_HANDBOOK.md` §1。

### ✅ M1 实测结论（2026-10-16）—— **#13548 未复现，Slint 可用**

CI run `38774543781`（branch `spike/slint-ui`，绿）。六条及格线逐条判定：

| # | 及格线 | 结果 |
|---|---|---|
| 1 | 200 条中文标题 + 高亮 + 自动换行全部渲染、不 panic | ✅ **布局覆盖 200/200**，全程未 panic |
| 2 | 关键词落在**第 2 行**也 OK | ✅ 构造覆盖（前缀 50..=109 逐字扫过软换行边界），未 panic |
| 3 | 关键词**正好压在换行边界**也 OK | ✅ 同上，扫描步长 1 个汉字 |
| 4 | 西里尔（真实素材） | ✅ **布局覆盖 50/50**，未 panic |
| 5 | 实测自包含 exe 体积 | ⚠️ **14.5 MB**（15,200,768 字节）—— 见下「体积对不上」 |
| 6 | `ListView` 虚拟化生效 | ✅ 2000 条 → **实测实例化 9 个** |

- 环境：slint **1.18.1**（2026-10-16 查 crates.io，**这就是最新版**，没有已修复的更新版）/ parley 0.11.1 / `SLINT_BACKEND=winit-software`（CI runner 无 GPU，femtovg 会挂）
- 方式：`slint-spike.exe --selftest` —— 真开窗口、真滚动、真渲染；判定 = **进程跑到底**
- ⚠️ **口径**：及格线 2/3 是**构造覆盖**（Rust 侧拿不到字体度量，无法反查行号），
  **不是**「实测关键词在第 2 行」。实测的是「这些情形会不会 panic」。
- 重建方式：代码从 git 悬空对象原样找回（`7d204f9` / `beaea27` / `74dde45` / `e50510c`，没被 gc）；
  新增的是 `src/m1.rs` 的 `--selftest` 与 workflow 的「真跑」步骤。

🔴 **最大的坑（第一轮就是这样被骗过去的）：只靠事件循环重绘 = 根本没重绘。**
第一版自检 440 步只花 **0.38s**（≈0.86ms/步）—— 全窗口 1180×800 的软件重绘不可能这么快。
`ListView` 的**实例化**确实发生了（行高固定 80px，建实例不需要量文本），所以「覆盖 200/200」是真的；
但 **parley 的 #13548 panic 发生在绘制期的文本布局里**，没重绘 = 没测到，那一轮的「绿」什么都证明不了。
**解法：每步调 `Window::take_snapshot()` 强制一次同步重绘**（它会重新渲染整个场景）。
本轮 580 次强制重绘全部成功。**以后凡是验 Slint 渲染路径，必须用这条，别信定时器。**

⚠️ **体积对不上 —— 旧数字「6.7MB」作废。** 同一条 `cargo build --release --locked` 这次实测
**14.5 MB**，是留档值的 **2.2×**，原因未查明（Rust 版本 / 依赖增长？`[profile.release]` 只有
`opt-level = 2`，**没开 lto、没 strip**）。S1/R2 的账要按 14.5MB 重算；真要压体积，先试
`lto = true` + `strip = true` + `opt-level = "z"` 再报数（本轮**没有**调 profile，报的是原始基线）。

（以下为 2026-10-07 spike 的原始留档，坑仍然有效）

- 产物体积是硬卖点：**6.7MB 自包含 exe，零运行时依赖**（对照 Electron 便携版 360MB / Tauri + node sidecar 89MB）
- ⚠️ `@markdown()` **会把插值转义**（官方原文 "Any text passed as an argument to the macro will be escaped"），
  所以 `@markdown("\{expr}")` 进去的内容永远是纯文本、不参与解析。运行时富文本必须：
  `.slint` 里属性类型写 **`styled-text`**（不是 `string`），Rust 侧 `slint::StyledText::from_markdown()`
- ⚠️ **别在 `init` 里写全局属性** —— 会形成属性依赖环，实测 200 条数据触发 **2628** 次实例化。
  计数要走 `callback` 进 Rust 的 `Cell`，显示属性只在用户点击时写
- ⚠️ **Slint #13548（1.18.0 / 1.18.1 均未修）**：内联样式跨软/硬换行、且换行后是多字节字符时
  parley 会 panic（`end byte index N is not a char boundary`）。最小复现 `@markdown("Это очень\nважно")`。
  **「中文标题 + 关键词高亮 + 自动换行」正中靶心 —— 这是选 Slint 的最大风险点，始终未能实测**
- `ListView` 自动虚拟化（元素只在可见时实例化，无需手动分页）；CJK 走 `default-font-family` + 系统回退
- `StyledText` 支持 `<font color>` / `<u>` / 斜体 / 删除线 / 行内代码 / 链接 / 列表；
  **没有 `wrap` 属性**；高亮色是烘进 markup 的，**切主题必须整体重建模型**

⚠️ **过渡期千万别做的事**：不要为了"干净"提前删 `public/` 或 `src/providers/`。
两者合计约 390KB，却是差分验证（M6）的对照物；删了等于凭记忆重写。

```
Cargo.toml              workspace 根（members = ["crates/*"]，exclude = ["src-tauri"]
                        — ⚠️ src-tauri 只在 feat/tauri 分支上，feat/rust 没有它）
crates/bt-core/         领域类型 + 归一化 + HTTP 公共层 + HTML 解析层
  src/normalize.rs      ← src/lib/normalize.js 的语义等价移植
  tests/normalize.rs    ← test/normalize.test.js 的断言原样搬来
  src/http.rs           ← src/lib/http.js 的移植（契约：永不返回 Err）
  tests/http.rs         ← 自起本地一次性 HTTP 服务，全程无外网
  src/dom.rs            ← cheerio 的替代层（scraper 0.27 = html5ever + selectors）
  tests/dom_probes.rs   ← 选择器语义对照：与 cheerio 真值（126 条 probe）逐条比对
crates/bt-providers/    一文件一站，对齐 src/providers/*.js 的组织方式  src/lib.rs            SearchOutcome { results, error, has_more }
                        （**聚合调度也放这里**；`Provider` trait 在 **M2** 建，见 `docs/ROADMAP.md` §2.4）
  src/value.rs          Value → NumOrText / String / min 的公共转换（含 v2nt_nonzero）
  src/tpb.rs            ← src/providers/tpb.js（GET，apibay）
  src/knaben.rs         ← src/providers/knaben.js（POST，官方 JSON API）
  src/torrentscsv.rs    ← src/providers/torrentscsv.js（GET，简单一层数组）
  src/yts.rs            ← src/providers/yts.js（GET，电影→种子两层结构）
  src/internetarchive.rs ← src/providers/internetarchive.js（GET，advancedsearch JSON）
  src/linuxtracker.rs   ← src/providers/linuxtracker.js（HTML，bt_core::dom）
  src/filemood.rs       ← src/providers/filemood.js（HTML，bt_core::dom）
  src/rutor.rs          ← src/providers/rutor.js（HTML，bt_core::dom；俄站 UTF-8）
  src/audiobookbay.rs   ← src/providers/audiobookbay.js（HTML；磁力惰性，resolve_magnet）
  src/therarbg.rs       ← src/providers/therarbg.js（HTML；size/date 在 data-order 属性里；resolve_magnet）
  src/limetorrents.rs   ← src/providers/limetorrents.js（HTML；info hash 在 itorrents 链接里；带真分页）
  src/torrent9.rs       ← src/providers/torrent9.js（HTML，法站；只取名字+详情链接，其余四格 JS 丢掉）
  src/oxtorrent.rs      ← src/providers/oxtorrent.js（HTML，法站；同上，错误串是 no_results）
  src/nyaa.rs           ← src/providers/nyaa.js（HTML；fixture 是 CI 抓的；⚠️ JS 列索引全错位）
  src/megapeer.rs       ← src/providers/megapeer.js（HTML；**windows-1251**，首个非 UTF-8 站点）
  src/zeromagnet.rs     ← src/providers/zeromagnet.js（HTML；名称被 <mark> 拆开；磁力在 input 的 value 属性）
  tests/common/mod.rs   本地一次性 HTTP 服务 + fixture 加载（provider 测试公用）
                        oneshot() 只要响应；oneshot_bytes() 可带 charset 头与原始字节（非 UTF-8 fixture 用）；
                        oneshot_capture() 另交出原始请求，用于钉请求契约
  tests/*.rs            每个 provider 一份离线测试（16 个）+ live_smoke.rs
  tests/live_smoke.rs   联网冒烟；**默认跳过**，只有 BT_LIVE_SMOKE=1 才打外网

（待建）`crates/bt-downloaders/`（4 客户端）· `crates/bt-app/`（**Slint 应用 —— 唯一的 UI**）
⚠️ `bt-torznab` **本期不建**（D17：只留抽象口子）；旧的 `docs/BT_APP_DESIGN.md` 已删
```

**移植进度**：

| JS 源 | Rust 目标 | 验收 | 旧 JS 状态 |
|---|---|---|---|
| `src/lib/normalize.js`（161 行） | `crates/bt-core/src/normalize.rs` | `test/normalize.test.js` 的断言全搬到 `tests/normalize.rs`，**27 passed** | 保留(对照) |
| `src/lib/http.js`（71 行） | `crates/bt-core/src/http.rs` | `tests/http.rs` 覆盖成功 / 4xx / 5xx / JSON / 非 JSON / 超时 / 连接失败 / 请求头，**12 passed** | 保留(对照) |
| `src/providers/tpb.js`（53 行） | `tpb.rs` | 真 fixture（100 条）逐字段 + 三条失败路径，**7 passed** | **待删** |
| `src/providers/knaben.js`（70 行） | `knaben.rs` | 真 fixture 逐字段 + 分类区间 + 请求 body 契约 + divergence，**9 passed** | **待删** |
| `src/providers/torrentscsv.js`（32 行） | `torrentscsv.rs` | 真 fixture 首末条 + query 编码契约 + falsy 日期，**9 passed** | **待删** |
| `src/providers/yts.js`（55 行） | `yts.rs` | 真 fixture 三条结果 + 请求 URL 契约 + 缺省值 + 两条跳过规则，**11 passed** | **待删** |
| `src/providers/internetarchive.js`（58 行） | `internetarchive.rs` | **合成** fixture（见下）+ 分类映射 + item_size 三态 + `no_docs` 错误语义，**14 passed** | **待删** |
| `src/lib/scraper.js`（HTML 解析底座） | `crates/bt-core/src/dom.rs` | cheerio 语义对照层 + 11 条单测 + **126 条 probe** 与 cheerio 1.2.0 真值逐条比对（`dom_probes` **通过**） | 保留(对照) |
| `src/providers/linuxtracker.js`（95 行） | `linuxtracker.rs` | 真 fixture：43 候选 → 18 条结果（三个数字都由 cheerio 交叉验证）+ 11 条集成 + 8 条单测 | **待删** |
| `src/providers/filemood.js`（74 行） | `filemood.rs` | 真 fixture：65 行 → 20 条数据行（cheerio 交叉验证）+ 11 条集成 + 11 条单测 | **待删** |
| `src/providers/rutor.js`（99 行） | `rutor.rs` | 真 fixture：101 行（含表头）→ 100 条（cheerio 交叉验证）+ 13 条集成 + 13 条单测 | **待删** |
| `src/providers/audiobookbay.js`（92 行） | `audiobookbay.rs` | 真 fixture：9 条结果 + 详情页（还原 info hash）+ 15 条集成 + 11 条单测（含上游 size/date bug 的钉住） | **待删** |
| `src/providers/therarbg.js`（91 行） | `therarbg.rs` | 真 fixture：39 条结果（size/date 走 `data-order`）+ 详情页磁力 + 18 条集成 + 7 条单测 | **待删** |
| `src/providers/limetorrents.js`（95 行） | `limetorrents.rs` | 真 fixture：41 行（含表头）→ 40 条（hash 从 itorrents 链接抠）+ 10 条集成 + 9 条单测（含两个上游 bug 的钉住） | **待删** |
| `src/providers/torrent9.js`（126 行） | `torrent9.rs` | 真 fixture：3 条结果（只有名字！）+ 详情页磁力 + 11 条集成 + 5 条单测（含「缺 h1 就不给磁力」的守卫） | **待删** |
| `src/providers/oxtorrent.js`（61 行） | `oxtorrent.rs` | 真 fixture：3 条结果（**无表头行**）+ 详情页磁力 + 12 条集成 + 4 条单测 | **待删** |
| `src/providers/nyaa.js`（64 行） | `nyaa.rs` | 真 fixture（**CI 抓的**）：1 条结果 + 8 条集成 + 6 条单测（把三处上游 bug 钉住） | **待删** |
| `src/providers/megapeer.js`（87 行） | `megapeer.rs` | 真 fixture（**CI 抓的**）：50 条结果（**windows-1251**）+ 详情页磁力 + 18 条集成 + 10 条单测（两处 divergence：编码 / 5-td 列语义） | **待删** |
| `src/providers/zeromagnet.js`（60 行） | `zeromagnet.rs` | 真 fixture（**CI 抓的**）：29 条结果（无表头）+ 详情页磁力（`input#input-magnet` 的 value）+ 14 条集成 + 4 条单测 | **待删** |

（另有 `bt-providers` 的 92 条单元测试（测 `src/value.rs` + `linuxtracker.rs` + `filemood.rs` + `rutor.rs` + `audiobookbay.rs` + `therarbg.rs` + `limetorrents.rs` + `torrent9.rs` + `oxtorrent.rs` + `nyaa.rs` + `megapeer.rs` + `zeromagnet.rs`）；
`tests/live_smoke.rs` 3 条默认跳过。合计 **338 passed / 0 failed**（2026-10-11 CI）。）

⚠️ `test/fixtures/internetarchive-ubuntu.synthetic.json`（文件名带 `.synthetic`）：
`docs[0..2]` 是 2026-10-08 从 CI 冒烟日志取回的**真实 doc**，`docs[3..]` 是手工构造的边界样本；
整体**不是逐字节快照**（本机对 `archive.org` 有 DNS 污染 —— 连续解析得到不同假 IP，
IPv6 落在 `2a03:2880:face:b00c` 黑洞前缀，直连 / 代理 / DoH / 中转全失败；
对照站点 torrents-csv.com 解析正常，说明不是网络整体不通）。
真实响应已验证：冒烟取回 97~98 条、`error=None`，字段名与类型与 fixture 一致
（真实数据里第一条就**没有 btih**，说明"缺 btih 就跳过"确实会命中）。
**换成完整真快照的条件**：在能访问 archive.org 的环境抓一次（或改进冒烟让它落盘成 artifact，
但 `gh run download` 在本仓库会卡死），然后删掉 fixture 里的 `_synthetic` 字段、
把文件名去掉 `.synthetic`（`fixture_is_still_marked_as_synthetic` 会逼你这么做）。

### ⚠️ 边搬边删（用户 2026-10-08 指定，2026-10-08 二次修订）

**原则（修订后 —— 别死板按文件删）**：

1. **以「功能」为粒度，不以「文件」为粒度。** 一个功能在 Rust 侧完整可用（含被
   `bt-app` 真正接通）之后，才考虑删它对应的旧 JS。
2. **删了会影响后续的，就先不删。** 判断标准：这个 JS 是否还在被别的东西引用/对照？
   是 → 留着，登记到下面的「待删清单」，阶段收尾时批量删。
3. **删之前必须登记。** 不登记就删 = 以后没人知道哪些该删。
4. 唯一红线不变：**不允许两套实现长期漂移**。所以每搬完一块，必须在下面的
   「移植进度」表里标注该 JS 的状态（`待删` / `保留(对照)` / `已删`）。

**为什么放宽**：验收点之一是「Rust 版结果与 Node 版**逐字段一致**」（= M6 的差分，见
`docs/ROADMAP.md` §3），Node 源码在这里是**验证工具**而不是待清垃圾。提前删掉 = 亲手毁掉对照物。

**待删清单（收尾时批量处理 —— "何时可删"以下表为准）**：

| 路径 | 为何现在留着 | 何时可删 |
|---|---|---|
| `src/providers/tpb.js`、`knaben.js`、`torrentscsv.js`、`yts.js`、`internetarchive.js`、`linuxtracker.js`、`filemood.js`、`rutor.js`、`audiobookbay.js`、`therarbg.js`、`limetorrents.js`、`torrent9.js`、`oxtorrent.js`、`nyaa.js` ✅已移植 | `test/run.js` 仍在跑 tpb/knaben/linuxtracker/filemood/rutor；离线对照要用 | `bt-app` 接通对应 provider + Node 测试块删除后 |
| `src/providers/*.js`（其余 28 个） | 尚未移植，是逐行对照的参照物 | 各自移植完成后按上条判断 |
| `src/lib/`、`server.js`、`test/run.js` | 聚合层尚未移植，本分支上 Node 版仍需可跑 | **聚合层跑通**（= 切 `main` 的门，口径 C）之后 |
| `public/` | Node 版的前端本体（D8 直接上 Slint，**不再有"阶段二"**；Slint 版能用后它就无用） | **M6 差分验证做完**（与 Node 版一起删，D18） |
| `electron/`、`scripts/`、`package.json`、`start.bat`/`stop.bat`、`.eslintrc.json`、`.prettierrc`、`build.yml`/`release.yml`/`tauri-build.yml` | Rust 版尚未能取代旧 Shell 发版 | Rust 版能独立发版后 |

**已删（已做）**：构建产物 + 依赖缓存（`node_modules`、`src-tauri/target`、`src-tauri/binaries`）→ 省 1.35GB；
`public/app.js`（**1634 行 / 69KB 死代码**，2026-10-11 —— `index.html` 只加载 `js/main.js`，
`build-frontend.js` 也只读 `public/js/*.js`，全仓零功能性引用；已同步到 main + feat/tauri）

**永久保留**：`test/fixtures/`（迁进 Rust 测试后也不删，是 golden 数据源）、
`LICENSE`、`SEARCH_ENGINE_PORT_COVERAGE.md`

⚠️ 反过来也要守：**别提前删还没搬的源码**。`src/providers/` + `public/` 一共才 460KB，
却是逐行对照的参照物；删了等于凭记忆重写。

### 📌 下一步计划（2026-10-08 用户定，**2026-10-11 改向**）

用户口径：**Q2 选 A → B，先只记录，不动手。**

推荐顺序 **A（纯 JSON 组）→ B（HTML 组）**，理由：JSON 组零解析风险、每个都快，
能快速把 provider 的组织形态（crate 内一文件一站、`tests/common/mod.rs` 复用）打磨稳；
HTML 组是唯一有「选择器语义可能与 cheerio 不一致」风险的地方，值得等前面都稳了再集中攻。

**A 组（纯 JSON API，无 HTML 解析）—— ✅ 全部完成**

| 顺序 | provider | 备注 |
|---|---|---|
| 1 | ~~`tpb.js`~~ | ✅ 7 tests |
| 2 | ~~`knaben.js`~~ | ✅ 9 tests |
| 3 | ~~`torrentscsv.js`~~ | ✅ 9 tests |
| 4 | ~~`yts.js`~~ | ✅ 11 tests |
| 5 | ~~`internetarchive.js`~~ | ✅ 14 tests（fixture 是合成的，见进度表上方说明） |

**✅ M2 已完成，这一节是"开工前"的勘明记录** —— `②`/`⑨` 那两条待定也在 M2 当轮拍定了。

原计划「继续搬 provider 直到 40 个齐」**已废止**（D13：剩余 24 个站点里 5 个确认死亡、
约 8 个两边环境都不通；而且「引擎多」只是感觉，不是需求）。
旧路线「阶段一 = 核心 + WebView」**也已作废**（D8/D9）—— 真正的目标是「小快原生的成品」。

**✅ M1 已完成（2026-10-16）：Slint #13548 未复现。**
六条及格线全部判定通过（覆盖率 + 580 次强制同步重绘 + 实测体积），
结论与踩坑记录见本文件上面的「Slint 探针结论（留档）」。
⇒ **架构不必重做**，`docs/ROADMAP.md` 的三条硬约束里「M1 必须最先」这条已经兑现。

**✅ M2 已完成（2026-10-16）—— 聚合层 + `Provider` trait 落地，CI 347 tests / 0 failed。**
三条 JS 语义逐条复刻并有独立验收（语义① 三条过滤 + `?providers=` 三个边界；
语义② **先回调再起下一个**（事件序钉死）+ 单站失败隔离 + 每站单独计时；
语义③ 真值表 8 条 + 端到端「paginated 的 provider 也能明确否决 has_more」）。
🔴 **D5 说的「切 `main` 的门 = 聚合层跑通」到此已经满足** —— 换不换、什么时候换是你拍板；
配套清单（merge 而非 reset、给 `rust.yml` 触发加 `main`、一个文件都不删）见本文件「🌿 分支现状」。

**✅ M3 已完成（2026-10-16）—— `bt-downloaders` 落地，4 客户端 / 34 条离线测试，CI 382 tests。**
`crates/bt-downloaders`：qBittorrent / Transmission / aria2·Motrix / Gopeed，
对 `src/lib/downloaders.js` 移植。**刻意不依赖 bt-core**（那层是抓第三方页面的，
这层是本机受信下载器：Basic Auth / 会话 cookie / JSON-RPC / 409 重试 / 失败靠 `Err`）。
两个手册点名的坑各有用例钉住：**qB 的 SID 要在全部 set-cookie 里找**（+只把 403 当凭证错）、
**`safeHttpUrl` 只放行 http/https**（放库里 + push/test 内部各再查一次）。
另外三处照抄容易改错的：axios 对非 2xx 会抛而 reqwest 不会（**必须自己查状态码**）、
axios 把解析不了的正文退化成字符串 ⇒「2xx + 非 JSON」算**成功**（软检查）、
JS 真值语义（`aria2` 的 `error: 0/""/false` 都是假）。详见 commit `c3fe36a`。

**🟢 M4 已完工（2026-10-16）—— 见下面的「M4 完成」块。**
`ui/app.slint`（**唯一的 UI**）+ `src/main.rs`（接线）+ `src/highlight.rs`（从 M1 spike 原样搬来）
+ `src/ui_model.rs`（纯映射层，**不 import slint**，11 条单测）。317→397 条测试里
新增 15 条，全在 `ui_model` / `highlight` 上 —— GUI 渲染路径 CI 测不了，但那两层的**行为**
可以，所以特意把它们从接线里拎出来。

**🔴 M4 抓到一个 M2 埋下的真缺陷 —— 这条最值得记：`BoxFuture` 少了 `+ Send`。**
三层各自 CI 全绿（338 → 382 tests），但第一次把它们**拼起来**就编译失败：
`future cannot be sent between threads safely`，因为 GUI 必须把搜索丢到 `tokio::spawn`
（要求 `Future + Send + 'static`），而 M2 把 `BoxFuture` 定义成
`Pin<Box<dyn Future + 'a>>` —— M2 当时**刻意**不加 `Send`，理由也写在那儿了
（「聚合层在当前任务上驱动，不需要 Send；将来要改成 tokio::spawn 时再加，那时
得逐个确认 provider」）。**那个「将来」就是 M4。** 逐个确认的结果：16 家全是
reqwest + `bt_core::dom`，`async` 体里没有 `Rc`/`RefCell`，所以是**纯放宽、一个
provider 都没改**；同一个要求传播到四处泛型约束（`BoxFuture` / `run_mirrors` 的
`F`+`Fut` / `async_pool` 的 `F`+`Fut`+`on_settle` / `search_stream` 的 `on_provider`）。
**教训：接口级问题只有"拼起来"才会暴露 —— 这正是 M4 早于 M5/M6 的价值。**

**Slint 又踩了两个新坑（都已修，别退回去）：**
1. **组件内的 `root` 指组件自己，不是窗口。** `ResultCard` 里直接写
   `root.push-row(...)` 编译不过 —— 必须在组件上开 `callback push(int)`，
   再在 `ListView` 的实例化处 `push(idx) => { root.push-row(idx); }` 转发。
2. **`VecModel` 的 `row_count` / `set_row_data` 是 `Model` trait 的方法**，
   不是固有方法 —— 少 `use slint::Model;` 会报
   `no method named row_count found for struct Rc<VecModel<..>>`。

**线程模型（`docs/ROADMAP.md` §2.2 点名"最容易写错"）已按要求落实**：
后台任务绝不直接改 UI 属性（数据装进 `invoke_from_event_loop` 回 UI 线程）；
后台闭包**不能捕获 `Rc`**（回调要求 `Send + 'static`）⇒ UI 侧可变状态放
**thread_local**，闭包只捕获「拥有所有权的数据 + `Weak<AppWindow>`」；
**每站一回来就更新**且列表模型**增量 `push`**（不是整块重建 —— 否则每来一站都会把
用户的滚动位置顶回去）。回调里用 `as_weak()` 而不是 `clone_strong()`：
回调存在组件内部，强引用会成环、组件永不释放。

**✅ M4 完成（2026-10-16）—— `--selftest` 全链路自检 12/12 通过；CI 397 tests + 自检都是硬门。**

> ⚠️ **本文件里出现的测试数都是「当时」的快照**（397 是 M4 完成那天；
> 2026-10-16 收尾时已 **471 条**）。要现在的数，看最近一次 `rust.yml` 的日志 ——
> 别把历史快照当成现状（这是"文档过时比缺失更坏"的又一例）。
自检走**和真程序同一条接线**（把 `main()` 里的回调抽成 `wire(ui)` 共用 —— 否则验的是
一个平行实现，等于没验），只换数据源：假注册表（alpha 3 条**无**磁力 / beta 2 条
**自带**磁力 / gamma **故意失败**）+ 本地 qB 桩。
于是 `搜索 → 每站状态 → 行模型 → 惰性取磁力 → 推送到下载器` 真的跑了一遍。

**🔴 自检证明了自己的价值：它抓到两个「编译期 / cargo test / clippy 全绿也发现不了」的运行期 bug。**

1. **回调里裸用 `tokio::spawn` 会 panic。** Slint 的事件循环线程**不在**任何 tokio
   运行时上下文里 ⇒ 真程序**一点搜索就崩**（`there is no reactor running…`）。
   修法：自己持有运行时，用 `spawn()` 包装。
2. **`enable_all()` 要靠 tokio 的 `net` 特性才会打开 IO driver**，少了它 reqwest 在
   运行期报 `IO driver not enabled` —— 同样编译期不报。bt-app 的 tokio features 补了 `net`。

**自检还抓到两条「断言自己写错」**（说明它不是摆设，真的会 FAIL）：
① 拿**未编码**的磁力去 match 请求体（体里是 `encodeURIComponent` 之后的）；
② 用 `ticks > 3` 这种**魔数**钉重绘 —— 链路快时 3 跳就跑完，那种写法本身就是 flaky，
改成钉「每跳都重绘成功 && 走完三个阶段」。

**防假绿**（M1 的教训）：每跳调 `Window::take_snapshot()` **强制同步重绘并计数**，
要求 `paints_ok == ticks && paints_err == 0 && ticks >= 3`。
**provider 线仍然后置**（D13），不是主推进方向。

### 📌 M2 开工前勘明（2026-10-16）—— **已执行完毕，留档备查**

> ⚠️ 下面是**开工前**的勘明，代码已按它写完（commit `b91546d`）。
> 保留它是为了：① 那几条「推翻了既有文档」的结论需要出处；② ⑨ 里两条待定还挂着。
> 与最终实现的差异只有一处：⑧ 里说宏「可选重复」有风险 → 实际写成**两条宏臂**。

勘察方式：读 `src/providers/index.js`（271 行，**唯一真值来源**）+ 现有 Rust 基座。
下面每条都是**实测/实读**的，不是回忆。

**① 真值来源与三条语义的位置**（`src/providers/index.js`）
| 语义 | 行 | 备注 |
|---|---|---|
| `resolveTargets` 三条过滤 | 147–158 | 第 3 条：`page > 1 && !p.paginated && !p.demo` |
| `runProvider`（计时 + 隔离） | 163–183 | 注意 catch 分支**没有 `hasMore` 键** |
| `asyncPool` | 204–219 | ⚠️ 见下面第 ③ 条 |
| `aggregateHasMore` | 190–198 | 三行判断，`typeof s.hasMore === 'boolean'` 是分水岭 |
| `MAX_CONCURRENT = 8` | 223 | |

**② 🔴 `has_more` 必须是 `Option<bool>`** —— 这是本轮最重要的发现
- Rust 现状：`SearchOutcome.has_more: bool`，而**全仓只有 3 处**引用它
  （`lib.rs` 的字段声明 + `ok()`/`err()` 两个构造），**16 个 provider 没有一个设过 `true`**
  —— 它是个死字段。爆炸半径实测 = 改 3 行，**测试里 0 处引用**，`SearchOutcome::ok/err`
  的 66 个调用点全都不受影响（构造器不收这个参数）。
- JS 侧：只有 `demo.js` 显式返回 `hasMore`；其余 16 家是 `undefined`。
- 而 `aggregateHasMore` 里 **`undefined` 与显式 `false` 走不同分支**：
  前者回退到 `!!p.paginated`，后者是 provider 的明确否决。
  ⇒ 用 `bool` 会在「provider 说 false 但它是 paginated」时给出**错的** `hasMore`。
  ⇒ **`Option<bool>`：`None` = 没表态。**

**③ ⚠️ 章节 §2.1② 的「onSettle 是唯一原因」说法要更正**
`asyncPool(tasks, limit, onSettle)` 的 **`onSettle` 参数在两个调用点都没被传**
（都只传 2 个参数，是死参数）。流式的真实来源是每个 task 自己链上的
`.then(...)`：provider 一 resolve 就立刻写 `perProvider` / `allResults` / 调 `onProvider`。
⇒ 验收测试钉「**provider 一返回就上报，不等慢的**」；池子那层单独钉「同时在飞 ≤ 8」。

**④ JS 的「没有 resolver」不是 `no_resolver`**
`server.js` 里是 **HTTP 404 + `{ error: 'no resolver' }`**（**有空格**）——
它是**端点级**行为，不是 provider 返回值。ROADMAP §2.4 草案写的 `no_resolver` 与 JS 不符。
⇒ Rust 侧没有 HTTP 层，收敛成 trait 默认方法的返回值，字符串**取 JS 原文 `"no resolver"`**。

**⑤ `providers` 参数的边界比想象中怪**（`?providers=a,b`）
| 输入 | JS 结果 |
|---|---|
| `null` / `""` | `wanted = null` → **全都要**（空串是 falsy） |
| `"  "`（纯空白） | `wanted = []` → **一个都不查**（不是"全都要"！） |
| `"a,,b"` | `["a","b"]` |

**⑥ 崩溃路径的形状**：JS catch 分支的 status 是
`{ status:'error', count:0, error: e.message || 'crash', ms }` —— **没有 `hasMore` 键**
⇒ Rust `has_more: None`、`error: Some("crash")`（JS 的兜底串就是 `'crash'`）。

**⑦ 16 家已搬站点的 id / name / 旗标（逐字核对过，`module.exports` 在 JS 文件末尾）**

| id | name | paginated | browseable | resolve_magnet |
|---|---|---|---|---|
| `tpb` | The Pirate Bay | | | |
| `nyaa` | NYAA | ✅ | | |
| `yts` | YTS | | ✅ | |
| `knaben` | Knaben | | | |
| `torrentscsv` | TorrentsCSV | | | |
| `limetorrents` | LimeTorrents | ✅ | | |
| `therarbg` | TheRarBg | | | ✅ |
| `rutor` | Rutor | ✅ | | |
| `torrent9` | Torrent9 | | | ✅ |
| `zeromagnet` | ZeroMagnet | | | ✅ |
| `oxtorrent` | OxTorrent | | | ✅ |
| `audiobookbay` | AudioBookBay | | | ✅ |
| `filemood` | FileMood | | | |
| `internetarchive` | InternetArchive | | | |
| `linuxtracker` | LinuxTracker | | | |
| `megapeer` | MegaPeer | | | ✅ |

⚠️ **注册表顺序** = JS `REGISTRY` 里这 16 个的相对顺序（`1337x` 等未搬的要跳过、
但**别重排**）——UI 显示顺序就是它。详见 `src/providers/index.js:79-121`。

**⑧ 设计定案（可以直接照这个写，但有两条待定，见下）**
- `Provider` trait：`Provider: Send + Sync`，方法用 **boxed future**
  （`Pin<Box<dyn Future + 'a>>`）而不是 `async fn in trait` —— **因为 AFIT 让 trait
  不能做 trait object**，而 D17 要留的 Torznab 口子正是「运行时插入带状态的
  provider（URL + API key）」⇒ 必须是 `Box<dyn Provider>`。**不加 `Send`**（与
  `run_mirrors` 的 `Fut` 约束一致，聚合用 `FuturesUnordered` 在**当前任务**上驱动）。
- 模块：`aggregate.rs`（三条语义 + 池）、`registry.rs`（16 家 impl + `registry()`）。
  ⚠️ 16 家的 impl 集中放 `registry.rs`、**不改 16 个 provider 文件** —— 理由：
  本机无 `cargo check`，新代码放新文件能把编译风险圈在"不会弄坏已有 16 个测试"里。
- 依赖：`bt-providers` 需新增 **`futures = "0.3"`**（`FuturesUnordered` / `StreamExt`）。
  `tokio` 只需留在 `[dev-dependencies]`（聚合层本身不需要运行时）。
  实测：`futures` 目前**不是**任何 crate 的依赖；根 `Cargo.lock` 未入库，CI 会现场生成。
- 状态形状：`ProviderStatus { status: ProviderState, count, error, has_more, ms }`，
  `ProviderState` 是 enum 但带 `as_str()` 回 `"ok"/"error"`（M6 差分要用）。
- 返回：`providers` 用 **`Vec<(String, ProviderStatus)>` 而不是 map** —— JS 的
  `perProvider` 是对象，**key 顺序 = provider 完成顺序**（`asyncPool` 按谁先 resolve 落库）。
  用 `BTreeMap` 会把这个顺序丢掉。`ms` 用 `Instant::elapsed().as_millis()`。

**⑨ 两条待定 —— ✅ 两条都已结案（M2 当轮拍定）**
1. **要不要 `catch_unwind` 兜住 provider 的 panic？** JS 的 `try/catch` 连
   "抛异常"也兜；Rust 的 provider 契约是「永不 panic」，所以 `SearchOutcome.error`
   已经覆盖了**错误**路径。但若某个 provider 违约 panic，Rust 会**带走整个聚合**
   （JS 不会）。加它需要 `AssertUnwindSafe(...).catch_unwind()`，而本机无 cargo、
   没法验证这个 API 组合 ⇒ **建议下一轮单独做、单独测**，别和主体混在一起。
2. `Provider` trait 是否要暴露 `enabled()`？JS 的 `resolveTargets` 第一条就是
   `if (!p.enabled ...)`，但 REGISTRY 里 16 家全是 `true`，用户级开关走 `wanted`。
   建议**建模**（默认 `true`），否则第 1 条语义少一个分支、测不全。

⚠️ **历史记录（2026-10-11 那次会话结束时的状态，早已不成立）**：当时 M2 **代码未写**，工作区干净，
上面 ⑧ 的定案可以直接落笔。下次开工：写 `Cargo.toml` + `lib.rs`（`Option<bool>` + trait）
+ `aggregate.rs` + `registry.rs` + `tests/aggregate.rs`，然后 `rustfmt --check` 后推 CI。

**进度存档（2026-10-11）**：`audiobookbay`…`nyaa` 9 个 + **`megapeer`（第 11 个）**
+ **`zeromagnet`（第 12 个）**都已搬完。⭐ 本机能抓到的候选早已用完；剩下站点的
fixture 靠 CI 抓（`fetch-fixture.yml`），探针链路支持 **per-fixture 编码声明**
（条目里的 `encoding` 字段，megapeer 是第一个 windows-1251）。

**候选状态（2026-10-11）**：

| 候选 | 状态 |
|---|---|
| `dmhy` | ⏸️ **暂缓** —— CI 抓取连试 3 次全部 60s 超时（0 字节；主页也一样）；agent 沙箱直连也 `fetch failed`。站点自己挂了/在维护，**不是**「本机不通但 CI 通」那一类。过些天再试 |
| `zeromagnet`（CI 29 行） | ✅ 已搬完（2026-10-11） |
| `xxxtracker`（CI 1 行） | ⏸️ provider 线最前，但 **provider 线整体后置**（D13：先摸存活再砍；当前里程碑是 **M1 验 Slint**，不是再搬一个） |
| `sukebei` / `nekobt` | 排队（域名是死是活待确认） |

⚠️ **B 组序号口径**：序号把 `#0 地基` 与 `#10 1337x（跳过）` 都算进去了，
所以「B 组第 12 个」= `zeromagnet` = **第 11 个真搬的 HTML provider**（commit message 里也是这个口径）。
**别把序号当数量**：HTML 侧实际只搬了 11 个，全仓合计 16 个。

抓 fixture 的姿势（本机不通的站点，`zeromagnet` 即为例）：

```bash
scripts/gh-retry.sh workflow run fetch-fixture.yml --ref feat/rust \
  -f url='https://9mag.net/search?q=ubuntu' \
  -f path='test/fixtures/zeromagnet-ubuntu.html'
# 20 秒后 pull 回来，按「探针流程」继续
```

**B 组（HTML 抓取，需引入 `scraper` crate 对标 cheerio）** —— ✅ 地基 + 11 个 provider 完成

- ✅ `crates/bt-core/src/dom.rs`：cheerio 的替代层（`scraper` 0.27 = html5ever + selectors）
  语义对照表写在文件头；自带 11 条单元测试
- ✅ 选择器等价性对照机制：`test/fixtures/html-probes.json`（**126 条 probe / 17 个 fixture**）+
  `scripts/html-probes.cjs`（cheerio 侧，真值来源）+ `crates/bt-core/tests/dom_probes.rs`（Rust 侧）
  + `test/fixtures/html-probes.expected.json`（真值，cheerio 1.2.0 生成）
  ⚠️ **别给大页面加 `text_first` on `html`**（整页 text 拼接的超长单行）—— 实测会让 CI 日志
  从那一行起被截断，后面的 probe 真值全拿不到（2026-10-10 在 therarbg 上踩过）。
  整页拼接的语义已有 rutor / audiobookbay 两条覆盖，不必每个 fixture 都加。
  ⚠️ **非 UTF-8 站点**（megapeer=windows-1251）：条目里加 `"encoding": "windows-1251"` ——
  cheerio 侧走 `TextDecoder(label)`、Rust 侧走 `encoding_rs::Encoding::for_label(label)`；
  fixture 存**原始字节**（别转码），两个解码器对同一标签必须给出一致文本。
- ✅ `linuxtracker.js` + `filemood.js` + `rutor.js` + `audiobookbay.js` + `therarbg.js` + `limetorrents.js` + `torrent9.js` + `oxtorrent.js` + `nyaa.js` + `megapeer.js` + `zeromagnet.js` 十一个 HTML provider 落地，全部离线可测

**B 组进度**

| # | provider | 状态 |
|---|---|---|
| 0 | 地基（`dom.rs` + 选择器对照机制） | ✅ **126 条 probe** 与 cheerio 1.2.0 逐条一致 |
| 1 | `linuxtracker.js` | ✅ 18 条结果 + 11 集成 + 8 单测 |
| 2 | `filemood.js` | ✅ 20 条结果 + 11 集成 + 11 单测 |
| 3 | `rutor.js` | ✅ 100 条结果 + 13 集成 + 13 单测（**本机与 CI 两边都 ✅ 的第一个**） |
| 4 | `audiobookbay.js` | ✅ 9 条结果（**两边都 ✅**）+ 15 集成 + 11 单测；本项目**第一个带惰性 `resolve_magnet`** 的 provider |
| 5 | `therarbg.js` | ✅ 39 条结果（两边都出结果；CI 那轮 403，属机房 IP 被拦）+ 18 集成 + 7 单测；第二个 `resolve_magnet` |
| 6 | `limetorrents.js` | ✅ 40 条结果（本机 ✅ 45 行 / CI ⛔403）+ 10 集成 + 9 单测；第一个**带真分页**的 HTML provider |
| 7 | `torrent9.js` | ✅ 3 条结果（本机 ✅ 4 行 / CI ⛔403）+ 11 集成 + 5 单测；⚠️ 卡片上除名字外全是 `—`（JS 把同页的日期/大小/peers 全丢了） |
| 8 | `oxtorrent.js` | ✅ 3 条结果（本机 ✅ 3 行 / CI ⛔403）+ 12 集成 + 4 单测；无表头行；错误串是 `no_results` |
| 9 | `nyaa.js` | ✅ 1 条结果（CI ✅ 2 行，**本机不通 → fixture 由 CI 抓**）+ 8 集成 + 6 单测；⚠️ 三种上游 bug（见下） |
| 10 | `1337x.js` | ⛔ **已调查，当前不可抓取 → 跳过**，见下面专门小节 |
| 11 | `megapeer.js` | ✅ 50 条结果（**CI ✅ 50 条、charset 解码实测通过**）+ 18 集成 + 10 单测；**第一个 windows-1251 站点**、两处有意 divergence（编码 / 5-td 列语义），见下面专门小节 |
| 12 | `zeromagnet.js` | ✅ 29 条结果（CI ✅29，fixture 由 CI 抓）+ 14 集成 + 4 单测；⚠️ 名称被 `<mark>` 拆开→拼接；meta 列（6.2GB/日期）白扔（照抄）；磁力在详情页 `input#input-magnet` 的 **value 属性** |
| — | 俄站编码 | ✅ 已验清：`rutor` 是 UTF-8（`getWin1251()` 名误导）；**`megapeer` 真的是 windows-1251**（响应头带 charset） |

### 🔤 `megapeer.js`（B 组第 11 个）—— 第一个 windows-1251 站点，两处有意 divergence（2026-10-11）

**1. 编码：Rust 修了 JS 的乱码 bug（有意，不是回归）**

- 站点响应字节是 **windows-1251**（响应头确实带 `charset=windows-1251`，2026-10-11 CI 冒烟实测）
- **JS 版是坏的**：axios 1.7.x 对文本响应一律 `buffer.toString('utf8')`（源码写死，不读 charset）
  → 所有西里尔标题变 `U+FFFD`。Rust 的 reqwest `res.text()` 读 Content-Type 的 charset
  （encoding_rs）→ 正确解出俄语
- 判断依据：这是**数据级**错误（且「照抄乱码」反而要在 Rust 里刻意写 lossy 才能复现）→ 修
- 测试钉住：`divergence_js_style_utf8_decoding_would_mangle_the_cyrillic`（同时展示
  `from_utf8_lossy` 的 � 版与正确解码版的对照）

**2. 5-td 行的列语义：JS 固定列索引取错（有意修）**

- 50 行里 **7 行多一格评论数**（恒在 td3）：`[日期][名称][评论][大小][peers]`；其余 43 行是
  `[日期][名称(colspan=2)][大小][peers]`
- JS 的 `td:nth-child(3)` 当 size、`td:nth-child(4) > font` 当 peers → **5-td 行的 size 变成评论数、peers 落空**
- Rust 改成「**倒数第 2 格 = size、最后一格 = peers**」（两种行都对；rutor 的「按内容找 size」是同类问题的另一种解法）
- 探针（`sizes.texts_first_5` 等）就是修正后的语义；`divergence_5td_row_takes_real_size_not_the_comment_count` 钉住

**基础设施（服务非 UTF-8 站点，后续别的编码站点直接用）**：

- `html-probes.json` 条目新增可选 `encoding`；`html-probes.cjs` 用 Node 内置 `TextDecoder`（零依赖）
- `tests/common/mod.rs`：新增 `fixture_bytes()`（原始字节）+ `oneshot_bytes()` / `oneshot_capture_bytes()`
  （Content-Type 可指定 —— 带 `charset=windows-1251` 发给本地服务，走 reqwest 真解码路径）
- live_smoke 新增 `smoke_megapeer_checks_real_world_charset_decoding`（实测结果见上）

⚠️ **push 前跑一遍 rustfmt 能省一轮 CI**：本机 `.rustup` toolchain 里的 `rustfmt.exe` 一直可用
（见 CLAUDE.md），`--edition 2021 --check <files>` 后再推 —— 这次就是靠它做到 fmt 门从不红。

### ⛔ `1337x.js` —— 2026-10-09 调查完毕，**跳过**

**结论：1337x 用普通 HTTP 客户端拿不到结果页** —— 不是选择器的问题，也不是本移植引入的
（**JS 版在同样环境下一模一样地失败**）。全部候选域名实测：

| 域名 | 结果 |
|---|---|
| `1337x.to` / `1337x.is` / `x1337x.se` | **DNS 不返回 A 记录**（本机 `nslookup` 只回路由器地址 + 污染 IPv6；对照 `torrents-csv.com` 正常解析到 51.15.62.20） |
| `1337x.st` / `x1337x.ws` / `x1337x.eu` / `x1337x.cc` | **Cloudflare 挑战页**：403 + 5.5KB，`Just a moment... Enable JavaScript and cookies to continue` |
| `13377x.to`（旧 fixture 里那个跳转域名） | **已变成域名停放广告页**（`mode:"iframe"` → `yfdpco5.com/sk-park.php`） |
| `1337x.tw` | 301 → `www.1337x.tw` → 404 |
| `1337x.net` / `1337x.am` / `1377x.is` / `1337x.piratic.org` / `1337x.pages.dev` | 停放页 / 空响应 / 不可信的"买流量"页 |
| `1337x.proxyninja.org` / `1337x.torrentsbay.org` | 同样撞 Cloudflare 挑战页 |

⚠️ **顺带修正一条旧认知**：`test/fixtures/1337x-ubuntu.html` 里那个 FingerprintJS 挑战页
**当年其实是可以跟的** —— 它只是在同域 URL 上加 `fp=<指纹>` 重定向，而且页面自带
`<a href="...&fp=-3">Click here to enter</a>` 这个**给非 JS 客户端的降级入口**，
跟着那个隐藏链接走就绕过去了。但**那个域名现在已停放**，所以这条路彻底断了；
今天挡住我们的是 **Cloudflare + DNS 级封锁**，机制与当年完全不同。

**要继续，可选路径**（按成本排序，都需要用户先点头）：
1. **换个能访问的环境抓一次 fixture** —— 成本最低。但 Cloudflare 对机房 IP 更凶，
   在 `live-smoke.yml` 里试一次可能也拿不到
2. **第三方 1337x API / 代理服务** —— 引入外部依赖 + 信任问题，与原设计（自抓 HTML）不符
3. **浏览器自动化过 Cloudflare** —— 本机 Playwright 不可用（Node 不能 spawn 子进程），
   只能走 CDP 启 Chrome；给 app 引入重量级依赖，且 Cloudflare 对自动化浏览器同样会拦
4. **放弃这个引擎** —— 项目已有 40+ 引擎，1337x 并非不可替代

**决定：先跳过**（用户 2026-10-09 拍板）。`src/providers/1337x.js` 与
`test/fixtures/1337x-ubuntu.html` 原样保留（那个 fixture 仍是「JS 挑战页长什么样」的真实样本，
probe 用 `rows.count == 0` 把事实钉住）。

⚠️ 因此在 `bt-providers` 里**不要**实现 `1337x`：写出来只会是一个永远报错的 provider。

### 🐞 移植中发现的上游 bug（JS 版既有，值得单独修）

| 位置 | 现象 | 本移植怎么处理 |
|---|---|---|
| `linuxtracker.js` 的 `detailUrl` | `` `${base}${href}` `` 而 base 无结尾斜杠 → `https://linuxtracker.orgindex.php?…` **死链，点开 404**（抓取不受影响，所以一直没暴露） | **修掉**（`join_base` 补斜杠）+ 单列 divergence 测试 |
| `linuxtracker.js` 的 `parseEuDate` | 用 `new Date(y,m-1,d)`（本地时区零点），而 `dateText` 按 UTC 格式化 → **东八区显示早一天**（站上 28/04 显示 04-27） | **照抄**（纯显示问题，且与时区绑定；要修得两版一起改） |
| `linuxtracker.js` 的列索引 | 主表里夹着 19 个 td 的「展开描述行」，列含义不同 → 这些结果的 size/seeders 落在错误列上 | **照抄**（否则与 Node 版对不上） |
| `audiobookbay.js` 的 size/date | `infoText.split('\n')` 想按行切字段，但页面靠 `<br />` 分隔，而 **cheerio 的 `.text()` 不给 `<br>` 补换行**（探针 `info.first_text` 实证：整段挤成一行）→ size 恒 `null`；又因 `else if`，date 被赋成整段垃圾串（`Date.parse` → NaN → `null`）。**两个站上真实存在的字段全丢** | **照抄**（属「数据缺失 / 纯显示」；要修得两版一起改：按字段名切分，`src/lib/normalize.js` 的 `parseSize` 本来就能吃 `19.24 GBs`） |
| `therarbg.js` 的 `categoryFromRaw` | `switch` 只列了 `Anime/Apps/Books/Games/Movies/Music/XXX/Tv` 八个**精确**拼写，站上大量分类（`E-books`、`Other`、`TV`…）一律落 `Other` —— 实测 fixture 第 3 行站上写 `E-books`，归一化后变成 `Other` | **照抄**（分类只是给前端的粗粒度提示；要修该拉全量分类表，不是改一行） |
| `limetorrents.js` 的 `categoryFromRaw` | 同一个毛病，而且更彻底 —— `switch` 只认 `TV`/`Movie`/`Music`/`App`/`E-book`/`Anime`/`Games`，而站上写的是 `TV shows`/`E-books`/`Applications`（探针 `datecell.texts` 实证）→ **40 条结果全落 `Other`** | **照抄**（同上；测试里连“整页没有一个非 Other 的分类”都钉住了） |
| `limetorrents.js` 的日期 | 站上会把相对日期截断成 `1 Year+`，而 `normalize.parseDate` 只认带 `ago` 的写法 → **大部分结果没有日期**（`dateText` 显示 `—`）；只有 `9 months ago` 这类才认（而且是相对今天算的） | **照抄**（要修该把 `1 Year+` 映射成「约一年前」，属两版同步的活） |
| `torrent9.js` 的数据白扔 | 结果表第 2~5 格就摆着 `30/08/2018` / `1.9Go` / `3` / `3`，详情页也有 `Poids du torrent`（大小）、`Date d'ajout`（日期）、`Catégories`；但 `searchOn` 只传 `name` + `detailUrl`，`resolveMagnet` 只回 `{magnet, infoHash}` —— **其余字段全丢**，卡片上除名字外全是 `—` | **照抄**（属「数据没用上」；要修就是让 search 直接读那四格，或让 resolve 顺手补全字段） |
| `torrent9.js` 的 `parseDetail` | 它抽出的 `size`/`seeders`/`leechers`/`date`/`category` **没有任何调用方用到**（`resolveMagnet` 只要 magnet + hash）→ JS 里的死代码，连带 `categoryFromRaw` 也只服务于它 | **不搬**（Rust 侧只实现可达路径；文件头写了哪一天要补就照 JS 补） |
| `oxtorrent.js` 的数据白扔 | 与 torrent9 同一个毛病：同行就有 `700.4 MB` / `9` / `2`，第 1 格还带 `<i class="Logiciels">`（法语分类，本该映成 `Apps`）—— `searchOne` 只传 `name` + `detailUrl`，全不要 | **照抄**（探针 `cat_classes.attrs_first_3` 把「分类其实拿得到」这个事实钉住了） |
| `megapeer.js` 的编码 | axios 对文本响应一律 `toString('utf8')`（不读 charset），而站点是 **windows-1251** → 全部西里尔标题变替换字符（数据级损坏） | **修掉**（reqwest 按 charset 解码）+ divergence 测试；见下面 megapeer 专门小节 |
| `megapeer.js` 的列索引 | 50 行里 7 行多一格评论数（恒在 td3）→ `td:nth-child(3)` 当 size 在那些行取到评论数、`td:nth-child(4)` 的 peers 落空 | **修掉**（「倒数第 2 格 = size、最后一格 = peers」）+ divergence 测试 |
| `zeromagnet.js` 的数据白扔 | `td:nth-child(2)` 的 `result-meta` 里就有 `6.2GB` / `2026-08-07`，JS 只传 `name`/`detailUrl`/`category:'Porn'`，全丢（探针 `meta_sizes`/`meta_dates` 把「拿得到却不用」钉住） | **照抄**（与 oxtorrent/torrent9 同款毛病） |
| `nyaa.js` 的列索引 | 名字那格带 `colspan="2"`（评论列被并掉），一行只有 **8 个 td**，而 JS 按 9 列读 `tds.eq(2..5)` → 实际落到 Link/Size/Date/Seeders：`sizeText` 恒 `—`、`dateText` 恒 `—`、**`seeders` 变成日期串里的 2009**（实测 JS 真给这个数） | **照抄**（测试把 `seeders = Some(2009)` 都钉住了，免得以后有人以为 Rust 写错） |
| `nyaa.js` 的磁力 | `nameCell.find('a[href^="magnet:"]')` 在**名字格**里找，而磁力在 `tds.eq(2)`（Link 格）→ **magnet 恒 `null`、`needsMagnet` 恒 true**；而 `nyaa.js` **又没有 `resolveMagnet`**，于是前端「获取磁力」会打到 `/api/magnet` → 无 resolver → 回退到已死的 1337x → 报错。**结论：nyaa 现在拿不到磁力** | **照抄**（真要修：把搜索范围改成整行 + 补一个 `resolveMagnet`，两版一起改） |

口径：**功能性 bug（死链）就修，纯显示问题先照抄** —— 三条都在代码注释里写明了理由。
- HTML fixture 现状：

| fixture | 状态 |
|---|---|
| `linuxtracker-linux.html` | ✅ 真实结果页。43 个候选链接里 **33 条**是主表行（其余是 Top10 侧栏，行内只有 2 个 td） |
| `filemood-ubuntu.html` | ✅ 真实结果页。65 个 tr 里 **20 条**数据行 |
| `1337x-ubuntu.html` | ❌ **不是结果页** —— FingerprintJS 反爬跳转页（1.1KB，`window.location.replace`）。该域名 2026-10-09 已停放，详见上面的 `1337x.js` 小节 |
| `rutor-ubuntu.html` | ✅ 真实结果页，101 行（含表头）→ 100 条；**UTF-8**（名字叫 `getWin1251` 的那段实际没换编码） |
| `audiobookbay-ubuntu.html` | ✅ 真实结果页，9 条。字段靠 `<br />` 分隔，**卡片上的 size/date 是空的**（上游 bug，见上表） |
| `audiobookbay-detail.html` | ✅ 真实详情页（179KB / 1277 个 td）。`<td>Info Hash:</td>` 后面那格就是 40 位 hash；它也是 `resolve_magnet` 唯一的测试输入 |
| `therarbg-ubuntu.html` | ✅ 真实结果页，39 个 `tr.list-entry`（但**40** 个名称链接 —— 第 28 行多一个 IMDb 徽章）。size/date 在 `data-order` 里，文本里只有给人看的形式 |
| `therarbg-detail.html` | ✅ 真实详情页（150KB，`a[href^="magnet:?"]` 只有 1 条，848 字符、"&amp;" 编码）。同时是「长属性的实体解码」的对照样本 |
| `limetorrents-ubuntu.html` | ✅ 真实结果页，41 行含表头（第一行是 `<tr><th>`）。⚠️ 页面别处也有 `td:nth-child(2)` / `td.tdseed`（旁边速度榜的 `6572 KB/Sec`）—— 所以格子真值只能看 `rows.first_3_rows`（按行取） |
| `torrent9-ubuntu.html` | ✅ 真实结果页，**只有 3 条**。名称要跳 `<h3>` + `<span class="blue">` 拼出来；后四格（日期/大小/peers）JS 不要 |
| `torrent9-detail.html` | ✅ 真实详情页（48KB，861 字符的磁力链，19 个 tracker）。源玛里是**裸 `&`**（不是 `&amp;`）—— 与 therarbg 那条正好互补 |
| `oxtorrent-ubuntu.html` | ✅ 真实结果页，**3 行且没有表头行**。同行就有大小/peers/分类（`<i class="Logiciels">`），JS 都不要 |
| `oxtorrent-detail.html` | ✅ 真实详情页（20KB）。`div.btn-magnet > a` 1 条，同级还有个 `div.btn-download > a` 指向同一条磁力（JS 不认） |
| `nyaa-ubuntu.html` | ✅ 真实结果页（**CI 抓的**，本机不通），只有 1 条。⚠️ 名字格 `colspan="2"` → 一行 8 个 td，JS 的列索引全错位；磁力在 `td[2]` 而 JS 在 `td[1]` 找 |

**C 组（基础设施，可穿插）**：`src/lib/scraper.js` 的 `createProvider` 工厂 + `runMirrors` 镜像回退 ——
搬完 3~5 个 provider、看清共性后再定抽象。
**截至 2026-10-10 已搬 14 个**，三类共性全都有实例了：
镜像回退（`rutor` / `audiobookbay` / `therarbg` / `limetorrents` / `torrent9` / `oxtorrent`，各自的 `search_with` 逐字复刻 `runMirrors`）、
翻页（`rutor` 的 0/1 起始差异、`limetorrents` 的真分页 URL、`nyaa` 的**不** coerce）、
磁力惰性解析（`audiobookbay` / `therarbg` / `torrent9` / `oxtorrent` 的 `resolve_magnet` + `MagnetOutcome`，已因此从 provider 提到 `lib.rs`）。
→ ✅ **2026-10-11 已提取 `run_mirrors`**：10 份逐字副本（`audiobookbay` / `filemood` / `limetorrents` /
`linuxtracker` / `megapeer` / `oxtorrent` / `rutor` / `therarbg` / `torrent9` / `zeromagnet`）
合并成 `bt-providers::run_mirrors`，**净 −73 行**（`+77 / −150`，11 个文件）。
✅ **`Provider` trait 现在该建了（M2）** —— 时机变了：当时那 10 份的差异只是「站点名」，
一个 helper 就够（`run_mirrors` 已提取）；**但现在出现了三个真实消费者**：
① 聚合层要**并发遍历异构 provider 列表** ② D17 要给 Torznab 留口子
③ `resolve_magnet` 只 6/16 家有 → 正是 trait **默认方法**要表达的可选能力。
形状与反方意见见 `docs/ROADMAP.md` §2.4。
⚠️ 顺带记一处 **有意 divergence**：JS 的 `runMirrors` 是**并行**（`Promise.allSettled`），
Rust 侧是**顺序** `await`（最坏 `n × 10s`）—— 以前只在 `filemood` / `linuxtracker` 两处注释里提过，
现在统一写在 `run_mirrors` 的文档注释里（要不要改成并行 = `docs/ROADMAP.md` 的小项默认值 **d1：保持顺序**，想改就说）。

### 🧱 HTML provider 的四条铁律（写自 `dom.rs` 与历次对照的实测）

1. **裸 `<tr>`/`<td>` 片段会被解析器丢掉**（文档模式下不合法的表格标签被忽略，文本留下）。
   写测试时随手塞 `<tr><td>x</td></tr>` 会得到 0 个匹配，看着像"选择器写错"，其实是 HTML 规则
   —— cheerio 的 parse5 行为完全一样。**片段要包进 `<table>`。**
2. **类名匹配区分大小写**，标签名不区分（HTML 规则）。`td.LISTA` 不会命中 `class="lista"`。
3. **属性值是解码后的**：源码 `&amp;` → 取值 `&`。所以 `[href*=...]` 是对解码后的值做子串匹配
   （linuxtracker 的 `href^="index.php"` 就是靠这个把侧栏和主表区分开的）。
4. **`.text()` 拼接后代文本时一个字都不插**（不给 `<br>` 补换行、不给块级元素补空格）。
   实测（`audiobookbay`）：`Posted: 1 Sep 2026<br />Format: MP3<br />File Size: 19.24 GBs`
   的 `.text()` 是 `Posted: 1 Sep 2026Format: MP3File Size: 19.24 GBs` —— **没有换行**。
   所以 JS 里那些 `split('\n')` 在真页面上往往只有一行，靠它分字段的解析器看着很像在工作、实际全拿不到。
   → **别信 JS 注释里的假设，先跑探针把真字符串取回来。**

**移植约定**：
- JS 的运行时类型判别（`typeof x === 'number'`）在 Rust 里提为类型：`NumOrText::{Num, Text}`
- JS 的宽松数值解析要复刻，不能直接用 `f64::from_str`：`"1.2.3"` 在 JS `parseFloat` 下是 `1.2`；
  `parseInt("12abc")` 是 `12`。已手写 `js_parse_float` / `to_int`。
- `encodeURIComponent` 也手写复刻（只 `A-Za-z0-9-_.!~*'()` 原样）
- **集成测试（`tests/`）拿不到 crate 的普通依赖**，只能访问公开 API + `[dev-dependencies]`
  → 测试里要用 `chrono` / `serde` / `tokio` 都得在 `[dev-dependencies]` 再声明一遍
- 有意偏离 JS 的行为，一律单列成 divergence 测试并在注释里写明原因，别当 bug 修掉
- 测试**尽量不依赖外网**：`tests/http.rs` 自起本地一次性 HTTP 服务，这个套路可以直接复用到 provider 上

**Rust 本地工具现状（重要）**：
- ❌ 本机 `CARGO_HOME`（`D:\Vibe-Coding\.cargo`）已被清理，`cargo` 不在 PATH
  → **本地跑不了 `cargo build` / `cargo test` / `cargo clippy`**，全走 CI
- ✅ 但 **`rustfmt.exe` 是独立二进制，不需要 cargo**，提交前先本地自查能省一轮 CI（~1.5 分钟）：
  ```bash
  RF="/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe"
  "$RF" --check --edition 2021 <files>   # 只报告
  "$RF" --edition 2021 <files>           # 就地修好
  ```
  **必须带 `--edition 2021`**（独立运行时默认 2015，结果会不一致）。
- ⚠️ **不要用「行宽 ≤ 100」代替 rustfmt**：它默认 `fn_call_width = 60`，
  多参数宏/函数调用即使整行不到 100 也会被拆行（`assert_eq!(a, b, "msg")` 是重灾区）。
- ⚠️ **没有 `cargo check`，签名错误只能靠 CI 发现**（一次往返 ~1.5 分钟）。已踩过的三类：
  1. **turbofish 泛型个数**：`bt-core` 的 `post_json<T, B>` 是两个泛型参数，
     `post_json::<Value>(...)` 会报 E0107。→ 改成在绑定上标注：
     `let resp: JsonResponse<Value> = http.post_json(url, &body, None).await;`
  2. **`and_then` 的闭包签名**：`extract_info_hash` 收的是 `Option<&str>` 而不是 `&str`，
     不能直接 `and_then(extract_info_hash)`，要么 `.or_else(|| f(x.as_deref()))`，
     要么写 `|s| ...` 闭包。
  3. **`"#` 会把 raw string 提前结束**：测试里造 HTML 片段习惯写 `r#"…"#`，
     而内容里只要出现 `="#`（如 `bgcolor="#F4F4F4"`）字符串就在那里断了，
     报一堆 `prefix \`name\` is unknown`。→ 用 `r##"…"##`。
  提交前顺手核对一遍要调用的每个函数的真实签名，比等 CI 便宜。

### 🧩 provider 移植配方（照着 tpb / knaben 抄）

1. 读 `src/providers/<name>.js`，grep 出它调了 `getText` / `getJSON` / `postJSON` 哪些形态
2. 在 `crates/bt-providers/src/<name>.rs` 写 `KnabenRequest` 式的 payload + `search()` + `search_at(http, api, query)`
3. **期望值不要手抄** —— 写个临时 Node 脚本 require `src/lib/normalize.js`，
   把同一份 fixture 喂进去，把输出逐字段抄进 Rust 断言。这样「与 Node 版一致」才有实据
   （`normalize.js` **零依赖**，本机 `node -e "…"` 直接能跑，不需要 `node_modules` ——
   `audiobookbay` 的 `sizeText: "—"` / `date: null` 就是这么实测出来的）
4. `tests/<name>.rs`：真 fixture 逐字段 + 请求契约（用 `common::oneshot_capture`）+
   空结果 + HTTP 错误 + 非 JSON 正文
5. 若有**有意偏离** JS 的地方，单列一条 `divergence_*` 测试并写明原因
6. `git add` 前先跑 rustfmt 自查；**不要删对应的 `.js`**（见上面「边搬边删」）

**⚠️ 已踩的期望值坑（都是没按第 3 条做导致的）**：
- `dateText` 缺省是 **`"—"`** 不是空串（JS `formatDate(null)` 就返回 `—`）
- JS 的 `0` 是 falsy：`x ? Number(x) : null` 会让 `created_unix: 0` → 无日期，
  而不是 1970-01-01。用 `value::v2nt_nonzero`。而字符串 `"0"` 是 truthy，别一起过滤掉
- `categoryId` 之类的**数组取 min 而非首元素**
- yts 会把 infoHash **小写化**；tpb/knaben 原样保留大小写

**HTML provider 追加的步骤（`linuxtracker` 走通的路子）**：

7. 先用 probe 机制把页面结构问清楚，**别靠读 HTML 猜**：
   往 `test/fixtures/html-probes.json` 加该 fixture 的 probe（选择器、列索引候选、
   href、closest+find 的整行），跑 `.github/workflows/html-probes.yml` 拿 cheerio 真值。
   linuxtracker 就是这样才发现「43 个候选里只有 33 条主表行、最终 18 条」这种数字。
8. 结构里常有的**坑行/坑列**要单独成测试：侧栏行（td 少）、展开的描述行（列语义不同、
   名字链接为空）。**照抄 JS 的取舍**，别顺手"修"。
9. `runMirrors` 的真实语义要读 `src/lib/mirrors.js`（**不是**"出错就换下一个"）：
   并行请求、按声明顺序取第一个**结果非空**的；全空时错误被包成
   `"<name> unreachable (<err1>; <err2>)"`。所以「页面正常但没结果」算**错误**。
   **Rust 侧直接调 `bt_providers::run_mirrors`**，别自己再抄一遍循环；
   注意它是**顺序** `await`（有意 divergence，最坏 `n × 10s`），语义说明在该函数的文档注释里。
10. 移植中发现的上游 bug：**功能性 bug（死链）就修并单列 divergence 测试；
    纯显示问题先照抄**。已发现的若干条登记在上面的「🐞 移植中发现的上游 bug」。

## 🔴 CI 失败模式总账（2026-07-10 ~ 2026-10-10，223 个 run / **51 次失败**）

把远程所有失败 run 拉下来逐条归类后，结论是：**这 51 次里只有 3 次是真·环境问题，
其余都是自己造成的，而且大部分本机就能拦住**。

| 类 | 次数 | 现象 | 根因 | 以后怎么避免 |
|---|---|---|---|---|
| **A. workflow 文件变成空壳** | **15** | run 有结论、但**一个 job 都没有**；`gh run view` 只说 `likely failed because of a workflow file issue`，`--log-failed` 输出**完全为空** | `main` 上的 `.github/workflows/tauri-build.yml` 是 **0 字节**。来源于 `3ad0c59`（2026-08-14「同步 Tauri Build workflow 到 main」）—— 那次同步写出了一个空文件，一直挂到 08-20 的 `8d06e5e` 才修。期间 main 上**每次 push 都失败一次** | ① 新增 `ci-guard.yml` + `scripts/check-workflows.py`（见下）；② 同步/恢复文件后立刻 `gh run view <id>`，**看不到 job 就是文件坏了**，别等日志 |
| **B. 探针真值没跟上** | 7 | `dom_probes::selectors_and_accessors_match_cheerio_on_real_fixtures` 失败，打印 "期望值里没有这个 probe" | 先推了「fixture + 新 probe」，而 `html-probes.expected.json` 还是旧的 —— **两步流程的必然红灯**（每搬一个 HTML provider 就来一次） | 见「探针两步流程」；根治办法（待做）：让 `html-probes.yml` 自己 commit 回 expected.json |
| **C. 测试返回值写错** | 7 | 各种 `assertion left == right` | 手算的期望值错：`dateText` 默认是 `—` 不是空串、`1 MB` 会格式化成 `1.0 MB`、`date` 不含 `Posted:` 前缀、base 多一个尾斜杠、`ends_with` 写成了带 slug 的、把线上域名写进本地服务测试 | 期望值**一律先跑真 JS**：`node -e "require('./src/lib/normalize.js')"` —— 本机零依赖，能跑（实测过） |
| **D. 自己代码编译不过 / 单测片段写错** | 6 | `error[E0107] expected 2 generic arguments`、`error[E0308] &str vs String`、`error: prefix \`x\` is unknown`、3 次 dom.rs 单测（裸 `<tr>`/`<td>` 片段被解析器丢掉） | 本机没 cargo，签名/泛型靠 CI 发现；测试里随手塞不合法表格标签 | 见「本机三道门」+ 「HTML provider 铁律 #1」 |
| **E. `cargo fmt --check`** | 2 | `Diff in crates/...` | 本地没跑 rustfmt，或跑了没看退出码 | 见「本机三道门」第 1 条 —— **必须用 `&&` 串，不能用 `;`** |
| **F. 打包 / 发版** | 14 | EPERM×5（npm cleanup 删不掉被进程锁住的 `@tauri-apps`/`@electron` 原生二进制）、GH_TOKEN×2（electron-builder 想自动 publish）、E0593×2（`src-tauri` closure 参数个数）、NSIS 配置不合法×1、`未找到 electron 运行时`×1、`cat docs/RELEASE_ARTIFACTS.md: No such file`×1、pwsh `working directory` 不存在×1、无日志无说明×1 | 基本上是**同一台 Windows runner 上的文件锁 / 上游 schema 变动 / 分支上缺文件**，跟被测代码无关 | EPERM：不要在同一 job 里先 `npm ci` 再就地删 node_modules（已修）；GH_TOKEN：别让 electron-builder 自动 publish，改 artifact 上传（已修）；引用文件前先 `test -f` |
| **G. Slint spike** | 1 | `Cannot convert string to styled-text`（build.rs 挂了） | spike 分支已删，结论留在「Slint 探针结论（留档）」 | M1 重建 Slint 时别再踩 |

（组合：A15 + B~E22（Rust 门禁）+ F14 + G1 = 51；Rust 那一组 22 次里
**没有一次是环境问题**。）

### 🚧 本机三道门（提交前必须过，任一条不过就别 push）

```bash
# 1) 格式化 —— ⚠️ 必须用 && 串起来！用 ; 的话前面失败了你还会继续 commit+push
RF="/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe"
"$RF" --edition 2021 <改动的 .rs 文件> && "$RF" --check --edition 2021 <改动的 .rs 文件> && \
  git commit ... && git push ...

# 2) 期望值先跑真 JS（normalize.js 零依赖，本机能跑）
node -e "const {normalize}=require('./src/lib/normalize.js'); console.log(normalize({...}))"

# 3) workflow 文件体检
python3 scripts/check-workflows.py
```

> 2026-10-10 实测：`limetorrents` 那次 raw string 编译错**本机 rustfmt 已经报出来了**，
> 是我用 `;` 串命令、`echo $?` 之后照样 commit+push，白丢一轮 CI。

### 🔒 workflow 文件守卫（`ci-guard.yml` + `scripts/check-workflows.py`）

workflow 文件坏掉时**它自己不跑**，所以要另一条无关的 workflow 来体检：

- 触发：push / PR 改动 `.github/workflows/**` 或 `scripts/check-workflows.py`（也可手动触发）
- 检查：每个 `*.yml` ① 体积 ≥ 200 字节（抓空壳）② 能 YAML 解析 ③ 有 `on:` 与 `jobs:`
  ④ 每个 job 有 `runs-on` 或 `uses`（没装 pyyaml 就退化成字符串级检查）
- ⚠️ **`ci-guard.yml` 与 `scripts/check-workflows.py` 现在已经两个分支都有了**（2026-10-16 实测
  `git ls-tree main .github/workflows/` 与 `git ls-tree main scripts/` 均在列）——
  旧笔记说「main 上还没有、下次动 main 时带过去」**已过期**，别再重复搬一遍

### 🐢 探针流程（2026-10-10 改造：CI 自己提交真值，中间不再必红）

搬一个 HTML provider 时，按下面走 —— **feat/rust 上不会出现「probe 有了、真值还没」的中间态**：

**0. 先看本机能不能抓到真页面**

- **能**（大多数站）：自己抓一份存成 `test/fixtures/<site>-<query>.html`（详情页要单独再抓一次）
- **不能**（`dmhy` / `megapeer` / `zeromagnet` 这类本机探不通的）：让 CI 去抓

  ```bash
  scripts/gh-retry.sh workflow run fetch-fixture.yml --ref feat/rust \
    -f url='<结果页 URL>' -f path='test/fixtures/<site>-<query>.html'
  # 20 秒左右，CI 会把文件 commit 回 feat/rust
  git pull --rebase
  ```

  ⚠️ 抓回来**先看一眼内容**（`head -c 300`）—— Cloudflare 挑战页 / 停放页也是 200。

**1. 加 probe**：往 `test/fixtures/html-probes.json` 里加这个 fixture 的条目，
选择器就拄 JS 里那几条（`rows.count` / `row_cells` 的整行 / 每个用到的格子各一条）。

⚠️ **站点不是 UTF-8 时**（如 megapeer=windows-1251），条目里加 `"encoding": "windows-1251"`：
cheerio 侧走 `TextDecoder(label)`、Rust 侧走 `encoding_rs`，**同一标签两侧必须解出一致文本**；
fixture 一律存原始字节，不要转码。

**2. 在临时分支上把真值跑出来**（别在 feat/rust 上先推，那样 rust.yml 必红一次）：

   ```bash
   git checkout -b probes/scratch
   git add test/fixtures/<新 fixture> test/fixtures/html-probes.json
   git commit -m "test(<site>): 抓真 fixture + N 条探针"
   GIT_SSL_NO_VERIFY=true git push -u origin probes/scratch
   scripts/gh-retry.sh workflow run html-probes.yml --ref probes/scratch   # 20 秒左右
   git fetch origin probes/scratch
   git checkout feat/rust            # ← 先切回来
   git restore --source=origin/probes/scratch -- test/fixtures/html-probes.json test/fixtures/html-probes.expected.json
   ```

   ⚠️ 取文件要用 `git restore --source=<ref>` **从 feat/rust 上**拿 —— 千万别先 `git checkout feat/rust`
   再想拿临时分支独有的文件（拿不到，而且会把它们从工作区删掉；2026-10-10 踩过，靠 `git reflog` 才救回来）。

**3. 回 `feat/rust` 一次推完**：fixture + probe + 真值 + provider + 测试 → 一次就绿
**4. 删临时分支**：`git branch -D probes/scratch && GIT_SSL_NO_VERIFY=true git push origin :probes/scratch`

为什么这么改：`html-probes.yml` 现在**自己 commit** `expected.json` 回触发它的 ref
（`permissions: contents: write`），所以人只负责 fetch 文件。
以前是「先推 probe（rust.yml 必红）→ 再从日志手抠 JSON」，那个红灯在 CI 失败总账里出现了 **7 次**。

残留的坑：
- 如果 CI 提交失败（分支保护之类），日志里还有一份可抠：`scripts/gh-retry.sh run view <id> --log`，
  **按 `\t` 切掉前两段前缀、去掉 `\uFEFF`、从单独一行的 `{` 读到单独一行的 `}`**，
  再和旧的 expected.json 逐 fixture 比对（老 fixture 必须逐字一样，不然就是抓错了）
- ⚠️ **别给大页面加 `text_first` on `html`** —— 超长单行会让 CI 日志从那一行起被截断
  （therarbg 实测）；现在真值走文件、不走日志，但那条 probe 依旧没意义，别加
- ⚠️ CI 提交后**远端比本地新**，接着推之前先 `git pull --rebase`，否则会被拒

---

## Testing

**过渡期两套测试并存**：

| | 命令 | 跑在哪 | 去向 |
|---|---|---|---|
| Node（旧） | `npm test` | 本机（需 node） | 随「边搬边删」逐步缩小，最终退休 |
| Rust（新） | `cargo test --workspace` | **只能走 CI**（`.github/workflows/rust.yml`） | 本机无 cargo |

### 测试策略（Q5 决策）

- **CI 默认只跑离线 fixture**（`test/fixtures/` 的真实快照）—— 稳定的门，作用是防"解析逻辑退化"
- ⚠️ **但离线 fixture 证明不了"现在还能用"**：快照多半是 2026-07 的，站点早已改版。
  测试全绿也可能一个结果都搜不出来。这条必须记住，别把绿灯当成"抓取正常"。
- 因此另设一条 **联网冒烟**，只有手动触发、**允许失败**：
  - 测试：`crates/bt-providers/tests/live_smoke.rs`（**默认跳过**，只有 `BT_LIVE_SMOKE=1` 才打外网）
  - workflow：`.github/workflows/live-smoke.yml`（仅 `workflow_dispatch`）
  - 本地跑法：`BT_LIVE_SMOKE=1 cargo test -p bt-providers --test live_smoke -- --nocapture --test-threads=1`
    （**注意：本机跑没用** —— archive.org 被 DNS 污染；要在 CI 上跑，见下）
  - 用途 ① 判断站点还能不能用 ② **抓真实响应形态**（日志里有 `[smoke] <provider> first: ...`
    与 IA 的原始 doc，`gh run view --log` 可读）
  - 「允许失败」= 它不在任何 push/PR 的门上；**不是**把它设成 `continue-on-error`
    （那会让红变绿，等于把信号藏起来）
  - ⚠️ **已归为已知噪声**：`tpb` 在 CI 上稳定 403（apibay 对机房 IP 限流），
    测试里单列 `[smoke] NOTE` 不触发红灯；但「**0 条结果且无错误**」会**计入失败**
    （那才是选择器/字段名失效的信号）

**冒烟基线（2026-10-08 首次跑，run 37735017845：5 个 provider，0 个问题）**

| provider | 结果 | 备注 |
|---|---|---|
| `knaben` | 300 条 | ✅ |
| `torrentscsv` | 25 条 | ✅ |
| `yts` | 24 条 | ✅ |
| `internetarchive` | 97~98 条 | ✅ 顺带拿到真实字段形态（见进度表上方） |
| `tpb` | 403 | ⚠️ 机房 IP 被限流，非代码问题 |
| `megapeer` | 50 条 | ✅ 2026-10-11 加入（独立 test）；标题西里尔完好 → 真实 `charset=windows-1251` 头实锤 |

以后跑冒烟时，拿这些数字做对比：**数量级突然掉到 0 或个位数**才是真信号。
（2026-10-11 复跑：knaben 300 / torrentscsv 25 / yts 24 / internetarchive 95 / tpb 403 / megapeer 50，0 问题 —— 与基线一致。）

### 🔍 provider 探活：全部 40 个引擎，`scripts/provider-health.mjs`

冒烟只覆盖 5 个 JSON provider。要问「**其他 35 个现在还能用吗**」，用这个脚本 ——
零依赖（只用 Node 内置 `fetch`，不用装 `node_modules`），对每个引擎的**真实搜索 URL**
发一次请求，打印 `HTTP / 字节 / 结果行数`：

```bash
# CI（推荐，出口干净）
scripts/gh-retry.sh workflow run provider-health.yml --ref feat/rust
scripts/gh-retry.sh run view --job=<JOB_ID> --log | grep -E '^\| `'

# 本机（得加两个开关，见下）
NODE_TLS_REJECT_UNAUTHORIZED=0 node --use-env-proxy scripts/provider-health.mjs
```

**⚠️ 本机结果不可全信 —— 已实测**：
- Node 的 `fetch` 默认不读 `HTTP_PROXY`（要 Node 24+ 的 `--use-env-proxy`）
- 本机那层代理是 **MITM** 的，Node 不信任其证书 → 报 `DEPTH_ZERO_SELF_SIGNED_CERT`
- 同一域名**前后两次探活结论会不同**（实测 `linuxtracker` 一次 200、几分钟后 000；
  `filemood` 报 ECONNRESET，但我们手里有它真实的结果页快照）
- 本机对 `archive.org` 等还有 DNS 污染

→ 所以「本机探不通」**不等于**「站点挂了」。要权威结论就用 CI，或两边都跑、对比着看。

**⚠️ 反过来，CI 也不等于用户环境**：CI 是机房 IP，有些站对机房 IP 更凶
（apibay 对机房 IP 直接 403）。**两边都跑，才是完整答案。**

**2026-10-16（M5）只跑 CI 一侧 —— 目的是「要不要砍引擎」，结论是「一个都不砍」。**
关键词 `ubuntu`，40 家，12 家有结果。只列**已移植的 16 家**：

| 引擎 | HTTP | 字节 | 行 | 判断 |
|---|---|---|---|---|
| `knaben` | 200 | 81522 | 1 | ✅ |
| `torrentscsv` | 200 | 6106 | 25 | ✅ |
| `yts` | 200 | 157 | 1 | ✅（「ubuntu」对电影站本就近乎无结果） |
| `internetarchive` | 200 | 1975 | 1 | ✅ |
| `linuxtracker` | 200 | 51148 | 26 | ✅ |
| `audiobookbay` | 200 | 36672 | 36 | ✅ |
| `megapeer` | 200 | 10552 | 14 | ✅（win1251） |
| `nyaa` | 200 | 16201 | 2 | ✅ |
| `rutor` | 200 | 89974 | 111 | ✅（俄站） |
| `zeromagnet` | 200 | 9008 | 29 | ✅ |
| `filemood` | 429 | 3322 | 0 | ↪ 限流，**不是死** |
| `tpb` | 403 | 5660 | 0 | ⛔ 被拦 |
| `limetorrents` | 403 | 5705 | 0 | ⛔ 被拦 |
| `torrent9` | 403 | 5708 | 0 | ⛔ 被拦 |
| `oxtorrent` | 403 | 5653 | 0 | ⛔ 被拦 |
| `therarbg` | 403 | 5701 | 0 | ⛔ 被拦 |

**🔴 M5 的结论：不砍任何引擎 —— 这张表根本不支持「砍」这个动作。**

那 5 个 403 **形状完全一致**（403 + 5.6~5.7KB 正文）= Cloudflare 一类的 WAF 挑战页，
而不是「站点没了」；而 CI 是**机房 IP**（上面那条早就写了）。拿机房 IP 的 403 去砍引擎，
等于把「只是拦机房」的站点当死站删掉 —— 那 5 家里有 4 家
（`limetorrents` / `torrent9` / `oxtorrent` / `therarbg`）在下面 2026-10-09 的
**本机**那一侧是可用的。

→ **D13「先探活再砍」的正确姿势是：两边都跑，只有两边都 ❌ 才算死。**
单边数据（无论哪一边）**都不足以**支撑砍任何东西。这条比那张表本身更值得记。

**顺带三个别的发现**（都**未移植**，按 D13 不主动扩，但记下来）：
- **`dmhy` 活了**（200 / 49754B / 10 行）—— 它当初是因为 CI 连取三次 fixture 超时被
  「暂停」的，现在站点自己回来了。要移植它是**可行**的，但那在 D13 之外。
- **`xxxtracker` 200 / 12615B / 1 行** —— 同样活着。
- `nekobt` / `mikan` / `mypornclub` / `animetosho` 是 200 + **0 行** ⇒ 选择器可能变了；
  `anilibria` / `bangumimoe` / `subsplease` 只回 2~55 字节（几乎没有正文）；
  `sukebei` 连接超时。这些都不在注册表里，不影响当前状态。

**2026-10-09 两个环境各跑一次 —— 结论必须两边合起来看**（同一脚本、同一关键词 `ubuntu`）：

| 引擎 | 本机 | CI | 引擎 | 本机 | CI |
|---|---|---|---|---|---|
| `tpb` | ✅ 100 行 | ⛔ 403 | `rutor` | ✅ 111 行 | ✅ 111 行 |
| `audiobookbay` | ✅ 36 行 | ✅ 36 行 | `torrentscsv` | ✅ 25 行 | ✅ 25 行 |
| `filemood` | ❌ ECONNRESET | ✅ 65 行 | `linuxtracker` | ❌ 000 | ✅ 26 行 |
| `internetarchive` | ❌ timeout | ✅ 1 行 | `knaben` | ✅ 1 | ✅ 1 |
| `yts` | ✅ 1 | ✅ 1 | `limetorrents` | ✅ 45 行 | ⛔ 403 |
| `therarbg` | ✅ 39 行 | ⛔ 403 | `torrent9` | ✅ 4 行 | ⛔ 403 |
| `oxtorrent` | ✅ 3 行 | ⛔ 403 | `dmhy` | ❌ timeout | ✅ 10 行 |
| `megapeer` | ⛔ 403 | ✅ 14 行 | `nyaa` | ❌ timeout | ✅ 2 行 |
| `zeromagnet` | ❌ timeout | ✅ 29 行 | `xxxtracker` | ❌ timeout | ✅ 1 行 |
| `1337x` | ⛔ 403 | ⛔ 403 | `blueroms` | ⛔ 403 | ⛔ 403 |
| `eztv` | ⛔ 451 | ⛔ 403 | `torrentdatabase` | ⛔ 403 | ⛔ 403 |
| `uindex` | ⛔ 403 | ⛔ 403 | `btdigg` | ❌ timeout | ↪ 429 |

**怎么读这张表（重要）**：
- **两个环境合起来，至少 18 个引擎真的出结果** —— 所以「剩下的全都坏了」是**不成立**的
- **两边都 ✅** 才是最硬的（`rutor` / `audiobookbay` / `torrentscsv` / `knaben` / `yts`）
- **一边 ✅ 一边 ❌ 的，说明问题在「网络出口」而不在站点**：
  - CI ✅ / 本机 ❌ → 本机网络挡着（`linuxtracker` / `filemood` / `internetarchive` / `dmhy` / `nyaa` / `megapeer` / `zeromagnet` / `xxxtracker`）
  - 本机 ✅ / CI ⛔ → **机房 IP 被 Cloudflare 拦**（`tpb` / `limetorrents` / `therarbg` / `torrent9` / `oxtorrent`）
    ⚠️ 所以 **CI 上的 403 是「下限」不是「真相」** —— 本机跑得通就说明用户那儿也能用
- **两边都 ⛔**：`1337x` / `blueroms` / `eztv` / `torrentdatabase` / `uindex` —— 这几个基本可以判死刑
- `btdigg` 的 **429** = 限流（不是封锁）；`animetosho` / `mikan` 是本机/CI 都 200 但
  粗判标记没命中 → **要按站点看真实选择器**，不能只看这个数字

**顺带发现**：`megapeer` 用的是 `getWin1251()` —— 就是那条「俄站**编码**要专门验」的实例。
**`eztv` 的 451**（Unavailable For Legal Reasons）是法律性封锁 —— 写多少 UA 都没用，
和 Cloudflare 的 403 要分开对待。

### 🚧 受阻引擎清单（2026-10-09，**先记下来，以后再看有没有办法**）

用户口径：**先搬能用的，不能用的记录在案**。按「有没有救」分四档：

| 档 | 引擎 | 现象 | 还有什么办法 |
|---|---|---|---|
| **A. 基本没救** | `eztv` | **451 法律性封锁**（两个环境都是） | 没有。法律性封锁不是技术问题 |
| | `blueroms` / `torrentdatabase` / `uindex` / `1337x` | **Cloudflare 403**（两个环境都是） | 参考 1337x 小节那四条路（换环境抓 fixture / 第三方 API / 浏览器自动化 / 放弃），成本都高 |
| **B. 机房 IP 被拦，本机可用** | `tpb` / `limetorrents` / `therarbg` / `torrent9` / `oxtorrent` | 本机 ✅ 出结果，CI ⛔ 403 | **不是问题** —— 用户在家里跑就能用。只是别拿 CI 的 403 当真相 |
| **C. 两个环境都不通** | `anirena` / `bitsearch` / `bt4g` / `mypornclub` / `tokyotoshokan` / `torrentdownload` / `torrentdownloads` / `torrentkitty` / `xxxclub` | CI ⛔ 403，本机 timeout/连不上 | CI 全 403 → 大概率也是 Cloudflare；要确认得**换个住宅 IP 的环境**再探一次 |
| | `sukebei` / `nekobt` | 两边都连不上（`UND_ERR_CONNECT_TIMEOUT` / timeout） | 确认域名是否还活着（`sukebei` 是 `nyaa.si` 的子域，可能一起被网络层挡了） |
| **D. 页面在但拿不到结果行** | `animetosho` / `mikan` | 两边都 200，但粗判 0 行 | **不是坏事** —— 很可能只是我的粗判标记（数 `<tr`）跟它们的结构不符。按站点核一遍真实选择器即可 |
| | `anilibria` / `subsplease` / `bangumimoe` | 200 但响应只有 2~55 字节 | 空 JSON（`[]`）—— 说明接口通、只是这个关键词没结果。**换关键词再探一次就能定性** |

**重新评估的触发条件**（满足任一条就该重探，不用等）：
1. 探活里某个引擎从 ⛔/❌ 变成 200+有行（可能只是它自己换了域名/放开了）
2. 有志愿者/上游给出可用的镜像或 API
3. 真要上浏览器自动化（CDP）时 —— 那是一次性成本，摊到多个 Cloudflare 站上才划算

⚠️ 别忘了「探活 ≠ 能解析」：`provider-health.mjs` 的行数是**粗判**。
D 档那几个要真搬，第一步仍然是**加探针、拿 cheerio 真值**（见下面的移植配方）。

---

### ⚠️ 本机 `gh` 会间歇性 403 —— 一律用 `scripts/gh-retry.sh`

本机有一层**透明拦截代理**，对 `api.github.com` 的请求会**间歇性直接返回 403**
（0.08 秒秒拒、响应体为空、带 `Via: Caddy`）。实测 2026-10-09 各路线成功率：

| 路线 | 成功率 |
|---|---|
| 本机代理 `:1267` | 9/10 → 8/12 → 4/12（会波动） |
| 本机代理 `:80` | 5/12 |
| 直连（绕开代理） | 7/10 |

**哪条路都不稳，而且 403 成簇出现（见过连续 5 次）。** 这不是 GitHub 拒绝、也不是 token 问题 ——
`gh auth status` 是 ✓，token 是 `gho_` 格式且 scope 含 `repo` / `workflow` / `admin:org`。

→ **把命令里的 `gh` 换成 `scripts/gh-retry.sh`**（同一个命令，参数原样传）：

```bash
scripts/gh-retry.sh run list --branch feat/rust
scripts/gh-retry.sh run view --job=<JOB_ID> --log
scripts/gh-retry.sh workflow run live-smoke.yml --ref feat/rust
```

只在输出含 `403` 时重试（其它错误立刻透传），stdout 不缓冲所以 `--log` / `watch` 照常。

⚠️ **旧笔记「`gh` 必须绕开代理（加 `env -u HTTP_PROXY ...`）」已失效** ——
那是 2026-10-07 在另一个代理实例上的观察；现在绕不绕都会间歇 403。

⚠️ **本机 `git push` 会报 SSL 证书错**（`fatal: unable to access …: SSL certificate problem:
unable to get local issuer certificate`）—— 同上，本机那层 MITM 代理的证书不在
`~/.gitconfig` 指向的 `C:\Users\LeGo\.git-ca-bundle.crt` 里。临时绕：

```bash
GIT_SSL_NO_VERIFY=true git push origin feat/rust
```

（2026-10-10 实测可用；`scripts/gh-retry.sh` 走的是 `gh` 自己的 TLS 栈，不受影响。）

---

⚠️ **`gh workflow run <file>` 要求 workflow 已存在于默认分支（`main`）上**，否则报
`HTTP 404: workflow ... not found on the default branch`。本项目 workflow 目前的状态：

| workflow | 在哪 | 能手动触发吗 |
|---|---|---|
| `build.yml` / `release.yml` / `tauri-build.yml` | `main` | 能 |
| `live-smoke.yml` | `main`（**为能手动触发而特意放的**）+ `feat/rust` | 能（触发时用 `--ref feat/rust` 才检出 Rust 代码） |
| `rust.yml` | `main` + `feat/rust`（2026-10-16 切 main 后**必须**在 main 上） | 能（`--ref main`） |
| `html-probes.yml` | `main` + `feat/rust` | 能（选分支 `feat/rust`，生成 cheerio 真值**并自己提交**） |
| `fetch-fixture.yml` | `main` + `feat/rust` | 能（带 `url`/`path`，让 CI 去抓本机探不通的页面快照，抓完自己提交） |
| `provider-health.yml` | `main` + `feat/rust` | 能（全部 40 个引擎的联网探活） |

`live-smoke.yml` 用 `hashFiles('crates/bt-providers/Cargo.toml')` 兜底：
在还没有 Rust workspace 的 ref 上会优雅跳过，不给假红灯。
`html-probes.yml` 同理（判 `test/fixtures/html-probes.json` 在不在）。
改这两个文件时记得 **`main` 与 `feat/rust` 上各有一份，要同步**。

Node 侧仍然使用 **Node.js 内置 `assert` 模块**做 golden-file 测试，无第三方框架：

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
- ✅ `knaben.js` — JSON API (POST) parsing, category mapping
- ✅ `linuxtracker.js` — HTML parsing, infoHash extraction from URL, date/size/seeds parsing
- ✅ `filemood.js` — HTML parsing, infoHash extraction from detail URL, size/seeds parsing
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
public/               ← static frontend (index.html, styles.css, ambient.js)
public/js/            ← ES modules (state, render, actions, history, settings, utils, main)
server.js             ← Express entry point; exports { app, start }
electron/main.js      ← Electron wrapper; requires server.js and calls start(port)
src/providers/        ← one file per search engine (42 built-in + Torznab)
src/lib/http.js       ← shared axios instance (UA rotation, 10 s timeout, never throws)
src/lib/normalize.js  ← size/date/magnet parsing → canonical TorrentResult shape
src/lib/mirrors.js    ← mirror retry helper (runMirrors)
src/lib/scraper.js    ← createProvider() factory + scrapeRows() helper
src/lib/downloaders.js← qB/TR/aria2/Gopeed push/test/detect
```

- `server.js` only auto-listens when run directly (`require.main === module`). When required by Electron it returns the `app` without binding.
- All providers export `search(query, { page }) → { results, error, hasMore }`. Add new engines here.
- `src/lib/http.js` wrappers (`getText`, `getJSON`, `postJSON`) never throw; they return `{ data|html, error }`. Match this pattern in new providers.
- `src/lib/scraper.js` provides `createProvider({ id, name, mirrors, searchOn })` to eliminate boilerplate.
- The `demo` provider is offline-only and always enabled — useful for testing the UI without network.

## Adding a provider

1. Create `src/providers/<name>.js` exporting `{ id, name, search }`.
   - Use `createProvider({ id, name, mirrors, searchOn })` from `src/lib/scraper.js` for the common pattern.
   - Use `scrapeRows()` for simple row-iteration HTML scraping.
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

1. ~~**Safe Mode**~~ ✅ 已完成 — one toggle that auto-disables NSFW providers and hides NSFW results.
2. ~~**Viewed / dead-torrent filtering**~~ ✅ 已完成 — mark "already viewed" results as dimmed, stored in localStorage.
3. ~~**Browse (top/latest)**~~ ✅ 已完成 — 无关键词浏览最新/热门，`browseable: true` 标记支持引擎（当前 Demo + YTS），`/api/search?browse=1` 只查询这些引擎。
4. **Bookmarks export/import** — we already persist favorites in localStorage; upstream
   adds export-to-file / import. Natural for a desktop app; guards against cache clears.
5. ~~**Richer details (poster/screenshots/description)**~~ ✅ 已完成（海报/描述，通用 og:meta 提取）— upstream detail screen has
   media poster, screenshot previews, Markdown description. Lower ROI, nice-to-have.

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
`main` 原有 `build.yml`，2026-08-14 已同步补齐 `release.yml` / `tauri-build.yml`（两边现一致）；
正式发布（含 Tauri 产物）走 `release.yml`（建议从 `feat/tauri` 打 tag，已包含 Tauri 源码）。

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

### ✅ 已修复（2026-08-20 审查新发现）

修复 Tauri sidecar 后顺带复查 `server.js` / `public/app.js` / `src/lib/*`。

1. ✅ **CSV 公式注入**（`public/app.js` `batchExportCsv`）：`cell()` 已加 `/^[=+\-@\t\r]/` 前缀单引号防御。
2. ✅ **localStorage 解析无容错**：已抽 `loadJSON(key, fallback)` 统一容错，覆盖 history/favorites。
3. ✅ **DNS rebinding 防御**：`server.js` 已加 Host header 校验中间件，非 127.0.0.1/localhost 返回 403。

### ✅ 已修复（2026-08-23 全量审查）

以下从编码者角度梳理的优化项，已全部收口：

1. ✅ **`public/app.js` 1634 行单体巨石** — 已拆为 7 个 ES 模块：`js/state.js`、`js/render.js`、`js/actions.js`、`js/history.js`、`js/settings.js`、`js/utils.js`、`js/main.js`。
   ⚠️ **该文件本身已于 2026-10-11 删除**（拆完一直留着，见上面「已删（已做）」）。
2. ✅ **Provider 解析逻辑重复** — 新建 `src/lib/scraper.js`：`createProvider()` 工厂 + `scrapeRows()` 辅助，已迁移 eztv/torrentkitty/torrentdownload。
3. ✅ **请求速率控制** — `src/providers/index.js`：`asyncPool()` 限制 8 并发，替代 `Promise.all` 无限制请求。
4. ✅ **`PROVIDER_LABEL` 初始态不完整** — 已预填充全部 42 个引擎名称，消除首屏 raw id 闪现。
5. ~~Build 步骤 — `esbuild` minify → `npm run build:frontend`，输出到 `public/dist/`。~~
   ❌ **已删除（2026-10-16 审计）**：`public/index.html` 只加载 `ambient.js` + `js/main.js`，
   `build.yml` 也从不调它 ⇒ 产出的 `public/dist/` **没有任何东西会加载**，
   这条「已完成」是假的。脚本、npm script、`esbuild` 依赖一并删掉
   （Rust 版终局会连 `public/` 一起删，给它接线没有意义）。
6. ✅ **Provider 级别超时策略** — `scrapeRows`/`createProvider` 支持 `timeout` 选项，各 provider 可覆盖默认 10s。

### ✅ 已修复（本轮审查 — phantom fixes 落地，2026-08-23）

以下 8 项此前仅在文档中标记为已修复，代码实际未落地，现已全部收口：

1. ✅ **SSE 监听器泄漏** — `done`/`error` 监听器加 `{ once: true }`。
2. ✅ **筛选输入无防抖** — 加 150ms debounce。
3. ✅ **`relevanceScore` 副作用** — 改为临时 `scored` 数组，不修改 `state.groups`。
4. ✅ **「打开磁力」新标签** — 改为 `window.open(m, '_blank', 'noopener')`。
5. ✅ **搜索结果计数** — 加 `#result-summary` 概览行（筛选后 / 去重前 / 排序 / 第 N 页）。
6. ✅ **清空搜索历史确认** — 加 `confirm()` 二次确认。
7. ✅ **搜索可取消** — 搜索中按钮变红「取消」，点击中断 SSE 流。
8. ✅ **批量推送并发** — 改为每批 5 条 `Promise.allSettled`。

### ✅ 已修复（本轮审查 — 剩余未修项，2026-08-23 收口）

1. ✅ **无限滚动无页码指示** — 已加 `#result-summary` 概览行（含第 N 页）+ `#back-to-top` 按钮。
2. ✅ **引擎状态搜索前不可见** — `renderStatus({})` 从缓存恢复上次状态，init 时调用。
3. ✅ **缺 lint/format 工具链** — 已加 ESLint + Prettier（`npm run lint` / `npm run format`）。
4. **测试覆盖不足** — 36 个 provider 仍零覆盖。当前 `tpb.js`、`knaben.js`、`linuxtracker.js`、`filemood.js`、`normalize.js` 有测试。（待后续）
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

⚠️ **`public/app.js` 已于 2026-10-11 从三个分支全部删除**（1634 行死代码，见「已删（已做）」）。
下面这一段**只作为历史背景**保留（解释当初为什么会存在两份 app.js）—— 文件已经没了，别再去同步它。

> ~~`feat/tauri` 的 `public/app.js` 整体落后于 `main`，不是「另一个修好的版本」。~~
> ~~它是多客户端改造**之前**的 qB-only 版本（`state.qb` / `autoDetectQB()` / `#qb-url` 系列 DOM）。~~
> ~~往那边找 bug 修复会白跑一趟；反过来，从 `main` 同步下载相关功能到 `feat/tauri` 时，
> 要连带把整套多客户端改造（含下面的 6 处修名）一起带过去，不能只摘单个提交。~~

**~~Known recurring conflict — `PROVIDER_LABEL` / `loadProviders`~~ 已消解（2026-08-10）:**
两分支曾在这里分叉——`main` 是只含 ~4 项的静态字面量，`feat/tauri` 是 `PROVIDER_LABEL = {}`
加 `loadProviders` 里 `providers.forEach((p) => { PROVIDER_LABEL[p.id] = p.name; });` 动态填充。
现在 `main` 已采用与 `feat/tauri` 相同的动态填充写法，这段代码两边一致，不再产生冲突。

若日后再在此处分叉：解冲突时取 incoming（`main`）逻辑，**但务必保留那行动态填充**，
丢了它会让所有徽章/状态栏显示名退化成 provider 原始 id。

## 历史：Node / Electron 时代的收尾记录（⚠️ **不是当前计划**）

> ⚠️ 本节是**历史存档**：其中"下一个里程碑是 M1"之类的话早已过期 —— **M0–M7 全部完成**（见 `docs/ROADMAP.md` §3）。
> 下面这一段记录的是 **Rust 重写之前** Node/Electron 那条线的收尾工作，保留只为查历史。

### ✅ 已完成：多客户端下载器同步到 `feat/tauri`

⚠️ **以下描述的是 Node 版（`public/app.js` 已于 2026-10-11 删除，见「已删（已做）」）。**
当时 `main` 和 `feat/tauri` 两分支的 `public/app.js` 完全一致，均支持：
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
3. ~~Browse 浏览~~ ✅ 已完成（Demo + YTS）
4. ~~详情海报/描述~~ ✅ 已完成（og:meta 通用提取）
5. 收藏导出/导入（ROI 递减，待做）

### ✅ 已完成：本轮全量优化（2026-08-23）

编码者 6 项 + 用户 6 项，详见 README 路线图。
- 前端模块化拆分（1634 行 → 7 个 ES 模块）
- Provider 工厂 + scrapeRows 辅助
- 搜索并发控制（8 路）
- PROVIDER_LABEL 预填充
- esbuild 构建步骤
- 差异化超时
- 键盘快捷键 / 亮色主题 / 收藏搜索 / 引擎预设 / 移动端适配 / 导出完整性提示

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

---

## 仓库纪律与事故复盘（2026-08-14 恢复，务必阅读）

### 事故简述

2026-08-14 本地项目仓库的 `.git` 被重建（`git init` + 整个工作区打成 1 个
`commit (initial)` + 重建 tag），**全部历史与 `feat/tauri` 分支从本地消失**，
本地看起来「release 构建流程丢了」（release.yml/tauri-build.yml 本来只在
feat/tauri）。实际流程从未丢失：GitHub 远程与父仓库 `D:\Vibe-Coding` 都完整
保留了历史（远程 main=bd289f5c 系、feat/tauri=dc05e1c；父仓库 main=7e9c774、
feat/tauri=dc05e1c，90 条提交 + 4 个真 tag）。

### 恢复方法（存档，供再次发生同类事故时参考）

`git fetch` 在受限环境下可能失败（git 本地传输要起子进程建管道被沙箱拦截），
此时可用**纯文件级恢复**，全程不碰子进程管道：

```bash
# 1) 把父仓库的 pack 复制进项目 .git（对象全部来自父仓库）
Copy-Item D:\Vibe-Coding\.git\objects\pack\pack-*.pack  .git\objects\pack\
Copy-Item D:\Vibe-Coding\.git\objects\pack\pack-*.idx   .git\objects\pack\
# 2) 删除过期的 multi-pack-index（否则新 pack 可能查不到）
Remove-Item .git\objects\pack\multi-pack-index -Force
# 3) 用 update-ref 直接写引用（单进程，无子进程）
git update-ref refs/remotes/local/main      7e9c774b52079e2741db022bc73406603b6aefd1
git update-ref refs/remotes/local/feat/tauri dc05e1c9518233a09990ce1a8c70110777143417
git update-ref refs/tags/v0.3.0-beta        c8eb5dd5dd44e52e83d96c05b7730267efe66b25
# 4) 对齐分支
git checkout -B main refs/remotes/local/main
git branch feat/tauri refs/remotes/local/feat/tauri
# 5) 找回未提交工作（若曾 stash）：git stash pop
```

`refs/remotes/local/*` 是恢复时留下的线索 ref，可随时删除。
恢复后记得 `git fetch origin` 与远程对齐，**不要 force-push**。

### 纪律（每条都是一次事故换来的）

1. **仓库边界**：任何 git 操作前先 `git rev-parse --show-toplevel` 确认当前仓库。
   父目录 `D:\Vibe-Coding` 是独立仓库（保留本项目完整历史，无 remote）；
   在 `torrent-search-app/` 目录里工作时只操作本项目仓库。
2. **禁止 `git init` + 单提交来「迁移/重建」仓库**：会丢失全部历史。
   拆分/迁移必须用 `git clone` / `git worktree` / `git filter-repo`。
3. **未提交工作当日提交并推送**：远程是唯一异地备份。这次 bitsearch/rutor/sukebei
   等修复「做了没入库」，全靠恢复才找回来。
4. **tag 纪律**：用 `git tag -a` 打注释 tag，`git push origin <tag>` 并确认远程
   后再算完成；打 tag 前先跑 `npm run release:check`（检查 feat/tauri 分支、
   release.yml/tauri-build.yml 是否存在、工作区是否干净）。
   ⚠️ **不要创建与分支同名的 tag**（如 `feat/tauri`）—— 短名 ref 解析会被 tag 抢走，
   `git rev-parse` / `cat-file` / `git describe` 全都会指向**另一个提交**，
   连带 `release:check` 查错 ref、文档里的 `git worktree add <tmp> feat/tauri` 检出错误提交。
   2026-10-11 实测踩到（那个 tag 指向 main 线旧提交 `bd289f5`）：已删除（本地+远程），
   并在 `scripts/release-check.js` 里统一改用完整 ref。发版 tag 一律 `v*`。
5. **依赖安装用 `npm ci`**（严格按 lockfile + tarball 完整性校验）；在残缺的
   node_modules 上 `npm install` 会打地鼠（本仓库曾出现 axios/express 缺失、
   asynckit/debug/iconv-lite 文件级损坏）。`npm install <pkg>` 会把传递依赖
   误写进 package.json 的 dependencies，改完要检查。
6. **main 与 feat/tauri 的修复必须同步**（见上文铁律），发布入口是
   `feat/tauri` 的 `release.yml`（Electron+Tauri 双构建）。
