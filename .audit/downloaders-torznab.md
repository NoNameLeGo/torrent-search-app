# Audit — bt-downloaders + bt-torznab

Read-only static review. No cargo on this machine ⇒ nothing was built or run.
Reference for the downloader side is `src/lib/downloaders.js` (the ported-from source).

Ranked, most severe first.

---

## 1. [HIGH] crates/bt-downloaders/src/qbittorrent.rs:79 — HTTP 200 + body `Fails.` is reported as a successful push

`push()` only checks `!resp.status().is_success()` before returning `Ok`. qBittorrent's
`/api/v2/torrents/add` answers **HTTP 200 with the literal body `Fails.`** whenever the add
is rejected (invalid magnet, duplicate torrent, `/savepath` missing, …). So the UI at
crates/bt-app/src/main.rs:669 prints `已推送到 qbittorrent（HTTP 200）` and marks the row `Done`
while nothing was added — the exact silent-failure this review was asked to hunt for.

Trigger: push the same magnet twice, or push a magnet whose `dn` qB rejects. Second push
reports success.

Smallest fix: capture the body (`let body = resp.text().await.map_err(net_err)?;`) and
`if body.trim().eq_ignore_ascii_case("fails.") { return Err("qBittorrent 拒绝了这个磁力（Fails.）".into()); }`.
(`src/lib/downloaders.js:59-63` has the same gap — both sides need it.)

## 2. [HIGH] crates/bt-torznab/src/provider.rs:239 — a Torznab `<error>` response is indistinguishable from "no results"

`parse_items` (crates/bt-torznab/src/lib.rs) returns `Ok(vec![])` for any well-formed RSS
with zero `<item>`s, and `search()` maps that to `SearchOutcome::ok(empty)`. Torznab/Jackett/
Prowlarr signal failure as **HTTP 200** plus
`<error code="100" description="Invalid API key"/>`. Result: an indexer with a wrong/absent
API key shows up in the UI as `✓ 0 结果`, identical to a genuine empty search.

Aggravated by two adjacent facts: `add_indexer` (crates/bt-app/src/main.rs:812) does not
extract `apikey` from a pasted Jackett feed URL (`normalize_api_url` deliberately drops the
query — lib.rs provider.rs:76-80), and there is no `t=caps` probe to catch it either. The
most common setup mistake therefore presents as "configured fine, finds nothing".

Smallest fix: in `parse_items`, after collecting items, if the list is empty and the root has
an `<error>` child, return `Err(description)` (falling back to `code <n>`).

## 3. [HIGH] crates/bt-torznab/src/provider.rs:86 — byte-slicing panic on a URI that ends in non-ASCII

```rust
let ends_with_api = u.len() >= 4 && u[u.len() - 4..].eq_ignore_ascii_case("/api");
```
`u` is `url.trim()` of a user-supplied string; `u.len() - 4` is a raw byte offset. A URL whose
tail is multi-byte (e.g. `http://localhost:9117/中文`) makes that index land on a UTF-8
continuation byte ⇒ **panic**. This is reachable: `add_indexer` only checks the *scheme*
(`safe_http_url`, crates/bt-app/src/main.rs:812) and then stores the raw string, so the URL
never gets normalized to percent-encoded form. `normalize_api_url` runs on every search, so
the panic recurs per search (the provider's `catch_unwind` turns it into a per-indexer
`crash`, but the indexer is then permanently unusable with no useful message).

Smallest fix: `u.get(u.len() - 4..).is_some_and(|t| t.eq_ignore_ascii_case("/api"))`.

## 4. [MED] crates/bt-app/src/main.rs:812 (and :741) — the allowlist validates but its result is thrown away

Both trust boundaries call `downloaders::safe_http_url(&url)` purely as a predicate:
`if downloaders::safe_http_url(&url).is_none() { … return; }`. The *normalized* URL the
function returns is discarded and the raw, untrimmed input is stored in the config
(`cfg.url = url.trim().to_string()` / `bt_torznab::IndexerConfig { url: url.trim()… }`).
So `safe_http_url`'s normalization contract is dead code on this path, and every downstream
`with_path` / `strip_trailing_slashes` / `normalize_api_url` ends up re-parsing an unvalidated
string. It is also what makes finding 3 reachable.

Smallest fix: `let Some(url) = downloaders::safe_http_url(&url) else { …status…; return; };`
and store the returned value.

## 5. [MED] crates/bt-torznab/src/provider.rs:162,170 — `Box::leak` now runs once per search, not once per config

The file header (lines 21-24) justifies the leak with "**泄漏是「每个配置索引器一次」** …
**这不是「每请求泄漏」**". That justification is stale: providers are no longer built once.
`rebuild_dynamic_factories` (crates/bt-app/src/main.rs) installs a closure that does
`Box::new(bt_torznab::TorznabProvider::new(cfg.clone()))`, and `provider_set()` constructs a
fresh set **on every search**. So each search leaks 2 `String`s (id + name) per indexer,
unbounded for the process lifetime. Small per iteration, but the documented ceiling is wrong.

Smallest fix: keep the leak but hoist it out of the hot path — have `rebuild_dynamic_factories`
leak `id`/`name` once per config and pass the `&'static str`s into `TorznabProvider::new`.
(Or take the header's own option ① and change `Provider::id()` to `&str`.)

## 6. [MED] crates/bt-torznab/src/lib.rs:164-170 — `btih()` finds the offset in a lowercased copy, then slices the original

```rust
let lower = magnet.to_lowercase();
let idx = lower.find("btih:")? + "btih:".len();
let hex: String = magnet[idx..]…
```
`to_lowercase` is not byte-length preserving for every char (U+212A KELVIN SIGN → `k` is
**3 bytes → 1**, U+0130 → 2 bytes → 3). The doc comment claims this is safe because
`xt=urn:btih:` is always at the front — it is not guaranteed: a malformed/hostile magnet from
an indexer (untrusted input) can put the `btih:` after a `dn=` full of case-changing chars.
Depending on the accumulated shift this either silently misses the hash or indexes into the
middle of a multi-byte char and panics (`magnet[idx..]` is not char-boundary checked).

Smallest fix: search a byte window of the *original* string case-insensitively
(`magnet.as_bytes().windows(5).position(|w| w.eq_ignore_ascii_case(b"btih:"))`) and slice from
that offset. Deletes the wrong comment too.

## 7. [MED] crates/bt-app/src/main.rs:746 — an unrecognised downloader kind silently becomes qBittorrent

```rust
let kind = Kind::parse(&ui.get_downloader_kind()).unwrap_or(Kind::Qbittorrent);
```
If the dropdown ever yields a value `Kind::parse` doesn't know (label/`as_str` drift, a
hand-edited `config.json`), the user's credentials are sent to the qBittorrent login endpoint
of whatever host is configured. Failure is then reported as a qBittorrent login error, which
misdirects the user.

Smallest fix: on parse failure, set a status like `未知的下载器类型：<raw>` and return,
instead of defaulting.

## 8. [MED] crates/bt-app/src/main.rs:1145 — `load_indexers()` re-enters the search path without the scheme allowlist

The startup path reads `indexers.json` and feeds it straight to `rebuild_dynamic_factories`
with no `safe_http_url` check — only the UI's `add_indexer` validates. A hand-edited (or
externally written) `indexers.json` with `"url": "file:///C:/…"` or `"ftp://…"` therefore
reaches `bt_core::http`. `bt_core::http` / reqwest reject non-http(s) schemes, so the blast
radius is "indexer always errors" rather than SSRF, but the documented trust boundary is
inconsistent between the write path and the load path.

Smallest fix: filter the loaded list through `safe_http_url` (or `enabled: false` + a status
note) at config_store.rs:170.

## 9. [LOW] crates/bt-torznab/src/lib.rs:88-93 — `attr()` is case-sensitive and only scans direct children

```rust
item.children().filter(|c| c.is_element() && c.tag_name().name() == "attr")
    .find(|c| c.attribute("name") == Some(name))
```
JS (`src/providers/torznab.js:59-63`) lowercases both sides
(`($a.attr('name') || '').toLowerCase() === name.toLowerCase()`) and uses `$el.find(...)`,
i.e. **all descendants**. Our version misses `<torznab:attr name="Seeders" …>` (rare; the spec
says lowercase) and attrs nested one level deeper (e.g. wrapped in the item's `<torznab:…>`
container). Silent data loss: seeders/leechers/infohash/category become `None`.

Smallest fix: `.attribute("name").is_some_and(|n| n.eq_ignore_ascii_case(name))` and iterate
`item.descendants()` instead of `children()`.


## 10. [LOW] crates/bt-torznab/src/lib.rs:113 — `<size>` that is not a bare integer is dropped

`text("size").and_then(|s| parse_int(&s))`. The JS fallback assigns the *string*
(`size = $el.find('size').first().text().trim()`) and lets `normalize`'s `parseSize` handle
`"1.5 GB"` → a real size. Ours returns `None` for anything but leading digits, so an indexer
that emits a human-readable `<size>` (with no numeric `<enclosure length>`) shows `—` where the
Node build showed a size. (Enclosure-integral indexers — the normal case — are fine.)

Smallest fix: carry `size_text: Some(s)` alongside `size_bytes: None` so the UI has something
to render.

## 11. [LOW] crates/bt-app/src/main.rs:833 — two indexers with the same name collapse into one provider id

`add_indexer` always sets `id: String::new()`, so `TorznabProvider::new` derives
`torznab:<name>`; the same name is also the overwrite key
(`list.iter().position(|c| c.name == name)`). Adding "Prowlarr" twice with different URLs (a
legitimate thing to do for two Prowlarr instances) yields two configs sharing
`torznab:Prowlarr` — identical provider ids in the status list and in every result row's
`provider` label, so the two are indistinguishable and any future id-keyed logic silently
merges them.

Smallest fix: derive the id from `name` + the URL's host:port, or reject duplicates in the UI
with a clear message.

## 12. [LOW] crates/bt-downloaders/src/aria2.rs:70 — the push reports a fabricated `HTTP 200`

`Ok(PushOutcome { status: 200 })` is hardcoded (faithfully ported from JS), and
crates/bt-app/src/main.rs:670 renders it as `（HTTP 200）`. The real HTTP status is available on
the response but discarded, so the number shown to the user carries no information for
aria2/Motrix. Cosmetic, but it is the one line the user sees.

Smallest fix: `PushOutcome { status }` from the actual response, as gopeed.rs:70 already does.

## 13. [LOW] crates/bt-torznab — the missing `t=caps` probe compounds finding 2

Known gap, not re-reported as such. Noting only the coupling: with no caps probe there is no
second signal that an indexer is reachable-and-authenticated, so a bad key / non-Torznab URL
has exactly one symptom — "0 results" — which finding 2 already makes indistinguishable from a
genuine empty search. Fixing 2 (surface `<error>`) is the cheap half of this.

## 14. [LOW] crates/bt-downloaders/src/lib.rs:414 — `net_err` echoes the whole reqwest error, URL included

`format!("请求下载器失败 ({e})")` interpolates `reqwest::Error`, whose `Display` includes the
request URL, and that string is shown verbatim in the status bar (main.rs:676). Credentials
normally live in `Config.user`/`pass`/`token` and never appear in the URL, so the practical
impact is nil — but a user who inlines `http://user:pass@host` in the address box would see the
secret echoed back. Cheapest guard: strip a `user:pass@` userinfo segment before formatting.
(Verified: no path formats `Config`'s `Debug`, and there is no `println!`/`dbg!` in either crate.)

## 15. [LOW] crates/bt-torznab/src/lib.rs:80-86 — `text()` returns only the first text child

The helper uses `Node::text()`, which yields the element's first text node only. An item whose
`<title>` mixes text and CDATA (`<title>Foo<![CDATA[ & Bar]]></title>`) loses everything after
the first node — the title then looks plausible rather than empty, so the item is kept.
Real-world rarity: low (indexers normally emit a single node).

Smallest fix: collect over `n.descendants().filter_map(|d| d.text())` for the element.

---

## Checked and looks fine

- **qBittorrent SID / credential signal** — `header_values` walks *all* `set-cookie` headers for
  `SID=` (skipping `QBT_SID_<port>`), and the credential error is keyed on HTTP 403, never on
  body text matching `/fails|failed/i` (qbittorrent.rs:45-61).
- **Transmission 409 flow** — reads `x-transmission-session-id`, resends exactly once, treats
  409-without-header as an error, and additionally rejects `result != "success"`. `test()` is
  login-only (no side effects).
- **Explicit non-2xx checks** — all four clients check `!resp.status().is_success()` themselves
  (reqwest, unlike axios, does not throw); aria2's JSON-RPC `error` goes through `js_truthy`, so
  `0`/`""`/`false` are correctly *not* failures while an object with a `message` is.
- **URL allowlist** — `safe_http_url` accepts only `http`/`https` and is re-checked inside
  `push()`/`test()` via `ensure_http_url`; the magnet is percent-encoded with a hand-written
  `encode_uri_component` (byte-for-byte with JS `encodeURIComponent`) rather than `.form()`.
- **Secret handling** — no `Debug`/`println!`/`dbg!` of `Config` in either crate; error paths
  interpolate user/pass-free data only; `config_store::describe()` prints names/URLs only; the
  API key is never rendered into the UI status string.
- **Torznab parse branches** — enclosure-`url`-then-`link` magnet fallback, `infohash`-attr-then-
  magnet-regex, `peers`→`leechers`, `seeders=0` surviving as `Num(0)`, `comments`→`link`→`guid`
  detail fallback, and roxmltree-decoded `&amp;`/`&#NN;` — all present, in the documented order.
