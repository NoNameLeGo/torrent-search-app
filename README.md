# BT 聚合搜索 · Torrent Meta-Search

一个面向桌面浏览器的 **BT 种子聚合搜索 Web 应用**，参考 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch) 的"多引擎并行聚合"思路，用户输入一个关键词即可同时从多个种子站点检索结果。

> 采用 **服务端代理** 架构：所有对第三方站点的抓取都在 Node 后端完成，前端只与本地 API 通信，天然规避浏览器的跨域（CORS）限制。

> ⚠️ **本项目由 AI 辅助生成**（基于 AI 编码助手，参考 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch) 的设计思路）。代码可供学习、修改与再分发，但**不保证完整性与安全性**，使用前请自行审阅。

## 运行

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

## 桌面应用（Electron，推荐日常使用）

除了用浏览器访问，本项目也打包成了 **Electron 桌面应用**，行为像一个普通本地软件：

- **开 = 启动**：双击应用，后端随主进程一起拉起，窗口直接打开搜索界面；
- **关 = 全关**：关掉窗口，整个进程退出，后端被操作系统一并回收，**不留任何残留进程**（不用再手动 stop）。

### 方式 A：免安装便携版（已生成，直接双击即可）

仓库已附带打包好的便携版，位于：

```
dist/portable/BT聚合搜索/BT聚合搜索.exe
```

**直接双击 `BT聚合搜索.exe` 就能用**——无需安装、不写注册表、不占 C 盘（所有文件都在项目目录 `dist/portable/` 下，约 360MB，全部在 D 盘）。
需要转移或删除时，整个 `dist/portable/BT聚合搜索/` 文件夹剪切/删除即可，不留任何残留。

> 便携版是手动拼装的：`node_modules/electron/dist/` 改名为 `BT聚合搜索.exe`，应用代码放在 `resources/app/` 下，生产依赖（express/axios/cheerio）只装一份、已裁剪掉 electron/electron-builder 等开发依赖。行为与下方安装包完全一致：开=起后端，关=净退出。

### 方式 B：开发模式 / 自行打包安装包

```bash
npm install          # 已含 electron / electron-builder（开发依赖）
npm run electron     # 开发模式：直接开桌面窗口运行
npm run dist         # 打包成 Windows 安装包（输出到 dist/，形如 BT-Search-Electron-Setup-<版本>.exe）
```

> 打包用的是 `electron-builder`（NSIS 安装包，可自选安装目录）。安装包约 70–90MB、解压后硬盘占用约 150–200MB——体积主要来自 Electron 自带的 Chromium 运行时，与项目代码量无关。
> 后端在 Electron 主进程内以 `require('./server').start(port)` 方式启动（见 `electron/main.js`），因此无需单独的 node 进程，关闭即净退出。

## 需求对照

| # | 需求 | 实现 |
|---|------|------|
| 1 | 聚合多引擎搜索（1337x / TPB / NYAA） | `src/providers/` 下每个引擎一个模块，统一接口；`search()` 并行查询所有启用引擎，单个引擎失败不影响其它 |
| 2 | 结果含名称/大小/时间/做种/下载/磁力，卡片展示 | 规范化结果模型，前端卡片网格展示，配色徽章区分来源 |
| 3 | 按大小/做种数/上传时间排序与筛选 | 客户端排序（相关度/做种/大小/时间 + 升降序）+ 筛选（最小做种、最小大小、名称包含） |
| 4 | 分页 / 无限滚动 | `IntersectionObserver` 触底自动加载下一页，去重后追加 |
| 5 | 简洁直观、响应式、兼容桌面浏览器 | 暗色主题、卡片自适应栅格、桌面优先并兼顾窄屏 |
| 6 | 复制磁力 / 调用本地工具下载 | 一键"复制磁力""打开磁力"（`magnet:` 协议交给系统默认客户端如 qBittorrent / 迅雷，**零配置**）；另支持 qBittorrent WebUI 一键推送，首次加载自动探测本机 `localhost:8080/8081/9090` 默认账号，命中即免配置启用 |

## 架构

```
browser (public/)  ──HTTP──▶  Express server (server.js)
                                  │
                                  ├─ /api/providers    列出可用引擎
                                  ├─ /api/search       聚合搜索（并行调用各 provider）
                                  ├─ /api/magnet       惰性解析详情页磁力（如 1337x）
                                  └─ /api/download/qbittorrent  代理推送至 qBittorrent WebUI
                                  │
                        src/providers/{tpb,1337x,nyaa,demo}.js
                                  │
                        src/lib/{http,normalize}.js  请求封装 + 结果规范化
```

各引擎模块导出 `search(query, {page})`，返回 `{ results, error }`。公共层负责：
- 解析大小字符串（`1.2 GB` → 字节）、相对时间（`2 hours ago` → 时间戳）；
- 构造磁力链接（`magnet:?xt=urn:btih:<hash>`）；
- 统一字段到 `TorrentResult` 结构。

## 已集成的引擎

- **The Pirate Bay** — 走公开 JSON API `apibay.org`（本环境可直接访问，已实测）。
- **1337x** — HTML 抓取搜索结果表；磁力链接在其详情页，点击"获取磁力"时惰性解析。内置多个镜像域名并行尝试。
- **NYAA** — HTML 抓取，结果行内直接含磁力链接。
- **Demo（离线）** — 生成确定性的拟真数据，保证在网络受限 / 引擎被墙时界面、排序、筛选、分页依然可演示。

## 说明与边界

- 第三方站点可能随区域 / 网络被屏蔽；本环境中 1337x、NYAA 偶尔不可达，属正常网络现象，状态栏会逐引擎显示 ✓/✕。
- 抓取依赖公开页面结构，若目标站点改版，对应 provider 的解析选择器需同步更新。
- 生产部署建议补充：请求速率限制、结果缓存、以及遵守目标站点 `robots.txt` 与当地法律法规。
- 本工具仅做"搜索聚合"，不托管、不分发任何版权内容。

## 路线图 / 计划中的功能

以下是从用户视角整理的候选功能，按优先级大致排序。欢迎按需取用或调整顺序。

- [x] **画质快捷筛选 + 关键词高亮** — 4K / 1080p / 720p / HDR·Dolby 快捷标签，比"名称含"手输更顺手；标题里命中搜索词做高亮。（纯前端，改动小，影视场景高频）
- [x] **搜索历史 / 收藏** — 最近搜索词下拉（可单条删除 / 清空）；顶栏「收藏」视图切换，收藏的种子（含 magnet / 来源快照）存 localStorage，跨会话可直接打开磁力 / 推送。（纯前端，天天用）
- [x] **引擎分组 / 预设** — 引擎按"动漫 / 影视 / 综合 / 成人 / 其他"分组展示，每组一键全选 / 全不选；选中的引擎存 localStorage，下次打开还在。Torznab 自建索引器归入「自定义」组。（纯前端）
- [x] **批量操作** — 勾选多条卡片浮出工具条，一键批量推送 qBittorrent、批量复制磁力、导出 CSV（Excel 友好，带 BOM）。（找整季剧集很实用）
- [x] **详情预览** — 点开聚合信息弹窗（做种 / 大小 / 分类 / 文件数 / infoHash / 磁力），并列出各来源详情页外链，跳站点自行核对文件列表。文件列表因站点各异未做统一抓取。
- [ ] **更多下载客户端** — 现仅支持 qBittorrent WebUI，可加 Transmission RPC、Aria2 RPC、Deluge。

> 已在 `v0.1.0-beta` 完成的相关项：**结果 SSE 实时流入 + 每引擎耗时/状态显示**、**跨站按 infoHash 去重合并单卡**。

## 许可证

本项目以 **GNU GPL-3.0** 发布。

> 选用 GPL-3.0 而非 AGPL-3.0 的原因：本项目是**本地运行的桌面应用**（用户自行运行 `node server.js` 访问 `localhost`），并非托管给公众的网络服务，因此 AGPL 针对"网络交互即须开源"的额外条款在此用不上。若你打算把它部署为公开的在线搜索服务，建议改用 **AGPL-3.0**。

完整许可证文本见 [LICENSE](./LICENSE)。
