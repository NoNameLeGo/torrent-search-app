# 安装包说明

> 本说明由 `release.yml` 自动追加到每次 Release 末尾。产物文件名里的 `<版本>` 即本 Release 的 tag 去掉前缀 `v`
> （如 tag `v0.4.5-beta` → 文件名里的 `0.4.5-beta`），实际文件名以本页 **Assets** 列表为准。

本版本提供 **3 个 Windows 产物**：Rust 主线的 exe 与 zip，加上一个 Tauri 安装包。
⚠️ 两者的**搜索源数量不一样**，功能与用法基本一致 —— 下面把差别说清楚。

## 产物一览

| 产物 | 文件名（示例） | 体积（约） | 一句话定位 |
| --- | --- | --- | --- |
| **Rust 单文件 exe** | `BT-Search-Rust-<版本>.exe` | **约 20 MB** | ⭐ **主线**：原生 UI、零运行时依赖、双击即用 |
| Rust 版 (zip) | `BT-Search-Rust-<版本>.zip` | **约 9 MB** | 同上，打包成 zip（有些环境对裸 exe 更敏感） |
| Tauri 安装包 (NSIS) | `BT-Search-Tauri-Setup-<版本>.exe` | 约 30 MB | 源更多（40 家）、需要 WebView2 的安装版 |

## Rust 版（主线）—— 先读这段再选

**实打实的好处**：

- **单文件、约 20 MB**，拷到任意 Windows 上**双击就跑**，不用安装；
- **零运行时依赖** —— 不装 Node、不装 WebView2、不装 VC++ 运行库
  （MSVC 运行时已静态链进 exe，CI 里有机器断言守着，见 `.github/workflows/rust-size.yml`）；
- **双击不弹命令行窗口** —— release 走 Windows GUI 子系统，CI 里同样有断言；
- **原生 UI**（Slint，非 WebView），启动快、内存低。

**能用的功能**：

- **三种模式**：磁力 / 视频 / 网盘（顶栏切换，结果不混流）
- **磁力：28 家 BT 站 + 离线 demo**；**在线影视：5 家**（打开原站，不内嵌播放器）；**字幕：3 站**；**网盘：PanSou 客户端**
- 排序（做种 / 相关度 / 体积 / 日期，升降序）、分类筛选、关键词高亮、翻页、站点开关
- **Torznab 索引器**（Jackett / Prowlarr / \*arr）：界面里直接填地址与 API key，配置持久化
- **四种下载器推送**（qBittorrent / Transmission / aria2·Motrix / Gopeed），配置持久化
- **数据源可扩展**：设置里能导入 TVBox 单仓/多仓、LibreTV、Prowlarr 目录，能手写规则、加订阅并刷新（统一存进 `sources.json`）
- 失败提示区分「网络层不通」与「站点层拒绝」；连续多轮空手的引擎在状态条上打 ⚠

**⚠️ 但它不能完全替代 Tauri 版，差距是明确的**：

| | Rust 版（本 exe） | Tauri 安装包 |
| --- | --- | --- |
| 搜索源 | **28 家 BT** + 离线 demo + 5 家影视 + PanSou 网盘 | **40 家** + 离线 demo（跑的是 Node 后端） |
| 字幕 / Torznab / 下载器推送 | ✅ 都有 | ✅ 都有 |
| 收藏 / 搜索历史 / 结果缓存 | ❌ 没有 | ✅ 有 |
| 运行时要求 | 无 | 需 WebView2 Runtime |
| 「零运行时依赖」的正式验证 | ⚠️ 仍是旁证（CI 断言了不依赖 MSVC 运行时 DLL；「拷到干净 Windows 双击能跑」只能人工验，尚未做） | 不适用 |
| 成熟度 | 新，功能在补齐中 | 多年在用 |

## Tauri 安装包是什么

标准 Windows 安装程序（安装到 `Program Files`、建开始菜单与桌面快捷方式、可卸载），构建于 **`feat/tauri` 分支**：

- 那个分支**自带一份 Node 后端（40 家 provider）** —— 所以它的源比 Rust 版多；
  这份后端**已不在 `main` 上**（2026-11-27 退役），也就无法从 `main` 构建它；
- 用系统自带的 **WebView2** 取代 Chromium，所以安装包只有约 30 MB（旧 Electron 版是 150 MB+）；
- 需系统已装 **WebView2 Runtime**（Win10 / Win11 一般已预装；缺失时安装程序会提示下载）。

## 该选哪个？

- **想绿色免安装、要最小体积、日常就用 28 家 BT + 5 家影视 + 网盘** → **Rust 单文件 exe**（Torznab 索引器也能用，界面里填地址与 key 即可）
- **要源最多（40 家）、需要收藏 / 历史、习惯用安装版** → **Tauri 安装包**

拿不准就先下 Rust 的 exe：它解压即用、不写注册表，不合适再换另一个。

## 旧版本

`v0.4.5-beta` 及更早的 Release 里还有 **Electron 安装包与便携版** —— 那是已退役的 Node/Electron 实现，
**新版本不再产出**，也不再维护。需要那两份产物请回旧 Release 取。