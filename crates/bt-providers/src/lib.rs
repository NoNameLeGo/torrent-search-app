//! # bt-providers
//!
//! 各站点 provider 的集合，一文件一站（对齐 `src/providers/*.js` 的组织方式）。
//!
//! ## 现状
//! 已搬 **16 个站点**（对齐 `src/providers/*.js`）：
//!
//! - **JSON 组（5）**：`tpb`、`knaben`、`torrentscsv`、`yts`、`internetarchive`
//! - **HTML 组（11）**：`linuxtracker`、`filemood`、`rutor`、`audiobookbay`、`therarbg`、
//!   `limetorrents`、`torrent9`、`oxtorrent`、`nyaa`、`megapeer`、`zeromagnet`
//!
//! ## 抽象：已提取的共用件 + 仍未建的 trait
//!
//! - ✅ [`run_mirrors`]：镜像回退的公共实现（2026-10-11 从 10 份逐字副本里提上来）
//! - ✅ [`MagnetOutcome`]：惰性磁力解析的返回形状（6 个 provider 在用）
//! - ⏳ **`Provider` trait：M2 再建** —— 现阶段（16 个 provider）的差异是「站点名」而不是
//!   「行为」，一个 `run_mirrors` helper 就够。但 M2 的聚合层要**并发遍历异构的 provider
//!   列表**、D17 要给 Torznab 留口子、`resolve_magnet` 只 6/16 家有（**可选能力** =
//!   trait 默认方法）—— 那时才该建。形状与反方意见见 `docs/ROADMAP.md` §2.4。
//!   ⚠️ 旧的 `docs/BT_APP_DESIGN.md` 已删（其「先做 HTTP/WebView」的前提被推翻）。
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
pub mod megapeer;
pub mod nyaa;
pub mod oxtorrent;
pub mod rutor;
pub mod therarbg;
pub mod torrent9;
pub mod torrentscsv;
pub mod tpb;
pub mod value;
pub mod yts;
pub mod zeromagnet;

use bt_core::TorrentResult;
use std::future::Future;

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

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`：逐个尝试镜像，取第一个
/// **结果非空**的；全都拿不到结果时，把各次尝试的错误拼成
/// `"<name> unreachable (<err1>; <err2>)"`。
///
/// 2026-10-11 从 10 个 provider 里提上来（`audiobookbay` / `filemood` /
/// `limetorrents` / `linuxtracker` / `megapeer` / `oxtorrent` / `rutor` /
/// `therarbg` / `torrent9` / `zeromagnet`）——十份副本逐字相同，只有站点名不同。
///
/// `name` 必须与 JS 逐字一致（`'audiobookbay'` 全小写、`'OxTorrent'` 驼峰……），
/// 因为错误串要能对齐 JS 版；各 provider 的测试钉住了这一点。
///
/// ## ⚠️ 有意 divergence：这里是**顺序**，JS 是**并行**
///
/// JS 用 `Promise.allSettled` 并行请求所有镜像，再按声明顺序取第一个非空的。
/// 这里逐个 `await`，最坏延迟 ≈ `bases.len() × 10s`（`http.js` 的超时）。
///
/// 选顺序是因为它更容易推理，且与提取之前各 provider 的实现一致（纯重构，无行为变化）。
/// 域名多于一个时值得改成并行 —— 但那时**两版必须一起改**，否则就是行为漂移。
///
/// 另外注意 JS 的 `errs` 会先过一遍 `filter(Boolean)`：**没有错误文本的尝试不占位**。
/// 「页面正常但没结果」在走这条路径的 provider 里算**错误**，不是空结果。
///
/// ## ⚠️ 别把签名里的 `'a` 「简化」掉
///
/// 写成 `F: Fn(&str) -> Fut`（隐式 HRTB）会编译失败：
/// `lifetime may not live long enough ... return type of closure contains a lifetime`
/// —— 因为闭包返回的 future 借用了传进去的那个 `&str`。
/// 必须让闭包参数与 `bases` 共用同一个 `'a`（2026-10-11 用独立 std-only 片段实测过：
/// 当前写法 ✅、去掉 `'a` 的写法 ❌）。
pub async fn run_mirrors<'a, F, Fut>(name: &str, bases: &'a [&'a str], attempt: F) -> SearchOutcome
where
    F: Fn(&'a str) -> Fut,
    Fut: Future<Output = SearchOutcome>,
{
    let mut errs: Vec<String> = Vec::new();

    for base in bases {
        let outcome = attempt(*base).await;
        if !outcome.results.is_empty() {
            return SearchOutcome::ok(outcome.results);
        }
        if let Some(e) = outcome.error {
            errs.push(e);
        }
    }

    SearchOutcome::err(format!("{name} unreachable ({})", errs.join("; ")))
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
