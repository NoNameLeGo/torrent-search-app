# agent 操作手册

> 配套 `docs/ROADMAP.md`（why & where）。**本文件是 how** —— 每步怎么做、怎么算做完、哪里不能碰。
> 动手前必读 §0；然后按 `docs/ROADMAP.md` §3 的里程碑顺序往下走。

---

## 0. 开工前必读

### 0.1 本机能做什么、不能做什么（硬约束）

| | 状态 |
|---|---|
| `cargo build` / `test` / `clippy` | ❌ **本机跑不了**（`CARGO_HOME` 已清、磁盘也满：C: 剩 31G / D: 剩 41G）→ **一律走 CI** |
| `rustfmt.exe` | ✅ **能跑**（独立二进制，不需要 cargo） |
| `node` | ✅ 能跑（普通脚本；`normalize.js` 零依赖，可直接 require） |
| `npm install` | ❌ **不要** —— 会重新拉 `node_modules` 吃磁盘。用户原话：“不要在本地新增什么构建环境” |
| `gh` | ⚠️ 间歇 403 → **一律用 `scripts/gh-retry.sh`** 包一层 |
| `git push` | ⚠️ 报 SSL 证书错 → 加 `GIT_SSL_NO_VERIFY=true` |

### 0.2 三道门（提交前必须过，任一条不过就别 push）

```bash
# 1) 格式化 —— ⚠️ 必须用 && 串！用 ; 的话前面失败了你还会继续 push
RF="/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe"
"$RF" --edition 2021 <改动的 .rs 文件> && "$RF" --check --edition 2021 <改动的 .rs 文件> && \
  git commit ... && git push ...

# 2) 期望值先跑真 JS（normalize.js 零依赖，本机能跑）
node -e "const {normalize}=require('./src/lib/normalize.js'); console.log(normalize({...}))"

# 3) workflow 文件体检（防 0 字节空壳 workflow）
python3 scripts/check-workflows.py
```

> 历史：**本机 rustfmt 曾经报出过 raw string 的编译错**，是我用 `;` 串命令照样 push，白丢一轮 CI。

### 0.3 本机没有 `cargo check` —— 签名错只能靠 CI 抓

已踩过的三类（提交前自己核一遍真实签名）：

1. **turbofish 泛型个数**：`bt-core` 的 `post_json<T, B>` 是**两个**泛型参数，`post_json::<Value>(...)` 会 E0107。改成在绑定上标注类型。
2. **闭包签名**：`extract_info_hash` 收的是 `Option<&str>` 而不是 `&str`，不能直接 `.and_then(extract_info_hash)`。
3. **`"#` 会提前结束 raw string**：测试里造 HTML 片段写 `r#"…"#`，内容里只要出现 `="#`（如 `bgcolor="#F4F4F4"`）字符串就在那里断了 → 用 `r##"…"##`。

**还有一招能省掉一轮 CI：** 泛型签名/lifetime 有疑问时，写一个 **std-only 的等价片段**用 toolchain 里的 `rustc --emit=metadata` 编译（不碰项目依赖、不建 `target/`）。

```bash
RUSTC=/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustc.exe
"$RUSTC" --edition 2021 --crate-type lib --emit=metadata --out-dir /tmp/mmcheck /tmp/mmcheck/shape.rs
```
（2026-10-11 用这招提前验证了 `run_mirrors` 的 `'a` 是必需的 —— 反向确认去掉它会 `lifetime may not live long enough`。）

### 0.4 禁改清单

- ❌ **不要提前删 `public/`**（它是 Node 版前端，也是差分验证的对照面；删除时机见 M6 / D18）
- ❌ **不要提前删 `src/providers/*.js`**（差分验证 M6 要靠它们当**真值来源**）
- ❌ **不要删 `test/fixtures/`** —— **永久保留**（golden 数据源）
- ❌ **不要在本地跑 `npm install` / `cargo build`**
- ❌ **不要创建与分支同名的 tag**（会遮蔽分支；详见 `AGENTS.md` 的 tag 纪律）
- ⚠️ `main` / `feat/tauri` 与 `feat/rust` 的关系：改公共文件（workflow、`scripts/`）时**三个分支都要同步**；改 Rust 路线内容时只动 `feat/rust`

---

## 1. M1 —— Slint 风险验证（✅ **2026-10-16 已完成：`#13548` 未复现**）

> **结论**：六条及格线全部通过 —— 200 条中文标题覆盖 200/200、西里尔 50/50、
> 580 次强制同步重绘、2000 条只实例化 9 个、自包含 exe **14.5MB**，全程未 panic。
> 完整口径 + 两个坑（「不重绘的假绿」与「体积对不上」）见 `AGENTS.md`「Slint 探针结论（留档）」。
> **本节以下内容保留**：既是当年的操作手册，也是以后验 Slint 渲染路径时的清单
> （尤其 §1.3 的四个坑和 §1.4 的 Plan B，Plan B 现在只是备用，不再是当前路径）。

### 1.1 为什么要单独开一个里程碑

Slint **#13548**（1.18.0 / 1.18.1 均未修）：**内联样式跨软/硬换行、且换行后是多字节字符时，parley 会 panic**（`end byte index N is not a char boundary`）。

已知最小复现是 `@markdown("Это очень\nважно")` 那一类。

而这个应用**最基本的渲染需求**就是：

> **中文标题 + 关键词高亮 + 自动换行**

三条全踩。**这是选 Slint 的最大风险点，而且到现在一次都没实测过。**

⚠️ 因为 D8 是「跳过 Tauri 直接上 Slint」，所以这个验证**必须在 M2 之前做** —— 一旦 Slint 不可用，M2 的整个架构（库 + 进程内直调 + 不做 HTTP）都要重新考虑。

### 1.2 及格线（6 条，必须**全部**通过）

| # | 标准 | 怎么做 |
|---|---|---|
| 1 | **200 条中文标题 + 高亮 + 自动换行，全部渲染，不 panic** | ⚠️ **fixture 里没有任何中文**（实测确认过）→ 用**合成**标题：把关键词（如 `ubuntu`）插进不同长度的中文串，**长度必须超过容器宽度**以强制换行。200 条是照当年 spike 的规模 |
| 2 | 关键词出现在**第 2 行**（= 跨软换行）也 OK | 构造长前缀使关键词落到第二行 |
| 3 | 关键词**正好落在换行边界**（行尾 / 行首）也 OK | 逐字调前缀长度逼近边界 |
| 4 | **西里尔也过一遍**（真实素材，且与已知 repro 同字符集） | `test/fixtures/megapeer-1080p.html` 是 **windows-1251**；用 `TextDecoder('windows-1251')` 解码可拿到 106 段西里尔文本 |
| 5 | **报出实测体积** | `--release` 打包 → 报自包含 exe 的字节数（这正是 R2 缺的那个数字，顺手拿到） |
| 6 | **`ListView` 虚拟化生效** | 对比「数据条数」与「实际实例化次数」（照当年 spike 的计数法：走 `callback` 进 Rust 的 `Cell`） |

### 1.3 必须遵守的坑（上次 spike 实测结论 —— 别再踩一遍）

- **富文本属性类型写 `styled-text`（不是 `string`）**；Rust 侧用 `slint::StyledText::from_markdown()`。
  ⚠️ `@markdown()` **会把插值转义**（官方原文："Any text passed as an argument to the macro will be escaped"）→ `@markdown("\{expr}")` 进去的内容**永远是纯文本、不参与解析**。
- **别在 `init` 里写全局属性** → 会形成属性依赖环，实测 200 条数据触发 **2628 次**实例化。计数走 `callback` 进 Rust 的 `Cell`；显示属性**只在用户点击时**写。
- `StyledText` **没有 `wrap` 属性**；**高亮色是烘进 markup 的** → **切主题必须整体重建模型**。
- CJK 走 `default-font-family` + 系统字体回退。
- `StyledText` 支持 `<font color>` / `<u>` / 斜体 / 删除线 / 行内代码 / 链接 / 列表。

### 1.4 Plan B —— 如果 #13548 真的 panic（按成本排序）

1. **⭐ 分段拼装（首选）**：**不把高亮做进一段富文本**。把标题按关键词切成「前 / 中（高亮）/ 后」三段，用 `HorizontalLayout` / `FlowLayout` 放**多个 `Text` 元素**。
   → 这样**绕开 markdown 解析与内联样式的换行路径**，直接回避病灶。
   ⚠️ 代价：断行交给布局引擎，**不能任意位置断行**；CJK 换行体验可能变差 → 这项改动也要实测。
2. **降级高亮**：只有「不换行的短标题」才高亮；长标题整行单色。
3. **关掉高亮**：保住「小快原生」，牺牲一个次要特性。
4. **等上游修**（不推荐：不可控，1.18.0/1.18.1 都没修）。
5. **退回 Tauri WebView** —— 会推翻 D8/D9，成本最高；而且我们**没有**「Tauri + Rust 核心」的体积实测数字，退回去等于赌另一边。

### 1.5 交付物

- 一个能跑的最小示例 —— `spike/slint-ui` 重建（当年约 **500 行**，分支与 `slint-spike.yml` 已于 2026-10-09 删除，需要重建）
- 一份结论：**及格 / 不及格** + 实测体积 + 失败时的 **panic 栈原文**
- ⚠️ **结论必须写回 `AGENTS.md` 的「Slint 探针结论（留档）」段** —— 上次分支删了，结论差点丢；这次不许再丢

### 1.6 构建方式

⚠️ **spike 也要走 CI**（本机无 cargo）。重建一条 `slint-spike.yml`（触发方式：`workflow_dispatch` + 推 `spike/slint-ui` 时触发）。
参考 `tauri-build.yml` 的写法；**放 `main` 上才能手动触发**（见 `AGENTS.md` 的 workflow 表）。

---

## 2. M2 —— 聚合层 + `Provider` trait

### 2.1 要复刻的 JS 语义（逐条，照 `src/providers/index.js`）

这是**唯一的真值来源**，别凭记忆写：

**① 目标筛选 `resolveTargets`**
```js
if (!p.enabled || (wanted && !wanted.includes(p.id))) return false;
if (browse && !p.browseable) return false;              // 无关键词浏览：只查 browseable
if (page > 1 && !p.paginated && !p.demo) return false;  // 第 2 页起：丢掉单发引擎
```
⚠️ 第 3 条是**为了防重复**：单发引擎忽略 `page`，翻页只会返回同一批结果、再被去重掉。

**② 并发池 `asyncPool(tasks, 8, onSettle)`**
- 上限 **8**（`MAX_CONCURRENT`；注释写明是「够灌满管道，又不至于像 DDoS」）
- ⚠️ **`onSettle` 必须在下一个任务开始前调用** —— 这是「谁先回来先显示谁」的**唯一**原因。若写成「全部 `join_all` 再一起 emit」，**8 路并发的体验就没了**（用户会觉得变慢）。**这是本层最容易写错、且错了不明显的一条。**
- 单个 provider 失败**不影响其他**（`runProvider` 内部 try/catch）
- 每个 provider **单独计时** `ms`

**③ 聚合 hasMore**
```js
if (!s || s.status !== 'ok' || s.count === 0) return false;  // 出错或没结果 → 没有下一页
if (typeof s.hasMore === 'boolean') return s.hasMore;         // provider 明确说的优先
return !!p.paginated;                                         // 否则按 paginated 猜
```

**④ 每站状态形状**（UI 的状态栏按这个渲染）
```
{ status: 'ok' | 'error', count: <usize>, error: <Option<String>>, hasMore: <Option<bool>>, ms: <u64> }
```

### 2.2 `Provider` trait

形状与理由见 `docs/ROADMAP.md` §2.4。要点：
- `search` 必需；**`resolve_magnet` 用默认方法**表达「可选能力」（6/16 家有）
- `paginated` / `browseable` 有默认值，单个 provider 覆盖
- ⚠️ 引入 trait 会让 16 个 provider **各加一个 impl，代码量净增** —— 这不是为省行数，是为了让聚合层与 D17（Torznab 口子）成立

### 2.3 验收

- 三条语义各有**独立测试**（照着上面 ①②③ 逐条钉）
- `onSettle` 的「下一任务前调用」要有测试能证明（例如断言事件顺序）
- CI 绿（`cargo test --workspace` + clippy + fmt）

---

## 3. M3 —— `bt-downloaders`（D16：四个都要）

### 3.1 四个客户端（照 `src/lib/downloaders.js`）

| kind | label | 认证 | 默认地址 |
|---|---|---|---|
| `qbittorrent` | qBittorrent | `userpass` | `http://localhost:8080` |
| `transmission` | Transmission | `userpass` | `http://localhost:9091` |
| `aria2` | aria2 / Motrix | `token` | `http://localhost:16800/jsonrpc` |
| `gopeed` | Gopeed | `token` | `http://localhost:9999` |

对外形状：`push(kind, cfg, magnet)` / `test(kind, cfg)` / `detect()` / `META`。
`detect()` = 探测本机标准端口 + 默认/空凭据，**返回第一个应答的**。

### 3.2 两个已修过的坑（别退回去）

- ⚠️ **qB 登录成功的判定不要 string-match 响应体**（`/fails|failed/i` 跨版本很脆）。正确做法：判 **HTTP 403** + **遍历所有 `set-cookie`** 找 `SID=`（旧实现只看第一个）。
- ⚠️ **URL 校验不能缺**：只放行 `http:`/`https:`（`safeHttpUrl` 的语义）。这是信任边界上的校验，**不许省**。

### 3.3 验收

- 4 家的 `push` / `test` / `detect` 都有**离线测试**（照 `tests/` 那套本地一次性 HTTP 服务的套路，不联网）
- 认证失败 / 连不上 / 非 JSON 正文 三条失败路径各有用例

---

## 4. M4 —— `bt-app` Slint MVP

### 4.1 界面范围（只做这些，别扩）

搜索框 · 结果列表（`ListView` 虚拟化）· 每站状态 · 详情（磁力 + 海报/描述）· 复制磁力 · 推送下载器。

### 4.2 线程模型（**本里程碑最容易错的地方**）

- 网络 IO 在 **tokio 运行时**；Slint UI 是**单线程**的
- ❌ **后台任务不能直接改 UI 属性**
- ✅ 必须 `slint::invoke_from_event_loop(..)` 或 `weak.upgrade_in_event_loop(..)` 跨回 UI 线程
- ✅ **每站点一个 callback** → 谁先回来谁先上屏（这正是原来 SSE 干的事）
- ❌ 不要「等全部跑完再一次性填」—— 那会把 8 路并发的价值全丢掉

### 4.3 记住 §1.3 的四条坑（别在 `init` 里写全局属性 / `styled-text` / 无 `wrap` / 切主题要重建模型）

### 4.4 验收

S6：**搜索 → 结果列表 → 取磁力 → 推送下载器，全链路手工走通**（这是 MVP 的定义）。

---

## 5. M5 —— 引擎体检 → 砍掉死的（D13）

```bash
# CI（推荐，出口干净）
scripts/gh-retry.sh workflow run provider-health.yml --ref feat/rust
scripts/gh-retry.sh run view --job=<JOB_ID> --log | grep -E '^\| `'
```

- **产出**：一张「现在谁还活着」的表 → 据此决定砍哪些
- ⚠️ **探活 ≠ 能解析**：`provider-health.mjs` 的行数是**粗判**（数 `<tr`）。D 档那几个是「200 但粗判 0 行」——**很可能是选择器不匹配，不是站点坏**。真要搬，第一步仍然是**加探针拿 cheerio 真值**。
- ⚠️ **本机结果不可全信**（MITM 代理破坏 TLS + DNS 污染 + 同一域名几分钟内结论会变）；**CI 的 403 也不等于用户那儿不行**（机房 IP 更容易被 Cloudflare 拦）。**两个环境合起来才是完整答案。**
- 已确认死亡的（`eztv` 451 / `1337x`·`blueroms`·`torrentdatabase`·`uindex` Cloudflare）**直接砍**，别再花力气。

---

## 6. M6 —— 差分验证（**必须在删 Node 之前**，D18）

### 6.1 做法（G1：fixture 回放差分，不打真站）

```
scripts/diff-providers.mjs
  ├─ 对每个 test/fixtures/<site>-<query>.{json,html}
  ├─ Node 侧：require('src/providers/<site>.js') + 现有 createMockHTTP() 喂 fixture → 规范化 JSON
  ├─ Rust 侧：cargo run -p bt-providers --example dump_provider -- <site> <fixture> → 规范化 JSON
  └─ 逐字段 diff；差异必须命中「已知 divergence 白名单」，否则非零退出
```

要点：
- 比的是 **`normalize()` 之后**的字段（这才对应「逐字段一致」的口径）
- **排序稳定**（按 `id`）后再比，避免顺序假差异
- **白名单必须双向**：差异命中即通过，**且必须命中**（防止哪天无意修好了却没人知道）

### 6.2 已知 divergence 白名单（`AGENTS.md` 的「🐞 移植中发现的上游 bug」表里有全部细节）

`megapeer` 编码 + 5-td 列语义 · `audiobookbay` size/date（`<br>` 不给 `.text()` 补换行）· `nyaa` 列索引错位（`seeders=2009`）+ 磁力恒 null · `limetorrents` 分类/日期 · `therarbg` 分类 · `torrent9`/`oxtorrent`/`zeromagnet` 数据白扔 · `linuxtracker` detailUrl 死链（已修）。

### 6.3 ⚠️ 收尾条件

**M6 全部命中白名单之后，才允许删 Node 版**（`src/providers/*.js`、`src/lib/`、`server.js`、`test/run.js`）。
`test/fixtures/` **永远不删**。

---

## 7. M7 —— 打包 + 发布（D15）

- 单文件自包含 exe（S1/S2）
- 产物名**纯 ASCII**（中文名会让下载 URL 被 percent-encode，老下载工具会拿到问号名）
- 发 GitHub Release；小圈子自助下载
- ⚠️ 参照现有教训：**别让打包工具自动 publish**（GH_TOKEN 会在 CI 上炸），改成显式上传 artifact

---

## 附：命令速查

```bash
# 推代码（本机必须加这个，否则 SSL 证书错）
GIT_SSL_NO_VERIFY=true git push origin feat/rust

# gh 一律包一层（本机会间歇 403）
scripts/gh-retry.sh run list --branch feat/rust
scripts/gh-retry.sh run view --job=<JOB_ID> --log

# 手动触发联网冒烟（允许失败，不在任何 push 门上）
scripts/gh-retry.sh workflow run live-smoke.yml --ref feat/rust

# 全部 40 个引擎的联网探活
scripts/gh-retry.sh workflow run provider-health.yml --ref feat/rust

# CI 抓一个本机探不通的页面快照（自动 commit 回分支）
scripts/gh-retry.sh workflow run fetch-fixture.yml --ref feat/rust \
  -f url='<结果页 URL>' -f path='test/fixtures/<site>-<query>.html'
# ⚠️ 抓回来先看内容（Cloudflare 挑战页/停放页也是 200），再 git pull --rebase

# 打 tag 前的预检
npm run release:check
```

## 附：新增 provider 的配方（M5 之后如果真要加）

见 `AGENTS.md` 的「🧩 provider 移植配方」+「HTML provider 的四条铁律」+「探针两步流程」。核心顺序：

1. 先抓真 fixture（本机不通就 `fetch-fixture.yml`）
2. **加 probe 拿 cheerio 真值**（别靠读 HTML 猜）→ 在临时分支上跑 `html-probes.yml` → `git restore --source=...` 取回
3. 期望值**先跑真 JS** 拿（`node -e "require('./src/lib/normalize.js')"`）
4. 移植 + 单列 `divergence_*` 测试
5. 提交前跑 rustfmt
