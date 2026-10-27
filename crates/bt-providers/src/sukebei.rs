//! Sukebei —— `sukebei.nyaa.si`，`nyaa.si` 的成人分站（B 组，2026-10-27 搬入）。
//!
//! 从 `src/providers/sukebei.js` 移植，但**没有第二份解析器**：两个站点由同一套
//! nyaa 服务端代码渲染，结果表结构逐字节同构（同一张 `table.torrent-list`、
//! `colspan="2"` 的名字格、磁力同样在 Link 格、分类同样读 `<a title="…">`）。
//! 所以这里只换三样东西 —— **域名、provider id、错误前缀** —— 解析直接走
//! [`crate::nyaa::search_as`]。
//!
//! ## 为什么不抄 JS 的 `CATEGORY_MAP`
//!
//! `sukebei.js` 有一张 `CATEGORY_MAP`（`1`→Anime、`4`→Porn…），但它比的是**分类码**。
//! 本移植与 `nyaa.rs` 一致：直接取 `td` 里 `<a title="…">` 的**可读名**
//! （sukebei 上是 `Art - Anime` / `Real Life` 这类）。可读名比码更准，
//! 也不用跟着站点改码表 —— 这是与 JS 的**已知且有意**的差异。
//!
//! ## 为什么它不在差分门（M6）里
//!
//! 差分的口径是「同一份 fixture 下 Node 与 Rust 逐字段比」。本站两个环境都探不通
//! （`UND_ERR_CONNECT_TIMEOUT`，见 `AGENTS.md`「受阻引擎清单」C 档 ⇒ 现在已搬），
//! 没有抓过真快照 ⇒ 抓不到就没法比。兜底是 `tests/sukebei.rs`：
//! 用 **nyaa 的真实 fixture**（结构同构）验证复用路径确实产出了 `provider = "sukebei"`

use bt_core::http::HttpClient;

use crate::SearchOutcome;

/// 唯一域名（JS 的 `DOMAINS` 也只有这一条）。
pub const BASE: &str = "https://sukebei.nyaa.si";

/// provider id —— 进 `TorrentResult.provider`，也是站点开关的 key。
pub const ID: &str = "sukebei";

/// 展示名，同时是错误串前缀（`Sukebei unreachable (...)`）。
pub const NAME: &str = "Sukebei";

/// 搜索。`page` 与 nyaa 同款：**原样进 URL**（JS 没过 `coercePage`）。
pub async fn search(http: &HttpClient, query: &str, page: u32) -> SearchOutcome {
    search_at(http, BASE, query, page).await
}

/// 用**指定基址**搜索 —— 只给离线测试用（生产走 [`search`]）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str, page: u32) -> SearchOutcome {
    crate::nyaa::search_as(http, base, query, page, ID, NAME).await
}
