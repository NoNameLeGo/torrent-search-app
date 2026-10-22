# BT 聚合搜索 · Torrent Meta-Search

<p align="center">
  <img src="./assets/readme/hero.svg" width="100%" alt="torrent-search-app — Desktop BT torrent meta-search with parallel engines and downloader integration">
</p>

一个面向桌面的 **BT 种子聚合搜索引擎**。输入一个关键词，**多个种子站点并行检索**，
结果跨站去重、合并成一张卡片。不依赖任何第三方服务，全量在本地运行。

> ⚠️ **本项目由 AI 辅助生成**，参考 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch)
> （Kotlin/Android 原生应用）的多引擎并行聚合思路。代码可供学习、修改与再分发，
> 但**不保证完整性与安全性**，使用前请自行审阅。

---

## 下载哪个？

三种 Windows 产物都在 [Releases](https://github.com/NoNameLeGo/torrent-search-app/releases)。
**它们的引擎数量并不相同**，选之前请看清：

| 产物 | 是什么 | 引擎 | 适合 |
|---|---|---|---|
| **`BT-Search-Rust-*.exe`** | **Rust 主线**：单文件、**零运行时依赖**、约 18 MB | **17 家** | **尝鲜 / 看体积与速度**；下载器配置会保存 |
| `BT-Search-Electron-Setup-*.exe` | Node 后端 + Electron 壳（自带 Chromium） | **40 家** + Torznab | **日常主力**（功能最全、最成熟） |
| `BT-Search-Tauri-Setup-*.exe` | 同一套 Node 后端 + 系统 WebView2 | 40 家 + Torznab | 想要更小体积的 Node 版 |

三者的搜索功能与界面**不在一个量级上**：Rust 版是新写的主线（引擎少但干净、无运行时依赖），
Node 版是多年在用的完整版。逐项差异与选择建议见
[`docs/RELEASE_ARTIFACTS.md`](./docs/RELEASE_ARTIFACTS.md)。

---

## Rust 版（`main` 主线，2026-10-16 起）

**终局形态**：Windows 桌面原生应用 —— 单文件、零运行时依赖、启动快、内存低。
浏览器 / 手机访问模式**已放弃**（它给不出"小快原生"这个成品）。

**现在有什么**

| | 能力 |
|---|---|
| 🔍 | **17 家引擎**并发搜索（16 个移植站点 + 离线 `demo`），每站独立超时、单站失败不影响其他，状态条逐个显示 ✓/✕/耗时 |
| 🏷️ | **排序**（做种数 / 相关度 / 体积 / 日期，升降序）+ **分类筛选**（chips）+ 关键词高亮 |
| 📄 | **分页**（"第 N 页 · 下一页"，是否还有下一页由引擎层回答，不是猜的） |
| 🎛️ | **站点开关**（勾选要搜的站；一个都不勾时拦住不搜，而不是偷偷搜全部） |
| 🧩 | **Torznab 索引器**：Jackett / Prowlarr / \*arr —— 在界面里填地址与 API key 即可，配置持久化 |
| ⬇️ | **四种下载器**一键推送：qBittorrent / Transmission / aria2·Motrix / Gopeed，配置持久化 |
| 🩺 | **失败说得清**：区分"网络层不通"与"站点层拒绝"，并提示该查网络还是换站 |
| ⚠️ | **引擎腐坏可见**：连续多轮空手的引擎在状态条上打 ⚠（`--health` 可看完整历史） |

**还差什么（如实说）**

- **引擎只有 17 家**（Node 版 40 家）；未移植的 24 家**有意推迟** —— 引擎多 = 启动慢、包大、
  维护重、失效多，先看存活再决定投不投（见 `docs/ROADMAP.md` 的 D13）
- **"零运行时依赖"仍是旁证**：CI 断言了 exe 不依赖任何 MSVC 运行时 DLL，但"拷到干净 Windows
  双击能跑"这条正式判据**只能人工验**，尚未做
- 无 Torznab 的 `t=caps` 能力探测、无缓存、无收藏 / 搜索历史（Node 版有）

**架构与文档**：核心是一个**库**（进程内直调，无 HTTP、无 SSE），UI 是 Slint。
真值对照靠与 Node 版逐字段差分（M6 覆盖 **20 家 × 15 字段**，0 处未豁免差异）。
细节见 [`docs/ROADMAP.md`](./docs/ROADMAP.md) 与 [`AGENTS.md`](./AGENTS.md)。

---

## Node 版（仍在维护，功能最全）

`server.js`（Express）+ 原生 JS 前端，被两个桌面壳复用：**Electron**（自带 Chromium）与
**Tauri**（复用系统 WebView2，体积更小）。所有抓取在本地后端完成，天然规避浏览器 CORS。

| | 能力 |
|---|---|
| 🔍 | **40 家引擎** + 离线 `demo` + **Torznab 自定义索引器**（Jackett / Prowlarr / \*arr），8 路并发 |
| ⚡ | **SSE 实时流**：逐引擎推送状态与结果，慢站不拖累快站；按 infoHash 跨站去重合并 |
| 🏷️ | 画质快捷筛选 / 内容分类 / 关键词高亮 / **无限滚动** |
| 📋 | 引擎分组（综合·动漫·影视·成人·其他·自定义）+ 三档预设（推荐 / 标准 / 全部） |
| 📚 | **搜索历史 & 收藏**（跨会话）、已浏览置灰、**详情预览**（海报 / 描述 / 多站外链） |
| 📤 | 批量推送 / 批量复制磁力 / **导出 CSV**（防公式注入） |
| ⬇️ | 四种下载器一键推送 + 启动自动探测本机端口 |
| ⌨️ | 快捷键（Ctrl+K / Ctrl+D / Ctrl+Shift+F）、亮色暗色主题、移动端响应式 |

**从源码跑（Node 版）**

```bash
npm install
npm run electron     # 桌面窗口（自动选空闲端口）
npm start            # 或 node server.js → http://localhost:3000
npm run dist         # 打 Windows 安装包 → dist/
```

> 本仓库**不在本机编译任何东西** —— 编译与打包全部走 GitHub Actions
> （`.github/workflows/`）。本地只写代码，产物从 Actions Artifacts 或 Releases 下载。

**架构细节**（分层、provider 契约、两个壳的差异、Tauri sidecar 方案）见
[`CLAUDE.md`](./CLAUDE.md) —— README 不重复那一层。

---

## ⚠️ 说明与边界

- 第三方站点可能因区域 / 网络被屏蔽；状态栏逐引擎显示 ✓/✕。抓取依赖公开页面结构，
  **站点改版就需要更新对应的 provider** —— 这也是 Rust 版做"引擎腐坏可见"的原因。
- 本工具**只做搜索聚合**，不托管、不分发任何版权内容。生产部署建议补充结果缓存、
  遵守站点 `robots.txt` 与当地法规。
- **凭据是明文存储的**：下载器密码 / token 存在 `%APPDATA%\bt-search\config.json`，
  Torznab API key 存在 `indexers.json`（Node 版是 `data/torznab.json`，已 gitignore）。
  它们都在你自己机器的用户目录下 —— 写在这里是为了**别让人以为它是加密的**。
- 若把 Node 版的后端部署成公开服务，请务必只绑定 `127.0.0.1`，避免暴露代理端点。

## 📄 许可证

**GNU General Public License v3.0**，完整文本见 [LICENSE](./LICENSE)。

> 选用 GPL-3.0 而非 AGPL-3.0：本项目是**本地运行的桌面应用**，不提供公开网络服务。
> 若你要把它部署成公开的在线搜索服务，建议改用 **AGPL-3.0**。
