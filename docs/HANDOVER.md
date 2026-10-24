# 交接说明（写于 2026-10-16 收盘，2026-10-21 更新）

> 这份是**给下一个接手的人/AI** 的现状快照。历史决策在 `AGENTS.md`（很长，按需查），
> 设计目标在 `docs/ROADMAP.md`，CI 事故总账在 `docs/CI_FAILURES.md`。

## 一句话状态

`main` = **`6916c8c`**（2026-10-23；2026-10-24 核对时其后只有文档提交 `9141602`，代码未动）。Rust 主线功能面 100%（M0–M7 全通过），
发版链已稳定：`v0.3.10` → `v0.3.11` → `v0.3.12` → `v0.3.13` → **`v0.3.14`** 连续五版一次成功。
工作区干净、`main` 与 `origin/main` 同步。

**最新发布 = `v0.3.14-beta`**（2026-10-23 06:2x UTC，GitHub Latest，tag `v0.3.14-beta` → `6916c8c`）。
产物 6 个：`BT-Search-Rust-0.3.14-beta.exe`（18.6MB，体积门 25MB 内）/ `.zip`、
`BT-Search-Electron-Setup-0.3.14-beta.exe`（96MB）、`BT-Search-Electron-Portable.zip`（146MB）、
`BT-Search-Tauri-Setup-0.3.14-beta.exe`（29.7MB）、**`latest.yml`**。
本版内容：`linuxtracker` 的「展开描述行」列映射修复 + 第 23 家引擎 `anirena`。
（`v0.3.13` 带的是 nyaa 磁力修复 + audiobookbay 的 size/date 修复。）

⚠️ **`latest.yml` 从 0.3.11 起就没了**（0.3.10 有）。机制已查清，不是玄学：

| 谁 | 干什么 | 是否产出 `latest.yml` |
|---|---|---|
| `build.yml`（tag 触发） | `npm run dist -- --publish=onTag`，**自己发 Release** | **会**（electron-builder 发布时才写） |
| `release.yml` 的 electron job | `npm run dist -- --publish=never` | **不会**（日志里只有 `building block map`） |
| `release.yml` 的 publish job | 先 `gh release delete`（保留 tag）再用自己的 6 个产物重建 | 于是 build.yml 那份 `latest.yml` 被连带删掉 |

**2026-10-23 实测证据**：发版过程中 `gh release list` 能看到一个 **Draft** `0.3.13-beta`
（`build.yml` 的 electron-builder 默认建 draft），最终留下的是 `draft=false` 那份，
而 `latest.yml` 数量 = 0 —— 与上表推断一致。

**同一天的 `v0.3.14-beta` 反过来验证了「谁最后跑完谁决定」**：这次 **有** `latest.yml`（383B）、
却**没有** `.exe.blockmap`。所以资产集是**竞态**结果，不是固定形态 ——
`0.3.11/0.3.12/0.3.13` 缺 latest.yml、`0.3.14` 有；blockmap 则相反。

**实际影响 ≈ 0**：全仓没有任何 `electron-updater` / `autoUpdater` 引用 ⇒ 没有自动更新在读它。
真要修（二选一，都得下次发版验证）：
① 去掉 `release.yml` 里那步 `gh release delete`（软 props 会按文件名合并，重复发同一 tag 时才会留下旧文件）；
② 反过来让 `release.yml` 的 electron job 也产出并上传 `latest.yml`（需要 electron-builder 在 `--publish=never` 下也生成它 —— 目前不会，得加 `--config.publish` 之类的技巧，不如①稳）。

⚠️ **`v0.3.9-beta` 从未发布成功**（历史更正，别再照旧文档复述）：
那次发版 run（`38897429252`）里 `electron` / `tauri` 两个 job 都绿，
只有 `Build Rust app` 失败 —— `895520a` 把版本 bump 到 0.3.9 却**没同步 `Cargo.lock`**，
`cargo build --release --locked` 直接报 `cannot update the lock file`；
`Publish to GitHub Release` 因此被 skip ⇒ **tag 打了、release 没生成**，之后 tag 被删。
那次的修复（`2dbf9f6` 补 lock）**当时没被任何门验证过**，本轮才真正跑绿。

## 这台机器上的硬约束（先读，否则会白折腾）

| 约束 | 事实 |
|---|---|
| **没有 cargo** | `/d/Vibe-Coding/.cargo` 不存在 ⇒ `cargo build/test/clippy` **本机跑不了**，一律走 CI；`AGENTS.md` 也明令不在本机编译（C 盘剩 31G / D 盘剩 41G） |
| **rustfmt 能单跑** | `/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe --edition 2021 <files>`（**必须带 edition**；先就地跑一遍再 `--check`，能省一轮 CI） |
| **没有 node_modules** | `node test/run.js` 跑不了（`src/lib/http.js` 要 axios）；`node test/normalize.test.js` 能跑（零依赖）。Node 测试的真门是 CI 的 `node.yml` |
| `gh` 间歇 403 | 一律 `scripts/gh-retry.sh <原样的 gh 参数>` |
| `git push` 报 SSL 错 | 加 `GIT_SSL_NO_VERIFY=true`（本机 MITM 证书不在 CA bundle 里） |
| 手动触发 workflow | `gh workflow run <file>` 要求该文件已在**默认分支**上 |
| 期望值不许手算 | 跑真 JS 拿：`node -e "require('./src/lib/normalize.js')…"`（零依赖，本机可用） |

## 最近这批改动（看 `git log --oneline 9cc732d..HEAD`）

背景：用户要求「看下还有什么要修正/改善的」→ 4 个只读 reviewer 分头审计
（`crates/bt-app` / `bt-providers`+`bt-core` / `bt-downloaders`+`bt-torznab` / 仓库卫生），
共 **50 条发现 / 15 HIGH**，原文 4 份 `.md` 在 `.audit/`（**本地、已 gitignore、不入库**）。

| 提交 | 内容 |
|---|---|
| `d1c860b` | 审计主体:bt-app 三个 race（第二次搜索无代数围栏 / `refresh_row` 坐标系错 / 网络回来后才写回的 `idx` 被排序重编号）· qB 的 `Fails.` 假成功（Rust + Node 两侧）· Torznab `<error>` 被当空结果 · `normalize_api_url` 裸字节切分 panic · 新增 `node.yml` CI 门 · `rust-size.yml` 从探针转成真门 · `check-workflows.py` 加「引用的脚本是否存在」 · 一批文档矛盾（AGENTS.md / AGENT_HANDBOOK.md）· `Cargo.toml` 版本 0.0.1→0.3.7-beta · `package.json` 清掉指向不存在的 `src-tauri/` 的脚本 · `server.js` async 路由 wrap + 错误中间件 · `electron/main.js` 的 `openExternal` scheme 白名单 · `catch_unwind` 包住 `search()` 调用本身 · 俄语日期大小写 · `btih` 下标不再用 `to_lowercase()` 副本 · `safe_http_url` 的归一化结果不再被丢 · `Kind::parse` 不再默认 qB |
| `55a9dca` / `d9bed30` | `Cargo.lock`（6781 行）入库；门与发版构建加 `--locked`（探针类 workflow **刻意不加**） |
| `ea20905` | 修我自己写错的用例期望值（没磁力的行是 `NeedsMagnet` 不是 `Ready`） |
| `5f1865d` / `c524723` | 0.3.9-beta 版本号对齐 + 文档版本引用 |
| `2b4a3e7` | 审计残留收口：① health 落盘不再占 UI 线程 + `--selftest` 不再写用户真实 `%APPDATA%`（新增 `BT_SEARCH_HOME`）② 用户配置改**原子写**（`write_atomic`: tmp + rename）③ Torznab `attr()` 对齐 JS（全后代 + 大小写不敏感）④ `v2nt` 补 `as_f64` 回退 ⑤ 删掉没人加载的 `build:frontend`（脚本 + `esbuild` 依赖）⑥ axios 锁到 1.20.0 |
| `895520a` / `2dbf9f6` | 0.3.9-beta 版本号对齐（**只改了 `Cargo.toml`/`package.json`，漏了 `Cargo.lock`**）→ 发版失败 → 补上 lock（这条修复当时**没被体积门验证**，原因见下一节） |

## 2026-10-21 这一轮：把发版链的两个缺口补上

起因：一次只读进度核查（`git log` / `gh run list` / `gh release list` 三处对账）发现
**「文档说已发 0.3.9、实际没有 release」**。根因不是代码，而是两道门都有洞：

| 缺口 | 后果 | 修法 |
|---|---|---|
| `rust-size.yml` 的 `paths` 只有 `Cargo.toml` / `crates/**` / 自身，**没有 `Cargo.lock`** | `2dbf9f6` 只改 lock ⇒ 体积门不触发 ⇒ 那次 `--locked` 失败**从未被这道门重新验证**，「修复完成」只是口头结论 | `paths` 补 `Cargo.lock`（换依赖版本本来就是体积会变的场景） |
| `release-check.js` 只比对 `package.json` ↔ `Cargo.toml`，**不查 `Cargo.lock`** | 正是 0.3.9 那次事故的形状：两个真源一致、lock 落后，`--locked` 在 CI 才炸，tag 已经推出去了 | 新增 6c 检查：扫 lock 里所有 `bt-*` 包的 version，不一致就红 |

另外把版本推到 **`0.3.10-beta`**（`Cargo.toml` / `package.json` / `Cargo.lock` 五处一起），
`feat/tauri` 的三处版本号（`tauri.conf.json` 是**真源** / `src-tauri/Cargo.toml` / `package.json`）
同步到同一版本 —— 否则发版出来的 Tauri 安装包会顶着旧版本号（`AGENTS.md` §C 记过这个坑）。

**结果（都实测过，不是推断）**：

| 步骤 | 证据 |
|---|---|
| main 五道门 | `Rust (test)` 3m52s ✅ · `Rust exe size` **12m+ ✅**（这次真的跑完了 —— 它由 `Cargo.lock` 触发，`--locked` 通过）· `Node (test)` ✅ · `Build Windows 发行包` ✅ · `CI guard` ✅ |
| `feat/tauri` 版本 | `Tauri Build (validate)` ✅（run `39627044618`） |
| 发版 | run `39629141627` **success**；`gh release view` 五个资产全在，Tauri 包名 = `0.3.10-beta` |
| 体积 | `BT-Search-Rust-0.3.10-beta.exe` = **19,398,144 字节（18.5MB）**，门限 25MB |

⚠️ 顺带踩到一个 git 坑（记下来省下一个人半小时）：`git worktree add <dir> refs/heads/<branch>`
用**完整 ref** 传进去会得到 **detached HEAD**，commit 不落到分支上 ——
于是 `git push origin refs/heads/<branch>` 报 `Everything up-to-date`（推的是没动过的旧引用）。
要么传短分支名，要么 `git push origin HEAD:refs/heads/<branch>`。

## 2026-10-23 这一轮：搬了 6 家 + 修掉 nyaa 的数据级 bug

**引擎 16 → 22 家**（`subsplease` / `dmhy` / `mikan` / `animetosho` / `bangumimoe` / `nekobt`），
全部同步进 M6 差分门 ⇒ **22 家全量、0 处未豁免差异**。发了 `v0.3.12-beta`。

三件值得记的事：

1. **`nyaa` 的三处上游 bug 已修（JS + Rust 同轮）**。原来照抄的结果是：
   磁力在 Link 格而代码在名字格里找 ⇒ **结果里根本没有磁力**；名字格带 `colspan="2"`
   导致列索引整体错位一格 ⇒ size/date 全空、**seeders 变成 2009**（把日期串喂给 parseInt）。
   修法：磁力在整行里找、索引改 3/4/5/6。**为什么现在敢修**：这是数据级 bug，不是外观差异；
   两侧同轮改，差分门仍一致（它比的是「同一份 fixture 下两边是否相同」）。
   ✅ **同类另两处也已在同一天修完**（见 `AGENTS.md` 的上游 bug 表）：
   `audiobookbay` 的 size/date 双丢（按字段名切，不再按 `\n` 切）、
   `linuxtracker` 的 19-td 展开行列错位（单独列映射 + 剥前缀）。
   ⚠️ linuxtracker 那条**不能靠"跳过行"修** —— 实测那个 torrent 全页只出现一次，
   是唯一一条真实结果；做法是给 19 格行单独映射，真值由新加的 cheerio 探针
   `expanded.all_rows` 在 CI 里 dump 出来。
2. **`fetch-fixture.yml` 支持 POST**（可选 `method` / `body`）—— 否则 `bangumimoe` 这类只吃
   POST 的 API 站连快照都抓不到。curl 参数必须用**数组**拼，body 里的引号会被 shell 再解释。
3. **探活结论**（别再白试）：`tokyotoshokan` CI 也 403（Cloudflare）；`sukebei` 超时、
   `bitsearch` 520、`eztv` 403。剩下可试的约 12 家。

## 2026-10-24 收尾：审计 LOW + `latest.yml` 竞态

一次只读进度核查之后的清账，四项改动都不改行为、只改"说谎的地方"：

| 改动 | 之前 | 之后 |
|---|---|---|
| `release.yml` 删掉 `gh release delete` 步骤 | 两个 workflow 都监听 tag push：`build.yml` 先建 Release 并放上 `latest.yml`，随后 `release.yml` 把整个 Release 删掉、只用自己 6 个产物重建 ⇒ `latest.yml` 连带消失 | 两侧合并到同一个 Release（`softprops/action-gh-release` **复用**同 tag 的既有 Release，且 `overwrite_files` 默认 true ⇒ 同名资产原地覆盖）。仓库规矩"绝不重打同名 tag"，所以"新旧文件共存"这个原担心不成立 |
| `aria2.rs` / `transmission.rs` 的 `push` | 硬编码 `PushOutcome { status: 200 }`（照抄 JS），UI 显示「已推送到 aria2（HTTP 200）」是个**编造的数字** | 两个 `call()` 都改成 `Result<(Value, u16), String>`，把**真实**响应状态码带出来（审计 #12） |
| `bt-downloaders/src/lib.rs` 的 `net_err` | `format!("请求下载器失败 ({e})")` —— reqwest 的 `Display` **带完整 URL**，而下载器地址常把 token 放 query、或内嵌 basic-auth；这段文案会进 UI 状态行 / `--health` 历史 / 日志 | `e.without_url()`（审计 #14） |
| `bt-torznab/src/lib.rs` 的 `text()` | `Node::text()` **只返回第一个 text 子节点** ⇒ `<title>a<b>x</b>c</title>` 静默丢掉 `c` | 拼接全部后代文本节点，与 cheerio 的 `.text()` 一致（审计 #15） |

**仍然「明确不修」**：`push_same_name_indexers_collapse_into_one_id`（审计 #11）——
UI 按**名字**去重，同名索引器不会共存，撞 id 只可能来自手改 `indexers.json`；
真要做先定 id 方案 + 迁移已有文件。理由见下面「明确不修」表。

## 还挂着的事（按「值得动」排序）

1. ✅ **已发 `v0.3.10-beta`**（2026-10-21）—— 含 0.3.8 之后的全部修复。下次照本轮顺序走：
   `npm run release:check`（现在会拦 lock 不同步）→ 等 main 五道门绿 → 打 tag。
   **绝不重打同名 tag**（仓库规矩）。
2. ✅ **已修**（本会话）：每个索引器一行「停用 / 启用」。~~索引器不能单独停用~~（真·产品缺口）。原文留档：`config_store::StoredIndexer.enabled` 与 `bt_torznab::IndexerConfig.enabled` 字段都在、`add_indexer`（`crates/bt-app/src/main.rs:854`）恒写 `true`，而 `crates/bt-app/ui/app.slint:365-390` 只有「添加 / 清空」两个按钮 ⇒ 某个索引器坏了只能全部清掉重加。最小做法：给每行加个开关 + `set_indexer_enabled` 回调 + `save_indexers`。
3. **S2「零运行时依赖」还差人工那一次**：`rust-size.yml` 已机器断言「无 vcruntime140/msvcp140」，但 `docs/ROADMAP.md:47` 的判据是**拷到干净 Windows 双击** —— 需要在虚拟机/另一台机上做。
4. **S3/S4 的性能目标还是「待你确认」**（`docs/ROADMAP.md:50-51`：冷启动 < 1s、空闲内存 < 100MB）—— 得先真机量一次。
5. ✅ **已修**（本会话）：粘贴 feed URL 会自动抠 `apikey`（`bt_torznab::extract_api_key`，只在 key 框空着时用）。原文留档：~~粘贴 Jackett 的 feed URL 不会自动抠 `apikey`~~（`normalize_api_url` 刻意丢 query），key 得手填。错误现在会明说「Invalid API key」，所以只是省一步。

## 明确「不修」的决定（别重新发现一遍）

| 项 | 为什么不动 |
|---|---|
| `crates/bt-torznab/src/provider.rs` 的 `Box::leak` | 要消掉必须把 `Provider::id()/name()` 从 `&'static str` 改成 `&str`，牵连 16 个 impl + 测试；收益是每 100 次搜索约 20KB。本机没 cargo，盲改 17 个文件的签名风险 > 收益 |
| `add_indexer`（`crates/bt-app/src/main.rs:874`）的 `id` 恒空 | UI 是按**名字**去重的（`position(\|c\| c.name == name)` 命中即替换）⇒ 同名索引器不会共存；撞 id 只可能来自手改 `indexers.json`。真要做先定 id 方案 + 迁移已有文件 |
| Node 侧 `/api/magnet`、`/api/torznab/test`、`/api/download/*` 只查 scheme | **已登记的设计决策**，缓解是只绑 `127.0.0.1`（`server.js:220`）。要放开远程访问必须先加 host 白名单 |
| Node 依赖树的 `@xmldom/xmldom` / `brace-expansion` 告警 | 全在 **dev 树**（eslint / prettier / electron-builder），不进用户机器。运行时那条（axios）已修 |

## 别的现状（免得重新确认）

- `crates/` 里 **TODO/FIXME 0 处**。
- 测试规模：Rust **478**（`bt-core` / `bt-providers` / `bt-downloaders` / `bt-torznab` / `bt-app`）+ M4 `--selftest` 全链路 + M6 差分（`m6-diff.yml`，探针不是门）。**别把某个数字抄进文档当现值**，要看就查最近一次 `rust.yml` 日志。
- workflow 共 **14 条**，`python3 scripts/check-workflows.py` 全绿（含体积、YAML、`on:`/`jobs:`、以及**引用的脚本是否存在**）。
- 引擎：Rust 版 **23 家**（2026-10-21/23 新增 `subsplease` / `dmhy` / `mikan` / `animetosho` / `bangumimoe` / `nekobt` / `anirena`；**23 家全部在 M6 差分门里**，逐字段比对 0 处未豁免差异）；Node 版 **40 家**仍在（`D18`：留着作对照，删除是按**函数粒度**的，还没开始）。
- 分支：`main`（Rust 主线）· `legacy/node-electron`（存档，停在 `9b9461c`）· `feat/rust`（合并后已与 main 同源）· `feat/tauri`（Tauri 壳，**有 78 个独有提交，必须保留**）· `spike/slint-ui`（M1 证据，3 个独有提交）。
