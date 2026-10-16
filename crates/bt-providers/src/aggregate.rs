//! M2 聚合层 —— 逐条复刻 `src/providers/index.js`（271 行，**唯一真值来源**）。
//!
//! 每条语义的 JS 行号写在对应函数上；改这里的判断之前先回去读 JS。
//!
//! ## 三条语义分别在哪儿
//!
//! | 语义 | JS 行 | 这里 |
//! |---|---|---|
//! | ① 目标筛选 `resolveTargets` | 147–158 | [`resolve_targets`] |
//! | ② 并发池 `asyncPool` + `runProvider` | 163–219 | [`async_pool`] / `run_provider` |
//! | ③ 聚合 `hasMore` | 190–198 | [`aggregate_has_more`] |
//!
//! ## 与 JS 的一处**有意**差异（写在 `AGENTS.md` 的「M2 开工前勘明」里）
//!
//! 1. **[`async_pool`] 不返回结果集。** JS 的 `asyncPool` 最后用
//!    `Promise.all(results)` 交出一份**按任务顺序**的数组；但两个调用点
//!    （`search()` / `searchStream()`）**都把它丢掉了**。这里不为一个没人用的
//!    返回值加复杂度 —— 需要的东西在 `on_settle` 回调里拿走。
//!
//! ## panic 兜底（2026-10-16 补上，此前**刻意不做**）
//!
//! JS 的 `runProvider` 用 `try/catch`，连「抛异常」一起兜；Rust 侧 provider 的契约是
//! **永不 panic**，失败一律走 `SearchOutcome::error`，所以**错误**路径本来就同构。
//! 但"契约"是**约定**不是**保证**：真有 provider 违约 panic 时，它会**带走整个聚合**
//!（别人已经返回的结果也一起没），而 JS 不会。
//!
//! 当时"刻意不做"的理由是「本机没有 `cargo`，不盲写没验证过的 API 组合」——
//! **那条理由后来失效了**（M3/M4/M6 都证明这条路上本机不编译也能交付）。
//! 现在 [`run_provider`] 用 `AssertUnwindSafe(..).catch_unwind()` 兜住，
//! 判据是「**违约 panic 的 provider 不会掀翻别人**」，配了同名测试。

use std::time::Instant;

use std::panic::AssertUnwindSafe;

use bt_core::http::HttpClient;
use bt_core::TorrentResult;
use futures::future::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};

use crate::{Provider, ProviderReport, ProviderState, ProviderStatus, SearchOutcome};

/// JS `MAX_CONCURRENT`（`src/providers/index.js:221-223`）。
///
/// 原文注释：「8 concurrent requests to third-party sites — enough to keep the pipe
/// full without looking like a DDoS. Bump if providers add more mirrors.」
pub const MAX_CONCURRENT: usize = 8;

/// JS `search(query, { providers, page, browse })` 的入参。默认值照 JS 抄。
#[derive(Debug, Clone)]
pub struct SearchArgs<'a> {
    /// JS 的 `?providers=a,b`（`resolveTargets` 里那个 `wanted`）。
    ///
    /// ⚠️ 边界**照抄** JS 的 `providers ? split : null`（`index.js:148-150`）：
    ///
    /// | 输入 | 结果 |
    /// |---|---|
    /// | `None` / `Some("")` | **全都要**（空串在 JS 里是 falsy） |
    /// | `Some("  ")`（纯空白） | `wanted = []` → **一个都不查**（不是"全都要"！） |
    /// | `Some("a,,b")` | `["a", "b"]` |
    pub providers: Option<&'a str>,
    /// 页码。JS 默认 **1**（不是 0）。
    pub page: u32,
    /// 无关键词的浏览模式：只查 `browseable` 的站点。
    pub browse: bool,
}

impl Default for SearchArgs<'_> {
    fn default() -> Self {
        Self {
            providers: None,
            page: 1,
            browse: false,
        }
    }
}

impl<'a> SearchArgs<'a> {
    /// `?providers=` 的解析 —— 复刻 `src/providers/index.js:148-150`：
    /// `providers ? providers.split(',').map(trim).filter(Boolean) : null`。
    ///
    /// 注意 `Some("")` 要走 `None` 那条（JS 里空串是 falsy），所以是先
    /// `filter(|s| !s.is_empty())` 再 split。
    fn wanted(&self) -> Option<Vec<&'a str>> {
        self.providers.filter(|s| !s.is_empty()).map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect()
        })
    }
}

/// 语义①：目标筛选 —— 复刻 `src/providers/index.js:147-158`。
///
/// ```js
/// if (!p.enabled || (wanted && !wanted.includes(p.id))) return false;
/// if (browse && !p.browseable) return false;
/// if (page > 1 && !p.paginated && !p.demo) return false;
/// return true;
/// ```
///
/// ⚠️ 第 3 条**是为了防重复**：单发引擎忽略 `page`，翻页只会返回同一批结果、再被
/// 去重掉。JS 这里还会 `concat(dynamicTorznabProviders())` —— 在 Rust 里，动态
/// provider 由**调用方**并进 `registry` 再传进来（D17 的口子就开在这个签名上：
/// 聚合层只认 `&[Box<dyn Provider>]`，不关心它是编译期的还是运行期来的）。
pub fn resolve_targets<'a>(
    registry: &'a [Box<dyn Provider>],
    args: &SearchArgs<'_>,
) -> Vec<&'a dyn Provider> {
    let wanted = args.wanted();

    registry
        .iter()
        // `&**p` 而不是 `p.as_ref()`：Box 的 `as_ref` 有多个候选实现，
        // 显式解引用没有歧义。
        .map(|p| &**p)
        .filter(|p| {
            if !p.enabled() {
                return false;
            }
            if let Some(w) = &wanted {
                if !w.contains(&p.id()) {
                    return false;
                }
            }
            if args.browse && !p.browseable() {
                return false;
            }
            if args.page > 1 && !p.paginated() && !p.demo() {
                return false;
            }
            true
        })
        .collect()
}

/// 语义②前半：`runProvider` —— 复刻 `src/providers/index.js:163-183`。
///
/// 单个 provider **隔离**运行并**单独计时**，返回 `(results, status)`（JS 原样）。
///
/// ⚠️ 错误路径的形状要逐字对齐 JS：
/// - `status: out.error ? 'error' : 'ok'`、`count: out.results.length`、
///   `error: out.error || null`、`hasMore: out.hasMore`（可能是 `undefined`）、
///   `ms: Date.now() - startedAt`。
/// - JS 的 **catch 分支没有 `hasMore` 键** ⇒ 崩溃时也是 `undefined` ⇒ 这里 `None`。
///   （本轮不做 catch —— 见模块文档第 2 条；`None` 的语义已经摆正。）
async fn run_provider<'a>(
    p: &'a dyn Provider,
    http: &'a HttpClient,
    query: &'a str,
    page: u32,
) -> (Vec<TorrentResult>, ProviderStatus) {
    let started = Instant::now();

    // ⚠️ **兜 panic**（2026-10-16 补上 —— 此前是"刻意不做"，见模块文档第 2 条）。
    //
    // provider 的契约是"**永不 panic**"，但契约是**约定**、不是**保证**。
    // JS 侧 `runProvider` 用 `try/catch`，连"抛异常"一起兜；Rust 不给兜的话，
    // 一个违约的 provider 会**带走整轮聚合** —— 别人已经返回的结果也一起没。
    // 这是全套移植里"Rust 比 JS 更脆"的唯一一处。
    //
    // ⚠️ 必须 `AssertUnwindSafe`：`&dyn Provider` 与 reqwest 的 future 都不是
    // `UnwindSafe`（内部有可变状态）。这是**有意的接受** —— panic 之后我们不再信任
    // **那一家** provider 的局部状态，但整轮搜索不该因此作废。
    let out = match AssertUnwindSafe(p.search(http, query, page))
        .catch_unwind()
        .await
    {
        Ok(out) => out,
        // 文案与其它失败同形（`<id> unreachable (...)` 那一类），前端可直接显示
        Err(_) => SearchOutcome::err(format!("{} panicked", p.id())),
    };

    let status = ProviderStatus {
        status: if out.error.is_some() {
            ProviderState::Error
        } else {
            ProviderState::Ok
        },
        count: out.results.len(),
        error: out.error,
        has_more: out.has_more,
        ms: started.elapsed().as_millis() as u64,
    };

    (out.results, status)
}

/// 语义②：并发池 —— 复刻 `src/providers/index.js:204-219`。
///
/// JS 的行为：任务**按需启动**、同时在飞 ≤ `limit`；**每个任务一完成就回调
/// `on_settle`**，而且**在下一个任务启动之前**（因为 JS 那边 `onSettle` 挂在
/// `Promise.race` 之前注册的 `.then` 上）。所以调用方能做到「谁先回来先显示谁」。
///
/// ## ⚠️ 勘明更正（2026-10-16）
///
/// 文档原先说「`onSettle` 是流式的**唯一**原因」——**不准确**。JS 里
/// `asyncPool(tasks, limit, onSettle)` 的 `onSettle` **在两个调用点都没被传过**
/// （`search()` 与 `searchStream()` 都只传 2 个参数，它是死参数）；流式实际来自
/// **每个 task 自己链上的 `.then(...)`**（`index.js:234-237` / `257-262`）。
///
/// 这里保留 `on_settle` 回调，因为它正是那个 `.then` 的等价物。验收测试同时钉两条：
/// ① 一完成就回调（不等慢的）；② 同时在飞 ≤ `limit`。
///
/// ## 契约
///
/// `on_settle` **恰好被调用一次/任务**，顺序 = **完成顺序**
/// （= JS 写 `perProvider` 的顺序，也就是 `providers` 的 key 顺序）。
///
/// ## `Send` 约束（2026-10-16 加，M4 逼出来的）
///
/// 与 [`crate::BoxFuture`] 同一件事故的传播：GUI 要把搜索丢到后台线程，
/// 于是整条链路上的 future 都得 `Send`。`on_settle` 会被跨 await 持有，
/// 所以它也必须 `Send`。详见 [`crate::BoxFuture`] 的说明。
pub async fn async_pool<F, Fut, T>(tasks: Vec<F>, limit: usize, mut on_settle: impl FnMut(T) + Send)
where
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output = T> + Send,
{
    // JS 的 limit 不会是 0（常量 8）；这里防一手，免得 limit=0 时一个都不启动。
    let limit = limit.max(1);

    let mut iter = tasks.into_iter();
    let mut pending = FuturesUnordered::new();

    // 先灌满 limit 个（JS：for 循环里每到 limit 就 await 一次，等效）。
    for _ in 0..limit {
        match iter.next() {
            Some(task) => pending.push(task()),
            None => break,
        }
    }

    while let Some(out) = pending.next().await {
        // ⚠️ 这两行的顺序不能反：**先把结果交出去，再启动下一个任务**。
        // 反过来的话，`on_settle` 就变成"下一批已经跑起来了才收到上一批的结果"，
        // 虽然「在飞 ≤ limit」依然成立，但「谁先回来先显示谁」的时效性会变差。
        on_settle(out);
        if let Some(task) = iter.next() {
            pending.push(task());
        }
    }
}

/// 语义③：聚合 `hasMore` —— 复刻 `src/providers/index.js:190-198`。
///
/// ```js
/// return targets.some((p) => {
///   const s = perProvider[p.id];
///   if (!s || s.status !== 'ok' || s.count === 0) return false;  // 出错或没结果 → 没有下一页
///   if (typeof s.hasMore === 'boolean') return s.hasMore;         // provider 明确说的优先
///   return !!p.paginated;                                         // 否则按 paginated 猜
/// });
/// ```
///
/// ⚠️ **`typeof s.hasMore === 'boolean'` 是分水岭** —— 这就是
/// [`crate::SearchOutcome::has_more`] 必须是 `Option<bool>` 的原因：
/// - `has_more == Some(false)` → **provider 明确否决** → `false`（哪怕它是 paginated）
/// - `has_more == None` → 没表态 → 回退到 `p.paginated()`
///
/// 另外 `!s`（provider 没上报）在 JS 里是「targets 里有、perProvider 里没有」的
/// 兜底；正常路径下不会发生，但照抄这个分支。
pub fn aggregate_has_more(
    targets: &[&dyn Provider],
    per_provider: &[(String, ProviderStatus)],
) -> bool {
    targets.iter().any(|p| {
        let status = match per_provider.iter().find(|(id, _)| id == p.id()) {
            Some((_, s)) => s,
            None => return false,
        };

        // 出错、或这一页没有结果 → 没有下一页。
        if status.status != ProviderState::Ok || status.count == 0 {
            return false;
        }

        // provider 明确表态（JS 里只有 demo 会）优先；没表态才按 paginated 猜。
        match status.has_more {
            Some(explicit) => explicit,
            None => p.paginated(),
        }
    })
}

/// JS `search()` 的返回 —— `{ results, providers, hasMore }`（`index.js:243`）。
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateOutcome {
    /// ⚠️ 顺序 = **provider 完成顺序**（照抄 JS 的 `allResults.push(...results)`，
    /// 它在每个 task 自己的 `.then` 里跑），provider 内部保持自己的结果顺序。
    pub results: Vec<TorrentResult>,
    /// ⚠️ 用 `Vec` 而不是 map：JS 的 `perProvider` 是对象，**key 顺序 = 完成顺序**
    /// （`perProvider[p.id] = status` 也是在各 task 的 `.then` 里执行的）。
    /// 换成 `BTreeMap` 会把这个顺序丢掉。
    pub providers: Vec<(String, ProviderStatus)>,
    /// 聚合出来的「还有下一页吗」，见 [`aggregate_has_more`]。
    pub has_more: bool,
}

/// JS `search()` —— `src/providers/index.js:227-244`。
///
/// 并发跑所有目标 provider（上限 [`MAX_CONCURRENT`]），**谁先回来先记谁**，
/// 最后汇总。
pub async fn search(
    http: &HttpClient,
    registry: &[Box<dyn Provider>],
    query: &str,
    args: SearchArgs<'_>,
) -> AggregateOutcome {
    let targets = resolve_targets(registry, &args);
    let page = args.page;

    let mut per_provider: Vec<(String, ProviderStatus)> = Vec::new();
    let mut all_results: Vec<TorrentResult> = Vec::new();

    let tasks: Vec<_> = targets
        .iter()
        .map(|p| {
            move || async move {
                let (results, status) = run_provider(*p, http, query, page).await;
                (*p, results, status)
            }
        })
        .collect();

    async_pool(tasks, MAX_CONCURRENT, |(p, results, status)| {
        per_provider.push((p.id().to_string(), status));
        all_results.extend(results);
    })
    .await;

    let has_more = aggregate_has_more(&targets, &per_provider);

    AggregateOutcome {
        results: all_results,
        providers: per_provider,
        has_more,
    }
}

/// JS `searchStream()` 的返回 —— `{ providers, hasMore }`。
///
/// ⚠️ **没有 `results`**（JS 原文就没有）：结果通过 `on_provider` 一站一站交出去。
#[derive(Debug, Clone, PartialEq)]
pub struct StreamOutcome {
    pub providers: Vec<(String, ProviderStatus)>,
    pub has_more: bool,
}

/// JS `searchStream()` —— `src/providers/index.js:251-269`。
///
/// 和 [`search`] 同一套并发池，区别只是**每站一回来就回调 `on_provider`**
/// （对应原来的 SSE `event: provider`；M4 之后是 Slint 的
/// `invoke_from_event_loop`）。这就是「8 路并发 + 慢站不拖累」的体验来源。
pub async fn search_stream(
    http: &HttpClient,
    registry: &[Box<dyn Provider>],
    query: &str,
    args: SearchArgs<'_>,
    mut on_provider: impl FnMut(ProviderReport) + Send,
) -> StreamOutcome {
    let targets = resolve_targets(registry, &args);
    let page = args.page;

    let mut per_provider: Vec<(String, ProviderStatus)> = Vec::new();

    let tasks: Vec<_> = targets
        .iter()
        .map(|p| {
            move || async move {
                let (results, status) = run_provider(*p, http, query, page).await;
                (*p, results, status)
            }
        })
        .collect();

    async_pool(tasks, MAX_CONCURRENT, |(p, results, status)| {
        // 顺序照 JS：先落 perProvider，再回调。
        per_provider.push((p.id().to_string(), status.clone()));
        on_provider(ProviderReport {
            id: p.id().to_string(),
            name: p.name().to_string(),
            results,
            status,
        });
    })
    .await;

    let has_more = aggregate_has_more(&targets, &per_provider);

    StreamOutcome {
        providers: per_provider,
        has_more,
    }
}
