# AGENTS.md — Torrent Search App

## Quick start

```bash
npm install   # express, axios, cheerio (runtime) + electron, electron-builder (dev)
npm start     # node server.js → http://localhost:3000
```

Electron mode: `npm run electron` (picks a free port automatically, no collision with a running `npm start`).

## No test / lint / typecheck

This repo has **no tests, linter, or formatter** configured. There is nothing to run before shipping. If you add tooling, document it here.

## Architecture (one screen)

```
public/               ← static frontend (index.html, app.js, styles.css)
server.js             ← Express entry point; exports { app, start }
electron/main.js      ← Electron wrapper; requires server.js and calls start(port)
src/providers/        ← one file per search engine
src/lib/http.js       ← shared axios instance (UA rotation, 10 s timeout, never throws)
src/lib/normalize.js  ← size/date/magnet parsing → canonical TorrentResult shape
```

- `server.js` only auto-listens when run directly (`require.main === module`). When required by Electron it returns the `app` without binding.
- All providers export `search(query, { page }) → { results, error, hasMore }`. Add new engines here.
- `src/lib/http.js` wrappers (`getText`, `getJSON`, `postJSON`) never throw; they return `{ data|html, error }`. Match this pattern in new providers.
- The `demo` provider is offline-only and always enabled — useful for testing the UI without network.

## Adding a provider

1. Create `src/providers/<name>.js` exporting `{ id, name, search }`.
2. Add `resolveMagnet(url)` if magnets require a detail-page fetch (see `1337x.js` for the pattern).
3. Register in `src/providers/index.js` — the array order is the UI display order.
4. Results should pass through `normalize()` from `src/lib/normalize.js`.

## Electron packaging

```bash
npm run dist          # NSIS installer → dist/BT-Search-Electron-Setup-<版本>.exe
npm run build:portable  # manual portable build → dist/portable/
```

Build caches are redirected to `.cache/` (project-local, gitignored) to avoid polluting `%LOCALAPPDATA%`.

## Platform note

This is a **Windows-first** project. `start.bat`/`stop.bat` are the primary dev launchers. `npm run electron` and `npm run dist` use `set` (not `export`) for env vars — they are Windows-only scripts.

## Conventions & gotchas

- **Test scrapers from the agent sandbox, not your machine.** The user's local Windows has DNS pollution for many BT sites (Facebook blackhole IPv6 `2a03:2880:face:b00c`), so a provider that fails locally may work from the agent and vice-versa. Verify reachability from the agent runtime; never ask the user to share their network. Status bar ✓/✕ reflects real reachability.
- **Russian sites (rutor, …):** UTF-8 not windows-1251; match table cells by *content* not column index (an extra `comments` column shifts indices); JS `\b` misses Cyrillic; require a full unit (`GB|MB|TB|ГБ|МБ|ТБ`) so a bare `B` doesn't match "Black Box".
- **Torznab dynamic indexers** (Jackett/Prowlarr): user-added indexers become runtime providers (`torznab:` id prefix) via `src/lib/torznabStore.js` → `data/torznab.json` (git-ignored, holds API keys). Full contract in `CLAUDE.md`.

## Tauri experiment branch (`feat/tauri`)

> **分支用途**：`feat/tauri` 是一条**独立的实验分支**，专门用来用 GitHub Actions 构建 Tauri 打包版本，与 `main` 上的 Electron 打包相互隔离。Tauri 相关代码（Rust shell、配置、CI）只在这一分支演进；要发布 Tauri 安装包，从这条分支打 `v*` tag 并推到 GitHub 即可触发统一发布工作流。

Route A proof-of-concept: keep the existing Node/Express backend as a **sidecar** bundled inside a Tauri app, so the 40 providers and `public/` frontend are reused unchanged.

```
src-tauri/                 ← Tauri v2 Rust shell + config (Cargo.toml, tauri.conf.json, src/main.rs, capabilities)
scripts/prepare-sidecar.mjs ← copies the real node.exe to src-tauri/binaries/server-<triple>.exe
scripts/gen-icon.mjs       ← regenerates src-tauri/icons/* (run if icons change)
```

- The sidecar binary (`src-tauri/binaries/server-<triple>.exe`) is a **copy of node.exe**, prepared by `npm run build:sidecar` and git-ignored. The backend code (`server.js`, `src/`) and `node_modules` are shipped as Tauri `bundle.resources`, so no `pkg`/compilation step is needed — robust against ESM deps (cheerio) and network restrictions. **Why not `pkg`**: we evaluated compiling `server.js` into a single exe with `pkg` first; when its prebuilt-Node download misses (common behind rate limits / in CI), `pkg` falls back to **compiling Node from source**, which is slow and fragile. The node.exe-copy sidecar avoids that entirely.
- `server.js` gains two optional flags for Tauri: `--port <n>` and `--public-dir <path>`. The frontend is shipped as a Tauri `bundle.resources` entry (`../public` → `public`), so express serves it from the real OS path at runtime.
- Dev: `npm run dev:tauri` — Tauri serves `http://localhost:3000` from a plain `node server.js` (no sidecar used in dev).
- Release: `npm run build:tauri` — Rust picks a free port, spawns the bundled sidecar with `--port`/`--public-dir`, waits for `/api/health`, then loads it.
- Route B (later): port the 40 providers to Rust/Tauri commands to drop the Node sidecar and shrink the bundle.

### Release 产物对照（哪个是 Tauri、哪个是 Electron）

统一发布工作流 `.github/workflows/release.yml` 在打 `v*` tag 时，会**并行构建 Electron 与 Tauri 两个安装包，并上传到【同一个】GitHub Release**。Asset 文件名区分如下：

| 产物 | 文件名（示例） | 来自 |
| --- | --- | --- |
| Electron 安装包 (NSIS) | `dist/BT-Search-Electron-Setup-<版本>.exe` | Electron job（基于 `main` 分支代码） |
| Electron 便携版 (zip) | `dist/portable/BT-Search-Electron-Portable.zip` | Electron job |
| Tauri 安装包 (NSIS) | `src-tauri/target/release/bundle/nsis/BT-Search-Tauri-Setup-<版本>.exe` | Tauri job（**显式 `checkout ref: feat/tauri`** 取 Tauri 代码） |

- **识别要点**：文件名带 `-Tauri-` 的是 **Tauri**（`release.yml` 的 tauri job 用 `mv` 重命名，版本号取自 `tauri.conf.json`）；带 `-Electron-` 的是 **Electron**。所有产物名都显式含 shell 标识，用户不会混淆。
- Electron 安装包名由 `package.json` 的 `build.win.artifactName` 决定；Electron 便携版 zip 名与 Tauri 安装包名都在 `release.yml` 里定（便携版是 `Compress-Archive` 的 `-DestinationPath`，Tauri 是 upload 前的 `Rename Tauri installer` 步）。**改命名要同时改这三处 + `docs/RELEASE_ARTIFACTS.md` 的示例名。**
- Tauri job 会单独 `checkout feat/tauri`，所以即使从其他分支打 `v*` tag，Tauri 包也始终基于 `feat/tauri` 分支。

### 直接正式发布，不走草稿

`release.yml` 的 publish job 用 `softprops/action-gh-release@v2` 且 **`draft: false`** —— 打 `v*` tag 会**直接发布正式 Release**，无需手动去 GitHub 页面点 "Publish"。（历史上曾是 `draft: true` 需手动转正，已改。）

### Release 末尾自动附「Tauri vs Electron 区别」

每次发布，`release.yml` 的 publish job 会把 `docs/RELEASE_ARTIFACTS.md` 的内容**自动追加到 Release 正文末尾**（GitHub 自动更新日志 + 分隔线 + 该模板），说明三种安装包的体积、运行方式、成熟度与选型建议。**要修改文案只改 `docs/RELEASE_ARTIFACTS.md` 一个文件即可，无需改动 workflow。**

### Tauri 构建验证（CI）

- `.github/workflows/tauri-build.yml`：push / PR 到 `feat/tauri` 时**仅构建验证，不发布**（省略 tagName/releaseName，tauri-action 不会创建 release）。

Note: `Cargo.lock` is generated on first `tauri build` (local Rust toolchain or CI) and should be committed for reproducible Windows builds.

### `feat/tauri` is a permanent CI branch, not throwaway

`feat/tauri` exists specifically so GitHub Actions can build the Tauri package: `release.yml`'s `tauri` job does an explicit `checkout` with `ref: feat/tauri`, and `tauri-build.yml` triggers on pushes to it. It is not meant to be squash-merged and deleted — keep the Tauri shell (`src-tauri/`, sidecar scripts) living here. The `main` branch carries the Electron shell.

### Electron vs Tauri — what actually differs

Both ship the **identical** backend (`server.js` + `src/`) and frontend (`public/`); only the desktop wrapper changes. Same search results, same download logic.

| | Electron | Tauri |
| --- | --- | --- |
| Renderer | bundled Chromium | system **WebView2** (Win10/11 usually preinstalled) |
| Installer size | ~150 MB+ | ~40 MB |
| Backend launch | in-process `require('./server').start(port)` | node.exe **sidecar** spawned with `--port`/`--public-dir` |
| Maturity | stable, default | experimental |
| Shell code | `electron/main.js` | `src-tauri/` (Rust) |
| Lives on branch | `main` | `feat/tauri` |

### Which release artifact is which

`release.yml` (on a `v*` tag) publishes **three** Windows installers to one GitHub Release. Tell them apart by filename:

| Filename pattern | Shell | Build step |
| --- | --- | --- |
| `BT-Search-Electron-Setup-<ver>.exe` | Electron | `npm run dist` (NSIS) |
| `BT-Search-Electron-Portable.zip` | Electron | `npm run build:portable` (green/portable) |
| `BT-Search-Tauri-Setup-<ver>.exe` | Tauri | `tauri-action` (NSIS), renamed in the `Rename Tauri installer` step |

Every artifact name carries an explicit `-Electron-`/`-Tauri-` tag so users can't confuse them. User-facing details of the three artifacts live in `docs/RELEASE_ARTIFACTS.md`, which `release.yml` appends to every Release body. The Release is published **directly (not as a draft)** — publish job uses `draft: false`, so a `v*` tag ships a live Release with no manual "Publish" click.

### Tauri v2 migration gotchas (learned the hard way)

These are compile-time / config-parse errors, not discoverable by reading code — verify against them whenever you touch `src-tauri/`.

- **`tauri.conf.json` `bundle.windows.nsis`** uses `installMode` (`currentUser` / `perMachine` / `both`). The v1 fields `oneClick` / `allowToChangeInstallationDirectory` are **rejected by the v2 schema** → build fails at config parse.
- **Don't declare `app.windows` in `tauri.conf.json` AND create the window in `setup`** → duplicate label `main` panics at runtime in release. Create the window in `setup` only (so the dynamic-port URL works), leave `app.windows` absent.
- **`WebviewWindow::load(url)` does not exist in v2.** Create the window with `WebviewWindowBuilder::new(app, "main", url).build()` inside `setup` instead.
- **`on_navigation(|url| …)` receives `&tauri::Url`, not `&str`** → match on `url.scheme() == "magnet"`, not `url.starts_with(...)`.
- **`tauri_plugin_shell::Shell::open` is deprecated** → use `tauri_plugin_opener::open_url(url, None::<&str>)` (the `None` must be typed `None::<&str>` or inference fails).
- **`Color` is `tauri::webview::Color`** in v2 (not `tauri::Color`).
- **`cargo check` (dev) does NOT compile code behind `#[cfg(not(debug_assertions))]`** — i.e. Tauri's release-only `setup` block (sidecar spawn, dynamic-port window). CI builds release, so run **`cargo check --release`** locally to actually validate it.
- **Validate `tauri.conf.json` before pushing**: run `ajv` against the bundled `node_modules/@tauri-apps/cli/config.schema.json` (skip `pattern` keywords — a Windows-path regex breaks `ajv` on Windows). This catches schema errors locally instead of burning a CI run.
- **`tauri-apps/tauri-action` is `@v1`** (not v0). Build-only (no release) is achieved by **omitting** `tagName`/`releaseName`, not by an `includeRelease: false` flag.
