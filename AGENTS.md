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
**结论：本地跑不了 cargo，Rust 的一切都必须走 CI。**

| 场景 | workflow | 触发 |
|---|---|---|
| Rust workspace（编译 + 测试） | `rust.yml` | push `feat/rust`、PR 改 `crates/**` |
| Electron 安装包 + portable | `build.yml` | push `main`、`v*` tag |
| Electron + Tauri 统一发版 | `release.yml` | `v*` tag |
| Tauri 编译校验 | `tauri-build.yml` | push `feat/tauri` |

**已知可回收空间**：`src-tauri/target` 约 1.1GB（`cargo clean` 即可，但本机已无 cargo）。

## 🦀 Rust 重写主线（`feat/rust` 分支，2026-10-08 起）

本项目**推倒重开、用 Rust 重写**，在 `feat/rust` 上进行，长期长成主分支。
`main` 上的 Node/Express + Electron/Tauri 实现继续可用；Rust 版每搬完一块就替代一块。

### 范围决策（2026-10-08 用户拍板，5 条 · 别再反复问）

| # | 问题 | 决定 |
|---|---|---|
| Q1 | 产品形态 | **只做桌面原生应用；浏览器 / 手机访问模式最终放弃**（过渡期它作为副产品先留着） |
| Q2 | UI 方案 | **先不动 UI**：复用现有 `public/` 前端把核心跑通；核心稳定后再实测 Slint 原生可不可行 |
| Q3 | 平台 | **暂时 Windows-only**（但代码里别写死平台相关的东西） |
| Q4 | 切换时机 | **核心齐了就切 `main`**（providers + 聚合层），UI 之后补 |
| Q5 | 站点改版怎么发现 | **CI 默认只跑离线 fixture**（稳定的门）；另加一条 `workflow_dispatch` **手动触发的联网冒烟**，允许失败 |

### 两阶段路线（由上述决策推导，有异议就说）

**阶段一（当前）—— Rust 核心 + 复用现有 WebView 前端**

- 补齐 `bt-providers` / `bt-torznab` / `bt-downloaders` / `bt-app`（聚合 + 8 路并发 + SSE）
- 壳：Tauri 壳**直接跑 Rust 核心**，干掉现在那个 89MB 的 node sidecar
- `public/` **一行不改** —— 阶段一结束时用户看到的界面和现在一样，但内存/体积大幅下降
- 技术验收点：Rust 版的搜索结果必须和 Node 版**逐字段一致**

**阶段二（阶段一稳了再启动，现阶段不要做）**

- 决定是否上 Slint 原生 UI
- 上：删 `public/` + Tauri 壳，并正式丢掉浏览器访问
- 不上：就停在阶段一（那终局就是「Rust 核心 + WebView」，与 Q1「桌面原生」有出入，届时再议）

**Slint 探针结论（留档）** —— `spike/slint-ui` 分支与 `slint-spike.yml` 已于 2026-10-09 删除，
结论必须留着，否则阶段二要重新踩一遍。要用时重建成本约 500 行。

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
两者合计才 460KB，却是逐行对照的参照物；删了等于凭记忆重写。

```
Cargo.toml              workspace 根（members = ["crates/*"]，exclude = ["src-tauri"]）
crates/bt-core/         领域类型 + 归一化 + HTTP 公共层 + HTML 解析层
  src/normalize.rs      ← src/lib/normalize.js 的语义等价移植
  tests/normalize.rs    ← test/normalize.test.js 的断言原样搬来
  src/http.rs           ← src/lib/http.js 的移植（契约：永不返回 Err）
  tests/http.rs         ← 自起本地一次性 HTTP 服务，全程无外网
  src/dom.rs            ← cheerio 的替代层（scraper 0.27 = html5ever + selectors）
  tests/dom_probes.rs   ← 选择器语义对照：与 cheerio 真值（28 条 probe）逐条比对
crates/bt-providers/    一文件一站，对齐 src/providers/*.js 的组织方式  src/lib.rs            SearchOutcome { results, error, has_more }
                        （暂不引入 Provider trait，等 3~5 个再定抽象）
  src/value.rs          Value → NumOrText / String / min 的公共转换（含 v2nt_nonzero）
  src/tpb.rs            ← src/providers/tpb.js（GET，apibay）
  src/knaben.rs         ← src/providers/knaben.js（POST，官方 JSON API）
  src/torrentscsv.rs    ← src/providers/torrentscsv.js（GET，简单一层数组）
  src/yts.rs            ← src/providers/yts.js（GET，电影→种子两层结构）
  src/internetarchive.rs ← src/providers/internetarchive.js（GET，advancedsearch JSON）
  src/linuxtracker.rs   ← src/providers/linuxtracker.js（HTML，bt_core::dom）
  src/filemood.rs       ← src/providers/filemood.js（HTML，bt_core::dom）
  src/rutor.rs          ← src/providers/rutor.js（HTML，bt_core::dom；俄站 UTF-8）
  tests/common/mod.rs   本地一次性 HTTP 服务 + fixture 加载（provider 测试公用）
                        oneshot() 只要响应；oneshot_capture() 另交出原始请求，用于钉请求契约
  tests/{tpb,knaben,torrentscsv,yts,internetarchive}.rs   各自的离线测试
  tests/live_smoke.rs   联网冒烟；**默认跳过**，只有 BT_LIVE_SMOKE=1 才打外网

（待建）crates/bt-torznab/ · bt-downloaders/ · bt-app/
```

**移植进度**：

| JS 源 | Rust 目标 | 验收 | 旧 JS 状态 |
|---|---|---|---|
| `src/lib/normalize.js`（161 行） | `crates/bt-core/src/normalize.rs` | `test/normalize.test.js` 的断言全搬到 `tests/normalize.rs`，**24 passed** | 保留(对照) |
| `src/lib/http.js`（71 行） | `crates/bt-core/src/http.rs` | `tests/http.rs` 覆盖成功 / 4xx / 5xx / JSON / 非 JSON / 超时 / 连接失败 / 请求头，**12 passed** | 保留(对照) |
| `src/providers/tpb.js`（53 行） | `tpb.rs` | 真 fixture（100 条）逐字段 + 三条失败路径，**7 passed** | **待删** |
| `src/providers/knaben.js`（70 行） | `knaben.rs` | 真 fixture 逐字段 + 分类区间 + 请求 body 契约 + divergence，**9 passed** | **待删** |
| `src/providers/torrentscsv.js`（32 行） | `torrentscsv.rs` | 真 fixture 首末条 + query 编码契约 + falsy 日期，**9 passed** | **待删** |
| `src/providers/yts.js`（55 行） | `yts.rs` | 真 fixture 三条结果 + 请求 URL 契约 + 缺省值 + 两条跳过规则，**11 passed** | **待删** |
| `src/providers/internetarchive.js`（58 行） | `internetarchive.rs` | **合成** fixture（见下）+ 分类映射 + item_size 三态 + `no_docs` 错误语义，**14 passed** | **待删** |
| `src/lib/scraper.js`（HTML 解析底座） | `crates/bt-core/src/dom.rs` | cheerio 语义对照层 + 11 条单测 + 28 条 probe 与 cheerio 1.2.0 真值逐条比对（`dom_probes` **通过**） | 保留(对照) |
| `src/providers/linuxtracker.js`（95 行） | `linuxtracker.rs` | 真 fixture：43 候选 → 18 条结果（三个数字都由 cheerio 交叉验证）+ 11 条集成 + 8 条单测 | **待删** |
| `src/providers/filemood.js`（74 行） | `filemood.rs` | 真 fixture：65 行 → 20 条数据行（cheerio 交叉验证）+ 11 条集成 + 11 条单测 | **待删** |
| `src/providers/rutor.js`（99 行） | `rutor.rs` | 真 fixture：101 行（含表头）→ 100 条（cheerio 交叉验证）+ 13 条集成 + 13 条单测 | **待删** |

（另有 `bt-providers` 的 36 条单元测试（测 `src/value.rs` + `linuxtracker.rs` + `filemood.rs` + `rutor.rs`）；
`tests/live_smoke.rs` 2 条默认跳过。合计 **174 passed / 0 failed**。）

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

**为什么放宽**：阶段一（Q4B）的技术验收点是「Rust 版结果与 Node 版**逐字段一致**」，
Node 源码在这个阶段是**验证工具**而不是待清垃圾。提前删掉 = 亲手毁掉对照物。

**待删清单（阶段一收尾 / 阶段二启动时批量处理）**：

| 路径 | 为何现在留着 | 何时可删 |
|---|---|---|
| `src/providers/tpb.js`、`knaben.js`、`torrentscsv.js`、`yts.js`、`internetarchive.js`、`linuxtracker.js`、`filemood.js`、`rutor.js` ✅已移植 | `test/run.js` 仍在跑 tpb/knaben/linuxtracker/filemood/rutor；离线对照要用 | `bt-app` 接通对应 provider + Node 测试块删除后 |
| `src/providers/*.js`（其余 34 个） | 尚未移植，是逐行对照的参照物 | 各自移植完成后按上条判断 |
| `src/lib/`、`server.js`、`test/run.js` | 聚合层尚未移植，本分支上 Node 版仍需可跑 | provider + 聚合全搬完 |
| `public/` | 阶段一的前端本体（`public/` 一行不改） | **阶段二**确认上 Slint 后 |
| `electron/`、`scripts/`、`package.json`、`start.bat`/`stop.bat`、`.eslintrc.json`、`.prettierrc`、`build.yml`/`release.yml`/`tauri-build.yml` | Rust 版尚未能取代旧 Shell 发版 | Rust 版能独立发版后 |

**已删（已做）**：构建产物 + 依赖缓存（`node_modules`、`src-tauri/target`、`src-tauri/binaries`）→ 省 1.35GB

**永久保留**：`test/fixtures/`（迁进 Rust 测试后也不删，是 golden 数据源）、
`LICENSE`、`SEARCH_ENGINE_PORT_COVERAGE.md`

⚠️ 反过来也要守：**别提前删还没搬的源码**。`src/providers/` + `public/` 一共才 460KB，
却是逐行对照的参照物；删了等于凭记忆重写。

### 📌 下一步计划（2026-10-08 用户定，**下次开工按这个走**）

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

**← 下次开工：按探活表挑「两边都 ✅」的继续搬（`audiobookbay` 36 行、`dmhy` CI 10 行、`nyaa` CI 2 行）。
`rutor` 已搬完，顺带把「俄站编码」验清楚了：**它是 UTF-8，不是 win1251**。
`1337x.js` 已调查完毕、判定当前不可抓取，**跳过**（见 B 组进度表下的专门小节）。**

**B 组（HTML 抓取，需引入 `scraper` crate 对标 cheerio）** —— ✅ 地基 + 3 个 provider 完成

- ✅ `crates/bt-core/src/dom.rs`：cheerio 的替代层（`scraper` 0.27 = html5ever + selectors）
  语义对照表写在文件头；自带 11 条单元测试
- ✅ 选择器等价性对照机制：`test/fixtures/html-probes.json`（**38 条 probe / 4 个 fixture**）+
  `scripts/html-probes.cjs`（cheerio 侧，真值来源）+ `crates/bt-core/tests/dom_probes.rs`（Rust 侧）
  + `test/fixtures/html-probes.expected.json`（真值，cheerio 1.2.0 生成）
- ✅ `linuxtracker.js` + `filemood.js` + `rutor.js` 三个 HTML provider 落地，全部离线可测

**B 组进度**

| # | provider | 状态 |
|---|---|---|
| 0 | 地基（`dom.rs` + 选择器对照机制） | ✅ 28 条 probe 与 cheerio 1.2.0 逐条一致 |
| 1 | `linuxtracker.js` | ✅ 18 条结果 + 11 集成 + 8 单测 |
| 2 | `filemood.js` | ✅ 20 条结果 + 11 集成 + 11 单测 |
| 3 | `rutor.js` | ✅ 100 条结果 + 13 集成 + 13 单测（**本机与 CI 两边都 ✅ 的第一个**） |
| 4 | `1337x.js` | ⛔ **已调查，当前不可抓取 → 跳过**，见下面专门小节 |
| — | 俄站（rutor 等，**编码**要专门验） | ⏭️ **下一个**：还没有 fixture，得先抓一个 |

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

口径：**功能性 bug（死链）就修，纯显示问题先照抄** —— 两条都在代码注释里写明了理由。
- HTML fixture 现状：

| fixture | 状态 |
|---|---|
| `linuxtracker-linux.html` | ✅ 真实结果页。43 个候选链接里 **33 条**是主表行（其余是 Top10 侧栏，行内只有 2 个 td） |
| `filemood-ubuntu.html` | ✅ 真实结果页。65 个 tr 里 **20 条**数据行 |
| `1337x-ubuntu.html` | ❌ **不是结果页** —— FingerprintJS 反爬跳转页（1.1KB，`window.location.replace`）。该域名 2026-10-09 已停放，详见上面的 `1337x.js` 小节 |
| 俄站（rutor 等） | 还没有 fixture；**编码**（UTF-8 vs win1251）要专门验，见「调试经验」 |

**C 组（基础设施，可穿插）**：`src/lib/scraper.js` 的 `createProvider` 工厂 + `runMirrors` 镜像回退 ——
搬完 3~5 个 provider、看清共性后再定抽象（A 组 5 个已落地，等 HTML 组也搬几个再一起看）。

### 🧱 HTML provider 的三条铁律（写自 `dom.rs` 与首次对照的实测）

1. **裸 `<tr>`/`<td>` 片段会被解析器丢掉**（文档模式下不合法的表格标签被忽略，文本留下）。
   写测试时随手塞 `<tr><td>x</td></tr>` 会得到 0 个匹配，看着像"选择器写错"，其实是 HTML 规则
   —— cheerio 的 parse5 行为完全一样。**片段要包进 `<table>`。**
2. **类名匹配区分大小写**，标签名不区分（HTML 规则）。`td.LISTA` 不会命中 `class="lista"`。
3. **属性值是解码后的**：源码 `&amp;` → 取值 `&`。所以 `[href*=...]` 是对解码后的值做子串匹配
   （linuxtracker 的 `href^="index.php"` 就是靠这个把侧栏和主表区分开的）。

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
- ⚠️ **没有 `cargo check`，签名错误只能靠 CI 发现**（一次往返 ~1.5 分钟）。已踩过的两类：
  1. **turbofish 泛型个数**：`bt-core` 的 `post_json<T, B>` 是两个泛型参数，
     `post_json::<Value>(...)` 会报 E0107。→ 改成在绑定上标注：
     `let resp: JsonResponse<Value> = http.post_json(url, &body, None).await;`
  2. **`and_then` 的闭包签名**：`extract_info_hash` 收的是 `Option<&str>` 而不是 `&str`，
     不能直接 `and_then(extract_info_hash)`，要么 `.or_else(|| f(x.as_deref()))`，
     要么写 `|s| ...` 闭包。
  提交前顺手核对一遍要调用的每个函数的真实签名，比等 CI 便宜。

### 🧩 provider 移植配方（照着 tpb / knaben 抄）

1. 读 `src/providers/<name>.js`，grep 出它调了 `getText` / `getJSON` / `postJSON` 哪些形态
2. 在 `crates/bt-providers/src/<name>.rs` 写 `KnabenRequest` 式的 payload + `search()` + `search_at(http, api, query)`
3. **期望值不要手抄** —— 写个临时 Node 脚本 require `src/lib/normalize.js`，
   把同一份 fixture 喂进去，把输出逐字段抄进 Rust 断言。这样「与 Node 版一致」才有实据
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
10. 移植中发现的上游 bug：**功能性 bug（死链）就修并单列 divergence 测试；
    纯显示问题先照抄**。已发现的三条登记在上面的「🐞 移植中发现的上游 bug」。

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

以后跑冒烟时，拿这些数字做对比：**数量级突然掉到 0 或个位数**才是真信号。
（2026-10-09 复跑：knaben 300 / torrentscsv 25 / yts 24 / internetarchive 96 / tpb 403，0 问题 —— 与基线一致。）

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

---

⚠️ **`gh workflow run <file>` 要求 workflow 已存在于默认分支（`main`）上**，否则报
`HTTP 404: workflow ... not found on the default branch`。本项目 workflow 目前的状态：

| workflow | 在哪 | 能手动触发吗 |
|---|---|---|
| `build.yml` / `release.yml` / `tauri-build.yml` | `main` | 能 |
| `live-smoke.yml` | `main`（**为能手动触发而特意放的**）+ `feat/rust` | 能（触发时用 `--ref feat/rust` 才检出 Rust 代码） |
| `rust.yml` | 只在 `feat/rust` | ❌ 不能，只能靠 push 触发 |
| `html-probes.yml` | `main` + `feat/rust` | 能（选分支 `feat/rust`，生成 cheerio 真值） |
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
2. ✅ **Provider 解析逻辑重复** — 新建 `src/lib/scraper.js`：`createProvider()` 工厂 + `scrapeRows()` 辅助，已迁移 eztv/torrentkitty/torrentdownload。
3. ✅ **请求速率控制** — `src/providers/index.js`：`asyncPool()` 限制 8 并发，替代 `Promise.all` 无限制请求。
4. ✅ **`PROVIDER_LABEL` 初始态不完整** — 已预填充全部 42 个引擎名称，消除首屏 raw id 闪现。
5. ✅ **Build 步骤** — `esbuild` minify → `npm run build:frontend`，输出到 `public/dist/`。
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
5. **依赖安装用 `npm ci`**（严格按 lockfile + tarball 完整性校验）；在残缺的
   node_modules 上 `npm install` 会打地鼠（本仓库曾出现 axios/express 缺失、
   asynckit/debug/iconv-lite 文件级损坏）。`npm install <pkg>` 会把传递依赖
   误写进 package.json 的 dependencies，改完要检查。
6. **main 与 feat/tauri 的修复必须同步**（见上文铁律），发布入口是
   `feat/tauri` 的 `release.yml`（Electron+Tauri 双构建）。
