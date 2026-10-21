//! SubsPlease —— 番剧资源，走公开的 JSON API（**不解析 HTML**）。
//!
//! 从 `src/providers/subsplease.js`（68 行）移植。
//!
//! ## 响应形状（与其它 JSON provider 都不同）
//!
//! ```text
//! {
//!   "One Piece - 1180": {
//!     "release_date": "Sun, 27 Sep 2026 16:50:17 +0000",  // RFC 1123
//!     "episode": "1180",
//!     "page": "one-piece",                                 // 详情页 slug
//!     "downloads": [ { "res": "480", "magnet": "magnet:?…&xl=382937687&…" } ]
//!   },
//!   …
//! }
//! ```
//!
//! ⇒ **一个 show 展开成 N 条结果**（每个画质一条），`name` = `"<show 名> [<res>p]"`。
//!
//! ## 两处需要解释的细节
//!
//! 1. **体积不在响应里**，得从磁力的 `xl=` 参数里抠（Kotlin 的 `formatBytes` 同理）。
//! 2. **`tz=$` 是字面量哨兵**，不是 shell 变量 —— 上游 Kotlin 就是这么发的
//!    （`/api?f=search&tz=$&s=<query>`），照抄。写成 `tz=` 或省略都会让服务端改行为。
//!
//! ## 有意 divergence（1 处）
//!
//! **结果顺序**：JS 用 `Object.entries()`（插入序 = 服务端给的顺序），
//! Rust 侧 `serde_json::Map` 默认是 `BTreeMap`（**按 show 名排序**）。
//! 要完全对齐得开 `serde_json/preserve_order`（引入 indexmap ⇒ `Cargo.lock` 要重算，
//! 而本机没有 cargo），收益只是顺序 —— 所以这里接受差异并写明。
//! 对用户可见的影响：同一屏里结果的先后不同，内容完全一致。

use bt_core::http::{HttpClient, JsonResponse};
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult,
};
use serde_json::Value;

use crate::value::v2string;
use crate::SearchOutcome;

/// 站点根（JS 的 `BASE`）。注意路径是 `/api`，不是 `/api/`。
pub const API: &str = "https://subsplease.org";

/// 用默认端点搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, API, query).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, api: &str, query: &str) -> SearchOutcome {
    // ⚠️ `tz=$` 里的 `$` 是**字面量**，不是格式化占位符 —— 它不在 `{}` 里，format! 不会碰它。
    let url = format!("{api}/api?f=search&tz=$&s={}", encode_uri_component(query));

    let resp: JsonResponse<Value> = http.get_json(&url, None).await;
    if let Some(e) = &resp.error {
        return SearchOutcome::err(format!("SubsPlease unreachable ({e})"));
    }

    // 与 `yts.rs` 同一条有意偏离：JS 遇到「200 + 非 JSON 正文」会静默返回空，
    // 这里交给上面的 `resp.error` 分支显式报错（`tests/subsplease.rs` 有 divergence 用例）。
    let Some(root) = resp.data.as_ref() else {
        return SearchOutcome::ok(Vec::new());
    };
    let Some(shows) = root.as_object() else {
        return SearchOutcome::ok(Vec::new());
    };

    let mut results = Vec::new();
    for (show_name, anime) in shows {
        // JS: `if (!animeObj || typeof animeObj !== 'object') continue;`
        if !anime.is_object() {
            continue;
        }

        let release_date = anime.get("release_date").and_then(v2string);
        let episode = anime.get("episode").and_then(v2string);
        let details_base = anime
            .get("page")
            .and_then(v2string)
            .map(|p| format!("{api}/{p}"));

        let downloads: &[Value] = match anime.get("downloads").and_then(Value::as_array) {
            Some(v) => v,
            None => &[],
        };

        for d in downloads {
            // JS: `const magnet = d && d.magnet; if (!magnet) continue;`
            let Some(magnet) = d.get("magnet").and_then(v2string) else {
                continue;
            };

            // JS 是模板字符串直接插值（`${res}`），缺失时会变成字面量 "undefined"。
            // 这里退化成空串 —— 只有畸形响应才走得到，不为它单列 divergence 测试。
            let res = d.get("res").and_then(v2string).unwrap_or_default();
            let name = format!("{show_name} [{res}p]");

            let info_hash = extract_info_hash(Some(&magnet));
            let size = size_from_magnet(&magnet);

            // JS: `${detailsBase}?ep=${encodeURIComponent(episode)}&res=${encodeURIComponent(res)}`
            let detail_url = details_base.as_ref().map(|b| {
                format!(
                    "{b}?ep={}&res={}",
                    encode_uri_component(episode.as_deref().unwrap_or_default()),
                    encode_uri_component(&res)
                )
            });

            results.push(normalize(&RawResult {
                provider: "subsplease".to_string(),
                id: None,
                name: Some(name),
                info_hash,
                magnet: Some(magnet),
                size: size.map(NumOrText::Num),
                seeders: None,
                leechers: None,
                date: release_date.clone().map(NumOrText::Text),
                category: Some("Anime".to_string()),
                detail_url,
                files: None,
            }));
        }
    }

    SearchOutcome::ok(results)
}

/// 从磁力的 `xl=` 参数里取**字节数**（JS 的 `sizeFromMagnet`）。
///
/// 逐字对齐 JS：
/// - 只取 `?` 之后那段，再按 `&` 切；
/// - 只认 `startsWith('xl=')` 的那一段（**大小写敏感**）；
/// - `parseInt` 失败 ⇒ `null`（不是 0）。
fn size_from_magnet(magnet: &str) -> Option<i64> {
    let query = magnet.split('?').nth(1).unwrap_or("");
    let xl = query.split('&').find(|p| p.starts_with("xl="))?;
    // JS `parseInt("382937687abc")` = 382937687（前缀数字即止），这里同口径：
    // 取开头的连续数字，剩下的忽略。
    let digits: String = xl[3..].chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::size_from_magnet;

    #[test]
    fn size_comes_from_the_xl_param() {
        let m = "magnet:?xt=urn:btih:abc&dn=x&xl=382937687&tr=http%3A%2F%2Fa";
        assert_eq!(size_from_magnet(m), Some(382_937_687));
    }

    #[test]
    fn size_is_none_without_xl() {
        assert_eq!(size_from_magnet("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(size_from_magnet("magnet:?xl="), None, "空值不是 0");
        assert_eq!(size_from_magnet("no-question-mark"), None);
    }

    #[test]
    fn size_ignores_a_trailing_junk_suffix() {
        // JS `parseInt` 遇到非数字即止
        assert_eq!(size_from_magnet("magnet:?xl=12abc"), Some(12));
    }

    #[test]
    fn xl_must_be_lowercase() {
        // JS 是 `startsWith('xl=')`，大小写敏感 —— 大写 `XL=` 认不出来
        assert_eq!(size_from_magnet("magnet:?XL=99"), None);
    }
}
