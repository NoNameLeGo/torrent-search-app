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

/**
 * **origin 归一化** —— `detail_url` 必须做，否则是**整列假差异**。
 *
 * ## 为什么需要它
 *
 * 几个 provider 的 `detail_url` 是**用 base 拼出来的**（`join_base(base, href)`）：
 * `filemood` / `rutor` / `linuxtracker` 至少这三家。
 * 而两侧喂的 base **本来就不同**：
 *   · Rust 侧 `search_at(http, base, …)` 的 base 是**本地桩 URL**（`127.0.0.1:port`）
 *   · Node 侧是**真实域名**（`linuxtracker.org` …）
 * ⇒ 不归一化的话差异来自 **harness**、不是来自移植。那种假差异比"没有差分"更坏：
 * 它会淹没真差异、让人开始往白名单里塞东西。
 *
 * ## 怎么归一化
 *
 * 把**自己这一侧实际用过的 origin**整串换成占位符 `<BASE>`：
 *   · Node 侧：从桩记录的 `calls` 里解析出来（不手写域名表 —— 那是第二份会漂的表）
 *   · Rust 侧：就是那个本地 base
 *
 * ⚠️ **只对 `detail_url` 做，别碰 `magnet`**：magnet 里的 tracker 是**页面给的**，
 * 两侧本来就一致；归一化它反而会把真差异洗掉。
 * ⚠️ 绝对指向别的域名的 URL（`tpb` 的 `thepiratebay.org` 是硬编码的）不会被改动 ——
 * 那是对的，两侧本来就该一样。
 */
function normalizeOrigins(s, origins) {
  let out = s;
  for (const o of origins || []) {
    if (o) out = out.split(o).join('<BASE>');
  }
  return out;
}

function canonResult(r, provider, origins) {
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
    // ⚠️ 唯一做 origin 归一化的字段（见 normalizeOrigins 的说明）
    detail_url: (() => {
      const u = optStr(r.detailUrl);
      return u === null ? null : normalizeOrigins(u, origins);
    })(),
    files: optInt(r.files),
    needs_magnet: Boolean(r.needsMagnet),
  };

  const ordered = {};
  for (const k of FIELDS) ordered[k] = row[k];
  return ordered;
}

function canonOutcome(provider, query, page, out, origins) {
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
    results: (o.results || []).map((r) => canonResult(r, provider, origins)),
  };
}

module.exports = { FIELDS, canonResult, canonOutcome, normalizeOrigins };
