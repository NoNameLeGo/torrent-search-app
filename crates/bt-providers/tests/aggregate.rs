//! M2 验收：`src/providers/index.js` 的三条语义**逐条钉死**。
//!
//! 真值来源只有那份 JS（271 行），断言里的注释都标了对应行号。
//! 每个用例都用**假 provider**，全程不联网。
//!
//! | 用例 | 钉住什么 |
//! |---|---|
//! | [`semantic_1_resolve_targets_replicates_js_filters`] | 语义① 三条过滤 + `?providers=` 的三个边界 |
//! | [`semantic_2_pool_settles_before_starting_the_next`] | 语义② **先回调再起下一个** + 在飞 ≤ limit |
//! | [`semantic_2_one_failing_provider_does_not_break_the_others`] | 语义② 单站失败不影响其他 |
//! | [`semantic_2_ms_is_per_provider_not_global`] | 语义② 每站单独计时 |
//! | [`semantic_3_aggregate_has_more_truth_table`] | 语义③ 真值表（含**显式 false vs 没表态**） |
//! | [`search_reports_has_more_through_the_whole_pipeline`] | ③ 与 ② 串起来 |
//! | [`search_stream_reports_a_fast_provider_before_a_slow_one_finishes`] | 「谁先回来先显示谁」 |
//! | [`registry_matches_the_js_registry_order_and_flags`] | 17 家的顺序与旗标 |
//! | [`providers_without_a_resolver_return_the_js_404_text`] | trait 默认方法的 `no resolver` |

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bt_core::http::HttpClient;
use bt_core::TorrentResult;
use bt_providers::aggregate::{
    aggregate_has_more, async_pool, resolve_targets, search, search_stream, SearchArgs,
};
use bt_providers::{
    registry, BoxFuture, Provider, ProviderReport, ProviderState, ProviderStatus, SearchOutcome,
};

// ---------------------------------------------------------------------------
// 夹具：一个完全可控的假 provider（永不联网）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Fake {
    id: &'static str,
    name: &'static str,
    enabled: bool,
    paginated: bool,
    browseable: bool,
    demo: bool,
    /// 返回几条结果
    hits: usize,
    /// 延迟多少毫秒（用来造「慢站」）
    delay_ms: u64,
    /// 返回 `SearchOutcome::err`
    fail: bool,
    /// 显式表态的 `hasMore`（`None` = 没表态，走 `paginated` 回退）
    has_more: Option<bool>,
}

impl Fake {
    fn new(id: &'static str) -> Self {
        Self {
            id,
            name: id,
            enabled: true,
            paginated: false,
            browseable: false,
            demo: false,
            hits: 1,
            delay_ms: 0,
            fail: false,
            has_more: None,
        }
    }

    fn paginated(mut self) -> Self {
        self.paginated = true;
        self
    }

    fn browseable(mut self) -> Self {
        self.browseable = true;
        self
    }

    fn demo(mut self) -> Self {
        self.demo = true;
        self
    }

    fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    fn hits(mut self, n: usize) -> Self {
        self.hits = n;
        self
    }

    fn delay(mut self, ms: u64) -> Self {
        self.delay_ms = ms;
        self
    }

    fn fail(mut self) -> Self {
        self.fail = true;
        self
    }

    fn stating_has_more(mut self, v: bool) -> Self {
        self.has_more = Some(v);
        self
    }
}

impl Provider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn paginated(&self) -> bool {
        self.paginated
    }

    fn browseable(&self) -> bool {
        self.browseable
    }

    fn demo(&self) -> bool {
        self.demo
    }

    fn search<'a>(
        &'a self,
        _http: &'a HttpClient,
        query: &'a str,
        page: u32,
    ) -> BoxFuture<'a, SearchOutcome> {
        Box::pin(async move {
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            if self.fail {
                return SearchOutcome::err(format!("{} boom", self.id));
            }
            let results = (0..self.hits)
                .map(|i| hit(&format!("{}-{query}-{page}-{i}", self.id)))
                .collect();
            let mut out = SearchOutcome::ok(results);
            out.has_more = self.has_more;
            out
        })
    }
}

fn hit(name: &str) -> TorrentResult {
    TorrentResult {
        id: name.to_string(),
        name: name.to_string(),
        size: None,
        size_text: String::new(),
        seeders: None,
        leechers: None,
        date: None,
        date_text: String::new(),
        magnet: None,
        info_hash: None,
        provider: "fake".to_string(),
        category: None,
        detail_url: None,
        files: None,
        needs_magnet: false,
    }
}

fn reg(items: Vec<Fake>) -> Vec<Box<dyn Provider>> {
    items
        .into_iter()
        .map(|f| Box::new(f) as Box<dyn Provider>)
        .collect()
}

fn args<'a>(providers: Option<&'a str>, page: u32, browse: bool) -> SearchArgs<'a> {
    SearchArgs {
        providers,
        page,
        browse,
    }
}

fn ids(ts: &[&dyn Provider]) -> Vec<&'static str> {
    ts.iter().map(|p| p.id()).collect()
}

fn status_of(out: &bt_providers::aggregate::AggregateOutcome, id: &str) -> ProviderStatus {
    out.providers
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, s)| s.clone())
        .unwrap_or_else(|| panic!("perProvider 里没有 {id}"))
}

fn st(state: ProviderState, count: usize, has_more: Option<bool>) -> ProviderStatus {
    ProviderStatus {
        status: state,
        count,
        error: None,
        has_more,
        ms: 0,
    }
}

// ---------------------------------------------------------------------------
// 语义①：目标筛选（src/providers/index.js:147-158）
// ---------------------------------------------------------------------------

#[test]
fn semantic_1_resolve_targets_replicates_js_filters() {
    let registry = reg(vec![
        Fake::new("a"),
        Fake::new("b").paginated(),
        Fake::new("c").browseable(),
        Fake::new("d").demo(),
        Fake::new("e").disabled(),
    ]);

    // 第 1 页、非 browse：所有 enabled 的都上（`e` 被 enabled 挡掉）
    assert_eq!(
        ids(&resolve_targets(&registry, &args(None, 1, false))),
        ["a", "b", "c", "d"]
    );

    // ⚠️ 第 3 条：page > 1 时丢掉「不 paginated 且不 demo」的 —— 为了防重复
    // （单发引擎忽略 page，翻页只会返回同一批结果再被去重掉）
    assert_eq!(
        ids(&resolve_targets(&registry, &args(None, 2, false))),
        ["b", "d"]
    );

    // browse：只要 browseable 的
    assert_eq!(
        ids(&resolve_targets(&registry, &args(None, 1, true))),
        ["c"]
    );

    // 第 1 条后半：`wanted` 过滤
    assert_eq!(
        ids(&resolve_targets(&registry, &args(Some("a,c"), 1, false))),
        ["a", "c"]
    );

    // ⚠️ 边界：空串 = **全都要**（JS 里 "" 是 falsy → wanted = null）
    assert_eq!(
        ids(&resolve_targets(&registry, &args(Some(""), 1, false))),
        ["a", "b", "c", "d"]
    );

    // ⚠️ 边界：纯空白 = **一个都不查**
    // （JS 里 "  " 是 truthy → split → ["",""] → filter(Boolean) → [] → includes() 恒 false）
    assert!(resolve_targets(&registry, &args(Some("  "), 1, false)).is_empty());

    // ⚠️ 边界：逗号里的空段被 filter(Boolean) 去掉，两侧空格被 trim
    assert_eq!(
        ids(&resolve_targets(
            &registry,
            &args(Some(" a ,, b "), 1, false)
        )),
        ["a", "b"]
    );
}

// ---------------------------------------------------------------------------
// 语义②：并发池（src/providers/index.js:204-219）
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Ev {
    /// 任务工厂被调用（= JS 里 `task()` 被调用的那一刻）
    Start(usize),
    /// `on_settle` 被调用（= JS 里 task 链上的 `.then` 跑的那一刻）
    Settle(usize),
}

/// ⚠️ **本层最容易写错、且错了不明显的一条**：`on_settle` 必须在**下一个任务启动之前**
/// 被调用。写成「全部跑完再一起回调」的话，8 路并发的「谁先回来先显示谁」就没了，
/// 而在飞上限可能仍然成立 —— 所以只测「≤ limit」抓不到它。
///
/// 这里用**事件序**钉死：第 k 个任务启动时，已经收到的结果数必须 ≥ `k - limit`。
#[tokio::test]
async fn semantic_2_pool_settles_before_starting_the_next() {
    const LIMIT: usize = 3;
    const N: usize = 12;

    let log: Arc<Mutex<Vec<Ev>>> = Arc::new(Mutex::new(Vec::new()));

    let tasks: Vec<_> = (0..N)
        .map(|i| {
            let log = Arc::clone(&log);
            move || {
                log.lock().unwrap().push(Ev::Start(i));
                async move {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    i
                }
            }
        })
        .collect();

    let settle_log = Arc::clone(&log);
    async_pool(tasks, LIMIT, move |i: usize| {
        settle_log.lock().unwrap().push(Ev::Settle(i));
    })
    .await;

    let log = log.lock().unwrap();
    let mut started = 0usize;
    let mut settled = 0usize;

    for ev in log.iter() {
        match ev {
            Ev::Start(_) => {
                started += 1;
                assert!(
                    started - settled <= LIMIT,
                    "同时在飞超过上限：{started} 个已启动 / {settled} 个已收到"
                );
                if started > LIMIT {
                    assert!(
                        settled >= started - LIMIT,
                        "第 {started} 个任务启动时只收到了 {settled} 个结果 —— \
                         on_settle 没有在下一次启动之前调用（这正是 JS 的 asyncPool 保证的）"
                    );
                }
            }
            Ev::Settle(_) => settled += 1,
        }
    }

    assert_eq!(started, N, "每个任务都该被启动一次");
    assert_eq!(settled, N, "每个任务都该回调一次（不多不少）");

    // 顺带把两个载荷也用上（它们之前只被当成占位符）：
    // 任务必须**按声明顺序**启动；settle 是对 0..N 的一个排列。
    let starts: Vec<usize> = log
        .iter()
        .filter_map(|e| match e {
            Ev::Start(i) => Some(*i),
            Ev::Settle(_) => None,
        })
        .collect();
    assert_eq!(starts, (0..N).collect::<Vec<_>>(), "任务按声明顺序启动");

    let mut settles: Vec<usize> = log
        .iter()
        .filter_map(|e| match e {
            Ev::Settle(i) => Some(*i),
            Ev::Start(_) => None,
        })
        .collect();
    settles.sort_unstable();
    assert_eq!(
        settles,
        (0..N).collect::<Vec<_>>(),
        "每个任务恰好 settle 一次"
    );
}

#[tokio::test]
async fn semantic_2_one_failing_provider_does_not_break_the_others() {
    let registry = reg(vec![
        Fake::new("ok1").hits(2),
        Fake::new("bad").fail(),
        Fake::new("ok2").hits(3),
    ]);

    let out = search(
        &HttpClient::new(),
        &registry,
        "ubuntu",
        args(None, 1, false),
    )
    .await;

    // 三家都上报了，两家的结果都在
    assert_eq!(out.providers.len(), 3);
    assert_eq!(out.results.len(), 5, "2 + 0 + 3");

    let ok1 = status_of(&out, "ok1");
    assert_eq!(ok1.status, ProviderState::Ok);
    assert_eq!(ok1.count, 2);

    let bad = status_of(&out, "bad");
    assert_eq!(bad.status, ProviderState::Error);
    assert_eq!(bad.count, 0);
    assert!(bad.error.unwrap().contains("boom"));
    // ⚠️ 出错路径**不填 hasMore**（对齐 JS catch 分支没有这个 key）
    assert_eq!(bad.has_more, None);
}

/// 「每个 provider 单独计时 `ms`」—— 如果是整池计时，快的那个也会是慢站的耗时。
#[tokio::test]
async fn semantic_2_ms_is_per_provider_not_global() {
    let registry = reg(vec![Fake::new("fast"), Fake::new("slow").delay(40)]);
    let out = search(&HttpClient::new(), &registry, "q", args(None, 1, false)).await;

    let fast = status_of(&out, "fast").ms;
    let slow = status_of(&out, "slow").ms;

    assert!(slow >= 40, "慢站至少睡够 40ms，实测 {slow}ms");
    assert!(
        fast < 40,
        "快站不该背慢站的耗时（说明 ms 是整池计时而不是每站计时）：{fast}ms"
    );
}

// ---------------------------------------------------------------------------
// 语义③：聚合 hasMore（src/providers/index.js:190-198）
// ---------------------------------------------------------------------------

#[test]
fn semantic_3_aggregate_has_more_truth_table() {
    let a = Fake::new("a").paginated();
    let b = Fake::new("b"); // 非 paginated
    let targets: Vec<&dyn Provider> = vec![&a, &b];

    let has_more = |entries: Vec<(&str, ProviderStatus)>| {
        let v: Vec<(String, ProviderStatus)> = entries
            .into_iter()
            .map(|(k, s)| (k.to_string(), s))
            .collect();
        aggregate_has_more(&targets, &v)
    };

    // 出错 → 没有下一页
    assert!(!has_more(vec![("a", st(ProviderState::Error, 5, None))]));

    // 没结果 → 没有下一页（哪怕它是 paginated）
    assert!(!has_more(vec![("a", st(ProviderState::Ok, 0, None))]));

    // ⚠️ **provider 明确说 false 就得听它** —— 即使它是 paginated。
    // 这条正是 `SearchOutcome.has_more` 必须从 `bool` 改成 `Option<bool>` 的理由：
    // 用 bool 的话「没表态」与「明确 false」分不开，这里会错判成 true。
    assert!(!has_more(vec![(
        "a",
        st(ProviderState::Ok, 3, Some(false))
    )]));

    // provider 明确说 true 也得听它 —— 即使它不是 paginated
    assert!(has_more(vec![("b", st(ProviderState::Ok, 3, Some(true)))]));

    // 没表态 + paginated → 按 paginated 猜「有下一页」
    assert!(has_more(vec![("a", st(ProviderState::Ok, 3, None))]));

    // 没表态 + 非 paginated → 没有下一页
    assert!(!has_more(vec![("b", st(ProviderState::Ok, 3, None))]));

    // JS 的 `!s` 兜底：targets 里有、perProvider 里没有
    assert!(!has_more(vec![("zzz", st(ProviderState::Ok, 3, None))]));

    // `some()` 语义：任一家说有就有
    assert!(has_more(vec![
        ("a", st(ProviderState::Ok, 3, None)),
        ("b", st(ProviderState::Ok, 3, Some(true))),
    ]));
}

#[tokio::test]
async fn search_reports_has_more_through_the_whole_pipeline() {
    let registry = reg(vec![
        Fake::new("page").paginated().hits(2),
        Fake::new("one").hits(2),
    ]);

    let out = search(&HttpClient::new(), &registry, "q", args(None, 1, false)).await;
    assert!(out.has_more, "paginated 那家有结果 → 有下一页");

    // 翻到第 2 页：非 paginated 的被筛掉，只剩 `page` 一家
    let out2 = search(&HttpClient::new(), &registry, "q", args(None, 2, false)).await;
    assert_eq!(out2.providers.len(), 1);
    assert_eq!(out2.providers[0].0, "page");
}

/// ⚠️ **`Option<bool>` 的存在理由，走整条流水线再钉一遍。**
///
/// 上面 [`semantic_3_aggregate_has_more_truth_table`] 是单元级；
/// 这里证明「provider 明确否决」能**穿过 `search()` 一路影响到聚合结果**。
/// ——`has_more: bool` 的话，本用例最后一条断言会失败（会错判成 `true`）。
#[tokio::test]
async fn a_paginated_provider_can_explicitly_veto_has_more() {
    // 两家都 paginated、都有结果、都没表态 → 按 paginated 猜「有下一页」
    let registry = reg(vec![
        Fake::new("p1").paginated().hits(2),
        Fake::new("p2").paginated().hits(2),
    ]);
    let out = search(&HttpClient::new(), &registry, "q", args(None, 1, false)).await;
    assert!(out.has_more, "都没表态 + 都 paginated → 猜有下一页");

    // 只有一家明确否决：`some()` 语义下 p2 还没表态 → 整体仍是 true
    let registry = reg(vec![
        Fake::new("p1").paginated().hits(2).stating_has_more(false),
        Fake::new("p2").paginated().hits(2),
    ]);
    let out = search(&HttpClient::new(), &registry, "q", args(None, 1, false)).await;
    assert!(out.has_more, "p2 没表态且 paginated → 整体仍有下一页");

    // 两家都明确否决 → **必须听 provider 的**
    let registry = reg(vec![
        Fake::new("p1").paginated().hits(2).stating_has_more(false),
        Fake::new("p2").paginated().hits(2).stating_has_more(false),
    ]);
    let out = search(&HttpClient::new(), &registry, "q", args(None, 1, false)).await;
    assert!(
        !out.has_more,
        "两家 paginated 的 provider 都明确说「没有下一页」——必须听它们的。\
         若 `has_more` 是 `bool`，这里会被错判成 true（这正是改成 Option<bool> 的原因）"
    );

    // 明确表态也要能透传进每站状态（UI 状态栏会读它）
    assert_eq!(status_of(&out, "p1").has_more, Some(false));
}

// ---------------------------------------------------------------------------
// 流式：一回来就上报（「谁先回来先显示谁」）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_stream_reports_a_fast_provider_before_a_slow_one_finishes() {
    let registry = reg(vec![Fake::new("fast"), Fake::new("slow").delay(60)]);

    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);

    let out = search_stream(
        &HttpClient::new(),
        &registry,
        "q",
        args(None, 1, false),
        move |rep: ProviderReport| sink.lock().unwrap().push(rep.id.clone()),
    )
    .await;

    let order = seen.lock().unwrap().clone();
    assert_eq!(order.len(), 2, "两家都该上报");
    assert_eq!(
        order[0], "fast",
        "快的必须先被上报 —— 如果实现是「全部跑完再一起 emit」，这里会是 slow 先（或顺序不定）"
    );

    // `providers` 的顺序 = 完成顺序（对齐 JS 对象的 key 顺序）
    let keys: Vec<String> = out.providers.iter().map(|(k, _)| k.clone()).collect();
    assert_eq!(keys, order);
}

// ---------------------------------------------------------------------------
// 注册表 & trait 默认方法
// ---------------------------------------------------------------------------

#[test]
fn registry_matches_the_js_registry_order_and_flags() {
    let list = registry::list();

    // 顺序 = JS REGISTRY 里已搬的那些的相对顺序（= UI 显示顺序）
    // ⚠️ 19 条：18 个站点 + **demo**。JS 的 REGISTRY 里 demo 也是**最后一条**
    // （`src/providers/index.js:121`），所以这里也放最后。
    // `dmhy` / `subsplease` 是 2026-10-21 搬的，按 JS 顺序插在 `torrentscsv` 之后
    // （JS 里它们之间还夹着 animetosho / bangumimoe / mikan 等**尚未搬**的站点）。
    let ids: Vec<&str> = list.iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        [
            "tpb",
            "nyaa",
            "yts",
            "knaben",
            "torrentscsv",
            "dmhy",
            "subsplease",
            "limetorrents",
            "therarbg",
            "rutor",
            "torrent9",
            "zeromagnet",
            "oxtorrent",
            "audiobookbay",
            "filemood",
            "internetarchive",
            "linuxtracker",
            "megapeer",
            "demo",
        ]
    );

    // 旗标逐条核对过（见 AGENTS.md 勘明 ⑦）
    // `dmhy` 在 JS 里是 `paginated: true`（2026-10-21 搬入）；`subsplease` 不是。
    let paginated: Vec<&str> = list.iter().filter(|p| p.paginated).map(|p| p.id).collect();
    assert_eq!(paginated, ["nyaa", "dmhy", "limetorrents", "rutor"]);

    // 全部 enabled（JS 的 19 条都是 enabled: true）
    assert!(list.iter().all(|p| p.enabled));

    // ⚠️ **恰好一家 demo，且它是最后一条** —— JS 里 `demo` 是唯一个 page>1 的豁免分支
    // （`index.js:155` 的 `page > 1 && !p.paginated && !p.demo`）。
    // 它也**不是** paginated（JS 没给它设那个旗标），但 browseable。
    let demos: Vec<&str> = list.iter().filter(|p| p.demo).map(|p| p.id).collect();
    assert_eq!(demos, ["demo"], "只有 demo 一家是 demo");
    assert_eq!(ids.last(), Some(&"demo"), "demo 排在最后（照 JS 顺序）");
    let demo = list.iter().find(|p| p.id == "demo").unwrap();
    assert!(!demo.paginated, "JS 没给 demo 设 paginated —— 别顺手改");
    // ⚠️ `ProviderInfo` 里**没有** `browseable` 字段（JS 的 `list()` 也只返回
    // `{ id, name, enabled, demo, paginated }`）⇒ 要查 browseable 得拿**真正的
    // Provider 实例**。第一版我直接写了 `demo.browseable` ⇒ CI 报 E0609。
    let demo_provider = registry::registry()
        .into_iter()
        .find(|p| p.id() == "demo")
        .expect("registry() 里应该有 demo");
    assert!(demo_provider.browseable(), "demo 是 browseable 的");

    // 名字抽查：与 JS `module.exports` 逐字一致（大小写敏感）
    let name_of = |id: &str| list.iter().find(|p| p.id == id).unwrap().name;
    assert_eq!(name_of("tpb"), "The Pirate Bay");
    assert_eq!(name_of("nyaa"), "NYAA");
    assert_eq!(name_of("therarbg"), "TheRarBg");
    assert_eq!(name_of("audiobookbay"), "AudioBookBay");
    assert_eq!(name_of("internetarchive"), "InternetArchive");
    assert_eq!(name_of("zeromagnet"), "ZeroMagnet");
    assert_eq!(name_of("dmhy"), "DMHY");
    assert_eq!(name_of("subsplease"), "SubsPlease");
    assert_eq!(name_of("demo"), "Demo (offline)", "JS demo.js 的 name");
}

/// 没有 resolver 的那 10 家走 trait 默认方法 —— 错误串必须是 JS 的 404 文案
/// （**有空格**；`docs/ROADMAP.md` §2.4 早期草案写的 `no_resolver` 是错的）。
#[tokio::test]
async fn providers_without_a_resolver_return_the_js_404_text() {
    let out = registry::Tpb
        .resolve_magnet(&HttpClient::new(), "https://example.invalid/detail")
        .await;

    assert_eq!(out.error.as_deref(), Some("no resolver"));
    assert!(out.magnet.is_none());
    assert!(out.info_hash.is_none());
}

/// **违约 panic 的 provider（异步段）。** 契约说"永不 panic"，这个故意违约。
///
/// ⚠️ 这里有**两个**违约点、必须各有一个用例，不能只留一个：
///   · 异步段（`panic` 在 `async move {}` 里，被 poll 时才炸）→ `Panicky`
///   · **同步段**（`search()` 本身当场炸，压根没返回 future）→ `PanickySync`
/// 后者曾经漏网：旧写法 `AssertUnwindSafe(p.search(..)).catch_unwind()`
/// 只包住了**返回的 future**，而 `search(..)` 的调用发生在守卫**之外** —— 同步段的
/// panic 照旧带走整轮聚合，而用例全绿（因为唯一那条用例把 panic 放在了 future 里）。
struct Panicky {
    id: &'static str,
}

impl Provider for Panicky {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.id
    }

    fn search<'a>(
        &'a self,
        _http: &'a HttpClient,
        _query: &'a str,
        _page: u32,
    ) -> BoxFuture<'a, SearchOutcome> {
        let id = self.id;
        Box::pin(async move {
            panic!("{id} violated the never-panic contract");
        })
    }
}

/// 判据：**违约 panic 的 provider 不会掀翻别人**（异步段）。
///
/// 这是全套移植里"Rust 比 JS 更脆"的**唯一一处** —— JS 的 `runProvider` 用
/// `try/catch` 兜住抛异常，而 Rust 此前不给兜：一个违约的 provider 会带走整轮聚合，
/// **别人已经返回的结果也一起没**。
///
/// ⚠️ 这条用例的价值全在"它会失败"：把 `run_provider` 里的 `catch_unwind` 去掉，
/// 这个测试就会以 panic 的形式红掉（而不是静默通过）。
///
/// ⚠️ **但只靠它不够** —— 它把 panic 放在 future 里，所以"调用 `search()` 在守卫之外"
/// 那个 bug 它能过。同步段那半在下面 [`a_synchronously_panicking_provider_is_also_isolated`]。
#[tokio::test]
async fn a_panicking_provider_does_not_take_down_the_others() {
    let good = Fake::new("good").hits(2);
    let registry: Vec<Box<dyn Provider>> =
        vec![Box::new(Panicky { id: "panicky" }), Box::new(good)];

    let out = search(
        &HttpClient::new(),
        &registry,
        "ubuntu",
        args(None, 1, false),
    )
    .await;

    // ① 好那家的结果必须**完整地**回来
    assert_eq!(out.results.len(), 2, "别人的结果不该被带走");
    assert!(
        out.results.iter().all(|r| r.id.starts_with("good-")),
        "回来的必须只有好那家的结果"
    );

    // ② 违约那家自己变成一条普通错误 —— 与"连不上"同形，前端不用特殊处理
    let bad = status_of(&out, "panicky");
    assert_eq!(bad.status, ProviderState::Error);
    assert_eq!(bad.error.as_deref(), Some("panicky panicked"));
    assert_eq!(bad.count, 0);
    // JS 的 catch 分支没有 hasMore 键 ⇒ `None`（与"连不上"一致）
    assert_eq!(bad.has_more, None);

    // ③ 好那家仍然是 Ok
    assert_eq!(status_of(&out, "good").status, ProviderState::Ok);
}

/// **违约 panic 的 provider（同步段）** —— `search()` 本身当场炸，不返回 future。
struct PanickySync {
    id: &'static str,
}

impl Provider for PanickySync {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.id
    }

    fn search<'a>(
        &'a self,
        _http: &'a HttpClient,
        _query: &'a str,
        _page: u32,
    ) -> BoxFuture<'a, SearchOutcome> {
        // ⚠️ 炸在**构造 future 之前** —— 这正是旧写法兜不住的那一段。
        // 谁想"顺手"把它挪进 async 块里，就是把这个用例废掉。
        panic!(
            "{} violated the never-panic contract synchronously",
            self.id
        );
    }
}

/// 判据：**同步段的 panic 同样不掀翻别人。**
///
/// ⚠️ 去掉 `run_provider` 里那个把调用包进闭包的写法（改成
/// `AssertUnwindSafe(p.search(..)).catch_unwind()`）这条就会红 —— 它存在的意义就是这个。
#[tokio::test]
async fn a_synchronously_panicking_provider_is_also_isolated() {
    let good = Fake::new("good").hits(2);
    let registry: Vec<Box<dyn Provider>> =
        vec![Box::new(PanickySync { id: "panicky-sync" }), Box::new(good)];

    let out = search(
        &HttpClient::new(),
        &registry,
        "ubuntu",
        args(None, 1, false),
    )
    .await;

    assert_eq!(out.results.len(), 2, "别人的结果不该被带走");
    let bad = status_of(&out, "panicky-sync");
    assert_eq!(bad.status, ProviderState::Error);
    assert_eq!(bad.error.as_deref(), Some("panicky-sync panicked"));
    assert_eq!(status_of(&out, "good").status, ProviderState::Ok);
}
