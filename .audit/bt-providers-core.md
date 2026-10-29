# Review — provider / aggregation core (`crates/bt-providers`, `crates/bt-core`)

Scope: `aggregate.rs`, `lib.rs`, `registry.rs`, `value.rs`, `bt-core/src/{normalize,http,dom}.rs`,
plus `tests/aggregate.rs` / `src/providers/index.js` / `test/normalize.test.js` as intended-semantics.
Method: **static reading only** (no cargo on this box) — every claim below was checked against the
line it cites and, where relevant, against the Node reference side by side.

**No HIGH finding.** The tri-state `has_more` truth table, the 8-wide pool, and the `never returns Err`
HTTP contract are faithful to the Node reference and are pinned by tests. The five items below are
the real defects I could substantiate; two of them are genuine silent divergences from `src/lib/*.js`.

---

## Findings (worst first)

### 1. [MED] `crates/bt-providers/src/aggregate.rs:166` — `catch_unwind` does not cover the `Provider::search()` call itself

```rust
let out = match AssertUnwindSafe(p.search(http, query, page))   // ← called OUTSIDE the guard
    .catch_unwind()
    .await
```
`p.search(..)` is evaluated first and only the returned future is wrapped, so a panic in the
**synchronous** part of a provider's `search()` aborts the whole aggregate (all other providers'
already-collected results die with it) — the exact failure mode `run_provider` exists to prevent,
and the module doc at `:29` states this is now "兜住". The panic test in `tests/aggregate.rs:641`
deliberately keeps the panic inside a polled future (`"不能在调用 search() 时就炸"`), so it passes
either way and no test covers the sync path.
Unreachable for the 16 built-ins (each `search` is just `Box::pin(crate::x::search(..))`, which cannot
panic before returning), but `Provider` is a public extension point and the planned Torznab path
constructs state per search. Smallest fix — move the call inside the guarded future:
`AssertUnwindSafe(async { p.search(http, query, page).await }).catch_unwind().await`.

### 2. [MED] `crates/bt-core/src/normalize.rs:422` — `Сегодня` / `Вчера` replaced case-sensitively, JS uses `/i`

```rust
let s = s.replace("Сегодня", "Today").replace("Вчера", "Yesterday");
```
Node: `s.replace(/Сегодня/i,'Today').replace(/Вчера/i,'Yesterday')` (`src/lib/normalize.js:116`, the
neighbouring `Мая` case is `(?i)` in Rust too). A lowercase date cell ("сегодня 14:22") stays
Cyrillic, so neither `CYR_WORD_RE` nor `parse_absolute_date` can match and `parse_date` returns
`None` → `dateText` `"—"`, where JS lands on local midnight. This is a **silent, unpinned
divergence**: `crates/bt-core/tests/normalize.rs:243` and `test/normalize.test.js:120` only ever feed
the capitalised form. Reachability caveat: no current fixture contains either spelling
(`grep -c 'сегодня|Сегодня' test/fixtures/rutor-ubuntu.html` = 0), so it is latent for the 16 shipped
providers — but `ru_date` is applied to raw site text by `rutor`/`megapeer`, and this is the only
branch JS treats case-insensitively. Smallest fix: two `(?i)`-anchored `Regex` replaces like `MAY_RE`.

### 3. [LOW] `crates/bt-providers/src/value.rs:12` — `v2nt` drops every JSON number that is not an `i64`, and every JS-truthy container

`Value::Number(n) => n.as_i64().map(NumOrText::Num)` returns `None` for `12.0`, `1e3` or anything
outside `i64`, while JS's `typeof x === 'number'` keeps any JS number; `true` / `[]` / `{}` likewise
become `None` where JS truthiness plus `Number()` coerces (`true`→1, `[]`→0). Trigger: a JSON API
emitting a float or exponent count (e.g. `"seeders": 12.0`) — `seeders` silently shows as `—` where
Node shows 12. `min_number` in the same file (`:44`) already falls back with
`.or_else(|| n.as_f64().map(|f| f as i64))`, so the two helpers disagree with each other. Smallest
fix: add the same fallback to `v2nt`.

### 4. [LOW] `crates/bt-providers/src/aggregate.rs:269` — duplicate provider ids make `has_more` read the wrong status

`per_provider.iter().find(|(id, _)| id == p.id())` returns the **first** settled entry for an id,
whereas the Node reference (`src/providers/index.js:190-198`) keys an object by id and therefore keeps
the **last** settled one. Both are wrong under a collision, but differently — and ids become
user-controlled once Torznab indexers are added through the UI (`indexers.json`), where a colliding id
is one typo away. Smallest fix: `debug_assert!`/reject duplicate ids in `resolve_targets`, or carry
the status in the tuple that `aggregate_has_more` already iterates instead of re-looking-it-up by
string.

### 5. [LOW] `crates/bt-core/src/http.rs:158` — `.expect("reqwest::Client 构建失败")` turns a TLS-init failure into "crash on first search"

`HttpClient::new()` is reached lazily via `default_client()`'s `OnceLock` (`:350`), i.e. at the first
user search, not at startup as the comment ("启动期硬故障") claims. On a machine whose root store /
schannel init fails, the user gets an abort the instant they press Search, and every later attempt is
the same poisoned `OnceLock`. Smallest fix: call `default_client()` once during app startup so it
fails early and visibly (or store `Result<HttpClient, String>` and return the error per request).

---

## Checked and looks fine

- **`has_more` tri-state truth table** (`aggregate.rs:264-285`): the `status != Ok || count == 0`
  short-circuit and `Some(explicit) => explicit / None => paginated()` match
  `index.js:190-198` line for line; `ProviderStatus.has_more: Option<bool>` is set from
  `SearchOutcome.has_more` with no lossy `unwrap_or` anywhere on the path. Panic/error rows cannot
  produce `Some(true)`.
- **Pool semantics** (`aggregate.rs:216-244`): `limit.max(1)` guards the `limit == 0` silent-drop,
  `on_settle` runs **before** the next task is created, and in-flight count stays ≤ `MAX_CONCURRENT = 8`
  — both properties have named tests in `tests/aggregate.rs` (order + single-settle event log).
- **`format_size`** (`normalize.rs:158-185`): the known Rust-vs-JS tie bug is actually fixed
  (`(v * 10.0).round() / 10.0`, and `v.round()` for `v >= 100 || i == 0`, matching `toFixed(v>=100||i===0?0:1)`);
  the unit table mirrors `SIZE_UNITS` exactly (`"GBs"` is rejected by JS too). Residual double-rounding
  risk is theoretical (needs the real product to sit inside a ~1e-16 window below `x.5`).
- **Byte indexing**: `js_parse_float` (`normalize.rs:110`) and `to_int` (`:440`) only advance `i` over
  ASCII digits/`.`/sign and call `s[..i]` / `t[start..i]` after early-returning when no digit was seen
  → no multi-byte-boundary panic. `parse_named_month_date` uses `l.get(0..3)?`, which is boundary-safe.
- **`http.rs`**: non-2xx and redirect-policy errors are mapped to errors, never to `html: Some(..)`;
  10 s total timeout, per-request UA rotation, gzip/brotli/deflate via reqwest features with **no**
  hand-written `Accept-Encoding` header that could defeat auto-decompression; `classify_error` is a
  pure function with a contract test.
- **`dom.rs`**: selector-parse failure yields empty instead of panicking, `.text()` concatenation /
  entity decoding / class-case-sensitivity match cheerio, and `closest_tag` walks self-inclusive with
  `None` (not panic) on miss — all pinned by the 126-probe suite plus inline tests.
- Documented-and-pinned divergences are **not** listed above: sequential `run_mirrors`, JSON-parse
  failure becoming an error, the absolute-date whitelist, `"a day ago"`, and the deprecated
  `run_mirrors` `has_more` drop (which matches JS, including the empty `"<name> unreachable ()"`).
