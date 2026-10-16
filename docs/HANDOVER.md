# 交接说明（写于 2026-10-16 收盘）

> 这份是**给下一个接手的人/AI** 的现状快照。历史决策在 `AGENTS.md`（很长，按需查），
> 设计目标在 `docs/ROADMAP.md`，CI 事故总账在 `docs/CI_FAILURES.md`。

## 一句话状态

`main` = **`2b4a3e7`**，CI 全绿：`Rust (test)` **478 passed / 0 failed**（含 `--selftest` 全链路、`cargo fmt --check`、`clippy --workspace --all-targets`）、
`Node (test)`、`Rust exe size`（25MB 门）、`Build Windows 发行包`、`CI guard`。
工作区干净、`main` 与 `origin/main` 同步。

**最新发布 = `v0.3.9-beta`**（tag `c524723`，GitHub 上已是 Latest）。
产物四个 + `latest.yml`：`BT-Search-Rust-0.3.9-beta.exe` / `.zip`、
`BT-Search-Electron-Setup-0.3.9-beta.exe`、`BT-Search-Electron-Portable.zip`、
`BT-Search-Tauri-Setup-0.3.9-beta.exe`（⚠️ 这次 Tauri 的名字**是对的** —— 上一版叫 0.3.6 的原因已修）。
⚠️ **该 tag 早于 `2b4a3e7`** ⇒ 那个发布**不含**下面「最近这批修复」里的最后一组。

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

## 还挂着的事（按「值得动」排序）

1. ✅ **已发 `v0.3.9-beta`**（本文档之后的那一次）—— 它含 0.3.8 之后那 5 条修复，**外加**下面第 2、5 条（索引器可单独停用、粘贴 feed URL 自动抠 apikey）。⚠️ 下一版记得走 `v0.3.10-beta`：**绝不重打同名 tag**（仓库规矩）。
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
- 引擎：Rust 版 **16 家**（对齐 Node 侧同名 16 家，M6 全量差分 0 处未豁免差异）；Node 版 **40 家**仍在（`D18`：留着作对照，删除是按**函数粒度**的，还没开始）。
- 分支：`main`（Rust 主线）· `legacy/node-electron`（存档，停在 `9b9461c`）· `feat/rust`（合并后已与 main 同源）· `feat/tauri`（Tauri 壳，**有 69 个独有提交，必须保留**）· `spike/slint-ui`（M1 证据）。
