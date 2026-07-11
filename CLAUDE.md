# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A desktop BT torrent **meta-search** app: one keyword fans out across 40 torrent-site providers (+ an offline `demo` engine, and user-added Torznab indexers) in parallel and aggregates the results. The core is a Node/Express backend that scrapes/queries third-party sites **server-side** (avoiding browser CORS), served to a static frontend. The same backend is reused unchanged by two desktop shells: Electron (`main` branch) and Tauri (`feat/tauri` branch, current).

Windows-first. UI strings and many code comments are in Chinese.

## Commands

```bash
npm install          # express, axios, cheerio (runtime) + electron, @tauri-apps/cli, electron-builder (dev)
npm start            # node server.js → http://localhost:3000  (also `npm run dev`)
npm run electron     # Electron dev window (picks a free port, no collision with npm start)
npm run dev:tauri    # Tauri dev: serves http://localhost:3000 from a plain node server.js
npm run build:tauri  # Tauri release NSIS installer (spawns bundled node sidecar)
npm run dist         # Electron NSIS installer → dist/
npm run build:portable  # manual Electron portable build → dist/portable/
```

`PORT=8080 node server.js` overrides the port. `start.bat`/`stop.bat` are the primary Windows launchers for the plain web mode.

**No tests, linter, formatter, or typecheck are configured** — there is nothing to run before shipping. If you add tooling, document it here and in `AGENTS.md`.

## Architecture

```
public/               static frontend (index.html, app.js, styles.css) — vanilla JS, no build step
server.js             Express entry; exports { app, start }. Auto-listens only when run directly.
electron/main.js      Electron shell — require('./server').start(port); close = full process exit
src-tauri/            Tauri v2 Rust shell (feat/tauri branch)
src/providers/        one file per search engine (40) + index.js registry
src/lib/http.js       shared axios instance; getText/getJSON/postJSON NEVER throw
src/lib/normalize.js  size/date/magnet parsing → canonical TorrentResult shape
src/lib/torznabStore.js  persists user-added Torznab indexers to data/torznab.json (git-ignored, holds API keys)
```

Key backend endpoints (`server.js`): `/api/search` (parallel aggregate), `/api/providers`, `/api/magnet` (lazy detail-page magnet resolution), `/api/download/qbittorrent` + `/detect` (qBittorrent WebUI proxy/auto-detect), `/api/torznab*` (indexer CRUD + `t=caps` test), `/api/health`.

### Provider contract

Every provider exports `{ id, name, search }` and is registered in `src/providers/index.js`. The `REGISTRY` array order is the UI display order.

- `search(query, { page }) → { results, error, hasMore }`. Results must pass through `normalize()` from `src/lib/normalize.js`.
- Providers whose magnet lives on a detail page (not in search results) also export `resolveMagnet(url) → { magnet, error }` and set `needsMagnet: true` on results — the frontend calls `/api/magnet` lazily on click. See `src/providers/1337x.js` for the canonical pattern (also demonstrates parallel mirror-domain fallback).
- **Never throw.** `index.js#search` isolates each provider so one failure can't break the aggregate, and the `http.js` helpers already return `{ ..., error }` instead of throwing — match that convention.
- The `demo` provider is offline-only, always enabled, and generates deterministic fake data for testing the UI without network.
- **Torznab** (`torznab.js`) is a provider *factory*, not a static provider: user-added Jackett/Prowlarr/*arr indexers stored in `torznabStore` become dynamic providers at request time (ids prefixed `torznab:`). `index.js` merges these into `list()`/`search()`/`getProvider()` on every call.

To add a provider: create `src/providers/<name>.js`, register it in `index.js`, normalize results. See `AGENTS.md` for the step list.

## Two desktop shells, one backend

Both shells run the **exact same** `server.js` + `src/` + `public/` — the shell only owns process lifecycle and magnet-link handoff to the OS.

- **Electron** (`main` branch): `electron/main.js` calls `require('./server').start(port)` in-process on a free port. Window close → whole process exits, backend GC'd (no residual node process). Single-instance lock focuses the existing window.
- **Tauri** (`feat/tauri` branch): "Route A" — the backend is bundled as a **sidecar that is a literal copy of `node.exe`** (prepared by `npm run build:sidecar` → `scripts/prepare-sidecar.mjs`, git-ignored). `server.js`, `src/`, `public/`, `node_modules` ship as Tauri `bundle.resources`; no `pkg`/compile step, so ESM deps (cheerio) and offline builds Just Work. Rust (`src-tauri/src/main.rs`) picks a free port, spawns the sidecar with `--port`/`--public-dir`, polls `/api/health`, then opens the window. In **dev** the sidecar is *not* used — Tauri points at a plain `node server.js` on port 3000.

`server.js` accepts two flags used only by the Tauri sidecar: `--port <n>` and `--public-dir <path>` (express serves the frontend from the real OS resource path at runtime).

Magnet links: WebView2/Electron won't auto-invoke `magnet:`, so both shells intercept navigation to a `magnet:` scheme and hand it to the OS default client (qBittorrent/迅雷) instead of navigating.

## CI (.github/workflows/)

Three workflows, split by shell and trigger:

- **`build.yml`** — Electron only. Runs on push to `main` (or manual). Builds the NSIS installer + portable zip, uploads as artifacts.
- **`release.yml`** — Electron **and** Tauri together. Runs on `v*` tags (or manual). Three parallel jobs: `electron` (checks out the trigger ref), `tauri` (explicitly checks out `feat/tauri`), then `publish` bundles both into a single **draft** GitHub Release, appending `docs/RELEASE_ARTIFACTS.md` as the body.
- **`tauri-build.yml`** — Tauri only, validation. Runs on push to `feat/tauri` and on PRs; builds but never releases.

`src-tauri/Cargo.lock` is committed for reproducible Windows builds. See the "Two desktop shells" section and `docs/RELEASE_ARTIFACTS.md` for how the resulting installers differ.

## Conventions & gotchas

- Build caches are redirected to project-local `.cache/` (git-ignored) to avoid polluting `%LOCALAPPDATA%`.
- `data/` (Torznab configs with API keys) and `dist/` (large build artifacts) are git-ignored.
- Windows-only npm scripts use `set` (not `export`) for env vars.
- Scraper providers depend on target-site HTML structure; when a site redesigns, its provider's selectors need updating (the status bar shows per-provider ✓/✕ so breakage is visible).
- `SEARCH_ENGINE_PORT_COVERAGE.md` tracks which upstream (prajwalch/TorrentSearch) engines have been ported.
