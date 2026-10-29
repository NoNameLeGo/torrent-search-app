# bt-app code review (read-only, static — no cargo on this machine)

Scope: `crates/bt-app/src/{main.rs,ui_model.rs,config_store.rs,health.rs,selftest.rs}`, `crates/bt-app/ui/app.slint`, `crates/bt-app/Cargo.toml`.

## Findings (ranked)

1. `[HIGH] crates/bt-app/src/main.rs:429-485 (+ ui/app.slint:291) — a second search started mid-flight is not fenced off; the old stream keeps writing state`
   `LineEdit { accepted(t) => { root.search(t); } }` is not gated on `searching` (only the 搜索 Button and 下一页 are), so pressing Enter during a run calls `start_search` again; the first `async move` block still delivers `apply_report` and its completion closure, and neither carries any search-generation token — old results are appended into the freshly cleared `rows`, highlighted with the *new* query/accent, and the old completion runs `st.status = st.app.summary(false)` + `st.app.has_more = outcome.has_more`, so the status line reports "共 N 条 · 2 家成功 / 0 家失败" while most sites are still pending.
   Trigger: type `ubuntu`, Enter, then type `debian` and Enter before the first search finishes.
   Smallest fix: keep a monotonic `Cell<u64>` generation in `STATE`, capture it into both closures, and `return` early when it no longer matches the current generation (plus `enabled: !root.searching` on the LineEdit is not available — so the generation check is the real fix).

2. `[HIGH] crates/bt-app/src/main.rs:712-721 — refresh_row indexes the *view* model with a *rows* index, so with a category filter active the clicked card never updates (or gets another row's data)`
   `ResultRow.idx` is the position in `AppState.rows`, but `rows_model` holds `AppState::view()` (a filtered subset). `refresh_row` does `set_row_data(idx)` without translating, so after `pick_category` the optimistic "解析中…" / final "已推送 ✓" either no-ops (`idx >= row_count()`) or overwrites the card that happens to sit at that model index with a *different* result's row data.
   Trigger: click a 分类 chip, then click 「取磁力并推送」on any visible card — the button label never changes, and for small `idx` a neighbouring card is repainted with the wrong content.
   Smallest fix: `if st.app.category.is_none() { refresh_row } else { rebuild_rows_model(ui, st) }` (or translate `idx` → view position via `view().iter().position(|r| r.idx == idx)`).

3. `[HIGH] crates/bt-app/src/main.rs:663-676 — resolve/push results are written back by positional index, which a sort invalidates`
   `push_row` captures `plan.idx` and, seconds later (a network round trip for the 6 `needs_magnet` sites), calls `set_magnet(idx, magnet)` / `set_action(idx, …)`. `sort_rows` renumbers `idx` and `rebuild_rows_model` swaps the model, so the resolved magnet and the "已推送" label land on whatever result now occupies that position — a later copy/push of that row uses the wrong magnet.
   Trigger: click 「取磁力并推送」on a no-magnet row, then click any 排序 chip before the resolve returns.
   Smallest fix: carry a stable key (`TorrentResult.id`, already unused in `RowData`, or `title_plain + provider`) in `Plan` and look the row up by it on completion; or disable the sort chips while a resolve/push is in flight.

4. `[MED] crates/bt-app/src/main.rs:413-419 — an empty target set destroys the current results instead of being refused`
   `resolve_targets` on page > 1 keeps only `paginated()` sites; `begin_search(&targets)` has already cleared `rows` and reset the chips before `targets.is_empty()` is tested, so the early return leaves the user with "没有可用的站点" and an empty list. `toggle_site` never clears `has_more`, so the 下一页 button stays enabled after the paginated sites are unchecked.
   Trigger: search (a paginated provider reports `has_more`), uncheck limetorrents/rutor/nyaa, click 「下一页」→ page-1 results are gone.
   Smallest fix: move the `targets.is_empty()` check above the `STATE.with(begin_search …)` block (and/or clear `has_more` in `toggle_site`).

5. `[MED] crates/bt-app/src/main.rs:482 (+ src/health.rs:157-168) — the health log is appended, and the whole file re-read for trimming, on the UI thread after every search`
   The comment says "16 行的小追加，放 UI 线程上可接受", but `append` → `trim_if_needed` does `read_to_string` of the entire `health.jsonl` (up to `MAX_LINES` 5000 records) and, when over the limit, rewrites all of it — all inside `invoke_from_event_loop`.
   Trigger: `%APPDATA%` on a roaming/redirected profile (network share), or just a full 5000-line history; every search completion blocks the Slint event loop for that IO.
   Smallest fix: build `batch` outside the closure and call `health::append(&batch)` in the background `async` block right after `.await` (it already owns `outcome`), leaving only the UI mutation in the closure.

6. `[MED] crates/bt-app/src/selftest.rs:313 — `--selftest` persists its throwaway stub into the user's real config and health log`
   `save_downloader(&ui)` is the production path (it calls `config_store::save`), so running the self-test locally writes `http://127.0.0.1:<random port>` over the user's saved downloader URL in `%APPDATA%\bt-search\config.json`, and the fake `alpha/beta/gamma` reports appended at main.rs:482 pollute `--health`. No user-visible warning; the next normal launch shows a dead downloader address.
   Trigger: `cargo run -p bt-app --bin bt-search -- --selftest` on a machine that already has a configured downloader.
   Smallest fix: thread a `persist: bool` through `save_downloader` (selftest passes false) and skip `health::append` when `INJECTED` is set.

7. `[MED] crates/bt-app/src/main.rs:449-451 — the incremental push assumes the model is a 1:1 prefix of `rows`; a filter breaks that`
   `for r in st.app.rows.iter().skip(st.rows_model.row_count())` counts *authoritative* rows using the *view* length. With a category filter applied mid-search the model is a subset, so non-matching rows get pushed into the filtered list and matching ones are skipped; the same arithmetic also assumes sorted order equals arrival order.
   Trigger: click a 分类 chip while a search is still streaming.
   Smallest fix: push only `st.app.view()` entries not yet present (`if st.app.category.is_none()` → the cheap tail push, else `rebuild_rows_model`).

8. `[MED] crates/bt-app/ui/app.slint:310 — the status line that carries the #2 diagnosis is clipped to 230px`
   `Text { text: root.status-text; width: 230px; }` with no wrap/elide/tooltip; the messages built specifically so the user knows what to do ("一条都没有：16 家**全部失败** —— **网络层不通**…", "更像是站点改版了（选择器失效）") are far longer than 230px and are cut off. Same for `downloader-status` (:349, 180px) and `indexer-status` (:383, 240px), which carry write-failure notes.
   Trigger: any all-failed search — the actionable half of the sentence is invisible.
   Smallest fix: `horizontal-stretch: 1` on those `Text`s (the row has room) or give them a real wrap/elide policy.

9. `[LOW] crates/bt-app/src/main.rs:524-531 (+ ui/app.slint:172-186) — the comment claims the sort chips are greyed out while searching; they are not`
   `ChipRow`'s `Button` has no `enabled` binding, so `pick_sort` is reachable mid-stream and produces a sorted prefix with an arrival-order tail appended by main.rs:449. Beyond the cosmetic ordering, the comment tells the next maintainer a guard exists that does not.
   Smallest fix: either add `in property <bool> enabled-by-parent`/`enabled: false` wiring in `ChipRow` fed by `root.searching`, or delete the claim from both comments (main.rs:524, app.slint near the sort ListView).

10. `[LOW] crates/bt-app/src/config_store.rs:102 and :224 (+ src/health.rs:168) — user-facing files are written non-atomically`
   `std::fs::write` truncates in place; a crash/power loss mid-write leaves a partial `config.json` / `indexers.json`. Reads tolerate corruption ("treat as never configured"), so the cost is silent loss of the configured downloader/indexers — the exact failure the Node version was fixed for (`torznabStore.saveAll` → write-temp-then-rename).
   Trigger: kill the app during 保存 / 添加 while the file is being rewritten.
   Smallest fix: write `path.with_extension("tmp")` then `fs::rename` (about 3 lines each).

11. `[LOW] crates/bt-app/src/main.rs:334 (+ src/ui_model.rs:447, main.rs:1113) — the app can show 「第 0 页」 before any search`
   `AppState::default()` has `page: 0`, and the startup `sync_small` (reached whenever `config_store::load()` returns a saved config) pushes `set_page(0)`, while the `.slint` default is 1 — the pagination button literally reads "第 0 页 · 下一页".
   Smallest fix: make `page` default to 1 (manual `impl Default`) or skip `set_page` when `page == 0`.

12. `[LOW] crates/bt-app/src/ui_model.rs:674-720 (+ main.rs:330) — two different 「共 N 条」 numbers are on screen at once when a filter is active`
   `sync_small` sets `total` from `self.app.view().len()` (its comment says the two differ under a filter), but `summary()` computes `N = self.rows.len()`; the sidebar says "共 3 条" while the status line says "共 300 条", and the 「全部」 chip agrees with the sidebar. Both are defensible, but unlabelled side by side they read as a bug.
   Smallest fix: report both in the status line ("共 300 条 · 筛选后 3 条") or use the view count in both places.

## Checked and looks fine

- Threading contract holds: every background→UI mutation goes through `slint::invoke_from_event_loop`; closures capture only owned `Send` data + `Weak<AppWindow>` (`as_weak()`, never `clone_strong()`); `spawn` uses the process-wide `runtime()` so the "no reactor running" panic cannot return, and tokio `net` is present for the IO driver.
- No reachable `unwrap`/`expect`/`panic!` on the app path: `row()`/`set_action()`/`set_magnet()` are bounds-checked, `SortKey::parse` falls back gracefully, casts go through `idx < 0` guards or `clamp_i32`.
- `config_store`/`health` read paths degrade correctly (missing file, corrupt JSON, missing `%APPDATA%` → none/empty); `StoredIndexer.enabled` defaults to `true`; an unknown downloader kind is rejected, never guessed.
- `sort_rows` renumbers `idx`, keeps `None` last in both directions, and `categories`/`cat_chips` tie-break deterministically; `view()` is a filter and never mutates `rows`.
- Palette has one source (`Theme` global) with `Tone`/`SiteData::label` mapped in Rust; `.slint` has no `init` handlers and uses only M1-verified widgets (no `Button.color`).
- No leak across repeated searches: each search/sort replaces `rows_model`/`providers`/`sites` wholesale so the old `VecModel`s are released, and detached `spawn` tasks hold only owned data.
