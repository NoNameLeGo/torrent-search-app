# Repo hygiene review — everything except `crates/`

Repo: `D:\Vibe-Coding\torrent-search-app`, branch `main`. Read-only pass; nothing modified.
Scope: `.github/workflows/` (12 files, not 13), `scripts/`, `server.js`, `src/`, `public/`, `electron/`, `test/`, `package.json`, docs.

Ranking within each group, most severe first.

## 1. Documentation drift

1. **[HIGH] `AGENTS.md:1765` — "当前计划 = …（下一个里程碑是 M1）".** M0–M7 are all recorded complete at `AGENTS.md:43` and `AGENTS.md:78`. An agent that starts reading from the bottom ("历史" section) is told M1 is next; the file contradicts itself 1700 lines apart. Smallest fix: change that line to "M0–M7 已完成（见上方主表）；当前为维护期".

2. **[HIGH] `AGENTS.md:660` — "⛳ 下次开工 = M2：聚合层 + `Provider` trait".** M2 landed in commit `b91546d` and `AGENTS.md:190` says so ("✅ M2 已完成 … CI 347 tests"). Same stale "next action" at `AGENTS.md:842` ("下次开工先拍") and `AGENTS.md:853` ("M2 代码未写"). Smallest fix: put the same "⚠️ 以下是开工前勘明，代码已按它写完" banner on this section that the neighbouring M2 block already has.

3. **[HIGH] `AGENTS.md:223` — "**还没做**：`bt-torznab` crate（解析 + provider 实现）".** False, and contradicted by `AGENTS.md:98` in the same file (crate exists with `roxmltree` parsing + `TorznabProvider` + `add_dynamic_factory`; only the UI add/remove screen is missing). A reader who lands on :223 will build the crate again.

4. **[MED] `docs/AGENT_HANDBOOK.md:125` and `:131` — says the `spike/slint-ui` branch and `slint-spike.yml` were deleted and "需要重建"/"重建一条 `slint-spike.yml`".** Both exist right now on `main` (`.github/workflows/slint-spike.yml`, branch `spike/slint-ui`). The handbook tells you to spend ~500 lines rebuilding an artifact that is in the tree. Smallest fix: replace with "已在 main 上，别重建".

5. **[MED] `docs/AGENT_HANDBOOK.md:262` and `:265` — references `scripts/diff-providers.mjs` and `cargo run -p bt-providers --example dump_provider`.** Neither exists (verified with `find`/`ls`: `scripts/` has no `diff-providers.mjs`; there is no `examples/` dir). The real M6 harness is `scripts/diff/*.cjs` + `crates/bt-providers/tests/dump_rust.rs`. This is the §6 "how to do M6" recipe, so the drift sends a reader down a dead path.

6. **[MED] Test counts presented as current.** `AGENTS.md:43` states "CI **397 passed / 0 failed**" as the progress snapshot and `.github/workflows/rust.yml:8` repeats "397 条测试" as the justification for a hard gate, while `AGENTS.md:724` says the file's own numbers are snapshots and the real 2026-10-16 count is 471. Smallest fix: make the workflow comment and the roadmap header point at "the latest `rust.yml` log" instead of a number.

## 2. CI reality

1. **[HIGH] Nothing in CI runs the Node test suite.** `package.json:21` defines `"test": "node test/run.js && node test/normalize.test.js"`, but `grep -rn "npm test\|node --test\|test/run.js\|normalize.test" .github/workflows/` returns zero hits. `test/run.js` covers tpb/knaben/linuxtracker/filemood, and the Node build is the one README calls the daily driver (`README.md:24`) — a parser regression in it can ship green. Smallest fix: add one step to `build.yml` (`run: npm test`) or a small `node-tests.yml`.

2. **[HIGH] `test/providers.test.js` (219 lines) is executed by nothing.** Not in `package.json:21`, no workflow mentions it, `test/README.md` doesn't list it (its "Test Structure" block names only `run.js` and `normalize.test.js`). It is a `node --test` suite that reads real fixtures — it looks like coverage and provides none. Smallest fix: either add it to the `test` script or delete the file.

3. **[MED] The "体积回归门" is not a gate.** `AGENTS.md:96` lists "体积回归门（25MB，`rust-size.yml`）✅", but `.github/workflows/rust-size.yml:10` says of itself "只挂 `workflow_dispatch`，不参与任何 push/PR 门". Nothing blocks a fat dependency from merging. Same class of stale instruction: `fetch-fixture.yml:13` and `provider-health.yml` still hand out `--ref feat/rust`, and `provider-health.yml` uses `actions/checkout@v4` while all 11 other workflows moved to `@v5` — `main` is the Rust mainline now, so the ref advice is wrong. Smallest fix: either give `rust-size.yml` a `push: branches: [main]` trigger or stop calling it a gate at `AGENTS.md:96`.

4. **[MED] `scripts/check-workflows.py:91-92` cannot catch the failure it exists for.** It checks size >= 200 bytes, YAML parses, `on:`/`jobs:` exist, `runs-on`/`uses` present — but it never checks that scripts referenced from a workflow actually exist, which is exactly the breakage recorded in `docs/CI_FAILURES.md` (#1) that left `provider-health.yml` / `html-probes.yml` calling missing `scripts/*.mjs` on `main`. Smallest fix: extract `scripts/...` paths from each workflow body and assert `os.path.exists`.

## 3. Dead weight with a real removal date

1. **[MED] `package.json:17-20` — four scripts that cannot run on `main`.** `tauri` / `dev:tauri` / `build:tauri` point at `src-tauri/` (verified absent: `ls src-tauri` → No such file or directory), `build:sidecar` points at `scripts/prepare-sidecar.mjs` (absent). `@tauri-apps/cli` (`package.json:61`) is therefore installed on every `npm ci` for nothing — on a machine that `AGENTS.md:20` documents as being short of disk. These belong to `feat/tauri`. Smallest fix: drop the four scripts + the dep from `main`.

2. **[MED] `package.json:24` `build:frontend` produces a bundle nothing loads.** `scripts/build-frontend.js` writes `public/dist/`, but `public/index.html:301-302` loads only `ambient.js` and `js/main.js`, `build.yml:28-49` never calls it, and electron-builder has no hook for it. So the "esbuild minify → `public/dist/`" step recorded as done in `AGENTS.md` ships nothing. Smallest fix: wire `index.html` to `dist/` or delete the script + the claim.  Inert for the same reason: `.env.example` documents `PORT=3000`, and `server.js:13` does read `process.env.PORT`, but nothing loads the file (no dotenv dependency, no --env-file in any script), so copying it changes nothing.

## 4. Node-side defects that still ship

1. **[HIGH] `server.js:118` (also `:141`, `:205`, `:212`, `:249`) — `async` route handlers with no `try/catch`, on Express 4 (`package.json:31`).** Express 4 does not forward rejected promises to the error handler, and `package.json:28-30` declares `node >=22.12.0`, where an unhandled rejection terminates the process — so any escaped throw becomes "Electron app dies with no dialog" instead of a 500. `resolveMagnet` implementations are documented never to throw, so this is a latent path rather than a known repro, but `/api/magnet` is the one endpoint whose whole body is a stray `await`. Smallest fix: a 5-line `const wrap = (fn) => (req,res,next) => Promise.resolve(fn(req,res,next)).catch(next)` applied to those five routes.

2. **[MED] `electron/main.js:39-41` — `shell.openExternal(url)` with no scheme allowlist.** The window-open handler forwards whatever URL the renderer asks for, and those URLs are scraped from third-party pages: `public/js/actions.js:112` builds an `href` from `esc(s.detailUrl)`. `esc()` escapes quotes and angle brackets but not a `javascript:` or `file:` scheme, so a malicious listing or user-added indexer can make the app hand an arbitrary URI to the OS handler. Electron's own guidance is to allowlist. Smallest fix: `if (!/^https?:\/\//i.test(url)) return { action: 'deny' };` inside the handler.

3. **[MED] `server.js:18-27` — `safeHttpUrl` only checks the scheme, so `/api/magnet:118`, `/api/details:141`, `/api/torznab/test:249`, `/api/download/*` are open SSRF / port-scan helpers for anything that can reach the port.** The Host middleware (`server.js:38-42`) validates the *incoming* Host, not the *target*, so `?url=http://192.168.1.1/` or `http://127.0.0.1:6379/` is fetched (and `/api/details` echoes back `og:description` / `title` from it). The 127.0.0.1 bind is the only real mitigation, i.e. it does not protect a shared Windows machine. This is a documented design decision, not an oversight — but it is still unfixed, and AGENTS.md itself already proposes the fix. Smallest fix: for `/api/magnet` + `/api/details` restrict the target host to the requesting provider's own domain.

4. **[LOW] `public/js/render.js:216-217` and `:262-263` interpolate `it.sizeText` / `it.dateText` into `innerHTML` unescaped, while `public/js/actions.js:131-132` escapes the same two fields.** Not reachable today — `src/lib/normalize.js:142,146` always recomputes both from numbers — but the two card renderers are exactly where a future provider-supplied text field would land, and the inconsistency means the next reader cannot tell intent from accident. Smallest fix: wrap both in `esc(...)`.

5. **[LOW] No Express error handler, so stack traces and absolute paths leak to the client.** With `NODE_ENV` unset (the normal Electron case) Express's default handler returns an HTML page containing the stack. Any sync throw in a route (for example a failing `/api/torznab` write) exposes filesystem layout to any local caller. Smallest fix: one terminal `app.use((err, req, res, next) => res.status(500).json({ error: 'internal_error' }))`.

## 5. Security & repo hygiene

1. **[LOW] No `Origin` / `Referer` check, despite the DNS-rebinding claim at `server.js:33-37`.** A page in the user's browser can issue cross-origin requests to `http://127.0.0.1:3000/api/...` and the browser sends `Host: 127.0.0.1`, which passes `server.js:38-42`. Reads are unreadable (no CORS headers are ever sent) and JSON-bodied writes need a preflight the server never approves, so the residual impact is fire-and-forget GETs — notably `/api/download/detect:205`, which makes the server probe a fixed set of local ports. Smallest fix: reject requests carrying an `Origin` header that is not `http://127.0.0.1:<port>` or `http://localhost:<port>`.

## Checked and looks fine

- **No secrets or personal paths in git.** `git ls-files | grep -iE '\.env|secret|token|\.key$|\.pem$'` returns only `.env.example`; `git grep -n "C:\Users\LeGo"` returns 0 hits. `data/torznab.json` (plaintext API keys) is gitignored and untracked.
- **Workflow permissions are minimal where they can be.** `ci-guard.yml:27-28`, `m6-diff.yml:19-20`, `live-smoke.yml`, `rust-size.yml`, `provider-health.yml` and `tauri-build.yml:11-12` are all `contents: read`; the write grants (`build.yml:10-11`, `release.yml:15-16`, `html-probes.yml:26-27`, `fetch-fixture.yml:25-26`) are actually needed for publish / push-back, both push-back jobs are `workflow_dispatch`-only, and `fetch-fixture.yml:37-47` validates its `path` input to `test/fixtures/` with no `..`.
- **The SSE cancel path is correct.** `server.js:88-89` records `req.on('close')` and every write (`:92`, `:104`, `:106`) is guarded by `if (!closed)`, so the frontend's cancel button cannot cause a write-after-end crash or a pile of unhandled errors.
- **`safeHttpUrl` is applied consistently** to every outbound route (`server.js:121,144,171,236,252`), and `torznabStore` swallows all read/write failures, so a corrupt `data/torznab.json` degrades instead of throwing.
- **Engine counts in the docs match the code**: 41 `REGISTRY` entries at `src/providers/index.js:79-121` = 40 engines + offline demo, versus "17 家" for the Rust build.
- **The M6 claim matches `rust.yml:89-109`** — no `--json-only` / `--only` remains, so all 16 providers are compared on every push to `main`; the only stale text there is the "397" comment at `rust.yml:8`.
