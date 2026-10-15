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
//! ## 抽象：已提取的共用件 + [`Provider`] trait
//!
//! - ✅ [`run_mirrors`]：镜像回退的公共实现（2026-10-11 从 10 份逐字副本里提上来）
//! - ✅ [`MagnetOutcome`]：惰性磁力解析的返回形状（6 个 provider 在用）
//! - ✅ **`Provider` trait + 聚合层（M2，2026-10-16）** —— 见 [`aggregate`] 与 [`registry`]。
//!   引入它的三个真实消费者：① 聚合层要并发遍历异构 provider 列表；
//!   ② D17 要给 Torznab 留「运行时插入」的口子；③ `resolve_magnet` 只有 6/16 家有
//!   （**可选能力 = trait 默认方法**）。⚠️ 代码量因此**净增**，这不是为了省行数。
//!
//! ## 约定（对齐 JS 版）
//! - 每个 provider 暴露一个 `async fn search(http, query, page) -> SearchOutcome`
//! - 再暴露一个 `search_at(http, api, query)` 用于**离线测试**（端点指向本地一次性服务）
//! - **永不 panic、永不返回 Err**：失败一律落在 `SearchOutcome::error`
//! - 结果必须过 `bt_core::normalize::normalize()` 再返回

pub mod aggregate;
pub mod audiobookbay;
pub mod filemood;
pub mod internetarchive;
pub mod knaben;
pub mod limetorrents;
pub mod linuxtracker;
pub mod megapeer;
pub mod nyaa;
pub mod oxtorrent;
pub mod registry;
pub mod rutor;
pub mod therarbg;
pub mod torrent9;
pub mod torrentscsv;
pub mod tpb;
pub mod value;
pub mod yts;
pub mod zeromagnet;

use bt_core::http::HttpClient;
use bt_core::TorrentResult;
use std::future::Future;
use std::pin::Pin;

/// provider 的搜索结果。字段名沿用 JS 版：`results` / `error` / `hasMore`。
#[derive(Debug, Clone, PartialEq)]
pub struct SearchOutcome {
    pub results: Vec<TorrentResult>,
    pub error: Option<String>,
    /// ⚠️ **`Option<bool>`，不是 `bool`** —— 对齐 JS：provider **不表态**时是 `undefined`。
    ///
    /// 而 `undefined` 与显式 `false` 在聚合层的 `aggregateHasMore`
    /// （`src/providers/index.js:190-198`）里走**不同分支**：
    /// `undefined` → 回退到 `!!p.paginated`；`false` → provider 的**明确否决**。
    /// 若用 `bool`，「provider 说 false、但它其实是 paginated」时聚合层会给出错的
    /// `has_more` —— 这正是 M2 勘明时发现、必须改掉的地方。
    ///
    /// 16 个已搬 provider 一律留 `None`（与 JS 一致：那边也只有 `demo.js` 会显式返回它）。
    pub has_more: Option<bool>,
}

impl SearchOutcome {
    /// 正常返回（**无结果也算正常**，`error` 保持 `None`）。
    ///
    /// `has_more` 留 `None` = 「provider 没表态」，不是「没有下一页」。
    pub fn ok(results: Vec<TorrentResult>) -> Self {
        Self {
            results,
            error: None,
            has_more: None,
        }
    }

    /// 出错了。`results` 一律为空 —— 对齐 JS：半截结果不如没有。
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            results: Vec::new(),
            error: Some(msg.into()),
            has_more: None,
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

// ===========================================================================
// M2（2026-10-16）：Provider trait + 聚合层的公共类型
// ===========================================================================

/// `Provider::search` / `resolve_magnet` 返回的 future。
///
/// ## 为什么是 boxed future，而不是 `async fn` in trait（AFIT）
///
/// AFIT 会让 trait **不能做 trait object**（`dyn Provider` 不成立）。而 D17 要留的
/// Torznab 口子恰恰是「**运行时**插入一个非编译期固定的 provider」——那种 provider
/// 带着自己的状态（URL + API key），必须是 `Box<dyn Provider>`。
/// `docs/ROADMAP.md` §2.4 的草案写的是 `async fn`，2026-10-16 开工勘明后改成这个形状。
///
/// 代价是每次调用一次装箱分配 —— 相对于一次网络往返可以忽略。
///
/// ## ⚠️ 刻意**不加** `Send`
///
/// 与 [`run_mirrors`] 的 `Fut` 约束保持一致。聚合层用 `FuturesUnordered` 在
/// **当前任务**上驱动，不需要 `Send`。将来若要改成 `tokio::spawn`，给这里加
/// `+ Send` 即可（一次机械改动，但那时也意味着 provider 的 future 必须全部 Send，
/// 需要逐个确认 —— 别顺手加）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// 「这个 provider 没有惰性磁力解析能力」的错误串。
///
/// ⚠️ **与 JS 的对照**：JS 里这**不是** provider 的返回值，而是 `/api/magnet`
/// 端点在找不到 resolver 时的 **HTTP 404 + `{ error: 'no resolver' }`**
/// —— 注意**中间有空格**（`docs/ROADMAP.md` §2.4 早期草案写的 `no_resolver` 是错的）。
/// Rust 侧没有 HTTP 层，于是把它收敛成 trait 默认方法的返回值；字符串取 JS 原文，
/// 这样万一它冒到界面上，两边文案一致。
pub const NO_RESOLVER: &str = "no resolver";

/// 一个搜索源。
///
/// `Send + Sync` 是为了让 `Box<dyn Provider>` 能放进 `Arc` 之类跨线程的东西里
/// —— 聚合层现在在单任务上驱动、用不到，但 Torznab 的动态 provider 会是长期存活的
/// 配置对象，先把约束摆正比事后补要便宜。
pub trait Provider: Send + Sync {
    /// 稳定 id，**与 JS 逐字一致** —— `?providers=` 过滤与每站状态的 key 都用它。
    fn id(&self) -> &'static str;

    /// 展示名，**与 JS 逐字一致**（UI 状态栏用）。
    fn name(&self) -> &'static str;

    /// 对齐 JS `REGISTRY` 里的 `enabled` 字段（16 个已搬站点全是 `true`）。
    ///
    /// ⚠️ 别和用户级开关搞混：**用户选哪几家走
    /// [`aggregate::SearchArgs::providers`]**，不是这个。
    fn enabled(&self) -> bool {
        true
    }

    /// 它的 `search` 真的会消费 `page` 吗（见 `docs/AGENT_HANDBOOK.md` §2.1①）。
    /// 不消费的引擎翻到第 2 页只会返回同一批结果，再被去重掉 —— 所以要被筛掉。
    fn paginated(&self) -> bool {
        false
    }

    /// 无关键词也能查（latest / trending）。
    fn browseable(&self) -> bool {
        false
    }

    /// 对齐 JS 的 `p.demo`：离线假数据、无关键词也能查、且 `page` 对它有效。
    ///
    /// JS 里只有 `demo.js` 一家，Rust 侧暂无。**但语义必须建模** ——
    /// `resolveTargets` 第 3 条是 `page > 1 && !paginated && !demo`，
    /// 少一个分支那条语义就测不全。
    fn demo(&self) -> bool {
        false
    }

    /// 查一页。
    ///
    /// **契约：永不 panic、永不返回 `Err`** —— 失败一律落在 [`SearchOutcome::error`]。
    fn search<'a>(
        &'a self,
        http: &'a HttpClient,
        query: &'a str,
        page: u32,
    ) -> BoxFuture<'a, SearchOutcome>;

    /// **可选能力**：16 家里只有 6 家能惰性解析磁力（`audiobookbay` / `megapeer` /
    /// `oxtorrent` / `therarbg` / `torrent9` / `zeromagnet`）。没有的就是没有 —— 照抄 JS。
    ///
    /// 这正是用 **trait 默认方法**表达「可选能力」的地方，也是当初决定引入 trait
    /// 的理由之一。
    fn resolve_magnet<'a>(
        &'a self,
        _http: &'a HttpClient,
        _url: &'a str,
    ) -> BoxFuture<'a, MagnetOutcome> {
        Box::pin(async { MagnetOutcome::err(NO_RESOLVER) })
    }
}

/// 每站状态里的 `status` 字段。JS 用的是字符串 `'ok' | 'error'`；这边用 enum
/// （Rust 里更顺手），但**必须能还原成 JS 的字符串** —— M6 差分要用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderState {
    Ok,
    Error,
}

impl ProviderState {
    /// 与 JS 逐字一致的小写串。
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderState::Ok => "ok",
            ProviderState::Error => "error",
        }
    }
}

/// 每站状态 —— **形状照抄 JS**（`src/providers/index.js` 的 `runProvider`）：
/// `{ status, count, error, hasMore, ms }`。UI 的状态栏按这个渲染。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub status: ProviderState,
    pub count: usize,
    pub error: Option<String>,
    /// ⚠️ **`Option<bool>`，不是 `bool`** —— 见 [`SearchOutcome::has_more`] 的说明：
    /// `None` = provider 没表态（JS 的 `undefined`），与显式 `false` 走不同分支。
    ///
    /// 另外注意 JS 的 **catch 分支根本没有这个 key**（崩溃时也是 `undefined`）
    /// ⇒ 崩溃路径同样填 `None`。
    pub has_more: Option<bool>,
    /// 单个 provider 自己的耗时（毫秒），照 JS 的 `Date.now() - startedAt`。
    pub ms: u64,
}

/// 流式回调的载荷：JS 的 `onProvider({ id, name, results, status })`。
///
/// `id` / `name` 用 `String` 是为了让回调不背生命周期（它们在 JS 里就是普通字符串）。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderReport {
    pub id: String,
    pub name: String,
    pub results: Vec<TorrentResult>,
    pub status: ProviderStatus,
}
