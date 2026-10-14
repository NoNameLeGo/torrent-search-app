# bt-app 设计草案（阶段一收口）

> **状态：草案 · 部分已拍板 · 未写代码**
> 日期：2026-10-11
> 依据：`main`/`feat/rust` 上的实测（不是文档转述），逐条标注了来源。
> 目的：在动手写 `bt-app` 之前，把**还没记录下来的决策**先定下来。
> 其中「决策 1」不拍板就没法开工 —— 它直接决定 `public/ 一行不改` 这条承诺能否成立。
>
> ✅ **2026-10-11 已拍板**：**D3 = 口径 C** —— 切 `main` 的门 = 「聚合层跑通」，
> 不要求「40 个引擎齐」、也不把差分门当硬门（G1/G2 降为自选验证工具）。
> 已同步进 `AGENTS.md` 的 Q4 行。**仍待拍板：D1·D2·D4~D7**。

---

## 0. 为什么先写这份

现状（实测）：

| 项 | 值 |
|---|---|
| 已搬 provider | **16 / 40**（5 JSON + 11 HTML） |
| `bt-core` / `bt-providers` | 4 文件 / 16 provider（5110 行 src + 5058 行 tests） |
| 聚合层（`bt-app`/`bt-torznab`/`bt-downloaders`） | **全部不存在 = 0%** |
| 待搬 JS 聚合层 | **1071 行**（`server.js` 274 + `index.js` 271 + `downloaders.js` 214 + `torznab.js` 133 + `torznabStore.js` 93 + `mirrors.js` 29 + `scraper.js` 57） |

阶段一的收益（内存 / 体积、干掉 89MB node sidecar）**100% 由聚合层决定，与 provider 数量无关**。
而剩余 24 个 provider 里，按 `AGENTS.md` 自己的受阻清单，**5 个确认已死**（`eztv` 451 法律封锁 ／ `1337x`/`blueroms`/`torrentdatabase`/`uindex` Cloudflare）、
约 8 个两边环境都不通、真正可搬的只有 5~6 个 D 档站。

→ **继续按 provider 数量往前推，边际收益已经很低。** 本文件建议把顺序翻过来。

---

## 1. 阶段一的验收定义（可执行，非口号）

> 原口径：「Rust 版的搜索结果必须和 Node 版**逐字段一致**」。

问题：现在这句话**没有自动化的执行方式**。目前是靠「手写 Node 脚本喂 fixture、把期望值抄进 Rust 断言」——
这验证的是**单 provider 解析**，不是**端到端一致**。并发调度、去重、hasMore 聚合、
SSE 事件顺序、`resolveMagnet` 回退链路这些真风险，全部要等到最后一步才暴露。

建议改成两条**可执行**的门：

| # | 门 | 手段 | 性质 |
|---|---|---|---|
| G1 | 单 provider 解析与 Node 逐字段一致 | 自动化 fixture 差分（§6） | 离线、确定性、每次 push 都跑 |
| G2 | 端到端（聚合 + SSE + 去重）与 Node 一致 | 同一个 fixture 回放服务，两个后端各打一遍，比对最终结果集 | 离线、确定性、每次 push 都跑 |

**核心设计决定：G1/G2 都走「fixture 回放」，不打真站。**

> ⚠️ **口径区分（重要）**：G1/G2 是**验证门**（盯「有没有解析回归」），
> **不是「切 `main` 的门」**。切分支的门 2026-10-11 已拍板 = **「聚合层跑通」**（口径 C），
> 差分只算**自选工具** —— 不做也不阻塞切换。
理由：真站两次请求之间内容会变（甚至限流），线上差分天然 flaky，测出来的红灯没人信。
回放则完全确定，且正好等于验收口径里的「逐字段一致」。

---

## 2. 决策 1（**必须拍板**）：WebView ↔ Rust 怎么通信

`AGENTS.md` 写的是「Tauri 壳**直接跑** Rust 核心」，但**没说 WebView 怎么跟它说话**。
这个缺口决定 `public/ 一行不改` 能否成立。

### 实测到的约束

前端**只用相对 URL**，实测全部 `/api/*` 调用点（`public/js/*.js`）：

```
/api/providers          /api/torznab          (GET/POST/DELETE)
/api/search/stream      /api/torznab/test
/api/magnet             /api/download/push
/api/details            /api/download/test
                        /api/download/detect
```

**没有一处出现 `http://localhost:3000` 这类绝对地址**，全是 `/api/...`；
SSE 走 `new EventSource('/api/search/stream?…')`。

### 两个选项

| | **A. 同源：Rust 起本地 axum 服务** | **B. Tauri IPC command 替代 `/api/*`** |
|---|---|---|
| `public/` 改动 | **0 行**（相对 URL 原样可用） | **必须改**：所有 `fetch`/`EventSource` 换成 `invoke()` |
| SSE | axum 原生支持 | Tauri 无 SSE；要自己用 channel 模拟事件流 |
| 前端可独立调试 | ✅ 浏览器直接开 | ❌ 必须跑在 Tauri 里 |
| 保留的代价 | 一个回环端口 + `Host` 头校验 | 无端口 |
| 与 `server.js` 的对应关系 | **逐条可翻译**（含 SSE） | 需要重新设计整层契约 |
| 端到端差分（G2） | ✅ 两个后端都是 HTTP，可直接比对 | ❌ Rust 侧没有 HTTP 面，没法跟 Node 比 |

### 建议：**选 A**

理由：
1. 实测前端零绝对地址 → A 真的能做到 `public/ 一行不改`；B 必然食言。
2. **G2 只有 A 做得到** —— 差分需要两个后端形状相同。
3. `server.js` 的 274 行几乎可以逐行翻译，B 则是重写。
4. 桌面单机应用绑回环（`127.0.0.1`）本身不是新成本 —— Node 版就是这么干的。

**顺带把已有的安全措施照搬**（`server.js` 里两条，都是**不能省**的）：
- `Host` 头校验：非 `127.0.0.1`/`localhost` → 403（防 DNS rebinding 读 `/api/torznab` 里的明文 API key）
- `safeHttpUrl()`：只放行 `http:`/`https:`（`/api/magnet`、`/api/details`、`/api/download/*` 都是 SSRF-ish 代理）
- 监听绑 `127.0.0.1`（**别改**）

> ⚠️ 这三条属于「永不简化」类：是信任边界上的输入校验，不是样板代码。

---

## 3. 决策 2：crate 边界

```
crates/bt-core/            ← 已有：类型 / normalize / http / dom
crates/bt-providers/       ← 已有：16 个 provider + 注册表
crates/bt-torznab/         ← 新建
  store.rs                 索引器持久化（data/torznab.json，写临时文件再 rename）
  client.rs                t=caps 探测 + 搜索（XML → TorrentResult）
crates/bt-downloaders/     ← 新建
  四个客户端：qbittorrent / transmission / aria2·motrix / gopeed
  META（标签 + 需要哪些认证字段）+ push / test / detect
crates/bt-app/             ← 新建
  src/lib.rs               serve(port) —— 路由 + SSE + 聚合
  src/aggregate.rs         resolveTargets / asyncPool(8) / aggregateHasMore
  src/magnet.rs            惰性磁力解析（含通用回退，见 §5.4）
  src/details.rs           og:meta 提取（海报 / 描述）
  src/main.rs              ← 独立可执行：cargo run -p bt-app ⇒ 一个能跟 Node 对比的 HTTP 服务
src-tauri/                 ← 从 feat/tauri 分支搬进来，改成薄壳
```

**两个刻意的「不建」**（YAGNI）：

- **不建 `bt-aggregate` crate** —— 聚合一共约 150 行，放进 `bt-app` 就够了，单开 crate 属于给一个消费者建抽象。
- **仍不建 `Provider` trait** —— 见下。

### 3.1 关于 `Provider` trait：**先建 helper，别建 trait**

实测：`pub async fn search_with(...)`（`runMirrors` 的复刻）在 **10 个文件**里逐字重复，
每份约 15 行，**只有错误串里的站点名不同**；另有 `search` 3 种签名、`search_at` 4 种签名。
`lib.rs` 与 `AGENTS.md` 自己都记着「再拖就是重复代码在同一形状上多抄几遍」。

但**这一批差值是「站点名」而不是「行为」**，那是 helper 的活，不是 trait 的活。
trait 真正被需要是在 `bt-app` 要**并发遍历异构 provider 列表**时 —— 而 `resolve_magnet`
只有 **6 / 16** 个 provider 有（可选能力），trait 设计到那一刻才会变复杂。
**现在定必然定错。**

→ ✅ **2026-10-11 已落地**：`bt-providers::run_mirrors`（净 −73 行；10 个 provider 的
`search_with` 各变一行，签名见 `crates/bt-providers/src/lib.rs`）。
trait 留到 `bt-app` 真开始写聚合时，看清「可选方法」怎么表达再定。

> ⚠️ **顺序 vs 并行**：JS 的 `runMirrors` 用 `Promise.allSettled` 是**并行**，
> Rust 侧是**顺序** `await`（最坏 `bases.len() × 10s`）。
> 2026-10-11 已在 `run_mirrors` 的文档注释里写明这是**有意 divergence**
> （此前 10 份副本里一处都没说明）。**要不要改成并行 = 本文的 D2，待拍板。**

---

## 4. 需要复刻的语义（照 `src/providers/index.js` 逐条）

### 4.1 目标筛选 `resolveTargets`

```js
if (!p.enabled || (wanted && !wanted.includes(p.id))) return false;
if (browse && !p.browseable) return false;              // 无关键词浏览：只查 browseable
if (page > 1 && !p.paginated && !p.demo) return false;  // 第 2 页起：丢掉单发引擎
```

⚠️ 第 3 条是**为了防重复**：单发引擎忽略 `page`，翻页只会返回同一批结果再被去重掉。

### 4.2 并发池 `asyncPool(tasks, 8, onSettle)`

- **上限 8**（`MAX_CONCURRENT`，注释写明是「够灌满管道又不至于像 DDoS」）
- ⚠️ **`onSettle` 必须在下一个任务开始前调用** —— 这是 `searchStream` 能「谁先回来先推谁」的原因。
  Rust 侧如果写成 `join_all` 再统一 emit，**SSE 就不是流式了**（用户会觉得变慢）。
  这是本层最容易写错、且错了不明显的一条。
- 单 provider 失败**不影响别人**（`runProvider` 内部 try/catch，错误进 `status.error`）
- `ms` 计时：每个 provider 单独算

### 4.3 hasMore 聚合

```js
if (!s || s.status !== 'ok' || s.count === 0) return false;   // 出错或没结果 → 没下一页
if (typeof s.hasMore === 'boolean') return s.hasMore;         // provider 明确说的优先（demo.js）
return !!p.paginated;                                         // 否则按 paginated 猜
```

### 4.4 SSE 事件（**逐字节契约**，来自 `public/js/main.js:153-200`）

```
event: provider
data: {"id":"…","name":"…","results":[…],"status":{"status":"ok|error","count":N,"error":null|"…","hasMore":bool,"ms":N}}

event: done
data: {"hasMore":bool}

event: error
data: {"error":"…"}
```

- 响应头：`Content-Type: text/event-stream`、`Cache-Control: no-cache`、`Connection: keep-alive`，写完立即 `flushHeaders`
- 客户端 `close` 后必须**停止工作**（JS 里是 `closed` 标志；Rust 侧要 drop 掉 stream）
- 前端收到的 `results` 会**按 `it.id` 去重**，再**按 `infoHash`（小写）分组**
  → 所以 `id` 与 `infoHash` 的生成规则必须与 `normalize.rs` 完全一致（已搬，应无风险）

### 4.5 端点清单（按前端是否真的调用分级）

**必须实现**（`public/` 真在用）：
`GET /api/providers` · `GET /api/search/stream` · `GET /api/magnet` · `GET /api/details`
`POST /api/download/push` · `POST /api/download/test` · `GET /api/download/detect`
`GET|POST /api/torznab` · `DELETE /api/torznab/:id` · `POST /api/torznab/test`

**前端没在用（实测 `public/js/` 里零调用点）** —— 建议**先不做**：
`GET /api/search`（非流式版）· `GET /api/download/clients` · `GET /api/health`
`POST /api/download/qbittorrent`（旧 qB-only 兼容路径，`localStorage` 键迁移已完成，无老客户端）

> 这 4 条属于 YAGNI：前端不调，做出来就是没人走的代码路径。
> 若担心外部脚本在用（curl 调试），保留 `GET /api/search` 一条即可，其余砍掉。

---

## 5. 移植中发现的两个真实缺陷（建议在 bt-app 里修掉）

### 5.1 ⚠️ 「通用磁力回退」指向一个**已知死亡**的引擎

`server.js:/api/magnet` 的兜底逻辑是：provider 没有 `resolveMagnet` → **借用 `1337x` 的解析器**当通用抓取器
（「抓页面里第一条 `magnet:` 链接」）。

但 `1337x` 已在 `AGENTS.md` 里被判定 **Cloudflare + DNS 封锁，跳过、不移植**。
连带后果（文档里已记录）：`nyaa` 没有 resolver → 前端点「获取磁力」→ 打到 `/api/magnet` →
无 resolver → 回退到**已死的 1337x** → 报错。**nyaa 现在拿不到磁力。**

**建议**：把这个「抓第一条 `magnet:`」**提成独立的小函数**（它就是 5 行），不要挂在 `1337x` 身上。
这样 `nyaa` 那条链路自动被修好，而且不需要移植一个永远报错的 provider。

> 这是「照抄 JS」与「功能可用」冲突的地方，我倾向修 —— 与 `AGENTS.md` 里
> 「功能性 bug（死链）就修」的口径一致：回退到死引擎属于功能性失效，不是纯显示问题。

### 5.2 `public/app.js` 是 **1634 行死代码**

实测：`public/index.html` 只加载 `ambient.js` 与 **`js/main.js`**（ES module），
**没有任何地方引用 `app.js`**。

而 `AGENTS.md` 已把「拆成 7 个 ES 模块」标为 ✅ 完成 —— 但旧巨石没删，还留在 `public/` 里
（会被 `express.static` 直接服务、也会进打包产物）。

**建议：删掉。** 它已被 `public/js/*` 完全取代（7 模块 1769 行 ⊇ 旧 1634 行），
留着只有两个坏处：误导阅读者「到底哪份在跑」、并且被静态服务无谓暴露。

---

## 6. 差分测试方案（G1 + G2 的落地）

### G1：单 provider 逐字段差分（自动化，替代现在的手抄）

现在的「移植配方」第 3 步是**手工**：写临时 Node 脚本 → 把输出抄进 Rust 断言。
把它变成一条命令：

```
scripts/diff-providers.mjs
  ├─ 对每个 test/fixtures/<site>-<query>.{json,html}
  ├─ Node 侧：require('src/providers/<site>.js') + 现有 createMockHTTP() 喂 fixture → 规范化 JSON
  ├─ Rust 侧：cargo run -p bt-providers --example dump_provider -- <site> <fixture> → 规范化 JSON
  └─ 逐字段 diff；差异必须命中「已知 divergence 白名单」，否则非零退出
```

要件：
- 输出**规范化后**（`normalize()` 之后）的字段，不是原始 parse 结果 → 这才对应验收口径
- 排序稳定（按 `id`）后再比，避免顺序假差异
- **已知 divergence 白名单**：把已登记的 10 条（megapeer 编码 / 5-td 列、audiobookbay size/date、
  nyaa 列索引与 `seeders=2009`…）写成结构化清单，diff 命中即通过、**且必须命中**（防止哪天无意修好了却没人知道）

> 这一步的收益最大：**以后每搬一个 provider，验收从「手抄 + 人眼」变成一条命令。**

### G2：端到端差分（聚合 + SSE）

```
1. 起一个「fixture 回放 HTTP 服务」（复用 tests/common/mod.rs 的 oneshot 思路，改成常驻）
2. node server.js  → 指向上面的回放服务（需要一个小 swizzle：改各 provider 的 base URL）
3. cargo run -p bt-app → 同样指向回放服务
4. 同一个 query 打两边，比对：每个 provider 的 status、最终结果集（去重后）、hasMore、SSE 事件序列
```

⚠️ 第 2 步的「swizzle」是**唯一有成本的地方**（JS 侧目前 base URL 写死在各自 `DOMAINS` 里）。
两条路：
- **(a)** 给 Node 侧加一个仅测试用的 base 覆盖（改动小，但动了「不删不改进」的 JS）
- **(b)** 用 HTTP 代理：把 `HTTP_PROXY` 指到回放服务，它按 Host 路由到 fixture（Node 侧零改动，但代理实现更绕）

**建议先只做 G1。** G2 价值高但成本明显更高，且 G1 已经能锁住 90% 的「解析回归」风险。
G2 等 `bt-app` 跑起来、真有端到端 bug 需要抓时再做。（YAGNI）

---

## 7. 建议落地顺序

| 阶段 | 内容 | 产出 |
|---|---|---|
| ~~M0~~ | ~~`run_mirrors` helper~~ | ✅ **已完成 2026-10-11**（净 −73 行，见 `AGENTS.md` C 组） |
| **M1** | **拍板 D1**（WebView ↔ Rust 通信）+ D2/D4~D7 | 一个决定 —— **不拍 D1 就没法开工** |
| **M2** | `bt-app` 骨架：axum + `/api/providers` + `/api/search/stream`（先接现有 16 个 provider） | **能跑起来**，前端可连 |
| **M3** | `bt-downloaders`（4 客户端）+ `/api/magnet` + `/api/details` | `public/` 全部功能可用 |
| **M4** | `bt-torznab` + 索引器持久化 | 功能对齐完成 |
| **▶ 切点** | **← 切 `main` 的门在 M4 后即已满足**（口径 C：「聚合层跑通」） | 可以切 |
| **M5** | G1 差分工具 | 解析回归可自动盯（**自选工具，不是门**） |
| **M6** | `src-tauri` 薄壳接 `bt-app`，去掉 node sidecar | 体积 / 内存收益 |
| — | 之后继续补 D 档 provider | 边际收益期 |

**关键点：M2 之后每一步都产出「可运行、可对比」的东西**，而不是继续攒 provider。
原顺序把最大风险（端到端一致性）压到最后一步、中途没有任何端到端验证 —— 这是本草案最想改的一件事。

⚠️ **一个口径 C 带出来的顺序问题**（不在 D1~D7 里，但影响体感）：
切点在 **M4 之后**，而**内存 / 体积的收益要到 M6（换掉 node sidecar）才兑现**。
若 M4 就切 `main`，用户短期看到的仍是那个 89MB sidecar。
**建议把 M6 提到切点之前**（严格说不阻塞 —— 门已经满足）。

---

## 8. 未决项（需要你拍板）

| # | 问题 | 我的建议 | 影响面 |
|---|---|---|---|
| **D1** | WebView ↔ Rust：**同源 axum** 还是 Tauri IPC？ | **axum（§2）** | 决定 `public/ 一行不改` 能否成立；不决定不能开工 |
| **D2** | `runMirrors` 要**并行**还是保持**顺序**？ | 顺序够用，但要写成显式 divergence + 注释 | 最坏延迟差 `n × 10s` |
| ~~**D3**~~ | ~~切 `main` 的门改成什么~~ | ✅ **2026-10-11 已拍板（口径 C）：门 = 「聚合层跑通」**（不要求 40 个齐，差分不当硬门） | 已定 |
| **D4** | 「通用磁力回退」提成独立函数（修 nyaa）还是照抄 1337x？ | **提成独立函数** | 修好 nyaa 的取磁力 |
| ~~**D5**~~ | ~~`public/app.js` 删不删~~ | ✅ **2026-10-11 已删**（1634 行 / 69KB，全仓零功能性引用） | 已定 |
| **D6** | G2 端到端差分现在做还是推迟？ | **推迟**，先做 G1 | 成本 vs 覆盖率 |
| **D7** | 「前端没在用的 4 个端点」要不要实现？ | **不实现**（保留 `GET /api/search` 即可） | 少写死代码 |

---

## 9. 明确「不做」（YAGNI）

- ❌ 不为聚合层单开 `bt-aggregate` crate
- ❌ 现在不建 `Provider` trait（等 `bt-app` 真需要异构遍历时再定）
- ❌ 不引入 `p-limit` 之类的并发库（Tokio `Semaphore` 就是现成的；JS 侧注释自己也写了「simple pool」）
- ❌ 不实现前端没调用的端点（`/api/download/clients`、`/api/health`、旧 qB 兼容路径）
- ❌ 不做 G2 端到端差分（先 G1）
- ❌ 不动 `public/js/*`、不动 `src/providers/*.js`（过渡期对照物，见 `AGENTS.md`）

---

## 10. 与既有决策的关系

不推翻 `AGENTS.md` 的 5 条范围决策（Q1~Q5），只补两处：

- **补 Q4**：切 `main` 的门**改为「聚合层跑通」**（2026-10-11 拍板，口径 C）——
  **不要求 40 个引擎齐**，也**不把差分门当硬门**（G1/G2 降为自选验证工具，对应 D3）
- **补一条新决策**：WebView ↔ Rust 通信方案（对应 D1）—— 这是 Q2「先不动 UI / 复用 public」
  能落地的前提，原文没写。

Slint 探针结论（尤其 **#13548：中文标题 + 关键词高亮 + 自动换行会 panic**）保持原样留档，
阶段二再议 —— 那条风险点至今**未实测**，是选 Slint 的最大不确定性。
