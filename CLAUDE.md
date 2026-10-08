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

**Testing**: golden-file tests via Node's built-in `assert` — run `npm test` (see `test/README.md`; fixtures under `test/fixtures/`). ESLint (`npm run lint`) and Prettier (`npm run format`) are configured; no typechecker. If you add tooling, document it here and in `AGENTS.md`.

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

**编译一律在 CI 里跑，不在本机跑。** 用户机器 C:/D: 两盘均 90%+ 占用，且明确要求不新增本地构建环境。本机的 `CARGO_HOME`（`D:\Vibe-Coding\.cargo`）已被清理、`cargo` 不在 PATH 上 —— **本地跑不了 cargo**。本地只写代码，编译/测试结果看 Actions 日志，产物从 Artifacts 下载。

Six workflows, split by shell and trigger:

- **`build.yml`** — Electron only. Runs on push to `main` (or manual). Builds the NSIS installer + portable zip, uploads as artifacts.
- **`release.yml`** — Electron **and** Tauri together. Runs on `v*` tags (or manual). Three parallel jobs: `electron` (checks out the trigger ref), `tauri` (explicitly checks out `feat/tauri`), then `publish` bundles both into a single GitHub Release (published **directly, `draft: false`** — no manual "Publish" click), appending `docs/RELEASE_ARTIFACTS.md` as the body. Every artifact name carries an explicit `-Electron-`/`-Tauri-` tag (`BT-Search-Electron-Setup-<ver>.exe`, `BT-Search-Electron-Portable.zip`, `BT-Search-Tauri-Setup-<ver>.exe`) so the two shells' installers can't be confused. Renaming lives in three places — `package.json` `build.win.artifactName` (Electron installer), `release.yml`'s `Zip portable` step (Electron portable) and `Rename Tauri installer` step (Tauri) — plus the example names in `docs/RELEASE_ARTIFACTS.md`.
- **`tauri-build.yml`** — Tauri only, validation. Runs on push to `feat/tauri` and on PRs; builds but never releases.
- **`rust.yml`** — the Rust rewrite (see below). Runs on push to `feat/rust` and on PRs touching `crates/**`. `cargo test --workspace` + `clippy` + `fmt --check`. **Push-only** — it doesn't exist on `main`, so `gh workflow run` returns 404.
- **`live-smoke.yml`** — `workflow_dispatch` only. Hits the real sites with `BT_LIVE_SMOKE=1` and prints `[smoke]` lines. **Deliberately excluded from every push/PR gate.** Exists on both `main` (required for manual dispatch) and `feat/rust` — keep the two copies in sync. Trigger with `gh workflow run live-smoke.yml --ref feat/rust`.
- **`html-probes.yml`** — `workflow_dispatch` only (also on both `main` and `feat/rust`). Installs cheerio and prints the ground-truth selector values used to build `test/fixtures/html-probes.expected.json`.

### Rust rewrite (`feat/rust`)

Since 2026-10-08 the project is being **rewritten from scratch in Rust** on the `feat/rust` branch, which is intended to eventually become `main`. The Node side is untouched and keeps working until each piece is replaced.

**Scope decisions (2026-10-08, settled — full table in `AGENTS.md`):**

| # | Decision |
|---|---|
| Q1 | Desktop-native only. **Browser/mobile access is being dropped** (kept as a free by-product during the transition). |
| Q2 | **Don't touch the UI yet** — reuse the existing `public/` frontend to get the core working; evaluate Slint native once the core is stable. |
| Q3 | Windows-only for now, but don't hardcode platform specifics. |
| Q4 | **Switch `main` once the core is done** (providers + aggregation); UI comes later. |
| Q5 | CI runs **offline fixtures only** (the stable gate). A manual `workflow_dispatch` online smoke test exists (`live-smoke.yml`) and is deliberately **off** every push/PR gate — allow-failure means "not a gate", not `continue-on-error`. Fixtures are a 2026-07 snapshot, so green does **not** mean scraping still works. |

**Two phases.** Phase 1 = Rust core + the existing WebView frontend (the Tauri shell hosts the Rust core directly, killing the 89MB node sidecar; `public/` unchanged). Phase 2 = decide on Slint, and only then delete `public/` plus the Tauri shell. The `spike/slint-ui` branch **was deleted on 2026-10-09** — its conclusions are archived in `AGENTS.md` ("Slint 探针结论（留档）") and must be read before re-attempting anything with Slint.

```
Cargo.toml              workspace root (members = ["crates/*"], exclude = ["src-tauri"])
crates/bt-core/         domain types + normalize + shared HTTP layer
  src/normalize.rs      port of src/lib/normalize.js (semantically equivalent)
  tests/normalize.rs    assertions from test/normalize.test.js, ported verbatim
  src/http.rs           port of src/lib/http.js — never returns Err, mirrors the JS contract
  tests/http.rs         spins up a throwaway local HTTP server; needs no network
crates/bt-providers/    one file per site, mirroring src/providers/*.js
  src/lib.rs            SearchOutcome { results, error, has_more } (no Provider trait yet)
  src/value.rs          shared Value -> NumOrText / String / min conversions (+ v2nt_nonzero)
  src/tpb.rs            port of src/providers/tpb.js (GET, apibay)
  src/knaben.rs         port of src/providers/knaben.js (POST, official JSON API)
  src/torrentscsv.rs    port of src/providers/torrentscsv.js (GET, flat array)
  src/yts.rs            port of src/providers/yts.js (GET, movie -> torrents, two levels)
  src/linuxtracker.rs   port of src/providers/linuxtracker.js (HTML via bt_core::dom)
  tests/common/mod.rs   throwaway HTTP server + fixture loader shared by provider tests
                        oneshot() = response only; oneshot_capture() = also hands back the raw request
  tests/{tpb,knaben,torrentscsv,yts}.rs   per-provider offline tests
(pending)  crates/bt-torznab/ · bt-downloaders/ · bt-app/
```

**Porting a provider (recipe, copy tpb/knaben/torrentscsv/yts):** read the JS, write `src/<name>.rs` with a payload struct (if it's a POST) + `search()` + `search_at(http, api, query)` (the latter is what makes offline tests possible), then `tests/<name>.rs` with field-by-field assertions. **Don't hand-write the expected values** — feed the same fixture through `src/lib/normalize.js` with a throwaway Node script and copy its output into the assertions; that is what makes "same as the Node version" verifiable. Pin any deliberate divergence as a named `divergence_*` test.

Known pitfalls when guessing expected values (all three have already cost a CI round): `dateText` defaults to `"—"`, not `""`; JS treats `0` as falsy, so `x ? Number(x) : null` means `created_unix: 0` yields no date (use `value::v2nt_nonzero`, but note the *string* `"0"` is truthy); array fields like `categoryId` take the **min**, not the first element.

**Order for the remaining providers:** `filemood` next (the JSON group is done), then `1337x` — whose fixture turned out to be a FingerprintJS anti-bot interstitial, not a results page.

**HTML providers (worked path, from `linuxtracker`):** parse with `bt_core::dom` (the cheerio-compatible layer over `scraper`). **Ask the page structure via the probe harness instead of guessing from the HTML** — add probes to `test/fixtures/html-probes.json`, run `.github/workflows/html-probes.yml` to get cheerio's ground truth, and only then write the provider. Two things bit us there: cheerio's `.map()` flattens arrays (use `.toArray().map()`), and bare `<tr>`/`<td>` snippets are dropped by the HTML parser (wrap them in `<table>`).

`runMirrors` (`src/lib/mirrors.js`) does **not** mean "retry on error": it fires all mirrors in parallel and takes the first with **non-empty results**; otherwise the error becomes `"<name> unreachable (<err1>; <err2>)"`. So "page loaded but no hits" counts as an **error**.

Upstream bugs found while porting: `linuxtracker`'s `detailUrl` misses a slash (`https://linuxtracker.orgindex.php?...` — a dead link; **fixed** here, pinned as a divergence test); its `parseEuDate` builds local-midnight dates while `dateText` formats in UTC, so results show one day early east of UTC (**copied as-is**); expanded description rows land their size/seeders in the wrong columns (**copied as-is**). Policy: fix functional bugs, copy cosmetic ones.

**Deleting the old JS — function-level, not file-level (revised 2026-10-08).** Do *not* mechanically delete `<provider>.js` as soon as its Rust port lands. Delete only once that function is fully usable from Rust (wired into `bt-app`), and never when the JS is still the reference for an accepted equivalence check. Anything kept for now must be logged in the "待删清单" table in `AGENTS.md` and deleted in a batch at the end of a phase. The one hard rule that survives: two implementations must never be allowed to drift, so every ported file's status (`待删` / `保留(对照)` / `已删`) goes in the progress table.

**Next step (agreed, do not start without the user):** continue with the **JSON group first, then the HTML group**. Recommended order: `knaben` → `torrentscsv` → `yts` → `internetarchive`, then introduce the `scraper` crate for the HTML providers (`linuxtracker`, `filemood`) and verify selector equivalence against cheerio. Don't define a `Provider` trait until 3–5 providers exist.

Porting conventions (details in `AGENTS.md`): JS `typeof` runtime checks become the `NumOrText` enum; JS's lenient numeric parsing (`parseFloat("1.2.3") == 1.2`, `parseInt("12abc") == 12`) is reimplemented rather than replaced by `f64::from_str`; integration tests can't see the crate's normal dependencies, so `chrono`/`serde`/`tokio` must also be listed under `[dev-dependencies]`. Deliberate divergences from the JS behaviour are pinned as named tests — don't "fix" them.

**Rust tooling on this machine:** `cargo` is gone (CARGO_HOME was cleaned up), so `cargo build/test/clippy` only ever run in CI. `rustfmt.exe` *is* still usable standalone — run it with `--edition 2021` before pushing:

```bash
"/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe" --check --edition 2021 <files>
```

Because there is no local `cargo check`, a wrong function signature only shows up in CI (~1.5 min per round). Two that have already bitten: turbofish arity on `post_json<T, B>` (use a typed binding instead), and `and_then` closures whose target takes `Option<&str>` rather than `&str`. Re-read the real signature before calling anything.

Don't substitute a line-width check for it: rustfmt's default `fn_call_width` is 60, so multi-arg calls get split even when the line is well under 100 chars.


**Version tags increment forward — never re-tag the same version.** Bump `version` in both `package.json` and `src-tauri/tauri.conf.json`, then tag the *next* number (`v0.0.2`, `v0.1.0`, …). Only reuse a tag when the user explicitly says to overwrite a specific version. Rationale: same-tag re-release means moving a published tag, and `softprops` **appends** assets to the existing Release rather than replacing them (leaving stale files behind) — so `release.yml`'s publish job first `gh release delete <tag> --yes` (keeping the tag) before re-creating, but forward-incrementing avoids the whole hazard.

`src-tauri/Cargo.lock` is committed for reproducible Windows builds. See the "Two desktop shells" section and `docs/RELEASE_ARTIFACTS.md` for how the resulting installers differ.

## Conventions & gotchas

- Build caches are redirected to project-local `.cache/` (git-ignored) to avoid polluting `%LOCALAPPDATA%`.
- `data/` (Torznab configs with API keys) and `dist/` (large build artifacts) are git-ignored.
- Windows-only npm scripts use `set` (not `export`) for env vars.
- Scraper providers depend on target-site HTML structure; when a site redesigns, its provider's selectors need updating (the status bar shows per-provider ✓/✕ so breakage is visible).
- **Test scrapers from the agent's sandbox, not the user's machine.** The user's local Windows box has DNS pollution for many BT sites (Facebook's blackhole IPv6 `2a03:2880:face:b00c` hijacks some domains), so a provider that fails from their terminal may work from the agent environment and vice-versa. Verify provider reachability from the agent runtime; **do not** ask the user to share their network or tunnel traffic. The ✓/✕ status reflects real reachability at request time.
- **Russian-site providers (rutor, etc.)**: serve **UTF-8**, not windows-1251 despite what upstream may imply. Match table cells by *content* (size regex, seeder/leecher `<span>`s), never by column index — these sites inject extra columns (e.g. a comments cell) that shift indices. JS `\b` does not match Cyrillic characters, and a bare unit letter like `B` false-matches titles such as "Black Box"; require a full unit (`GB|MB|KB|TB|ГБ|МБ|КБ|ТБ`).
- `SEARCH_ENGINE_PORT_COVERAGE.md` tracks which upstream (prajwalch/TorrentSearch) engines have been ported.
- **Tauri validation** (legacy `feat/tauri` branch): `cargo check` in dev mode skips code behind `#[cfg(not(debug_assertions))]` — Tauri's release-only `setup` block — so only a release build actually exercises it. ⚠️ **This can no longer be done locally** (`cargo` is gone from this machine); it now only happens in `tauri-build.yml`. Also validate `tauri.conf.json` against `node_modules/@tauri-apps/cli/config.schema.json` with `ajv` before pushing (skip `pattern` keywords). The full Tauri v2 gotcha list lives in `AGENTS.md`.
