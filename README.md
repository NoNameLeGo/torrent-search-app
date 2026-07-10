# BT 聚合搜索 · Torrent Meta-Search

一个面向桌面浏览器的 **BT 种子聚合搜索 Web 应用**，参考 [prajwalch/TorrentSearch](https://github.com/prajwalch/TorrentSearch) 的"多引擎并行聚合"思路，用户输入一个关键词即可同时从多个种子站点检索结果。

> 采用 **服务端代理** 架构：所有对第三方站点的抓取都在 Node 后端完成，前端只与本地 API 通信，天然规避浏览器的跨域（CORS）限制。

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
