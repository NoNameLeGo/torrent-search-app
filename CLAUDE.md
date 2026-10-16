# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A desktop BT torrent **meta-search** app: one keyword fans out across many torrent-site providers in parallel and the results are aggregated.

⚠️ **There are two implementations on `main`, and you must know which one you're touching.**

1. **The Rust rewrite — this is the mainline since 2026-10-16** (M0–M7 all done, ships as an 18 MB self-contained exe). Workspace under `crates/`; see the section further down. It has **17 providers** (16 ported sites + an offline deterministic `demo`) and a Slint UI. **New feature work goes here.**
2. **The original Node/Express version.** The core is a Node backend that scrapes/queries third-party sites **server-side** (avoiding browser CORS), served to a static frontend and reused unchanged by two desktop shells: Electron (built from `main`) and Tauri (`feat/tauri`). It has **40 providers** + an offline `demo` engine + Torznab indexers. It is no longer the mainline, but it **still builds, still publishes, and is still the feature-complete one**.

Why both are still here: the Node version was kept as the **differential's reference** (M6 compares the two field by field; `test/fixtures/` is kept forever). D18 now permits deleting it, but **function by function, never file by file** — log each removal in the 待删清单 in `AGENTS.md`.

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
public/               static frontend (index.html + js/*.js ES modules, styles.css, ambient.js) — vanilla JS
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

Twelve workflows, split by shell/trigger and by **gate vs probe**. ⚠️ See `AGENTS.md` for the authoritative table.

> 🔴 **CI 红过 9 次 —— 开工前读 [`docs/CI_FAILURES.md`](docs/CI_FAILURES.md)。**
> 数据是从 Actions 日志拉的真实记录（`scripts/ci-failure-audit.sh`），不是回忆。
> 一句话结论：**设计几乎没错过，错的全是机械性的** ——
> 断言靠心算（4/9）、本机没编译器猜 API（3/9）、用了没验过的 Slint 属性（1/9）。
> 各自的防线都在那篇里；**最省事的一条**：只要断言依赖"某函数内部怎么算"，
> 就先在本机跑一遍（`node -e` / `python -c`）再写 —— 用过的都没返工。

**Gates** (run on push/PR, must be green):

- **`ci-guard.yml`** — validates the workflow files themselves (`scripts/check-workflows.py`). Runs on any `.github/workflows/**` change.
- **`build.yml`** — Electron only. Runs on push to `main` (or manual). Builds the NSIS installer + portable zip, uploads as artifacts.
- **`rust.yml`** — ⚠️ **the main gate on `main`** (it used to be `feat/rust`-only; since the 2026-10-16 switch it runs on **`main`**, and `gh workflow run rust.yml` now works). Steps: `cargo test --workspace` → `bt-app --selftest` (with `SLINT_BACKEND=winit-software`) → **M6 differential vs Node** (`dump_rust` ignored test + `scripts/diff/*.cjs`, all 16 providers, `--only`-free) → `clippy` → `fmt --check`.
- **`tauri-build.yml`** — Tauri only, validation. Runs on push to `feat/tauri` and on PRs; builds but never releases.
- **`release.yml`** (`Release (Rust + Electron + Tauri)`) — runs on `v*` tags. Four jobs: `rust` (the Rust mainline, from the trigger ref), `electron` (explicitly `main`), `tauri` (explicitly `feat/tauri`), then `publish`. ⚠️ **`publish` is gated on `startsWith(github.ref, 'refs/tags/')`** — a manual `workflow_dispatch` is a **dry run**, otherwise it would create a public Release literally named after the branch (`BT 聚合搜索 main`). Published **directly, `draft: false`**, appending `docs/RELEASE_ARTIFACTS.md` as the body. Artifact names carry an explicit tag so they can't be confused (`BT-Search-Rust-<ver>.exe|.zip`, `BT-Search-Electron-Setup-<ver>.exe`, `BT-Search-Electron-Portable.zip`, `BT-Search-Tauri-Setup-<ver>.exe`). ⚠️ **Version-bump hazard**: the names come from **three different files** — `package.json` `build.win.artifactName` (Electron installer) + `feat/tauri`'s own `package.json` version (Tauri installer) + `release.yml`'s `Zip portable` / `Rename Tauri installer` steps. Bump **all** of them or an installer ships with a stale version in its filename.

**Probes** (`workflow_dispatch` only, **deliberately not gates** — see Q5: allow-failure means "not a gate", not `continue-on-error`):

- **`live-smoke.yml`** — hits the real sites with `BT_LIVE_SMOKE=1` and prints `[smoke]` lines. Trigger with `gh workflow run live-smoke.yml --ref main`.
- **`html-probes.yml`** — installs cheerio and prints the ground-truth selector values used to build `test/fixtures/html-probes.expected.json`.
- **`provider-health.yml`** — `scripts/provider-health.mjs` hits the real search URL of all 40 engines and prints `HTTP / bytes / result rows`. The "is this engine still usable" check that offline fixtures cannot provide. **Run it on CI, not locally** — locally the MITM proxy breaks Node's TLS trust and the sandbox/DNS makes results unstable (the same host gave 200 then 000 minutes apart). CI is a datacenter IP though, so treat its 403s as a floor, not the user's reality.
- **`m6-diff.yml`** — the **full 16-provider** differential report. `rust.yml`'s M6 step is the gate; this is the "show me everything, even if it's red" view. Uploads both JSON exports as artifacts for offline recomputation.
- **`rust-size.yml`** — **S1/S2 probe**: builds the release exe in **two CRT modes** (static = the S2-compliant form, dynamic = isolates the `lto`/`strip` win) and asserts the DLL dependency list contains **no MSVC runtime** (`^(vcruntime|msvcp|concrt|api-ms-win-crt)` → `::error`). Not a gate; S2's real criterion is "copy to a clean Windows and double-click", which only a human can do.

### Rust rewrite (**on `main` since 2026-10-16**)

Since 2026-10-08 the project is being **rewritten from scratch in Rust**. ⚠️ **`main` is now the Rust mainline** (the `feat/rust` branch is fully merged and retained only as a pointer). M0–M7 are **all done**; the Node side still lives on `main` too — **D18 allows deleting it now that M6 is done, but only function-by-function** (see the 待删清单 in `AGENTS.md`), and `test/fixtures/` is kept forever as the differential's reference.

**Scope decisions.** ⚠️ **Re-scoped 2026-10-11** — the full register now lives in `docs/ROADMAP.md` §5 (D-numbers). The Q-numbers below are the 2026-10-08 set; **Q2 and Q6 are superseded**.

| # | Decision |
|---|---|
| Q1 | Desktop-native only. **Browser/mobile access is dropped** (= D2). |
| ~~Q2~~ | ~~Don't touch the UI yet — reuse `public/`~~ ❌ **superseded**: go **straight to Slint** (D8). |
| Q3 | Windows-only for now, but don't hardcode platform specifics. |
| Q4 | **Switch `main` when the aggregation layer runs** (gate = 聚合层跑通, D5). Not "40 engines done", and the diff harness is *not* a hard gate. |
| Q5 | CI runs **offline fixtures only** (the stable gate). A manual `workflow_dispatch` online smoke test exists (`live-smoke.yml`) and is deliberately **off** every push/PR gate — allow-failure means "not a gate", not `continue-on-error`. Fixtures are a 2026-07 snapshot, so green does **not** mean scraping still works. |
| ~~Q6~~ | ~~WebView ↔ Rust transport~~ ❌ **superseded** — no WebView, so no transport question (D9). |

**New (2026-10-11):** D7 goal = the small/fast/native **artifact** (search is the vehicle) · D8 go straight to Slint · D9 the core is a **library** (no HTTP, no SSE) · D13 probe engine liveness **before** porting more · D16 all four download clients · D17 Torznab deferred (leave an abstraction hook only) · D18 keep the Node version until the diff (M6) is done.

**Start here:** `docs/ROADMAP.md` (goals / architecture / milestones M1–M7 / risks) and **`docs/AGENT_HANDBOOK.md`** (how to do each milestone, the 3 local gates, the do-not-touch list).

⚠️ **`AGENTS.md` is ~110KB and often too large to be auto-loaded — read it on demand.**
Sections there that matter right now:
- **「Slint 探针结论（留档）」→ M1 实测结论（2026-10-16）**: M1 **PASSED** (#13548 not reproduced).
  Also records the trap that faked a green run ("timer-driven redraw never redrew").
- **「🌿 分支现状」**: ⚠️ **the D5 gate for switching `main` (「聚合层跑通」) is now MET** —
  M2 landed 2026-10-16. Read this section before touching `main`.
- **「📌 M2 开工前勘明（2026-10-16）」**: kept as the audit trail for the three findings that
  **overrode existing docs** (`has_more` must be `Option<bool>`; the `onSettle` story was wrong;
  `"no resolver"` not `no_resolver`). M2 is **done** (2026-10-16) and so is **M3**
  (`bt-downloaders`, 4 clients) — the next milestone is **M4** (the Slint app).

**Plan (re-scoped 2026-10-11 — see `docs/ROADMAP.md`).** The old "Phase 1 = Rust core + the existing WebView frontend, Phase 2 = reconsider Slint" plan is **dead**. The goal turned out to be *the artifact itself* (small / fast / native), which a WebView can never deliver — so we go **straight to Slint**, and the core becomes a **library** (no HTTP, no SSE; Slint calls it in-process). Milestones are **M1..M7** in `docs/ROADMAP.md` §3.

**M1 is DONE (2026-10-16) — Slint #13548 was NOT reproduced** (200 CJK titles + 50 Cyrillic, 580 forced synchronous repaints, no panic). So the Slint architecture stands. The `spike/slint-ui` branch and `slint-spike.yml` were **rebuilt** (the 2026-10-09 deletion was undone from dangling git objects — the old code was still in `.git`); the spike's `--selftest` is the CI gate. Conclusions + the two traps are in `AGENTS.md` ("Slint 探针结论（留档）").

```
Cargo.toml              workspace root (members = ["crates/*"], exclude = ["src-tauri"] — src-tauri exists only on `feat/tauri`, not on `feat/rust`)
crates/bt-core/         domain types + normalize + shared HTTP layer
  src/normalize.rs      port of src/lib/normalize.js (semantically equivalent)
  tests/normalize.rs    assertions from test/normalize.test.js, ported verbatim
  src/http.rs           port of src/lib/http.js — never returns Err, mirrors the JS contract
  tests/http.rs         spins up a throwaway local HTTP server; needs no network
crates/bt-providers/    one file per site, mirroring src/providers/*.js
  src/lib.rs            SearchOutcome { results, error, has_more } + MagnetOutcome + run_mirrors
                        (Provider trait comes in **M2** — see docs/ROADMAP.md §2.4)
  src/value.rs          shared Value -> NumOrText / String / min conversions (+ v2nt_nonzero)
  src/tpb.rs            port of src/providers/tpb.js (GET, apibay)
  src/knaben.rs         port of src/providers/knaben.js (POST, official JSON API)
  src/torrentscsv.rs    port of src/providers/torrentscsv.js (GET, flat array)
  src/yts.rs            port of src/providers/yts.js (GET, movie -> torrents, two levels)
  src/linuxtracker.rs   port of src/providers/linuxtracker.js (HTML via bt_core::dom)
  src/filemood.rs       port of src/providers/filemood.js (HTML via bt_core::dom)
  src/rutor.rs          port of src/providers/rutor.js (HTML via bt_core::dom; Russian site, UTF-8)
  src/internetarchive.rs  port of src/providers/internetarchive.js (advancedsearch JSON)
  src/audiobookbay.rs   port of src/providers/audiobookbay.js (HTML; lazily resolves the magnet)
  src/therarbg.rs       port of src/providers/therarbg.js (HTML; size/date in data-order attrs)
  src/limetorrents.rs   port of src/providers/limetorrents.js (HTML; info hash inside itorrents links)
  src/torrent9.rs       port of src/providers/torrent9.js (HTML, French site; name + detail link only)
  src/oxtorrent.rs      port of src/providers/oxtorrent.js (HTML, French site; error string is no_results)
  src/nyaa.rs           port of src/providers/nyaa.js (HTML; JS column indices are all off)
  src/megapeer.rs       port of src/providers/megapeer.js (HTML; **windows-1251**, first non-UTF-8 site)
  src/zeromagnet.rs     port of src/providers/zeromagnet.js (HTML; names split by <mark>; magnet in input value)
  tests/common/mod.rs   throwaway HTTP server + fixture loader shared by provider tests
                        oneshot() = response only; oneshot_bytes() = custom charset header + raw bytes;
                        oneshot_capture() = also hands back the raw request
  tests/*.rs            one offline test file per provider (16 today) + live_smoke.rs
crates/bt-downloaders/  the 4 download clients (qB / Transmission / aria2·Motrix / Gopeed),
                        ported from src/lib/downloaders.js; **deliberately does NOT use bt-core**
                        (that layer is for scraping third-party pages with never-throw; this one is
                        for trusted local clients: Basic Auth, session cookie, JSON-RPC, 409 retry,
                        failures as Err). Errors are `Result<_, String>` — the opposite convention.
                        ⚠️ axios throws on non-2xx but reqwest does NOT → every "axios would throw"
                        site must check the status itself; and a non-JSON 2xx body counts as SUCCESS
                        (the JS success checks are soft: `data && data.result && …`).
crates/bt-app/          the Slint app (**the only UI**, M4 — **DONE**). `ui/app.slint` +
                        `src/main.rs` (wiring) + `src/highlight.rs` (ported from the M1 spike) +
                        `src/ui_model.rs` (**pure** mapping, does NOT import slint, so it is unit-testable
                        offline — that is the part of M4 CI can actually regress).
                        ⚠️ Thread model: network IO on tokio, Slint UI is single-threaded → background tasks
                        must NEVER touch UI properties; hand data back via `slint::invoke_from_event_loop`.
                        UI-side mutable state therefore lives in a **thread_local** (the callback requires
                        `Send + 'static`, so it cannot capture an `Rc`), and the closure captures only owned
                        data + a `Weak<AppWindow>`. Use `as_weak()` (not `clone_strong()`) in callbacks —
                        callbacks live inside the component, so a strong ref cycles and never frees it.
                        Row models are **push-incrementally** updated, never rebuilt wholesale (rebuilding
                        would reset the user's scroll position on every provider report).
                        Two new Slint traps: `root` inside a component means *that component*, not the window
                        (forward via a `callback` + `x(idx) => { root.y(idx); }`); and `VecModel`'s
                        `row_count`/`set_row_data` are **`Model` trait** methods — `use slint::Model;`.
                        ⚠️ Two RUNTIME bugs that compile/cargo-test/clippy CANNOT see, found by `--selftest`:
                        (a) a bare `tokio::spawn` in a callback PANICS — Slint's event-loop thread is not
                        inside a tokio runtime (`there is no reactor running…`), so the app must hold its own
                        `Runtime` and use the local `spawn()` wrapper; (b) `enable_all()` only opens the IO
                        driver if tokio's **`net`** feature is on, or reqwest fails at runtime with
                        "IO driver not enabled". Both are now in place.
                        ✅ `--selftest` (this crate's CI gate, run with `SLINT_BACKEND=winit-software`) drives
                        the SAME wiring as the real app (`wire(ui)` is shared — otherwise it would test a
                        parallel implementation) with a fake registry (one provider with no magnets → lazy
                        resolve, one with inline magnets, one that always fails) + a local qB stub, walking
                        search → per-provider status → row model → lazy magnet → push. It forces a
                        **synchronous** repaint every tick and counts them (`paints_ok == ticks &&
                        paints_err == 0`) so a "never actually repainted" fake-green run cannot pass.
                        12 assertions; exits 1 on failure.

crates/bt-torznab/      Torznab indexers (Jackett / Prowlarr / *arr) — **built 2026-10-16**:
                        `parse_items` (roxmltree) + request layer + `TorznabProvider`.
                        ⚠️ It depends on `bt-providers` (for the `Provider` trait) and
                        `bt-providers` does **not** know about it — so the "static 17 +
                        dynamic indexers" assembly happens in `bt-app`
                        (`add_dynamic_factory`). Still missing: **the UI to add indexers**
                        (today you hand-edit `%APPDATA%\bt-search\indexers.json`).
```

**Porting a provider (recipe, copy tpb/knaben/torrentscsv/yts):** read the JS, write `src/<name>.rs` with a payload struct (if it's a POST) + `search()` + `search_at(http, api, query)` (the latter is what makes offline tests possible), then `tests/<name>.rs` with field-by-field assertions. **Don't hand-write the expected values** — feed the same fixture through `src/lib/normalize.js` with a throwaway Node script and copy its output into the assertions; that is what makes "same as the Node version" verifiable. Pin any deliberate divergence as a named `divergence_*` test.

Known pitfalls when guessing expected values (all three have already cost a CI round): `dateText` defaults to `"—"`, not `""`; JS treats `0` as falsy, so `x ? Number(x) : null` means `created_unix: 0` yields no date (use `value::v2nt_nonzero`, but note the *string* `"0"` is truthy); array fields like `categoryId` take the **min**, not the first element.

**The provider line is deferred (2026-10-11, D13).** 16 ports are done (5 JSON + 11 HTML). Do **not** keep porting by default — the goal is the artifact, not engine count. Run `provider-health.yml` to see which engines are actually still alive, cut the dead ones, and only then decide whether more are worth it. `xxxtracker` / `dmhy` / `sukebei` / `nekobt` remain unported but are **not** the next step (that is **M1**, the Slint risk test). Full candidate table and per-provider progress live in `AGENTS.md`.

**HTML providers (worked path, from `linuxtracker`):** parse with `bt_core::dom` (the cheerio-compatible layer over `scraper`). **Ask the page structure via the probe harness instead of guessing from the HTML** — add probes to `test/fixtures/html-probes.json`, run `.github/workflows/html-probes.yml` to get cheerio's ground truth, and only then write the provider. Two things bit us there: cheerio's `.map()` flattens arrays (use `.toArray().map()`), and bare `<tr>`/`<td>` snippets are dropped by the HTML parser (wrap them in `<table>`).

`runMirrors` (`src/lib/mirrors.js`) does **not** mean "retry on error": it fires all mirrors in parallel and takes the first with **non-empty results**; otherwise the error becomes `"<name> unreachable (<err1>; <err2>)"`. So "page loaded but no hits" counts as an **error**.

Upstream bugs found while porting: `linuxtracker`'s `detailUrl` misses a slash (`https://linuxtracker.orgindex.php?...` — a dead link; **fixed** here, pinned as a divergence test); its `parseEuDate` builds local-midnight dates while `dateText` formats in UTC, so results show one day early east of UTC (**copied as-is**); expanded description rows land their size/seeders in the wrong columns (**copied as-is**). `megapeer` is the other *fixed* case: axios hard-decodes text as utf8 and mangles the site's windows-1251 Cyrillic, while reqwest honours the charset header — a data-level bug, so Rust deliberately differs (pinned as a divergence test). Policy: fix functional bugs, copy cosmetic ones; the full table is in `AGENTS.md`.

**Deleting the old JS — function-level, not file-level (revised 2026-10-08).** Do *not* mechanically delete `<provider>.js` as soon as its Rust port lands. Delete only once that function is fully usable from Rust (wired into `bt-app`), and never when the JS is still the reference for an accepted equivalence check. Anything kept for now must be logged in the "待删清单" table in `AGENTS.md` and deleted in a batch at the end of a phase. The one hard rule that survives: two implementations must never be allowed to drift, so every ported file's status (`待删` / `保留(对照)` / `已删`) goes in the progress table.

**Latest ports (2026-10-11):** the "✅ in both environments" picks are done (`audiobookbay`, `nyaa`), plus `megapeer` (the first **windows-1251** site) and `zeromagnet` (29 rows with no header row, names split by `<mark>` so child text must be concatenated, magnet lives in `input#input-magnet`'s **value attribute**, and the meta columns hold `6.2GB` / dates that the JS drops — copied as-is). `dmhy` is **paused** — a CI fixture fetch timed out three times (60s, 0 bytes; the homepage too), so the site itself is down, not just unreachable from this machine. Porting more is **deferred** (D13) — see the note above; the next milestone is **M4** (the Slint app), not another provider.

**`megapeer.js` (done, 2026-10-11) — first windows-1251 site, ported with two deliberate divergences.** The site serves windows-1251 bytes with a matching `charset=windows-1251` response header (verified live by CI smoke). JS axios hard-decodes text as utf8 (`buffer.toString('utf8')` — it ignores the charset), so **every Cyrillic title became `U+FFFD`**; Rust's reqwest honours the header's charset via encoding_rs and decodes correctly — deliberately *not* copied, because it is a data-level bug (pinned by `divergence_js_style_utf8_decoding_would_mangle_the_cyrillic`). Second: result rows come in two shapes (43× 4-td, 7× 5-td with a comment cell at td3) and JS's fixed indices (`td:nth-child(3)` as size, `td:nth-child(4) > font` as peers) grab the comment count and leave peers empty on 5-td rows — the port uses "second-to-last cell = size, last cell = peers" instead (pinned by `divergence_5td_row_takes_real_size_not_the_comment_count`). Infrastructure added: per-fixture `encoding` in `html-probes.json` (TextDecoder on the JS side, encoding_rs on the Rust side), `fixture_bytes()` + `oneshot_bytes()` in `tests/common` (the charset header travels to reqwest, same as production), and a live-smoke test.

**Blocked engines are catalogued in `AGENTS.md` ("🚧 受阻引擎清单")** — before starting any provider, check that list; four of them (`eztv`, `blueroms`, `torrentdatabase`, `uindex`) are dead in both environments, and `1337x` has its own section.

**`1337x` is investigated and skipped (2026-10-09).** A plain HTTP client cannot get a result page — and neither can the JS version, so this is not a porting regression. `1337x.to`/`1337x.is`/`x1337x.se` return **no A record** from this machine's resolver; `1337x.st`/`x1337x.ws`/`x1337x.eu`/`x1337x.cc` all serve Cloudflare's `Just a moment...` challenge; the old fixture's redirect domain `13377x.to` is now a **parked ad lander**; every third-party mirror tried is either Cloudflare or a squatter page. Note the old fixture's FingerprintJS gate *was* followable (it had an `<a href="…&fp=-3">Click here to enter</a>` fallback for non-JS clients) — that path died with the domain, and what blocks us now is Cloudflare + DNS-level blocking, a different mechanism. **Do not implement a `1337x` provider in `bt-providers`** — it would only ever return an error. Full findings and the four options for revisiting live in `AGENTS.md`.

`filemood.js` (done, 2026-10-09) is worth reading as the second HTML template: 65 `<tr>` → 20 data rows (kept by `a.btn-success`), title text spans multiple `<span class="highlated">` children so it must be concatenated, infoHash is scraped out of the detail URL's tail (`/name-<40hex>.html`), and its hrefs start with `/` so the base join is actually correct — unlike linuxtracker. One JS quirk is copied on purpose: empty body + no HTTP error yields `filemood unreachable ()`.

Porting conventions (details in `AGENTS.md`): JS `typeof` runtime checks become the `NumOrText` enum; JS's lenient numeric parsing (`parseFloat("1.2.3") == 1.2`, `parseInt("12abc") == 12`) is reimplemented rather than replaced by `f64::from_str`; integration tests can't see the crate's normal dependencies, so `chrono`/`serde`/`tokio` must also be listed under `[dev-dependencies]`. Deliberate divergences from the JS behaviour are pinned as named tests — don't "fix" them.

**Rust tooling on this machine:** `cargo` is gone (CARGO_HOME was cleaned up), so `cargo build/test/clippy` only ever run in CI. `rustfmt.exe` *is* still usable standalone — run it with `--edition 2021` before pushing:

```bash
"/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe" --check --edition 2021 <files>
```

Because there is no local `cargo check`, a wrong function signature only shows up in CI (~1.5 min per round). Two that have already bitten: turbofish arity on `post_json<T, B>` (use a typed binding instead), and `and_then` closures whose target takes `Option<&str>` rather than `&str`. Re-read the real signature before calling anything.

Don't substitute a line-width check for it: rustfmt's default `fn_call_width` is 60, so multi-arg calls get split even when the line is well under 100 chars.

**`gh` on this machine returns 403 intermittently — always wrap it in `scripts/gh-retry.sh`.** A transparent intercepting proxy randomly rejects requests to `api.github.com` (0.08s instant reject, empty body, `Via: Caddy`). Measured 2026-10-09: proxy `:1267` 9/10 → 8/12 → 4/12, proxy `:80` 5/12, direct 7/10 — **no route is reliable, and the 403s come in bursts (5 in a row observed)**. It is *not* GitHub rejecting you and *not* a token problem (`gh auth status` is ✓, scopes include `repo`/`workflow`). Retry is the only fix:

```bash
scripts/gh-retry.sh run list --branch feat/rust
scripts/gh-retry.sh run view --job=<JOB_ID> --log
scripts/gh-retry.sh workflow run live-smoke.yml --ref feat/rust
```

The note in older docs saying "`gh` must bypass the proxy (`env -u HTTP_PROXY …`)" is **obsolete** — that was a different proxy instance on 2026-10-07.


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
