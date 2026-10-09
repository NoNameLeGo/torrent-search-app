//! # bt-providers
//!
//! 各站点 provider 的集合，一文件一站（对齐 `src/providers/*.js` 的组织方式）。
//!
//! ## 现状
//! `tpb`、`knaben`、`torrentscsv`、`yts`、`internetarchive`（纯 JSON 组，A 组已齐）；
//! `linuxtracker`、`filemood`（HTML 组，B 组进行中）。
//! **暂时不引入 `Provider` trait** —— 等 HTML 组落地后，
//! 看清楚它们真正的共性（镜像回退、翻页、磁力惰性解析…）再定抽象，现在定容易定错。
//!
//! ## 约定（对齐 JS 版）
//! - 每个 provider 暴露一个 `async fn search(http, query, page) -> SearchOutcome`
//! - 再暴露一个 `search_at(http, api, query)` 用于**离线测试**（端点指向本地一次性服务）
//! - **永不 panic、永不返回 Err**：失败一律落在 `SearchOutcome::error`
//! - 结果必须过 `bt_core::normalize::normalize()` 再返回

pub mod filemood;
pub mod internetarchive;
pub mod knaben;
pub mod linuxtracker;
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
