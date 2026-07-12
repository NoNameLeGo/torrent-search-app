# 安装包说明

> 本说明由 `release.yml` 自动追加到每次 Release 末尾，产物文件名中的 `<版本>` 即本 Release 的 tag（如 `0.0.1-beta`），实际文件名以本页 **Assets** 列表为准。

本版本同时提供 **三种 Windows 安装包**，搜索功能与结果完全一致，区别仅在**体积、运行方式与成熟度**。请按需选择。

## 产物一览

| 产物 | 文件名（示例） | 体积（约） | 一句话定位 |
| --- | --- | --- | --- |
| Electron 安装包 (NSIS) | `BT-Search-Electron-Setup-<版本>.exe` | 150 MB+ | 标准安装，最成熟稳定 |
| Electron 便携版 (zip) | `BT-Search-Electron-Portable.zip` | 150 MB+ | 解压即用，绿色免安装 |
| Tauri 安装包 (NSIS) | `BT-Search-Tauri-Setup-<版本>.exe` | 40 MB | 小巧轻量，实验性尝鲜 |

## 三者有什么区别？

**1. Electron 安装包 (NSIS)**
标准 Windows 安装程序：安装到 `Program Files`、创建开始菜单与桌面快捷方式、写入卸载信息。基于 Electron（内置 Chromium 内核），**兼容性最好、最成熟**，适合绝大多数用户。

**2. Electron 便携版 (zip)**
把上面的安装版原样打包成 zip。**解压到任意目录、双击 `BT聚合搜索.exe` 即用**，不写入系统目录、不留注册表，可放 U 盘或随项目带走。功能与安装版完全相同，只是少了安装/卸载步骤。

**3. Tauri 安装包 (NSIS)**
实验性分支（`feat/tauri`）产物。复用同一套后端（`server.js` + 40 个搜索引擎 provider）与前端，**功能与 Electron 版一致**，但用系统自带的 **WebView2** 取代 Chromium：
- 安装包体积约 **3–4 倍更小**（~40 MB vs ~150 MB+）；
- 启动更快、内存占用更低；
- 需系统已安装 **WebView2 Runtime**（Win10 / Win11 一般已预装；若缺失，安装时会提示下载）。

## 该选哪个？

- 求稳、求兼容 → **Electron 安装包**
- 想要绿色 / 便携、不想安装 → **Electron 便携版 (zip)**
- 想要小巧、启动快、愿意尝鲜 → **Tauri 安装包**

三种安装包的搜索结果、下载逻辑完全一致，任选其一即可。
