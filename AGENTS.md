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

## Candidate features (borrowed from upstream `prajwalch/TorrentSearch`)

Compared against the upstream Android app. Our search aggregation, multi-client
download push (qB/TR/aria2/Gopeed), batch ops, and CSV export already exceed it.

**Done:** Category system — every result is normalized into one of a few standard
buckets (`movies`/`series`/`anime`/`games`/`apps`/`books`/`music`/`porn`/`other`) by
`normalizeCategory()` in `public/app.js`: provider-supplied `category` (透传 via
`normalize.js`) wins, else a high-confidence title-based inference (`categoryFromTitle`)
fills the gap. The UI renders a category-filter chip row (`renderCategoryFilters`) built
from the buckets actually present in the current results. This is *orthogonal* to engine
grouping (source dimension) — category is the *content* dimension.

The remaining gaps worth closing, ranked by ROI:

1. **Safe Mode** — one toggle that auto-disables NSFW providers (we already have an
   "adult" group: Sukebei/XXXClub/…) and hides NSFW results. Cheap: a localStorage
   flag wired into engine selection + result filtering. High value for demo/家用.
   The `porn` category bucket is already in place to feed the result-hiding half.
2. **Viewed / dead-torrent filtering** — dead-torrent (seeders=0) filtering mostly
   exists via the min-seeders filter. Missing: mark "already viewed" results (opened
   details / copied magnet) as dimmed. Store viewed `infoHash` set in localStorage.
3. **Browse (top/latest)** — browse trending/latest without a query. Needs providers
   to support query-less top/latest fetching (not every site has this) — higher cost,
   mid-term, start with the few engines that support it.
4. **Bookmarks export/import** — we already persist favorites in localStorage; upstream
   adds export-to-file / import. Natural for a desktop app; guards against cache clears.
5. **Richer details (poster/screenshots/description)** — upstream detail screen has
   media poster, screenshot previews, Markdown description. Depends on each site's
   detail-page structure — lower ROI, nice-to-have.

Suggested order: **Safe Mode + viewed filtering** next — both pure frontend, low risk,
and Safe Mode is interrelated (the `porn` category feeds its result-hiding half).

## Syncing features between `main` and `feat/tauri`

The two branches are maintained in parallel: same commit messages, different hashes. Do **not** bulk cherry-pick the whole `feat/tauri..main` range — most of those commits are the parallel twins and would apply duplicate changes. Cherry-pick only the genuinely new commit(s).

**Workflow that avoids losing commits:** do the cherry-pick in a temporary worktree, then **push to the remote *before* removing the worktree**. If you delete the worktree first, the cherry-picked commit is unreachable (the branch ref never pointed at it) and gets garbage-collected — the sync silently vanishes.

```bash
git worktree add <tmp> feat/tauri
# cd into <tmp>, cherry-pick, resolve conflicts, commit
git push origin feat/tauri     # push FIRST
git worktree remove <tmp>      # clean up AFTER push confirmed
```

**Known recurring conflict — `PROVIDER_LABEL` / `loadProviders` in `public/app.js`:** the two branches diverge here.
- On `main`, `PROVIDER_LABEL` is a static literal with only ~4 entries.
- On `feat/tauri`, `PROVIDER_LABEL = {}` (empty) and `loadProviders` populates it dynamically with `providers.forEach((p) => { PROVIDER_LABEL[p.id] = p.name; });` — this is what gives every engine a real display name in badges/status bar.

When a synced feature touches `loadProviders`, resolve the conflict by taking the incoming (`main`) logic **but keeping the dynamic-fill line**. Dropping it makes all badges on `feat/tauri` degrade to raw provider ids.
