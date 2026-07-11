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
npm run dist          # NSIS installer → dist/BT聚合搜索-Setup-1.0.0.exe
npm run build:portable  # manual portable build → dist/portable/
```

Build caches are redirected to `.cache/` (project-local, gitignored) to avoid polluting `%LOCALAPPDATA%`.

## Platform note

This is a **Windows-first** project. `start.bat`/`stop.bat` are the primary dev launchers. `npm run electron` and `npm run dist` use `set` (not `export`) for env vars — they are Windows-only scripts.

## Tauri experiment branch (`feat/tauri`)

Route A proof-of-concept: keep the existing Node/Express backend as a **sidecar** bundled inside a Tauri app, so the 44 providers and `public/` frontend are reused unchanged.

```
src-tauri/                 ← Tauri v2 Rust shell + config (Cargo.toml, tauri.conf.json, src/main.rs, capabilities)
scripts/prepare-sidecar.mjs ← copies the real node.exe to src-tauri/binaries/server-<triple>.exe
scripts/gen-icon.mjs       ← regenerates src-tauri/icons/* (run if icons change)
```

- The sidecar binary (`src-tauri/binaries/server-<triple>.exe`) is a **copy of node.exe**, prepared by `npm run build:sidecar` and git-ignored. The backend code (`server.js`, `src/`) and `node_modules` are shipped as Tauri `bundle.resources`, so no `pkg`/compilation step is needed — robust against ESM deps (cheerio) and network restrictions.
- `server.js` gains two optional flags for Tauri: `--port <n>` and `--public-dir <path>`. The frontend is shipped as a Tauri `bundle.resources` entry (`../public` → `public`), so express serves it from the real OS path at runtime.
- Dev: `npm run dev:tauri` — Tauri serves `http://localhost:3000` from a plain `node server.js` (no sidecar used in dev).
- Release: `npm run build:tauri` — Rust picks a free port, spawns the bundled sidecar with `--port`/`--public-dir`, waits for `/api/health`, then loads it.
- CI: `.github/workflows/tauri-build.yml` validates the build on push/PR (no release); `.github/workflows/tauri-release.yml` produces the NSIS installer on `tauri-v*` tags.
- Route B (later): port the 44 providers to Rust/Tauri commands to drop the Node sidecar and shrink the bundle.

Note: `Cargo.lock` is generated on first `tauri build` (local Rust toolchain or CI) and should be committed for reproducible Windows builds.
