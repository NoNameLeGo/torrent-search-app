# 交接说明（2026-10-26 核对，取代 2026-10-21 版）

> 这份是**给下一个接手的人/AI** 的现状快照。历史决策在 `AGENTS.md`（很长，按需查），
> 设计目标在 `docs/ROADMAP.md`，CI 事故总账在 `docs/CI_FAILURES.md`。
> 本文件只写「现在是什么状态」和「别重复踩什么」，**不写流水账**。

## 一句话状态

`main` = **本文件所在的提交**（`git log -1 --format=%h`）。⚠️ 别再在这里写死哈希：
2026-10-26 上一版就写歪过一次（写的是改这份文件之前的那条提交）。
最新发布 = **`v0.4.0-beta`**（2026-10-25 11:22Z，GitHub *Latest*，tag → `ba5fa1b`），
`release.yml` 一次跑绿（run `40187920449`，9m56s），产物 6 个：

| 资产 | 大小 |
|---|---|
| `BT-Search-Rust-0.4.0-beta.exe` | 20,018,688 B（**19.1MB**，门限 25MB） |
| `BT-Search-Rust-0.4.0-beta.zip` | 8,849,763 B |
| `BT-Search-Electron-Setup-0.4.0-beta.exe` | 100,921,104 B |
| `BT-Search-Electron-Portable.zip` | 152,774,518 B |
| `BT-Search-Tauri-Setup-0.3.15-beta.exe` | 31,116,828 B ⚠️ 见下 |
| `latest.yml` | 380 B |

**本版内容**：顶栏**三模式**（磁力 / 视频 / 网盘，结果不混流）· 在线影视 **5 家**
（APP影院、独播库、嘀嗒、追影、宅男，搜索后 `open_url` 打开原站）· **PanSou 网盘客户端**
（方案 A：外部服务，设置里填地址与 token，支持打开链接 + 复制提取码）· BT 引擎 **27 家**
（24 对齐 Node + 3 家 Rust 独有：`bdflixs` / `pomo` / `cilixiong`）。

⚠️ **Tauri 包名是上一版的号**：main 从 `0.3.15-beta` 走到 `0.4.0-beta` 时，只 bump 了 main 自己的
版本，`feat/tauri` 上那四处还是 `0.3.15-beta`，而 `release.yml` 是**显式 checkout `feat/tauri`**
来构建 Tauri 包的。已发布的资产**不改名、不重打 tag**（仓库规矩）；分支当日已对齐到 `0.4.0-beta`
（`c9af929`，`Tauri Build (validate)` 已跑），**下一版即可拿到正确包名** —— 这是唯一还欠验证的一环。

## 规模（2026-10-26 实测，自己数一遍别抄）

| 项 | 值 |
|---|---|
| Rust | **90 个 `.rs` / 26,107 行**，`TODO`/`FIXME` **0 处** |
| 引擎 | BT **27** + 在线影视 **5** + `pan`（网盘）+ `demo`（离线演示） |
| 测试 | `#[test]` **253** + `#[tokio::test]` **326**；另有 M4 `--selftest` 全链路 12/12 |
| workflow | **14 条**，`python3 scripts/check-workflows.py` 全绿（含「引用的脚本是否存在」） |
| 版本 | `Cargo.toml` / `package.json` / `Cargo.lock` 全 = `0.4.0-beta`，`node scripts/release-check.js` 通过 |

## 分支

| 分支 | ahead / behind `main` | 说明 |
|---|---|---|
| `main` | — | Rust 主线，唯一发布源 |
| `feat/tauri` | +79 / −354 | **永久 CI 分支**：`release.yml` 显式 checkout 它构建 Tauri 包。绝不能删。版本号要跟 main 走 |
| `spike/slint-ui` | +3 / −195 | M1 证据（Slint 风险探针） |
| `feat/rust` | +0 / −178 | 已并入 main，纯存档 |
| `legacy/node-electron` | +0 / −278 | Node/Electron 旧实现存档（停在 `9b9461c`） |

## 2026-10-26 第二轮：把「待确认」的四件事收口（都不改行为，除了一行假提示）

| 事 | 之前 | 之后 |
|---|---|---|
| **S2/S3/S4** | ROADMAP 里全是「旁证 / 待你确认」 | 拿**发布版 exe** 实测：导入表 24 个 DLL 全是 Windows 系统 DLL（无 vcruntime140 / msvcp140 / ucrtbase / api-ms-win-crt-\*），净化环境（`PATH` 只留 System32）从空目录启动成功；出窗口 **0.76–0.81 s**；空闲 **43.8 MB**（任务管理器口径，工作集 81.6 MB）。数字已写进 `docs/ROADMAP.md` §1 与 README |
| **Tauri 包名** | 只能「等下一版」验证 | 用 `workflow_dispatch` **干跑** `release.yml`（run `40283266380`，publish 步骤按设计跳过、不建 Release）：`tauri-dist` 里的文件是 `BT-Search-Tauri-Setup-0.4.0-beta.exe` ⇒ 版本对齐生效。**结论：不必为了验包名专门发一版** |
| **UI 假提示** | 状态栏写「Ctrl K 搜索 · Ctrl D 推送」，但全仓 `FocusScope` / `key-pressed` / 键盘处理 **0 处** —— 提示是假的 | 删掉这行。真要做快捷键得给整窗包一层 `FocusScope` 并转发焦点，本机不能编译、风险高于收益，留给以后 |
| **README** | 116 行，数字停在 24 家，没有三模式、没有实测 | 重写：三模式 / 27 BT + 5 视频 + PanSou / **实测指标表** / 快速上手（含 PanSou 起法）/ Node 对照 / 边界 |

## 2026-10-26 第一轮：三项清理（不改行为）

| 事 | 之前 | 之后 |
|---|---|---|
| 残留 Draft Release | 同 tag `v0.4.0-beta` 下留了一个 `isDraft=true` 的空壳（2026-10-24 删掉 `release.yml` 的 `gh release delete` 后，`build.yml` 里 electron-builder 建的 draft 就没人清了），内含 1 个孤儿 `.blockmap` | 按 **release id** 精确删除（不是按 tag —— 同 tag 有两个 release，`gh release delete <tag>` 有删错的风险）。正式 release 与 tag 未动 |
| Tauri 版本号 | `feat/tauri` 四处 = `0.3.15-beta`，导致 v0.4.0 的 Tauri 包顶旧号 | 四处对齐到 `0.4.0-beta`（`tauri.conf.json` 真源 / `src-tauri/Cargo.toml` / `package.json` / `src-tauri/Cargo.lock`）→ `c9af929` |
| 门缺口 | `release-check.js` 只校验 main 一侧（`Cargo.toml` / `Cargo.lock`），**完全没看 `feat/tauri`** | 新增 **6d**：直接读 `feat/tauri` 的四处版本号与当前版本比对，不一致直接红 —— 下次 bump 只改一边会被预检拦下 |

## 这台机器上的硬约束（先读，否则会白折腾）

| 约束 | 事实 |
|---|---|
| **没有 cargo** | `/d/Vibe-Coding/.cargo` 不存在 ⇒ `cargo build/test/clippy` **本机跑不了**，一律走 CI；`AGENTS.md` 也明令不在本机编译（2026-10-25 实测：C 盘剩 **15G**/96% 用、D 盘剩 **119G**/82% 用） |
| **rustfmt 能单跑** | `/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe --edition 2021 <files>`（**必须带 edition**；先就地跑一遍再 `--check`，能省一轮 CI） |
| **没有 node_modules** | `node test/run.js` 跑不了（`src/lib/http.js` 要 axios）；`node test/normalize.test.js` 能跑（零依赖）。Node 测试的真门是 CI 的 `node.yml` |
| `gh` 间歇 403 | 一律 `scripts/gh-retry.sh <原样的 gh 参数>` |
| `git push` 报 SSL 错 | 加 `GIT_SSL_NO_VERIFY=true`（本机 MITM 证书不在 CA bundle 里） |
| 手动触发 workflow | `gh workflow run <file>` 要求该文件已在**默认分支**上 |
| 期望值不许手算 | 跑真 JS 拿：`node -e "require('./src/lib/normalize.js')…"`（零依赖，本机可用） |

## 还挂着的事（按「值得动」排序）

1. **S2 的最后一步：干净 Windows 双击**（唯一还剩的人工判据）。本机能机器验的都验完了
   （导入表 + 净化环境启动），差的只是「一台没装 VC++ Redistributable 的机器」—— 虚拟机/另一台机做。
2. **S3/S4 没有 Electron 对照**：只量了 Rust 一侧（0.8s / 43.8MB）。要写「快多少倍」得装
   96MB 的 Electron 包量一遍；不装也能说清绝对值已达标。
3. **首次运行 1.1–1.6 s 超 1 s 目标**：换新路径可复现，主因是 Defender 首次扫描 + 冷盘读 20MB。
   要压下来得改启动路径（比如延迟加载字体/GL），但**本机不能编译**，盲改不划算。
4. **`latest.yml` 竞态**（2026-10-24 去掉 `gh release delete` 之后，这次**有** `latest.yml`、没有
   `.blockmap`）：全仓无 `electron-updater` / `autoUpdater` 引用 ⇒ **实际影响 ≈ 0**；
   资产集是「谁最后跑完谁决定」的竞态结果，不修也不影响用户。

## 明确「不修」的决定（别重新发现一遍）

| 项 | 为什么不动 |
|---|---|
| `crates/bt-torznab/src/provider.rs` 的 `Box::leak` | 要消掉必须把 `Provider::id()/name()` 从 `&'static str` 改成 `&str`，牵连全部 impl + 测试；收益是每 100 次搜索约 20KB。本机没 cargo，盲改签名风险 > 收益 |
| `add_indexer`（`crates/bt-app/src/main.rs`）的 `id` 恒空 | UI 按**名字**去重 ⇒ 同名索引器不会共存；撞 id 只可能来自手改 `indexers.json`。真要做先定 id 方案 + 迁移已有文件 |
| Node 侧 `/api/magnet`、`/api/torznab/test`、`/api/download/*` 只查 scheme | **已登记的设计决策**，缓解是只绑 `127.0.0.1`（`server.js`）。要放开远程访问必须先加 host 白名单 |
| Node 依赖树的 `@xmldom/xmldom` / `brace-expansion` 告警 | 全在 **dev 树**（eslint / prettier / electron-builder），不进用户机器 |
| 已发布的 Tauri 包名 `0.3.15-beta` | 改名会让「文件名与安装包内部版本」对不上，那是造假 |

## 别的现状（免得重新确认）

- `crates/` 里 **TODO/FIXME 0 处**；测试数与引擎数**别抄文档里的数字**，要看就查最近一次 `rust.yml`
  日志（或直接 `grep -c '#\[test\]'`）。
- M6 差分门覆盖 **24 家 BT**（0 处未豁免差异）；后加的 3 家 Rust 独有引擎在 Node 侧没有对照实现，
  不进差分门，靠单测 + fixture 兜。
- Node 版 40 家仍在 `main` 上共存（`D18`：留着作对照；删除按**函数粒度**推进，还没开始）。
- 最近 12 条 CI 全绿；唯一红的是 `feat(pan)` 那次（Rust test + 体积门），由紧随的 `dc868b5` 修好。
