//! M6 差分验证的 **Rust 侧导出器** —— 把 5 个 JSON provider 的搜索结果导成规范化 JSON。
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
//! - 键顺序**无所谓**：`serde_json` 默认按键名字典序，而 `compare.cjs` 是按字段名查的
//! - ⚠️ `error` 两边都导出但**不参与比较** —— Rust 的网络层文案是有意 divergence（M3）
//!
//! ## 两侧的 provider 表必须一致
//!
//! Rust 这份 `SPECS` 与 `scripts/diff/fixtures.cjs` 的 `JSON_PROVIDERS` 是**两份手写表**，
//! 会漂。兜底是 `compare.cjs`：它把"只在一边出现的 provider"直接判成结构性失败 ✅。
//! 所以漏加一边不会静默 —— 但**也别指望它替你同步 fixture 名**，那是查不出来的。

mod common;

use bt_core::http::HttpClient;
use bt_core::TorrentResult;
use bt_providers::{internetarchive, knaben, torrentscsv, tpb, yts, SearchOutcome};
use serde_json::{json, Value};

/// 一条 provider 规格。`fixture` 必须与 `scripts/diff/fixtures.cjs` 里那一份**同名**。
struct Spec {
    id: &'static str,
    fixture: &'static str,
    query: &'static str,
}

const SPECS: &[Spec] = &[
    Spec {
        id: "tpb",
        fixture: "tpb-ubuntu.json",
        query: "ubuntu",
    },
    Spec {
        id: "knaben",
        fixture: "knaben-ubuntu.json",
        query: "ubuntu",
    },
    Spec {
        id: "torrentscsv",
        fixture: "torrentscsv-ubuntu.json",
        query: "ubuntu",
    },
    Spec {
        id: "yts",
        fixture: "yts-matrix.json",
        // ⚠️ 与 fixtures.cjs 里的 query 必须一致 —— 它对结果有影响（YTS 是按词搜的）
        query: "matrix",
    },
    Spec {
        id: "internetarchive",
        fixture: "internetarchive-ubuntu.synthetic.json",
        query: "ubuntu",
    },
];

/// 五家的 `search_at(http, base, query)` 签名一致，用 match 派发。
///
/// （不能用 `fn` 指针：`async fn` 没法直接强转成"返回装箱 Future 的函数指针"，
/// 硬要做得给每家写一层包装，反而更绕。）
async fn run(spec: &Spec, http: &HttpClient, base: &str) -> SearchOutcome {
    match spec.id {
        "tpb" => tpb::search_at(http, base, spec.query).await,
        "knaben" => knaben::search_at(http, base, spec.query).await,
        "torrentscsv" => torrentscsv::search_at(http, base, spec.query).await,
        "yts" => yts::search_at(http, base, spec.query).await,
        "internetarchive" => internetarchive::search_at(http, base, spec.query).await,
        other => panic!("dump_rust 没覆盖 {other}（加了 provider 就同步 SPECS）"),
    }
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
        "detail_url": r.detail_url,
        "files": r.files,
        "needs_magnet": r.needs_magnet,
    })
}

fn canon_outcome(spec: &Spec, out: &SearchOutcome) -> Value {
    json!({
        "provider": spec.id,
        "query": spec.query,
        "page": 1,
        "error": out.error,
        // ⚠️ `Option<bool>`：`None` → null，`Some(false)` → false。
        // 两者**不能**混（M2 的发现：JS 里 `undefined` 与显式 `false` 不是一回事）。
        "has_more": out.has_more,
        "results": out.results.iter().map(canon_result).collect::<Vec<_>>(),
    })
}

#[tokio::test]
#[ignore = "M6 差分工具，不是回归测试；用 `-- --ignored --nocapture` 显式跑"]
async fn dump_rust() {
    let http = HttpClient::new();
    let mut all: Vec<Value> = Vec::new();

    for spec in SPECS {
        let body = common::fixture(spec.fixture);
        // 一次性本地服务：provider 各自发**一次**请求，正好对上
        // （Node 侧由 http-stub 干同一件事）
        let base = common::oneshot(200, &body).await;
        let out = run(spec, &http, &base).await;

        eprintln!(
            "[m6] {} <- {} : {} 条{}",
            spec.id,
            spec.fixture,
            out.results.len(),
            match &out.error {
                Some(e) => format!("  error={e}"),
                None => String::new(),
            }
        );
        all.push(canon_outcome(spec, &out));
    }

    let json = serde_json::to_string_pretty(&Value::Array(all)).expect("序列化失败");

    match std::env::var("BT_DIFF_OUT") {
        Ok(path) => {
            // ⚠️ **cargo 跑测试时 CWD 是「包根」（`crates/bt-providers`），不是仓库根。**
            // 所以相对路径必须相对**仓库根**解析 —— 否则 `.cache/diff/rust.json`
            // 会落到 `crates/bt-providers/.cache/` 里去，而 CI 下一步在仓库根找它，
            // 直接"文件不存在"。第一版就是这么写的（写这条注释时已经改掉了）。
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
