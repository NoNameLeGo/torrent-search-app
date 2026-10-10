//! # bt-providers
//!
//! 各站点 provider 的集合，一文件一站（对齐 `src/providers/*.js` 的组织方式）。
//!
//! ## 现状
//! `tpb`、`knaben`、`torrentscsv`、`yts`、`internetarchive`（纯 JSON 组，A 组已齐）；
//! `torrent9`、`linuxtracker`、`filemood`、`rutor`、`audiobookbay`、`therarbg`、`limetorrents`（HTML 组，B 组进行中）。
//! **暂时不引入 `Provider` trait** —— 等 HTML 组落地后，
//! 看清楚它们真正的共性（镜像回退、翻页、磁力惰性解析…）再定抽象，现在定容易定错。
//!
//! ## 约定（对齐 JS 版）
//! - 每个 provider 暴露一个 `async fn search(http, query, page) -> SearchOutcome`
//! - 再暴露一个 `search_at(http, api, query)` 用于**离线测试**（端点指向本地一次性服务）
//! - **永不 panic、永不返回 Err**：失败一律落在 `SearchOutcome::error`
//! - 结果必须过 `bt_core::normalize::normalize()` 再返回

pub mod audiobookbay;
pub mod filemood;
pub mod internetarchive;
pub mod knaben;
pub mod limetorrents;
pub mod linuxtracker;
pub mod rutor;
pub mod therarbg;
pub mod torrent9;
pub mod torrentscsv;
pub mod tpb;
pub mod value;
pub mod yts;

use bt_core::TorrentResult;

/// provider 的搜索结果。字段名沿用 JS 版：`results` / `error` / `hasMore`。
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOutcome {
    pub results: Vec<TorrentResult>,
    pub error: Option<String>,
    pub has_more: bool,
}

impl SearchOutcome {
    /// 正常返回（**无结果也算正常**，`error` 保持 `None`）。
    pub fn ok(results: Vec<TorrentResult>) -> Self {
        Self {
            results,
            error: None,
            has_more: false,
        }
    }

    /// 出错了。`results` 一律为空 —— 对齐 JS：半截结果不如没有。
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            results: Vec::new(),
            error: Some(msg.into()),
            has_more: false,
        }
    }
}

/// `resolve_magnet` 的返回形状，字段名对齐 JS（`magnet` / `infoHash` / `error`）。
///
/// 2026-10-10 从 `audiobookbay` 挑到这里 —— `therarbg` 也要用，
/// 两个消费者出现了才值得抽象（否则就是"只有一个实现的接口"）。
///
/// ⚠️ JS 侧各家的 key 并不完全一致：`audiobookbay` 会返回 `infoHash`，
/// `therarbg` 只返回 `{ magnet, error }`。这里统一成三个字段，
/// `therarbg` 那边 `info_hash` 恒为 `None`（照抄 JS，不从 magnet 里反推）。
#[derive(Debug, Clone, PartialEq)]
pub struct MagnetOutcome {
    pub magnet: Option<String>,
    pub info_hash: Option<String>,
    pub error: Option<String>,
}

impl MagnetOutcome {
    /// 取不到磁力：三个字段全空（`info_hash` 也是 `None`），只给一个错误串。
    ///
    /// 各家失败时的错误串不一样（`no_info_hash` / `no_html` / `no_magnet_on_page` …），
    /// 所以由调用方传进来 —— 跟 JS 逐字对齐。
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            magnet: None,
            info_hash: None,
            error: Some(msg.into()),
        }
    }
}
