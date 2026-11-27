# 交接说明（2026-10-29 核对，取代 2026-10-26 版）

> 这份是**给下一个接手的人/AI** 的现状快照。历史决策在 `AGENTS.md`（很长，按需查），
> 设计目标在 `docs/ROADMAP.md`，数据源分级在 `docs/DATA-SOURCES.md`，
> 数据源统一注册表的设计与批次账在 `docs/SOURCES-DESIGN.md`，界面规格（屏 01/02/03 + 未实现清单）在
> `docs/UI-SPEC.md`，CI 事故总账在 `docs/CI_FAILURES.md`。
> 本文件只写「现在是什么状态」和「别重复踩什么」，**不写流水账**。

## 一句话状态

`main` = **本文件所在的提交**（`git log -1 --format=%h`，别在这里写死哈希）。
最新发布 = **`v0.4.4-beta`**（2026-11-27，产物 6 个）。
上一版 `v0.4.3-beta`（2026-10-29）修掉了 0.4.2 的一处致命默认行为（见下）；0.4.4 收的是 10-31~11-04 那批已合入但没发版的修复（见「0.4.4-beta 增量」）。

> ⚠️ **0.4.2 的 Prowlarr 目录那条链是坏的**：6 个默认中文索引器里 5 个用了 `dateparse`，
> 而当时的校验会因「filter 不在白名单」整站拒绝 ⇒ 用户看到「成功 6/6」然后零结果，
> 且和「站点挂了」长得一样。0.4.3 已修（`validate_rule` 只校验结构，未知 filter 按原样
> 处理并在状态栏列出）。

## 0.4.4-beta 增量（2026-11-27 发版时补记）

`v0.4.3-beta` 之后、`v0.4.4-beta` 之前 main 上落地的 4 个提交（都不改数据源结构，属于「补洞 + 去风险」）：

| 提交 | 改了什么 | 为什么重要 |
|---|---|---|
| `d4d1a7c` fix(catalog) | 目录成员的「成功」改成**「能搜」** | 之前只要定义导入成功就报 ✓，站点实际搜不出东西也显示成功，和「站点挂了」长得一样 |
| `0b819e6` fix(import) | 重新导入目录时更新地址并合并成员，保留用户开关 | 换订阅地址不再丢用户的逐站开关 |
| `8c65956` docs(readme) | README 补齐 14 处「待补」占位符 | 发版页面的说明不再缺免责与下载入口 |
| `52733df` + `5c7c969` ci(release) | `build.yml` 改 `--publish=never`；`latest.yml`/`.blockmap` 拆成独立 artifact | 不再给每个 tag 留草稿 Release；附件缺失不再阻断发版 |

同批发版准备时顺带修的：`package-lock.json` 根版本停在 `0.4.1-beta` 一直没人管（`release:check` 不查它），已对齐。

## 规模（2026-10-29 实测，自己数一遍别抄）

| 项 | 值 |
|---|---|
| Rust | **98 个 `.rs` / 31,369 行**（`crates/`），`TODO`/`FIXME` **0 处** |
| 引擎 | BT **28 家**（另有离线 `demo`，不算真站）· 在线影视 **5 家** · 字幕 **3 站**（SUBHD / ASSRT / ZIMUKU）· PanSou 网盘 |
| 测试 | `#[test]` **325** + `#[tokio::test]` **330**（2026-11-27 重数；2026-10-29 是 321/329）；另有 M4 `--selftest` 全链路 |
| workflow | **14 条**，`python3 scripts/check-workflows.py` 全绿 |
| 版本 | `Cargo.toml` / `package.json` / `Cargo.lock` / `feat/tauri` 四处 = `0.4.4-beta`，`node scripts/release-check.js` 8 项全过 |

## 数据源体系（2026-10-29 新，**这是最大的结构变化**）

以前是「写死 28 家 + 5 家 + 手改 `videosources.json`」。现在：

**`%APPDATA%\bt-search\sources.json` 是唯一真源**，搜索从它派生，不再读旧文件。

| 字段 | 作用 |
|---|---|
| `adapter` | **唯一分叉点**：`builtin` / `maccms_json` / `html_rule` / `cardigann_catalog` / `torznab`。加新取数方式 = 加一个枚举值 + 一个实现，不动 registry |
| `enabled` | 用户开关。订阅刷新、熔断、迁移**都不动它** |
| `rule` | 用户手写的抓取规则（统一存成 Cardigann 形状的 JSON） |
| `members[]` | 目录（Prowlarr）的子站，带 `language`，逐站开关 |

设置面板六个入口：

| 行 | 作用 | 代码 |
|---|---|---|
| 数据源 | 摘要（`33 条（内置 33 · 导入 N）`） | `sources::describe` |
| 导入源 | TVBox 单仓/多仓、LibreTV v2、Prowlarr 清单、裸 URL **自动识别** | `source_import::guess_format` |
| 规则 | 贴 JSON/YAML → **测试**（真跑一次，报条数与耗时）→ 保存 | `main::test_rule` / `save_rule` |
| 订阅 | 贴源清单 URL → 添加即拉 → 刷新全部 → 启动自动刷 | `main::refresh_subscriptions` |
| 目录 | 拉取成员定义（只拉已启用且未缓存的） | `main::prepare_catalog` |
| 成员 | 逐站 ✓/✗ 开关 | `main::toggle_member` |

### 三条不能改的语义（都有单测钉着）

1. **订阅刷新只换定义**（`name` / `url`），`enabled` 一个字不动，手改的 `rule` / `members` 不覆盖
   —— 否则用户每次启动都会看到「自己没关过的源又亮了」（`sources::merge_refreshed`）。
2. **熔断是运行期事实，不写进配置**：连续 `health::STALE_AFTER`(3) 次空手就临时退出搜索，
   `enabled` 保持 true，数据好转自动回来（`SourceEntry::triggers_breaker`）。
3. **手写规则严格、第三方定义宽松**：手写的 filter 不在白名单 → 报错并**带名字**（用户能改）；
   Prowlarr 的定义 → 放行 + 状态栏报告（用户改不了，拒绝只会让功能看起来是坏的）。
   分叉在 `cardigann::strict_validate` vs `validate_rule`。

### 安全边界

| 风险 | 处置 |
|---|---|
| 远程 JSON 里的成员 `id` | `sources::safe_member_id` 白名单；不校验可 `../../` 写到配置目录外 |
| 用户随手贴的 URL | 只放行 `http(s)`；**不递归**多仓（只提示子仓地址） |
| 正则 ReDoS | 不额外防护，靠单站超时 + 熔断兜底（用户自己的规则） |
| 订阅回来的规则 | 不自动改开关、不覆盖手改内容 |

## 分支

| 分支 | ahead / behind `main` | 说明 |
|---|---|---|
| `main` | — | Rust 主线，唯一发布源 |
| `feat/tauri` | +83 / −402 | **永久 CI 分支**：`release.yml` 显式 checkout 它构建 Tauri 包。绝不能删。版本号要跟 main 走（4 处） |
| `spike/slint-ui` | +3 / −243 | M1 证据（Slint 风险探针） |
| `feat/rust` | +0 / −226 | 已并入 main，纯存档 |
| `legacy/node-electron` | +0 / −326 | Node/Electron 旧实现存档 |

## 这台机器上的硬约束（先读，否则会白折腾）

| 约束 | 事实 |
|---|---|
| **没有 cargo** | `D:\Vibe-Coding\.cargo` 已清理且不在 PATH ⇒ `cargo build/test/clippy` **本机跑不了**，一律走 CI（一轮约 1.5–4 分钟）。`AGENTS.md` 也明令不在本机编译（用户原话「别在本地新增构建环境，我电脑没空间了」，两盘长期 90%+） |
| **rustfmt 能单跑** | `D:\Vibe-Coding\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\rustfmt.exe --edition 2021 <files>`（**必须带 edition**）。push 前先就地跑一遍再 `--check`，能省一轮 CI；它同时是语法检查器 |
| **没有 node_modules** | `node test/run.js` 跑不了；`node test/normalize.test.js` 能跑（零依赖）。Node 测试的真门是 CI |
| `gh` 间歇 403 | 一律 `scripts/gh-retry.sh <原样的 gh 参数>`。长时间被拒时脚本也挡不住，等几分钟再试 |
| `git push` 报 SSL 错 | 加 `GIT_SSL_NO_VERIFY=true`（本机 MITM 证书不在 CA bundle 里） |
| 手动触发 workflow | `gh workflow run <file>` 要求该文件已在**默认分支**上 —— live-smoke / html-probes / fetch-fixture / provider-health 在 `main` 上有副本，**改要同步两份** |
| `Cargo.lock` 入库且 `--locked` | 只改 `Cargo.toml` 不改 lock ⇒ CI 直接红。改依赖后必须跑 `workflow run lockfile.yml --ref <branch>` 再 `git pull --rebase` |
| 改结构体字段 | **所有构造点都要补**（`Member` 加 `language` 时 `source_import` 里 E0063 炸过一次）；测试 helper 加参数后所有调用点同理 |
| 跨分支提交 | **永远显式列文件，别 `git add -A`**（2026-10-29 在 `feat/tauri` 上把本地 `.audit/`、`.workbuddy-ai/` 一起提交了 1829 行，已 `git rm --cached` 撤回） |
| 加 std-widgets 组件 | `app.slint` 是**显式列名** import：`import { LineEdit, ListView, TextEdit } from "std-widgets.slint";` 用了没列的组件直接 `Unknown element`（2026-10-28 踩过） |
| Slint 类型 | 所有 `set_xxx` 参数是 `SharedString` ⇒ `format!(...).into()`；`ModelRc` **不能** `From<Vec<T>>`，要 `ModelRc::from(Rc::new(VecModel::from(vec)))` |
| 用脚本改源码 | **非 ASCII 锚点会偶发对不上**（肉眼相同但不匹配）—— 用 ASCII 锚点（`pub fn save(`）更稳；heredoc 里的 `\n` 也可能被写坏成 `//n`（2026-10-29 把一个 YAML 测试串毁了） |

## 还挂着的事（按「值得动」排序）

1. **端到端从未真机验证**（唯一真正的大空白）。所有数据源功能都是「编译过 + 单测过」，
   没有一次在真机上点过界面、拉过 Prowlarr 定义、搜出过结果。要验的链：
   ```
   设置 → 导入源：贴 https://indexers.prowlarr.com/master/12 → 导入
   设置 → 目录：点「拉取定义」→ 看状态栏报了几个成功
   设置 → 成员：看 ✓ dmhy 之类的中文站
   磁力模式：搜一个词 → 看结果里有没有这些站
   ```
   任何一环断，状态栏那句错误文案都是可定位的（会带 filter 名 / 失败站名 / 失败成员名）。
   → **逐条勾选清单：[`docs/E2E-CHECKLIST.md`](E2E-CHECKLIST.md)**（每步都有预期文案与失败定位）。
2. **S2 的最后一步：干净 Windows 双击**。本机能机器验的都验完了（导入表 24 个全系统 DLL +
   净化环境启动），差的只是「一台没装 VC++ Redistributable 的机器」。
3. **Cardigann 只实现了 7 个 filter**（`replace` / `re_replace` / `append` / `prepend` / `trim` /
   `split` / `regexp`）。真实高频的 `dateparse` / `timeparse` / `andmatch` / `sort` 没做 ⇒
   目录站点的**日期字段是原始文本**（不影响标题 / 磁力 / 做种数）。要补就扩 `SUPPORTED_FILTERS`，
   注意同步改 `unsupported_filter` 的报错文案。
4. **首次运行 1.1–1.6 s 超 1 s 目标**：主因 Defender 首次扫描 + 冷盘读 20MB。要压下来得改启动路径
   （延迟加载字体/GL），但**本机不能编译**，盲改不划算。
5. **`latest.yml` 竞态**：全仓无 `electron-updater` 引用 ⇒ 实际影响 ≈ 0，不修。

## Node 版何时能整块退役（2026-11-27 审计结论）

结论先说：**现在还不能整块删**。Node/Electron 实现被 4 条硬依赖同时钉着，删任何一块都会让 CI 或产物直接坏：

| 依赖 | 位置 | 删掉的后果 |
|---|---|---|
| **M6 差分硬门** | `.github/workflows/rust.yml`（M6 步骤）+ `scripts/diff/fixtures.cjs`（24 家） | 门没有对照物 → 只能降级成探针（`m6-diff.yml` 本来就是探针形态） |
| **Node 自身测试门** | `.github/workflows/node.yml` → `npm test` | 门空转（这条最容易删，但也最容易删得没感觉） |
| **Electron 安装包** | `build.yml` + `release.yml` 的 electron job | 每次发版少一个产物，也就是「引擎全 40 家」那一份 |
| **Tauri 分支的同源后端** | `feat/tauri` 里的 `server.js` / `src/` / `public/` 副本 | `release.yml` 的 tauri job 打出来是白屏包 |

**退役顺序（每步都能单独回滚）**：

1. 把 M6 从 rust.yml 的**硬门**降级（删步骤，只留 `m6-diff.yml` 探针），同时下线 `node.yml`，并改掉 AGENTS 里 Q4/D18 的表述。
2. 再决定 Electron 停不停发：停 → 删 `build.yml`、`release.yml` 的 electron job、`electron/`、`public/`、`start.bat` / `stop.bat`、`package.json` 的 `build` 段。
3. 最后删 `src/providers/*`（按 AGENTS 待删清单逐条）、`src/lib/*`、`server.js`、`test/run.js`；⚠️ **`test/fixtures/` 永久保留**（Rust 侧测试直接读它）。
4. `feat/tauri` 要单独处理（它持有自己的副本），否则 tauri job 红。

审计数据（2026-11-27，逐文件核过引用）：`src/providers/` 42 个 provider **全在** `index.js` 的 REGISTRY，`src/lib/` 6 个文件**全被引用**（`scraper.js` 只被 eztv/torrentdownload/torrentkitty 三家用到，最容易误判成死代码）；全仓**真正零引用**的只有 5 个文件，已删（见 AGENTS「🗑 已清理」）。

## 明确「不修」的决定（别重新发现一遍）

| 项 | 为什么不动 |
|---|---|
| `Provider::id()/name()` 的 `&'static str` + `Box::leak` | 要消掉必须把签名改成 `&str`，牵连全部 impl + 测试（含新加的 `RuleProvider`）；收益是每 100 次搜索约 20KB。本机没 cargo，盲改签名风险 > 收益 |
| `add_indexer` 的 `id` 恒空 | UI 按**名字**去重 ⇒ 同名索引器不会共存；撞 id 只可能来自手改 `indexers.json` |
| Node 侧 `/api/magnet`、`/api/torznab/test`、`/api/download/*` 只查 scheme | 已登记的设计决策，缓解是只绑 `127.0.0.1`。要放开远程访问必须先加 host 白名单 |
| Node 依赖树的 `@xmldom/xmldom` / `brace-expansion` 告警 | 全在 **dev 树**（eslint / prettier / electron-builder），不进用户机器 |
| 已发布的 Tauri 包名对不上 | 改名会让「文件名与安装包内部版本」不一致，那是造假。0.4.1 起已对齐 |

## 别的现状（免得重新确认）

- M6 差分门覆盖 **24 家 BT**（0 处未豁免差异）；后加的 4 家 Rust 独有引擎在 Node 侧没有对照，
  不进差分门，靠单测 + fixture 兜。
- Node 版 40 家仍在 `main` 上共存（`D18`：留着作对照；删除按**函数粒度**推进，还没开始）。
- `videosources.json` / `indexers.json` 仍会被读，但**只用于一次性迁移**进 `sources.json`；
  写入已整体搬到 `sources.json`（`config_store::save_video_sources` 已删）。
- ~~同一 tag 下可能同时有 draft 和正式 release~~ —— **2026-10-31 已收口**：草稿是 `build.yml` 里
  electron-builder 的 `--publish=onTag`（默认 `releaseType=draft`）建的，每个 tag 留一个只剩
  `.blockmap` 的空壳（v0.4.1/0.4.2/0.4.3 三个已按 release id 删除）。现在 `build.yml` 是
  `--publish=never`，**只有 `release.yml` 的 publish job 建 Release**。
  删除 release 仍一律按 **release id**（先 `gh api .../releases` 拿 id），别 `gh release delete <tag>`。
