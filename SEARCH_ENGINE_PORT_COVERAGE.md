# 搜索引擎移植情况对比报告

> 对比对象：
> - 原仓库：`https://github.com/prajwalch/TorrentSearch`（Android / Kotlin 实现）
> - 当前项目：`D:\Vibe-Coding\torrent-search-app`（Node.js / Electron Web 实现）
> - 生成时间：2026-07-11

## 结论

**已全部移植完成。** 当前项目已覆盖原仓库 40 个内置站点引擎中的 **全部 40 个**（覆盖率 100%），并保留原仓库没有的本地 `Demo (offline)` 引擎（离线演示/测试用）。**唯一未实现的是** 原仓库的 Torznab 自定义索引器框架（Jackett / Prowlarr / *arr 接入），可后续单独做。详见下方“移植状态更新”。

---

## 原仓库引擎清单来源

原仓库的逐站搜索引擎定义于：
`app/src/main/kotlin/com/prajwalch/torrentsearch/providers/*.kt`

共 **40 个** 站点引擎文件 + **1 个** `TorznabSearchProvider.kt`（通过 Jackett / Prowlarr / *arr 接入自定义索引器）。

---

## 已移植（6 / 40）

| 原仓库引擎 (Kotlin) | 当前项目实现 | 文件 | 状态 |
|---|---|---|---|
| `ThirteenThirtySevenX` | `1337x` | `src/providers/1337x.js` | ✅ 已移植 |
| `Knaben` | `knaben` | `src/providers/knaben.js` | ✅ 已移植 |
| `Nyaa` | `nyaa` | `src/providers/nyaa.js` | ✅ 已移植 |
| `ThePirateBay` | `tpb` | `src/providers/tpb.js` | ✅ 已移植 |
| `TorrentsCSV` | `torrentscsv` | `src/providers/torrentscsv.js` | ✅ 已移植 |
| `Yts` | `yts` | `src/providers/yts.js` | ✅ 已移植 |

> 备注：`1337x` 引擎额外实现了 `resolveMagnet`（从详情页惰性解析磁力链接），`tpb` / `knaben` / `torrentscsv` / `yts` 标记了 `testable`，能力与原仓库基本对应。

---

## 移植状态更新（已全部完成 ✅）

> 更新于 2026-07-11：原报告撰写时的“未移植 34 个”现已全部移植完毕。
> 当前项目 `src/providers/` 已覆盖原仓库全部 **40 个内置站点引擎** + 1 个本地 `Demo` 引擎（共 41 个，见 `index.js` 的 REGISTRY）。

### 本次新增移植的 34 个引擎（对应原报告中的“未移植”清单）

| 原仓库引擎 | 当前项目 id | 文件 |
|---|---|---|
| AniLibria | anilibria | src/providers/anilibria.js |
| AniRena | anirena | src/providers/anirena.js |
| AnimeTosho | animetosho | src/providers/animetosho.js |
| AudioBookBay | audiobookbay | src/providers/audiobookbay.js |
| BTDigg | btdigg | src/providers/btdigg.js |
| BangumiMoe | bangumimoe | src/providers/bangumimoe.js |
| BitSearch | bitsearch | src/providers/bitsearch.js |
| BlueRoms | blueroms | src/providers/blueroms.js |
| Bt4g | bt4g | src/providers/bt4g.js |
| Dmhy | dmhy | src/providers/dmhy.js |
| Eztv | eztv | src/providers/eztv.js |
| FileMood | filemood | src/providers/filemood.js |
| InternetArchive | internetarchive | src/providers/internetarchive.js |
| LimeTorrents | limetorrents | src/providers/limetorrents.js |
| LinuxTracker | linuxtracker | src/providers/linuxtracker.js |
| MegaPeer | megapeer | src/providers/megapeer.js |
| Mikan | mikan | src/providers/mikan.js |
| MyPornClub | mypornclub | src/providers/mypornclub.js |
| NekoBT | nekobt | src/providers/nekobt.js |
| OxTorrent | oxtorrent | src/providers/oxtorrent.js |
| Rutor | rutor | src/providers/rutor.js |
| SubsPlease | subsplease | src/providers/subsplease.js |
| Sukebei | sukebei | src/providers/sukebei.js |
| TheRarBg | therarbg | src/providers/therarbg.js |
| TokyoToshokan | tokyotoshokan | src/providers/tokyotoshokan.js |
| Torrent9 | torrent9 | src/providers/torrent9.js |
| TorrentDatabase | torrentdatabase | src/providers/torrentdatabase.js |
| TorrentDownload | torrentdownload | src/providers/torrentdownload.js |
| TorrentDownloads | torrentdownloads | src/providers/torrentdownloads.js |
| TorrentKitty | torrentkitty | src/providers/torrentkitty.js |
| UIndex | uindex | src/providers/uindex.js |
| XXXClub | xxxclub | src/providers/xxxclub.js |
| XXXTracker | xxxtracker | src/providers/xxxtracker.js |
| ZeroMagnet | zeromagnet | src/providers/zeromagnet.js |

### 仍待实现（1 个框架引擎）

| 原仓库引擎 | 当前项目状态 |
|---|---|
| `TorznabSearchProvider`（Jackett / Prowlarr / *arr 自定义索引器接入） | ❌ 未移植 |

> 当前项目暂无 `torznab` 实现，无法通过 Torznab API 接入自托管索引器；可后续单独实现。

---

## 当前项目特有（原仓库无对应项）

| 当前项目引擎 | 文件 | 说明 |
|---|---|---|
| `demo` (Demo offline) | `src/providers/demo.js` | 原仓库不存在；本地离线演示/测试引擎，不参与真实搜索 |

---

## 统计汇总

| 维度 | 数量 |
|---|---|
| 原仓库内置站点引擎总数 | 40 |
| 原仓库 Torznab 框架引擎 | 1 |
| 当前项目已移植的站点引擎 | 6 |
| 当前项目未移植的站点引擎 | 34 |
| 当前项目特有引擎（Demo） | 1 |
| 站点引擎移植覆盖率 | 6 / 40 ≈ 15% |

---

## 建议

若需提升覆盖率，优先级建议（按站点通用性与中文用户友好度粗排）：
1. `Eztv`、`LimeTorrents`、`Rutor`、`Torrent9`、`TorrentKitty`、`TorrentDownloads` —— 通用影视/综合类，移植价值高。
2. `AnimeTosho`、`SubsPlease`、`Dmhy`、`Mikan` —— 动漫类补充（当前仅有 NYAA / Sukebei 缺位）。
3. 实现 `TorznabSearchProvider` 框架 —— 一次性接入 Jackett / Prowlarr，可间接覆盖海量索引器，性价比最高。

每个引擎的移植方式：在 `src/providers/` 下新增同名 `.js`，导出 `{ id, name, search, resolveMagnet? }`，并在 `src/providers/index.js` 的 `REGISTRY` 中注册即可。
