# BT 聚合搜索 · Torrent Meta-Search

一个面向桌面的 **BT 种子聚合搜索应用**，参考 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch) 的"多引擎并行聚合"思路，用户输入一个关键词即可同时从 **40+ 个种子站点**并行检索，结果实时流入、跨站去重合并展示。

> 采用 **服务端代理** 架构：所有对第三方站点的抓取都在 Node 后端完成，前端只与本地 API 通信，天然规避浏览器的跨域（CORS）限制。

> ⚠️ **本项目由 AI 辅助生成**（基于 AI 编码助手，参考 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch) 的设计思路）。代码可供学习、修改与再分发，但**不保证完整性与安全性**，使用前请自行审阅。

## 安装与运行

推荐直接用打包好的桌面应用（开=启动、关=净退出，行为像普通本地软件）；也可以用浏览器方式跑本地后端。

### 桌面应用（Electron，推荐日常使用）

#### 方式 A：免安装便携版（已生成，直接双击即可）

仓库已附带打包好的便携版，位于：

```
dist/portable/BT聚合搜索/BT聚合搜索.exe
```

**直接双击 `BT聚合搜索.exe` 就能用**——无需安装、不写注册表、不落 C 盘：所有文件都集中在 `dist/portable/BT聚合搜索/` 一个文件夹里，整包约 **360MB**（体积主要是 Electron 自带的 Chromium 运行时）。
需要转移或删除时，把整个文件夹剪切 / 删除即可，不留注册表项与残留进程。

> 便携版 vs 安装包：便携版是**解压即用**的整包（约 360MB，含运行时），适合放 U 盘 / 移动硬盘或不想装东西的场景；下方方式 B 的**安装包**约 70–90MB（安装时才展开运行时，装完硬盘占用约 150–200MB），走系统安装流程、可选安装目录、开始菜单有快捷方式。两者运行时行为完全一致（开=起后端，关=净退出），按习惯二选一即可。

- **开 = 启动**：双击应用，后端随主进程一起拉起，窗口直接打开搜索界面；
- **关 = 全关**：关掉窗口，整个进程退出，后端被操作系统一并回收，**不留任何残留进程**。

> 便携版是手动拼装的：`node_modules/electron/dist/` 改名为 `BT聚合搜索.exe`，应用代码放在 `resources/app/` 下，生产依赖（express/axios/cheerio）只装一份、已裁剪掉 electron/electron-builder 等开发依赖。行为与下方安装包完全一致：开=起后端，关=净退出。

#### 方式 B：开发模式 / 自行打包安装包

```bash
npm install          # 已含 electron / electron-builder（开发依赖）
npm run electron     # 开发模式：直接开桌面窗口运行（自动挑空闲端口，不与 npm start 冲突）
npm run dist         # 打包成 Windows 安装包（输出到 dist/，形如 BT聚合搜索-Setup-<版本>.exe）
```

> 打包用的是 `electron-builder`（NSIS 安装包，可自选安装目录）。安装包约 70–90MB、解压后硬盘占用约 150–200MB——体积主要来自 Electron 自带的 Chromium 运行时，与项目代码量无关。
> 后端在 Electron 主进程内以 `require('./server').start(port)` 方式启动（见 `electron/main.js`），因此无需单独的 node 进程，关闭即净退出。

#### 方式 C：Tauri 桌面应用（实验性）

除了 Electron，本项目还提供一个用 **Tauri** 打包的版本：用系统自带的 WebView（Windows 上是 WebView2）替代 Electron 自带的 Chromium，因此安装包和内存占用都更小；Node 后端则以 sidecar 二进制随应用启动（见 `src-tauri/binaries/`）。

Tauri 版**由 GitHub Actions 自动构建**——打 `v*` tag 时，CI 会在同一个 Release 里同时产出 Electron 与 Tauri 两套 Windows 安装包（见 `.github/workflows/release.yml`）。Tauri 包命名为 `BT-Search-Tauri-Setup-<版本>.exe`，去 [Releases](https://github.com/NoNameLeGo/torrent-search-app/releases) 下载即可，**本地无需安装 Rust 环境**。Tauri 的源码与构建配置在 `feat/tauri` 分支。

**Electron 与 Tauri 的区别**

| | Electron | Tauri |
|---|---|---|
| 前端渲染 | 自带 Chromium 运行时 | 复用系统 WebView（Windows 用 WebView2） |
| 后端 | 主进程内 `require('./server')` | 独立 Node sidecar 二进制（`server-*.exe`，约 84MB） |
| 安装包体积 | 约 70–90MB（另有便携版约 360MB） | 显著更小（WebView 不打进包里） |
| 运行时占用 | 每个应用一份 Chromium，内存开销较高 | 借用系统 WebView，内存更省 |
| 一致性 | 各机渲染完全一致（自带引擎） | 依赖系统 WebView 版本，Win10/11 一般已内置 WebView2，老系统可能需先装 |
| 成熟度 | 稳定、日常可用 | 实验性 |

简单说：**要稳定、开箱即用选 Electron；追求体积小、内存省可以试 Tauri 版**。两者搜索功能与界面完全一致，同一个 Release 里都能下到。

### 浏览器方式（本地后端）

**方式一（推荐，双击即用）**
直接双击项目里的 `start.bat` —— 它会自动启动后端、等端口就绪后打开浏览器。关服务双击 `stop.bat` 即可。

**方式二（命令行）**

```bash
cd torrent-search-app
npm install          # 安装 express / axios / cheerio
npm start            # 或 node server.js
# 打开 http://localhost:3000
```

默认端口 3000，可用 `PORT=8080 node server.js` 覆盖。

> **为什么需要后端、不能直接双击一个 HTML？** 真正的搜索请求发往 1337x / TPB / NYAA 等站点，浏览器有跨域（CORS）限制，网页无法直接抓取它们。所以必须由 Node 后端在服务端代为抓取，前端只跟本地 `localhost:3000` 通信。`public/index.html` 单独用 `file://` 打开是连不上后端的——这也是为什么提供了 `start.bat` 来一键把后端和浏览器都拉起来。

## 功能

| # | 需求 | 实现 |
|---|------|------|
| 1 | 聚合多引擎并行搜索 | `src/providers/` 下每个引擎一个模块，统一接口；`search()` 并行查询所有启用引擎，单个引擎失败不影响其它。结果经 SSE **实时流入**，每个引擎的耗时与 ✓/✕ 状态逐条显示 |
| 2 | 结果含名称/大小/时间/做种/下载/磁力，卡片展示 | 规范化结果模型，前端卡片网格展示，配色徽章区分来源；跨站按 `infoHash` **去重合并**为单卡 |
| 3 | 排序与筛选 | 客户端排序（相关度/做种/大小/时间 + 升降序）+ 筛选（最小做种、最小大小、名称包含）；4K / 1080p / 720p / HDR·Dolby **画质快捷标签**、标题命中搜索词**高亮** |
| 4 | 分页 / 无限滚动 | `IntersectionObserver` 触底自动加载下一页，去重后追加 |
| 5 | 引擎分组 / 预设 | 引擎按 动漫 / 影视 / 综合 / 成人 / 其他 / 自定义 分组，每组一键全选 / 全不选；选中项存 localStorage，下次打开还在 |
| 6 | 搜索历史 / 收藏 | 最近搜索词下拉（可单条删除 / 清空）；顶栏「收藏」视图，收藏的种子（含 magnet / 来源快照）存 localStorage，跨会话可直接打开磁力 / 推送 |
| 7 | 下载 | 一键"复制磁力""打开磁力"（`magnet:` 交给系统默认客户端如 qBittorrent / 迅雷，**零配置**）；另支持 qBittorrent WebUI 一键推送，首次加载自动探测本机 `localhost:8080/8081/9090` 默认账号，命中即免配置启用 |
| 8 | 批量操作 | 勾选多条卡片浮出工具条：批量推送 qBittorrent、批量复制磁力、导出 CSV（Excel 友好，带 BOM） |
| 9 | 详情预览 | 聚合信息弹窗（做种 / 大小 / 分类 / 文件数 / infoHash / 磁力），并列出各来源详情页外链 |
| 10 | 自定义索引器 | 支持接入 **Torznab**（Jackett / Prowlarr / *arr）自建索引器，归入「自定义」分组 |

## 架构

```
frontend (public/)  ──HTTP/SSE──▶  Express server (server.js)
                                      │
                                      ├─ /api/providers               列出可用引擎
                                      ├─ /api/search                  聚合搜索（并行调用各 provider）
                                      ├─ /api/search/stream (SSE)     结果实时流入
                                      ├─ /api/magnet                  惰性解析详情页磁力（如 1337x）
                                      ├─ /api/download/qbittorrent    代理推送至 qBittorrent WebUI
                                      └─ /api/torznab/*               Torznab 索引器管理
                                      │
                            src/providers/*.js   （40+ 内置引擎 + demo + 动态 Torznab）
                                      │
                            src/lib/{http,normalize}.js   请求封装 + 结果规范化
```

各引擎模块导出 `search(query, {page})`，返回 `{ results, error, hasMore }`。公共层负责：
- 解析大小字符串（`1.2 GB` → 字节）、相对时间（`2 hours ago` → 时间戳）；
- 构造磁力链接（`magnet:?xt=urn:btih:<hash>`）；
- 统一字段到 `TorrentResult` 结构。
- `src/lib/http.js` 的封装（`getText`/`getJSON`/`postJSON`）**永不抛异常**，失败返回 `{ error }`，保证单引擎故障不影响聚合。

## 已集成的引擎

已覆盖 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch) 原仓库的 **全部 40 个内置站点**，另加本地 Demo 引擎与 Torznab 自定义索引器。UI 里按下列分组展示，可整组一键全选 / 全不选（详见 [SEARCH_ENGINE_PORT_COVERAGE.md](./SEARCH_ENGINE_PORT_COVERAGE.md)）。

- **综合** — The Pirate Bay、1337x、Knaben、Torrents-CSV、BT4G、BTDigg、LimeTorrents、TorrentDownload(s)、TorrentDatabase、TorrentKitty、uindex、ZeroMagnet、BitSearch、Internet Archive、FileMood 等。
- **动漫 / 亚洲** — NYAA、AniLibria、AnimeTosho、AniRena、Bangumi Moe、动漫花园(dmhy)、Mikan、SubsPlease、東京トショカン、NekoBT。
- **影视 / 剧集** — EZTV、YTS、TheRARBG、Torrent9、OxTorrent、Rutor、MegaPeer。
- **成人** — Sukebei、MyPornClub、XXXClub、XXXTracker。
- **其他** — AudiobookBay（有声书）、BlueROMs（ROM）、LinuxTracker（Linux 发行版）。
- **自定义** — 用户添加的 Torznab 索引器（Jackett / Prowlarr / *arr）。
- **Demo（离线）** — 生成确定性的拟真数据，保证在网络受限 / 引擎被墙时界面、排序、筛选、分页依然可演示。

> 部分引擎的能力有差异：如 `1337x` 的磁力在详情页，点击"获取磁力"时惰性解析；支持翻页的引擎从第 2 页起才继续查询，单次返回型引擎在首页后不再重复请求。

## 说明与边界

- 第三方站点可能随区域 / 网络被屏蔽；本环境中部分引擎偶尔不可达，属正常网络现象，状态栏会逐引擎显示 ✓/✕。
- 抓取依赖公开页面结构，若目标站点改版，对应 provider 的解析选择器需同步更新。
- 生产部署建议补充：请求速率限制、结果缓存、以及遵守目标站点 `robots.txt` 与当地法律法规。
- 本工具仅做"搜索聚合"，不托管、不分发任何版权内容。

## 路线图 / 计划中的功能

大部分用户侧功能已完成（画质筛选 / 高亮、搜索历史 / 收藏、引擎分组 / 预设、批量操作、详情预览、SSE 实时流、跨站 infoHash 去重合并均已上线）。剩余候选：

- [ ] **更多下载客户端** — 现仅支持 qBittorrent WebUI，可加 Transmission RPC、Aria2 RPC、Deluge。
- [ ] **Tauri 版转正** — 打包流程已建好（CI 自动出包，见上文方式 C），待充分验证后从「实验性」升为并列推荐。

## 许可证

本项目以 **GNU GPL-3.0** 发布。

> 选用 GPL-3.0 而非 AGPL-3.0 的原因：本项目是**本地运行的桌面应用**（用户自行运行 `node server.js` 访问 `localhost`），并非托管给公众的网络服务，因此 AGPL 针对"网络交互即须开源"的额外条款在此用不上。若你打算把它部署为公开的在线搜索服务，建议改用 **AGPL-3.0**。

完整许可证文本见 [LICENSE](./LICENSE)。
