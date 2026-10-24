//! M6 差分验证的 **Rust 侧导出器** —— 把 23 个已移植 provider 的搜索结果导成规范化 JSON。
//!
//! ⚠️ **这不是回归测试，是差分工具**，所以 `#[ignore]`：
//! `cargo test --workspace` 不会碰它（否则每次跑测试都刷一大坨 JSON）；
//! `.github/workflows/rust.yml` 里用 `-- --ignored --nocapture` 显式跑它。
//! 显式跑是有意为之 —— 它失败必须是**响的**（CI 红），不能是"静默跳过"。
//!
//! 输出：设了 `BT_DIFF_OUT=<path>` 就写文件，否则打到 stdout。
//!
//! ## 契约：这份 JSON 必须和 `scripts/diff/dump-node.cjs` 的产出**可逐字比较**
//!
//! - 15 个字段与 `bt_core::TorrentResult` 一一对应（`canonical.cjs` 里有同一张表）
//! - `Option<T>` → `null`（serde 天然如此），bool → bool
//! - **`detail_url` 要做 origin 归一化**（见下）
//! - 键顺序无所谓：`serde_json` 按字典序，而 `compare.cjs` 是按字段名查的
//! - ⚠️ `error` 两边都导出但**不参与比较** —— Rust 的网络层文案是有意 divergence（M3）
//!
//! ## ⚠️ origin 归一化：不做就是「整列假差异」
//!
//! 几个 provider 的 `detail_url` 是**用 base 拼出来的**（`filemood` / `rutor` /
//! `linuxtracker` 至少这三家）。而两侧喂的 base 本来就不同：
//! 这里是**本地桩 URL**（`127.0.0.1:port`），Node 侧是**真实域名**。
//! 不归一化的话差异来自 **harness**、不是来自移植 —— 那种假差异比"没有差分"更坏，
//! 它会淹没真差异、逼人往白名单里塞东西。
//!
//! 做法：把本地 base 整串换成占位符 `<BASE>`，**只对 `detail_url`**。
//! Node 侧做同一件事（从桩记录的 URL 里解析 origin）。
//! ⚠️ **别对 `magnet` 做** —— 里面的 tracker 是页面给的，两侧本来就一致。
//!
//! ## 两侧的 provider 表必须一致
//!
//! 这份 `SPECS` 与 `scripts/diff/fixtures.cjs` 是**两份手写表**，会漂。
//! 兜底是 `compare.cjs`：它把"只在一边出现的 provider"直接判成结构性失败 ✅。
//! 但**fixture 名写错查不出来**（各自喂各自的、都"成功"）—— 只能靠人核对。

mod common;

use bt_core::http::HttpClient;
use bt_core::TorrentResult;
use bt_providers::{
    animetosho, anirena, audiobookbay, bangumimoe, dmhy, filemood, internetarchive, knaben,
    limetorrents, linuxtracker, megapeer, mikan, nekobt, nyaa, oxtorrent, rutor, subsplease,
    therarbg, torrent9, torrentscsv, tpb, xxxtracker, yts, zeromagnet, SearchOutcome,
};
use serde_json::{json, Value};

/// megapeer 是 windows-1251：必须喂**原始字节 + charset 头**，
/// reqwest 才能解对。Node 侧按 utf8 读会得到 U+FFFD —— 那正是 axios 的行为，
/// 所以两边的 name/size 等**本来就该不同**（那些差异在 `whitelist.json` 里）。
const HTML_1251: &str = "text/html; charset=windows-1251";

struct Spec {
    id: &'static str,
    /// 必须与 `scripts/diff/fixtures.cjs` 里同名
    fixture: &'static str,
    query: &'static str,
    /// 只有 `rutor`(0) / `limetorrents`(1) / `nyaa`(1) 用得到 —— 它们的
    /// `search_at` 多一个 page 参数，其余家忽略它
    page: u32,
    /// 见 `HTML_1251`
    bytes_1251: bool,
}

fn spec(id: &'static str, fixture: &'static str, query: &'static str) -> Spec {
    Spec {
        id,
        fixture,
        query,
        page: 1,
        bytes_1251: false,
    }
}

/// ⚠️ 用**函数返回 `Vec`** 而不是 `const SPECS: &[Spec]`：后者要求在常量上下文里
/// 求值，而 `spec(...)` 是普通 fn（不是 `const fn`），`const`/`static` 都会编译不过。
fn specs() -> Vec<Spec> {
    vec![
        // ---- 5 个 JSON provider（不需要 cheerio，本机也能跑）----
        spec("tpb", "tpb-ubuntu.json", "ubuntu"),
        spec("knaben", "knaben-ubuntu.json", "ubuntu"),
        spec("torrentscsv", "torrentscsv-ubuntu.json", "ubuntu"),
        spec("yts", "yts-matrix.json", "matrix"),
        spec(
            "internetarchive",
            "internetarchive-ubuntu.synthetic.json",
            "ubuntu",
        ),
        // 2026-10-22 新增：JSON API，一次请求
        spec("subsplease", "subsplease-one-piece.json", "one piece"),
        // POST 的 JSON API
        spec("bangumimoe", "bangumimoe-one-piece.json", "one piece"),
        // ---- 11 个 HTML provider（需要 cheerio，只能在 CI 跑）----
        spec("linuxtracker", "linuxtracker-linux.html", "linux"),
        spec("filemood", "filemood-ubuntu.html", "ubuntu"),
        Spec {
            // rutor 的 page 是 0 基（测试里就是 0）
            page: 0,
            ..spec("rutor", "rutor-ubuntu.html", "ubuntu")
        },
        spec("audiobookbay", "audiobookbay-ubuntu.html", "ubuntu"),
        spec("therarbg", "therarbg-ubuntu.html", "ubuntu"),
        Spec {
            page: 1,
            ..spec("limetorrents", "limetorrents-ubuntu.html", "ubuntu")
        },
        spec("torrent9", "torrent9-ubuntu.html", "ubuntu"),
        spec("oxtorrent", "oxtorrent-ubuntu.html", "ubuntu"),
        Spec {
            page: 1,
            ..spec("nyaa", "nyaa-ubuntu.html", "ubuntu")
        },
        Spec {
            bytes_1251: true,
            ..spec("megapeer", "megapeer-1080p.html", "1080p")
        },
        spec("zeromagnet", "zeromagnet-ubuntu.html", "ubuntu"),
        // ---- 2026-10-21/22 新搬的四家（两家 JSON 之外的三家 HTML）----
        // ⚠️ 这几份 fixture 都是 **CI 抓的真实页**（本机对 dmhy / mikan / animetosho 都不通）
        spec("dmhy", "dmhy-one-piece.html", "one piece"),
        spec("mikan", "mikan-one-piece.html", "one piece"),
        spec("anirena", "anirena-one-piece.html", "one piece"),
        spec("animetosho", "animetosho-one-piece.html", "one piece"),
        spec("nekobt", "nekobt-one-piece.html", "one piece"),
        // 2026-10-24：第 24 家（fixture 由 CI 抓，本机对 xxxtor.com 不通）
        spec("xxxtracker", "xxxtracker-one-piece.html", "one piece"),
    ]
}

/// 派发到各家的 `search_at`。注意 **`rutor` / `limetorrents` / `nyaa` 收四个参数**
/// （多的那个是 page），其余家是三个 —— 别抄错。
///
/// （不能用 `fn` 指针统一：`async fn` 没法直接强转成"返回装箱 Future 的函数指针"。）
async fn run(spec: &Spec, http: &HttpClient, base: &str) -> SearchOutcome {
    match spec.id {
        "tpb" => tpb::search_at(http, base, spec.query).await,
        "knaben" => knaben::search_at(http, base, spec.query).await,
        "torrentscsv" => torrentscsv::search_at(http, base, spec.query).await,
        "yts" => yts::search_at(http, base, spec.query).await,
        "internetarchive" => internetarchive::search_at(http, base, spec.query).await,
        "linuxtracker" => linuxtracker::search_at(http, base, spec.query).await,
        "filemood" => filemood::search_at(http, base, spec.query).await,
        "rutor" => rutor::search_at(http, base, spec.query, spec.page).await,
        "audiobookbay" => audiobookbay::search_at(http, base, spec.query).await,
        "therarbg" => therarbg::search_at(http, base, spec.query).await,
        "limetorrents" => limetorrents::search_at(http, base, spec.query, spec.page).await,
        "torrent9" => torrent9::search_at(http, base, spec.query).await,
        "oxtorrent" => oxtorrent::search_at(http, base, spec.query).await,
        "nyaa" => nyaa::search_at(http, base, spec.query, spec.page).await,
        "megapeer" => megapeer::search_at(http, base, spec.query).await,
        "zeromagnet" => zeromagnet::search_at(http, base, spec.query).await,
        // ⚠️ `dmhy` 收四个参数（它带分页）；另三家是三个
        "dmhy" => dmhy::search_at(http, base, spec.query, spec.page).await,
        "mikan" => mikan::search_at(http, base, spec.query).await,
        "animetosho" => animetosho::search_at(http, base, spec.query).await,
        "anirena" => anirena::search_at(http, base, spec.query).await,
        "nekobt" => nekobt::search_at(http, base, spec.query).await,
        "xxxtracker" => xxxtracker::search_at(http, base, spec.query).await,
        "subsplease" => subsplease::search_at(http, base, spec.query).await,
        "bangumimoe" => bangumimoe::search_at(http, base, spec.query).await,
        other => panic!("dump_rust 没覆盖 {other}（加了 provider 就同步 SPECS）"),
    }
}

/// **`detail_url` 的 origin 剥离** —— 与 `canonical.cjs` 的 `stripOrigin` **逐字对应**。
///
/// 两侧跑同一段逻辑，开头任何 `scheme://host` 一律换成 `<BASE>`。
///
/// ⚠️ 第一版是按「**本侧实际用过的 origin**」替换（Rust 用本地 base、Node 从桩记录的
/// URL 里解析）—— **那是错的，因为它不对称**：`internetarchive` 的 detail_url 用的是
/// **硬编码域名**，于是 Node 侧把它换成了 `<BASE>`、Rust 侧没换，
/// **本来完全一致的一家变成了 7 处差异**（CI run 38848479670）。
fn strip_origin(u: &str) -> String {
    let b = u.as_bytes();
    let mut i = 0usize;
    // scheme: [A-Za-z][A-Za-z0-9+.-]*
    if i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'+' | b'.' | b'-')) {
            i += 1;
        }
        // 上面的判断只认 ASCII 字节，所以 i 一定落在 char 边界上
        if u[i..].starts_with("://") {
            let rest = &u[i + 3..];
            // `[^/]*` —— 没有斜杠就吃到结尾（`https://x.orgindex.php` → `<BASE>`）
            let host_len = rest.find('/').unwrap_or(rest.len());
            return format!("<BASE>{}", &rest[host_len..]);
        }
    }
    u.to_string()
}

fn canon_result(r: &TorrentResult) -> Value {
    json!({
        "id": r.id,
        "name": r.name,
        "size": r.size,
        "size_text": r.size_text,
        "seeders": r.seeders,
        "leechers": r.leechers,
        "date": r.date,
        "date_text": r.date_text,
        "magnet": r.magnet,
        "info_hash": r.info_hash,
        "provider": r.provider,
        "category": r.category,
        // ⚠️ 唯一做 origin 剥离的字段
        "detail_url": r.detail_url.as_deref().map(strip_origin),
        "files": r.files,
        "needs_magnet": r.needs_magnet,
    })
}

fn canon_outcome(spec: &Spec, out: &SearchOutcome) -> Value {
    json!({
        "provider": spec.id,
        "query": spec.query,
        "page": spec.page,
        "error": out.error,
        // ⚠️ `Option<bool>`：`None` → null、`Some(false)` → false，两者不能混
        //（M2 的发现：JS 里 `undefined` 与显式 `false` 不是一回事）。
        "has_more": out.has_more,
        "results": out.results.iter().map(canon_result).collect::<Vec<_>>(),
    })
}

#[tokio::test]
#[ignore = "M6 差分工具，不是回归测试；用 `-- --ignored --nocapture` 显式跑"]
async fn dump_rust() {
    let http = HttpClient::new();
    let mut all: Vec<Value> = Vec::new();

    for spec in specs() {
        // ⚠️ 去掉结尾斜杠：Node 侧喂的是**真实域名**（没有结尾斜杠），
        // 这里要去形状对齐，否则 provider 拼出来的路径会多一个 `/`。
        let raw_base = if spec.bytes_1251 {
            common::oneshot_bytes(200, HTML_1251, &common::fixture_bytes(spec.fixture)).await
        } else {
            common::oneshot(200, &common::fixture(spec.fixture)).await
        };
        let base = raw_base.trim_end_matches('/').to_string();

        let out = run(&spec, &http, &base).await;

        eprintln!(
            "[m6] {:<15} <- {:<42} {:>4} 条{}",
            spec.id,
            spec.fixture,
            out.results.len(),
            match &out.error {
                Some(e) => format!("  error={e}"),
                None => String::new(),
            }
        );

        // 这一侧不再需要知道自己的 origin —— 通用剥离（见 strip_origin）
        all.push(canon_outcome(&spec, &out));
    }

    let json = serde_json::to_string_pretty(&Value::Array(all)).expect("序列化失败");

    match std::env::var("BT_DIFF_OUT") {
        Ok(path) => {
            // ⚠️ **cargo 跑测试时 CWD 是「包根」（`crates/bt-providers`），不是仓库根。**
            // 所以相对路径必须相对**仓库根**解析 —— 否则 `.cache/diff/rust.json`
            // 会落到 `crates/bt-providers/.cache/` 里去，而 CI 下一步在仓库根找它。
            let out = {
                let p = std::path::Path::new(&path);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../..")
                        .join(p)
                }
            };
            if let Some(dir) = out.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(&out, format!("{json}\n")).unwrap_or_else(|e| {
                panic!("写 BT_DIFF_OUT 失败（{}）：{e}", out.display());
            });
            println!("[m6] Rust 侧已写入 {}", out.display());
        }
        Err(_) => println!("{json}"),
    }
}
