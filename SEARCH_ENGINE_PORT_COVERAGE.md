# 搜索引擎移植情况对比报告

> 对比对象：
> - 原仓库：`https://github.com/prajwalch/TorrentSearch`（Android / Kotlin 实现）
> - 当前项目：`D:\Vibe-Coding\torrent-search-app`（Node.js / Electron Web 实现）
> - 生成时间：2026-07-11

## 结论

**未完整移植。** 当前项目仅移植了原仓库 40 个内置站点引擎中的 **6 个**（覆盖率约 15%），并且 **未实现** 原仓库的 Torznab 自定义引擎框架。此外，当前项目多出一个原仓库不存在的本地 `Demo (offline)` 引擎（仅用于离线演示/测试）。

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

## 未移植（34 / 40 站点 + 1 Torznab）

### 1. 未移植的内置站点引擎（34 个）

| # | 原仓库引擎 | 当前项目状态 |
|---|---|---|
| 1 | `AniLibria` | ❌ 未移植 |
| 2 | `AniRena` | ❌ 未移植 |
| 3 | `AnimeTosho` | ❌ 未移植 |
| 4 | `AudioBookBay` | ❌ 未移植 |
| 5 | `BTDigg` | ❌ 未移植 |
| 6 | `BangumiMoe` | ❌ 未移植 |
| 7 | `BitSearch` | ❌ 未移植 |
| 8 | `BlueRoms` | ❌ 未移植 |
| 9 | `Bt4g` | ❌ 未移植 |
| 10 | `Dmhy` | ❌ 未移植 |
| 11 | `Eztv` | ❌ 未移植 |
| 12 | `FileMood` | ❌ 未移植 |
| 13 | `InternetArchive` | ❌ 未移植 |
| 14 | `LimeTorrents` | ❌ 未移植 |
| 15 | `LinuxTracker` | ❌ 未移植 |
| 16 | `MegaPeer` | ❌ 未移植 |
| 17 | `Mikan` | ❌ 未移植 |
| 18 | `MyPornClub` | ❌ 未移植 |
| 19 | `NekoBT` | ❌ 未移植 |
| 20 | `OxTorrent` | ❌ 未移植 |
| 21 | `Rutor` | ❌ 未移植 |
| 22 | `SubsPlease` | ❌ 未移植 |
| 23 | `Sukebei` | ❌ 未移植 |
| 24 | `TheRarBg` | ❌ 未移植 |
| 25 | `TokyoToshokan` | ❌ 未移植 |
| 26 | `Torrent9` | ❌ 未移植 |
| 27 | `TorrentDatabase` | ❌ 未移植 |
| 28 | `TorrentDownload` | ❌ 未移植 |
| 29 | `TorrentDownloads` | ❌ 未移植 |
| 30 | `TorrentKitty` | ❌ 未移植 |
| 31 | `UIndex` | ❌ 未移植 |
| 32 | `XXXClub` | ❌ 未移植 |
| 33 | `XXXTracker` | ❌ 未移植 |
| 34 | `ZeroMagnet` | ❌ 未移植 |

### 2. 未移植的通用框架引擎（1 个）

| 原仓库引擎 | 当前项目状态 |
|---|---|
| `TorznabSearchProvider`（Jackett / Prowlarr / *arr 自定义索引器接入） | ❌ 未移植 |

> 当前项目无任何 `torznab` 相关实现，无法通过 Torznab API 接入自托管索引器。

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
