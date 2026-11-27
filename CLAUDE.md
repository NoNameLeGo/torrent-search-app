# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A desktop BT torrent **meta-search** app: one keyword fans out across many torrent-site providers in parallel and the results are aggregated.

**`main` now holds exactly ONE implementation: the Rust one** (`crates/`, since 2026-10-16; M0–M7 all done; ships as a ~21 MB self-contained exe with a Slint UI). **New feature work goes here.**

⚠️ **The Node/Express + Electron implementation was retired on 2026-11-27** (commit `012f5db`) — `server.js`, `src/` (40 providers), `public/`, `electron/`, the Node test runners, `package-lock.json` and the ESLint/Prettier configs are all **gone from `main`**. Consequences you must keep in mind:

- The **M6 differential gate is gone with it** — there is no longer a machine check that Rust behaves like Node. `rust.yml` only runs `cargo test` + `--selftest` + clippy + fmt. Provider parsing regressions are caught by unit tests + `test/fixtures/` only.
- **Electron is no longer built or published.** Release artifacts are now 3: `BT-Search-Rust-<ver>.exe`, `.zip`, `BT-Search-Tauri-Setup-<ver>.exe`.
- **`feat/tauri` still carries its own copy of the Node backend** (the Tauri installer runs it; `release.yml` checks that branch out to build). So the Node code still exists — just not on `main`.
- `test/fixtures/` is **kept forever** (the Rust tests read it directly); the Node runners that used it are gone.

Windows-first. UI strings and many code comments are in Chinese.

## Commands

⚠️ **There is no local build.** `cargo` is not installed on this machine (`D:\Vibe-Coding\.cargo` was cleaned; the user's disks are 90%+ full and they explicitly refuse a local build environment) — compile/test only ever happens in CI. The single local command that still works:

```bash
node scripts/release-check.js   # 发版前 8 项预检（版本四处一致 / 分支存在 / 工作区干净）
python scripts/check-workflows.py  # 改过 .github/workflows/** 后必跑
"…/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe" --check --edition 2021 <files>  # 省一轮 CI
```

**Testing** lives entirely on the Rust side: `cargo test --workspace` (CI). Fixtures under `test/fixtures/` are read by `crates/bt-providers/tests/*.rs` and `crates/bt-core/tests/dom_probes.rs`; see `test/README.md`.

## Architecture

```
crates/bt-core/         domain types + normalize + shared HTTP layer (never-throw) + cheerio-compatible DOM layer
crates/bt-providers/    one file per site (28 BT sites + offline `demo`) + Provider trait + run_mirrors
crates/bt-downloaders/  the 4 download clients (qB / Transmission / aria2·Motrix / Gopeed)
crates/bt-torznab/      Torznab indexers (Jackett / Prowlarr / *arr)
crates/bt-app/          the Slint app — the ONLY UI: ui/app.slint + main.rs (wiring) + ui_model.rs (pure mapping) + highlight.rs
test/fixtures/          frozen offline snapshots, read directly by the Rust tests (kept forever)
src-tauri/              legacy Tauri v2 shell — lives ONLY on the `feat/tauri` branch
```

No HTTP server, no SSE: the core is a library and Slint calls it in-process (D9). User config lives in `%APPDATA%\bt-search\` (override with `BT_SEARCH_HOME`): `sources.json` is the single source of truth for data sources; also `config.json` (downloaders), `indexers.json`, `pansou.json`, `health.jsonl`, `cardigann/`.

### Provider contract (Rust)

`crates/bt-providers` implements the `Provider` trait (`id()` / `name()` / `search()`; optionally a magnet resolver for sites whose magnet only lives on the detail page). Providers are registered via the `site!` macro **and** manually in `registry()` — the macro only declares the struct, it does not add it to the table (that gotcha is logged in `AGENTS.md`).

- `search()` returns `SearchOutcome { results, error, has_more }` — **never returns `Err`**: a failed site becomes `error: Some(..)`, exactly like the old JS contract. `run_mirrors` fires all mirrors in parallel and takes the first non-empty result; "page loaded but no hits" counts as an **error**.
- Results go through `bt_core::normalize` → `TorrentResult` (15 fields; `tests/normalize.rs` pins the JS-era semantics).
- Torznab (`crates/bt-torznab`) is a *factory*, not a static provider: user-added Jackett/Prowlarr/*arr indexers become dynamic providers at request time (ids prefixed `torznab:`), assembled in `bt-app` (`add_dynamic_factory` / `rebuild_dynamic_factories`).

The old equivalence-checking recipe ("feed the fixture through `src/lib/normalize.js` and copy its output") is **dead** — that JS is gone. Expected values now come from the frozen fixtures themselves plus whatever the commit history recorded; see `test/README.md`.

## The one remaining Node shell: Tauri (`feat/tauri`)

Electron is gone. The **only** Node-backed artifact left is the Tauri installer, and it is built **entirely from the `feat/tauri` branch**, which carries its own copy of `server.js` + `src/` + `public/` (that branch is a snapshot of the pre-retirement tree, with a Tauri v2 Rust shell on top). `release.yml` checks it out explicitly for the `tauri` job — **never delete that branch**, and keep its version numbers in step with `main` (4 places; `scripts/release-check.js` enforces it).

That branch's model: the backend is bundled as a **sidecar that is a literal copy of `node.exe`**; `server.js` / `src/` / `public/` / `node_modules` ship as Tauri `bundle.resources`, and `src-tauri/src/main.rs` picks a free port, spawns the sidecar with `--port` / `--public-dir`, polls `/api/health`, then opens the window. In dev the sidecar is not used — it points at a plain `node server.js` on port 3000.

Magnet links: WebView2 won't auto-invoke `magnet:`, so the shell intercepts navigation to a `magnet:` scheme and hands it to the OS default client (qBittorrent/迅雷).

## CI (.github/workflows/)

**编译一律在 CI 里跑，不在本机跑。** 用户机器 C:/D: 两盘均 90%+ 占用，且明确要求不新增本地构建环境。本机的 `CARGO_HOME`（`D:\Vibe-Coding\.cargo`）已被清理、`cargo` 不在 PATH 上 —— **本地跑不了 cargo**。本地只写代码，编译/测试结果看 Actions 日志，产物从 Artifacts 下载。

**Nine** workflows (14 before the 2026-11-27 Node retirement), split by trigger and by **gate vs probe**. ⚠️ See `AGENTS.md` for the authoritative table.

> 🔴 **CI 红过 9 次 —— 开工前读 [`docs/CI_FAILURES.md`](docs/CI_FAILURES.md)。**
> 数据是从 Actions 日志拉的真实记录（`scripts/ci-failure-audit.sh`），不是回忆。
> 一句话结论：**设计几乎没错过，错的全是机械性的** ——
> 断言靠心算（4/9）、本机没编译器猜 API（3/9）、用了没验过的 Slint 属性（1/9）。
> 各自的防线都在那篇里；**最省事的一条**：只要断言依赖"某函数内部怎么算"，
> 就先在本机跑一遍（`node -e` / `python -c`）再写 —— 用过的都没返工。

**Gates** (run on push/PR, must be green):

- **`ci-guard.yml`** — validates the workflow files themselves (`scripts/check-workflows.py`). Runs on any `.github/workflows/**` change.
- **`rust.yml`** — ⚠️ **the main gate on `main`**. Steps: `cargo test --workspace --locked` → `bt-app --selftest` (with `SLINT_BACKEND=winit-software`) → `clippy` → `fmt --check`. **The M6 differential step was removed on 2026-11-27** along with the Node reference it compared against.
- **`tauri-build.yml`** — Tauri only, validation. Runs on push to `feat/tauri` and on PRs; builds but never releases.
- **`release.yml`** (`Release (Rust + Tauri)`) — runs on `v*` tags. Three jobs: `rust` (the Rust mainline, from the trigger ref), `tauri` (explicitly `feat/tauri`), then `publish`. **The `electron` job was deleted on 2026-11-27.** ⚠️ **`publish` is gated on `startsWith(github.ref, 'refs/tags/')`** — a manual `workflow_dispatch` is a **dry run**, otherwise it would create a public Release literally named after the branch (`BT 聚合搜索 main`). Published **directly, `draft: false`**, appending `docs/RELEASE_ARTIFACTS.md` as the body. Artifact names carry an explicit tag so they can't be confused: **`BT-Search-Rust-<ver>.exe`**, **`BT-Search-Rust-<ver>.zip`**, **`BT-Search-Tauri-Setup-<ver>.exe`** (the Electron assets were removed on 2026-11-27). ⚠️ **Version-bump hazard** — the version in those names comes from **two different branches**: `main`'s `Cargo.toml` (Rust exe, via `GITHUB_REF_NAME` minus the `v`) and **`feat/tauri`'s `src-tauri/tauri.conf.json`** (`release.yml` reads it with `node -p "require('./src-tauri/tauri.conf.json').version"`; ⚠️ **NOT** that branch's `package.json`). Bump **both sides** (4 files on `feat/tauri`: `tauri.conf.json` / `src-tauri/Cargo.toml` / `src-tauri/Cargo.lock` / `package.json`) or a Tauri installer ships named after the *previous* version — **v0.3.6 + v0.3.7 both did exactly that**. `scripts/release-check.js` machine-checks all four.

**Probes** (`workflow_dispatch` only, **deliberately not gates** — see Q5: allow-failure means "not a gate", not `continue-on-error`):

- **`live-smoke.yml`** — hits the real sites with `BT_LIVE_SMOKE=1` and prints `[smoke]` lines. Trigger with `gh workflow run live-smoke.yml --ref main`.
- ~~`html-probes.yml` / `provider-health.yml` / `m6-diff.yml`~~ — **all three were deleted on 2026-11-27** with the Node implementation they depended on. `test/fixtures/html-probes.expected.json` is now read-only: the cheerio oracle that regenerated it is gone. `fetch-fixture.yml` (pure `curl`) survived and is still the way to refresh a fixture.
- **`rust-size.yml`** — **S1/S2 probe**: builds the release exe in **two CRT modes** (static = the S2-compliant form, dynamic = isolates the `lto`/`strip` win) and asserts the DLL dependency list contains **no MSVC runtime** (`^(vcruntime|msvcp|concrt|api-ms-win-crt)` → `::error`). Not a gate; S2's real criterion is "copy to a clean Windows and double-click", which only a human can do.

### Rust rewrite (**on `main` since 2026-10-16**)

Since 2026-10-08 the project is being **rewritten from scratch in Rust**. ⚠️ **`main` is now the Rust mainline** (the `feat/rust` branch is fully merged and retained only as a pointer). M0–M7 are **all done**, and **D18 is closed**: the Node side was deleted from `main` on 2026-11-27 (see the top of this file). `test/fixtures/` is kept forever.

**Scope decisions.** ⚠️ **Re-scoped 2026-10-11** — the full register now lives in `docs/ROADMAP.md` §5 (D-numbers). The Q-numbers below are the 2026-10-08 set; **Q2 and Q6 are superseded**.

| # | Decision |
|---|---|
| Q1 | Desktop-native only. **Browser/mobile access is dropped** (= D2). |
| ~~Q2~~ | ~~Don't touch the UI yet — reuse `public/`~~ ❌ **superseded**: go **straight to Slint** (D8). |
| Q3 | Windows-only for now, but don't hardcode platform specifics. |
| Q4 | **Switch `main` when the aggregation layer runs** (gate = 聚合层跑通, D5). Not "40 engines done", and the diff harness is *not* a hard gate. |
| Q5 | CI runs **offline fixtures only** (the stable gate). A manual `workflow_dispatch` online smoke test exists (`live-smoke.yml`) and is deliberately **off** every push/PR gate — allow-failure means "not a gate", not `continue-on-error`. Fixtures are a 2026-07 snapshot, so green does **not** mean scraping still works. |
| ~~Q6~~ | ~~WebView ↔ Rust transport~~ ❌ **superseded** — no WebView, so no transport question (D9). |

**New (2026-10-11):** D7 goal = the small/fast/native **artifact** (search is the vehicle) · D8 go straight to Slint · D9 the core is a **library** (no HTTP, no SSE) · D13 probe engine liveness **before** porting more · D16 all four download clients · D17 Torznab deferred (leave an abstraction hook only) · ~~D18 keep the Node version until M6~~ ✅ **执行完毕 2026-11-27**（Node/Electron 已从 `main` 删除，`feat/tauri` 上另有一份）。

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
                        (`add_dynamic_factory` / `rebuild_dynamic_factories`).
                        ✅ The UI to add/clear indexers is **in** (`app.slint` 的「索引器」行 +
                        `bt-app` 的 `add_indexer` / `clear_indexers`); config lives in
                        `%APPDATA%\bt-search\indexers.json`.
```

**Adding / fixing a provider (recipe):** copy a sibling such as `src/tpb.rs` / `src/knaben.rs`, write `search()` + `search_at(http, api, query)` (the latter is what makes offline tests possible), then `tests/<name>.rs` with field-by-field assertions. **The old JS is gone from the working tree but still readable in git history** — e.g. `git show 012f5db^:src/providers/<name>.js` (same for `src/lib/normalize.js`). That is also where the historical expected values came from; when you change a parsed field, pin intentional behaviour changes as a named test.

Known pitfalls when guessing expected values (all three have already cost a CI round): `dateText` defaults to `"—"`, not `""`; JS treats `0` as falsy, so `x ? Number(x) : null` means `created_unix: 0` yields no date (use `value::v2nt_nonzero`, but note the *string* `"0"` is truthy); array fields like `categoryId` take the **min**, not the first element.

**The provider line is deferred (2026-10-11, D13).** 16 ports are done (5 JSON + 11 HTML). Do **not** keep porting by default — the goal is the artifact, not engine count. ⚠️ `provider-health.yml` (the engine-liveness probe) was **deleted with the Node implementation** on 2026-11-27 — the only liveness signal left is `live-smoke.yml` plus actually running the app. `xxxtracker` / `dmhy` / `sukebei` / `nekobt` remain unported but are **not** the next step (that is **M1**, the Slint risk test). Full candidate table and per-provider progress live in `AGENTS.md`.

**HTML providers (worked path, from `linuxtracker`):** parse with `bt_core::dom` (the cheerio-compatible layer over `scraper`). **Don't guess a page structure from the HTML — read the frozen fixture** (`test/fixtures/<site>-*.html`), and cross-check the selectors against `test/fixtures/html-probes.expected.json` (the cheerio ground truth; ⚠️ the workflow that regenerated it was deleted 2026-11-27, so it is now read-only). Two things bit us there: cheerio's `.map()` flattens arrays (use `.toArray().map()`), and bare `<tr>`/`<td>` snippets are dropped by the HTML parser (wrap them in `<table>`).

`runMirrors` (`src/lib/mirrors.js`) does **not** mean "retry on error": it fires all mirrors in parallel and takes the first with **non-empty results**; otherwise the error becomes `"<name> unreachable (<err1>; <err2>)"`. So "page loaded but no hits" counts as an **error**.

Upstream bugs found while porting: `linuxtracker`'s `detailUrl` misses a slash (`https://linuxtracker.orgindex.php?...` — a dead link; **fixed** here, pinned as a divergence test); its `parseEuDate` builds local-midnight dates while `dateText` formats in UTC, so results show one day early east of UTC (**copied as-is**); expanded description rows land their size/seeders in the wrong columns (**copied as-is**). `megapeer` is the other *fixed* case: axios hard-decodes text as utf8 and mangles the site's windows-1251 Cyrillic, while reqwest honours the charset header — a data-level bug, so Rust deliberately differs (pinned as a divergence test). Policy: fix functional bugs, copy cosmetic ones; the full table is in `AGENTS.md`.

**The old JS is fully deleted (2026-11-27).** The former "function-level, never file-level" rule (D18) is **closed** — the whole Node tree went at once, with the user's explicit sign-off, because retiring Electron removed the last reason to keep a reference around. The 待删清单 in `AGENTS.md` is now a historical record; if you need a deleted file, read it from git (`git show 012f5db^:<path>`).

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


**Version tags increment forward — never re-tag the same version.** Bump the version in **all four places on `main`** (`Cargo.toml`, `Cargo.lock`'s 5 `bt-*` entries, `package.json`, `package-lock.json`*(gone since 2026-11-27)*) **plus the four on `feat/tauri`**, run `node scripts/release-check.js`, then tag the *next* number (`v0.0.2`, `v0.1.0`, …). Only reuse a tag when the user explicitly says to overwrite a specific version. Rationale: same-tag re-release means moving a published tag, and `softprops` **appends** assets to the existing Release rather than replacing them (leaving stale files behind) — so `release.yml`'s publish job first `gh release delete <tag> --yes` (keeping the tag) before re-creating, but forward-incrementing avoids the whole hazard.

`feat/tauri`'s `src-tauri/Cargo.lock` is committed for reproducible Windows builds. See "The one remaining Node shell" section and `docs/RELEASE_ARTIFACTS.md` for how the resulting installers differ.

## Conventions & gotchas

- Build caches are redirected to project-local `.cache/` (git-ignored) to avoid polluting `%LOCALAPPDATA%`.
- `data/` (Torznab configs with API keys) and `dist/` (large build artifacts) are git-ignored.
- Scraper providers depend on target-site HTML structure; when a site redesigns, its provider's selectors need updating (the status bar shows per-provider ✓/✕ so breakage is visible).
- **Test scrapers from the agent's sandbox, not the user's machine.** The user's local Windows box has DNS pollution for many BT sites (Facebook's blackhole IPv6 `2a03:2880:face:b00c` hijacks some domains), so a provider that fails from their terminal may work from the agent environment and vice-versa. Verify provider reachability from the agent runtime; **do not** ask the user to share their network or tunnel traffic. The ✓/✕ status reflects real reachability at request time.
- **Russian-site providers (rutor, etc.)**: serve **UTF-8**, not windows-1251 despite what upstream may imply. Match table cells by *content* (size regex, seeder/leecher `<span>`s), never by column index — these sites inject extra columns (e.g. a comments cell) that shift indices. JS `\b` does not match Cyrillic characters, and a bare unit letter like `B` false-matches titles such as "Black Box"; require a full unit (`GB|MB|KB|TB|ГБ|МБ|КБ|ТБ`).
- `SEARCH_ENGINE_PORT_COVERAGE.md` tracks which upstream (prajwalch/TorrentSearch) engines have been ported.
- **Tauri validation** (legacy `feat/tauri` branch): `cargo check` in dev mode skips code behind `#[cfg(not(debug_assertions))]` — Tauri's release-only `setup` block — so only a release build actually exercises it. ⚠️ **This can no longer be done locally** (`cargo` is gone from this machine); it now only happens in `tauri-build.yml`. Also validate `tauri.conf.json` against `node_modules/@tauri-apps/cli/config.schema.json` with `ajv` before pushing (skip `pattern` keywords). The full Tauri v2 gotcha list lives in `AGENTS.md`.
