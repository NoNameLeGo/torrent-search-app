# 安装包说明

> 本说明由 `release.yml` 自动追加到每次 Release 末尾，产物文件名中的 `<版本>` 即本 Release 的 tag 去掉前缀 `v`（如 tag `v0.3.5-beta` → 文件名里的 `0.3.5-beta`），实际文件名以本页 **Assets** 列表为准。

本版本提供 **四种 Windows 构建**。⚠️ **它们的能力并不相同** —— 三个旧壳（Electron / Tauri）跑的是 Node 版后端（40 家引擎 + Torznab），而 **Rust 版是新主线**，目前引擎更少（16 家）。请照下面的表选。

## 产物一览

| 产物 | 文件名（示例） | 体积（约） | 一句话定位 |
| --- | --- | --- | --- |
| **Rust 单文件 exe** | `BT-Search-Rust-<版本>.exe` | **18 MB** | 🆕 **新主线**：原生 UI、零运行时依赖、单文件 |
| Rust 版 (zip) | `BT-Search-Rust-<版本>.zip` | 18 MB | 同上，打包成 zip（有些环境对裸 exe 更敏感） |
| Electron 安装包 (NSIS) | `BT-Search-Electron-Setup-<版本>.exe` | 150 MB+ | 引擎最全、最成熟稳定 |
| Electron 便携版 (zip) | `BT-Search-Electron-Portable.zip` | 150 MB+ | 解压即用，绿色免安装 |
| Tauri 安装包 (NSIS) | `BT-Search-Tauri-Setup-<版本>.exe` | 40 MB | 旧壳里体积最小的 |

## Rust 版（新主线）——**请先读完这段再选**

**它的优势是实打实的**：

- **单文件、18 MB**（Electron 版的约 **1/8**），拷到任意 Windows 上**双击就跑**；
- **零运行时依赖** —— 不装 Node、不装 WebView2、不装 VC++ 运行库
  （MSVC 运行时已静态链进 exe；CI 里有机器断言在守这条，见 `.github/workflows/rust-size.yml`）；
- **原生 UI**（Slint，非 WebView），启动快、内存低；
- 引擎层与 Node 版**逐字段对齐** —— 16 家 provider 的每一项结果与 Node 版完全一致
  （`.github/workflows/rust.yml` 里有 16 家 × 15 字段的全量差分门，
  只有若干**已登记的有意分歧**例外）。

**⚠️ 但它现在**还不能替代 Node 版**，差距是明确的**：

| | Rust 版 | Node 版（Electron / Tauri） |
| --- | --- | --- |
| 搜索源 | **16 家** | **40 家** + 离线 demo |
| Torznab（Jackett / Prowlarr / \*arr） | ❌ 未做（D17 有意推迟） | ✅ |
| 下载器配置 | ✅ **已持久化**（`%APPDATA%\bt-search\config.json`） | ✅ 有配置文件 |
| 成熟度 | 新，功能在补齐中 | 多年在用 |

**所以：Rust 版目前适合"尝鲜 / 看体积和速度"，不适合当日常主力。** 日常用请选 Electron 版。

## 其余三种有什么区别？

**1. Electron 安装包 (NSIS)**
标准 Windows 安装程序：安装到 `Program Files`、创建开始菜单与桌面快捷方式、写入卸载信息。基于 Electron（内置 Chromium 内核），**兼容性最好、最成熟**，适合绝大多数用户。

**2. Electron 便携版 (zip)**
把上面的安装版原样打包成 zip。**解压到任意目录、双击 `torrent-search-app.exe` 即用**，不写入系统目录、不留注册表，可放 U 盘或随项目带走。功能与安装版完全相同，只是少了安装/卸载步骤。

**3. Tauri 安装包 (NSIS)**
轻量之选（构建于 `feat/tauri` 分支）。复用同一套后端（`server.js` + 40 个搜索引擎 provider）与前端，**功能与 Electron 版一致**，但用系统自带的 **WebView2** 取代 Chromium：
- 安装包体积约 **3–4 倍更小**（~40 MB vs ~150 MB+）；
- 启动更快、内存占用更低；
- 需系统已安装 **WebView2 Runtime**（Win10 / Win11 一般已预装；若缺失，安装时会提示下载）。

## 该选哪个？

- **日常主力、要引擎全 / 用 Torznab** → **Electron 安装包**（或便携版）
- 想绿色免安装 → **Electron 便携版 (zip)**
- 想在旧壳里挑最小的 → **Tauri 安装包**
- **想试新主线、看体积与速度** → **Rust 单文件 exe**（但请接受它现在只有 16 家引擎）

除 Rust 版外，其余三种的搜索源与下载逻辑完全一致，任选其一即可。
