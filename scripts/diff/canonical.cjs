'use strict';

// 差分用的**规范化形式**。
//
// 契约：两侧产出的 JSON 必须**可以逐字比较**，所以：
//   · 字段名统一用 **Rust 侧的 snake_case**（Node 的 camelCase 在这里转过来）；
//   · 15 个字段与 `crates/bt-core/src/normalize.rs` 的 `TorrentResult` 一一对应，
//     一个不多一个不少 —— 少一个就是"没比到"，多一个就是"比了不存在的东西"；
//   · **键顺序固定**（`FIELDS` 的顺序），于是 diff 可以按行看，也能整块 hash。
//
// ⚠️ 加字段时必须**同时**改 `FIELDS` 和 Rust 侧的 `TorrentResult`
//    —— 只改一边会让「少比一个字段」变成静默的假通过。

const FIELDS = [
  'id',
  'name',
  'size',
  'size_text',
  'seeders',
  'leechers',
  'date',
  'date_text',
  'magnet',
  'info_hash',
  'provider',
  'category',
  'detail_url',
  'files',
  'needs_magnet',
];

// 对应的 Rust 类型（写在注释里，方便对照）：
//   String / String / Option<i64> / String / Option<i64> / Option<i64> /
//   Option<i64> / String / Option<String> / Option<String> / String /
//   Option<String> / Option<String> / Option<i64> / bool

/** JS `undefined`/`null` → Rust `None`；其余 → `Some`。 */
function opt(v) {
  return v === undefined || v === null ? null : v;
}

/** 必填字符串：Rust 侧是 `String`（不是 Option），所以 `undefined` → `""`。 */
function str(v) {
  return v === undefined || v === null ? '' : String(v);
}

/** 可空字符串：空串也算 `None`（Rust 侧 `non_empty()` 就是这语义）。 */
function optStr(v) {
  const x = opt(v);
  return x === null || x === '' ? null : String(x);
}

/** 可空整数：非有限数一律 `None`（防止 `Number('abc')` 的 NaN 漏过去）。 */
function optInt(v) {
  const x = opt(v);
  if (x === null) return null;
  const n = Number(x);
  return Number.isFinite(n) ? n : null;
}

function canonResult(r, provider) {
  const row = {
    id: str(r.id),
    name: str(r.name),
    size: optInt(r.size),
    size_text: str(r.sizeText),
    seeders: optInt(r.seeders),
    leechers: optInt(r.leechers),
    date: optInt(r.date),
    date_text: str(r.dateText),
    magnet: optStr(r.magnet),
    info_hash: optStr(r.infoHash),
    provider: str(r.provider === undefined ? provider : r.provider),
    category: optStr(r.category),
    detail_url: optStr(r.detailUrl),
    files: optInt(r.files),
    needs_magnet: Boolean(r.needsMagnet),
  };

  const ordered = {};
  for (const k of FIELDS) ordered[k] = row[k];
  return ordered;
}

function canonOutcome(provider, query, page, out) {
  const o = out || {};
  // ⚠️ `hasMore` 的三种状态必须区分（M2 的发现：JS 的 `undefined` 与显式 `false`
  // 不是一回事）：`undefined`/`null` → Rust `None` → JSON `null`；其余 → bool。
  const hm = o.hasMore;
  return {
    provider,
    query,
    page,
    error: o.error ? String(o.error) : null,
    has_more: hm === undefined || hm === null ? null : Boolean(hm),
    results: (o.results || []).map((r) => canonResult(r, provider)),
  };
}

module.exports = { FIELDS, canonResult, canonOutcome };
