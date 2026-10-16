//! # bt-app —— 唯一的 UI（M4）
//!
//! ## ⚠️ 线程模型：这一层最容易写错的地方（`docs/ROADMAP.md` §2.2）
//!
//! 两边的约束是硬的，而且互相冲突：
//!
//! - **网络 IO 必须在 tokio 运行时上跑**（`bt_providers` / `bt_downloaders` 都是
//!   `async`，内部是 reqwest）。
//! - **Slint 的 UI 是单线程的**：组件、属性、模型都不是 `Send`，**只能**在跑事件循环
//!   的那个线程上碰。
//!
//! 所以规矩是：
//!
//! 1. 后台任务**绝不能直接改 UI 属性**。要改就把「数据 + 一个 `Weak<AppWindow>`」
//!    装进 `slint::invoke_from_event_loop(...)`，由事件循环线程去改。
//! 2. 因此后台闭包里**不能捕获 `Rc`**（`invoke_from_event_loop` 要求
//!    `FnOnce() + Send + 'static`）。UI 侧的可变状态因此放在
//!    [`STATE`] 这个 **thread-local** 里 —— 它只在 UI 线程被访问，
//!    后台闭包只捕获安全的、拥有所有权的数据。
//! 3. **每站一回来就更新**，不要「等全部跑完再一次性填」。这正是
//!    `bt_providers::aggregate::search_stream` 存在的理由：
//!    16 家并发、慢站不拖累快站，用户先看到先回来的那些。
//!    ⚠️ 而且列表模型要**增量 push**（[`UiState::rows_model`]），
//!    否则每来一站就重建模型会把用户的滚动位置顶回去。
//!
//! ## 状态放在哪
//!
//! - [`AppState`]（`ui_model`）：**权威**的行/状态条数据，纯逻辑、有单测。
//! - `ui.rows` / `ui.providers`：它的**镜像**，喂给 Slint 渲染。
//! - 行号 = `AppState.rows` 的下标；`.slint` 的按钮把行号回传（`push-row(int)`）。

mod config_store;
mod health;
mod highlight;
mod selftest;
mod ui_model;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use bt_core::http::HttpClient;
use bt_downloaders::{self as downloaders, Config, Kind};
use bt_providers::aggregate::{self, SearchArgs};
use bt_providers::{registry as provider_registry, Provider};
// ⚠️ `Model` 这个 trait 必须在作用域里：`VecModel` 的 `row_count` / `set_row_data`
// 是 **trait 方法**而不是固有方法 —— 少了它 CI 报
// `no method named row_count found for struct Rc<VecModel<..>>`（首轮 CI 就踩了）。
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use highlight::markup_to_styled;
use ui_model::{AppState, ChipData, RowData, SortKey, Tone};

// ---------------------------------------------------------------------------
// 进程级共享（都是 Send + Sync，可以安全地被后台任务借）
// ---------------------------------------------------------------------------

/// provider 的**工厂**：每次搜索调一遍，得到一个全新的实例。
///
/// ⚠️ **为什么是工厂而不是实例集合**（2026-10-16 定，见 AGENTS.md 遗留清单 #4）：
/// 要把「静态 16 家 + 运行期动态的」（Torznab / demo / 测试注入）并进**同一份**集合，
/// 就必须**拥有一份拷贝** —— 而 `Box<dyn Provider>` **不能 clone**。
/// 工厂每次现造，天然解决这件事；而 Torznab 本来就是"从存好的索引器配置**现造**一个
/// provider"，形态完全吻合。
pub type ProviderFactory = Box<dyn Fn() -> Box<dyn Provider> + Send + Sync>;

thread_local! {
    /// 运行期注入的 provider 工厂。
    ///
    /// ⚠️ 用 `thread_local` 而不是 `static`：注入只发生在 UI 线程（`--selftest` 与
    /// 真实启动都在主线程），而**集合本体**每次搜索现造、然后 move 进后台任务 ——
    /// 这里只存"怎么造"。
    ///
    /// ⚠️ 也正因为它不是 `OnceLock`，注入**可以重复**（旧版 `OnceLock::set` 第二次
    /// 会静默失败）。"运行期插入"本就该可重复 —— 那才是 Torznab 要的那个口子。
    static INJECTED: RefCell<Option<Vec<ProviderFactory>>> = const { RefCell::new(None) };
}

/// **本次搜索要用的 provider 集合** —— 每次现造一份。
///
/// 注入了就用注入的（`--selftest` 要完全离线、确定性），否则用静态注册表那 16 家。
/// 将来 Torznab 的动态 provider 在这里 `extend` 进来即可（那一步还没做）。
fn provider_set() -> Vec<Box<dyn Provider>> {
    INJECTED.with(|slot| match slot.borrow().as_ref() {
        Some(factories) => factories.iter().map(|f| f()).collect(),
        None => provider_registry::registry(),
    })
}

/// **只有 `--selftest` 用**：把 provider 集合换成假的（离线、确定性）。
///
/// 传的是**工厂**：下次 `provider_set()` 现造一套全新的假 provider ——
/// 状态天然重置，也不会跨用例串味。
fn install_registry(factories: Vec<ProviderFactory>) {
    INJECTED.with(|slot| *slot.borrow_mut() = Some(factories));
}

fn http() -> &'static HttpClient {
    static HTTP: OnceLock<HttpClient> = OnceLock::new();
    HTTP.get_or_init(HttpClient::new)
}

/// tokio 运行时。
///
/// ## 🔴 这个函数是 M4 自查抓出来的一个**运行期 panic** 的修复
///
/// Slint 的事件循环线程**不在**任何 tokio 运行时上下文里。所以在回调里直接写
/// `tokio::spawn` 会在**第一次搜索时 panic**：
/// `there is no reactor running, must be called from the context of a Tokio 1.x runtime`。
/// **编译期发现不了**，CI 全绿也发现不了 —— 只有真跑起来才会炸。
///
/// 修法：自己持有运行时，用 [`spawn`] 而不是裸 `tokio::spawn`。
///
/// ⚠️ `enable_all()` 需要 tokio 的 `net` 特性才会打开 **IO driver**；
/// 少了它 reqwest 会在运行期报 "IO driver not enabled"（编译期同样不报）。
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            // 起不来就没法工作；这是启动期的环境性失败，不是可恢复的运行期路径。
            .expect("构建 tokio 运行时失败")
    })
}

/// 把任务丢到运行时上（替代裸 `tokio::spawn`）。
fn spawn<F>(f: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    runtime().spawn(f);
}

// ---------------------------------------------------------------------------
// UI 线程上的状态
// ---------------------------------------------------------------------------

/// UI 侧的全部可变状态。**只在事件循环线程上访问** —— 所以可以放 `Rc`/`RefCell`。
struct UiState {
    app: AppState,
    /// 当前的 `.slint` 行模型。**增量 push**，不整块重建（否则滚动位置会被顶回去）。
    rows_model: Rc<VecModel<ResultRow>>,
    /// 本轮搜索的关键词（用来算高亮 markup）
    query: String,
    /// 高亮色。⚠️ 高亮是**烘进 markup** 的，所以换主题必须整体重建这里的 markup
    /// （M1 踩过的坑，见 AGENTS.md）。
    accent: String,
    downloader: Option<(Kind, Config)>,
    status: String,
}

thread_local! {
    /// ⚠️ 必须是 thread-local：UI 状态不是 `Send`，而
    /// `slint::invoke_from_event_loop` 的回调要求 `Send + 'static`。
    /// 把状态放这儿，后台闭包就只需要捕获「拥有所有权的数据 + Weak 句柄」。
    static STATE: RefCell<UiState> = RefCell::new(UiState {
        app: AppState::new(),
        // `VecModel::from(vec![])` 而不是 `VecModel::default()` —— 后者不保证有实现，
        // 而 `From<Vec<T>>` 是 M1 spike 里已经用过的那条路径。
        rows_model: Rc::new(VecModel::from(Vec::new())),
        query: String::new(),
        accent: "#f9e2af".to_string(),
        downloader: None,
        status: "就绪".to_string(),
    });
}

/// 从 `Theme` global 里读高亮色，转成 `#rrggbb`（markup 里要用）。
///
/// ⚠️ 调色板只有一处来源（`.slint` 的 `Theme`），别在这儿写死颜色。
fn accent_hex(ui: &AppWindow) -> String {
    let c = ui.global::<Theme>().get_hl();
    format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
}

/// `Tone` → 具体颜色（同样取自 `Theme`）。
fn tone_color(ui: &AppWindow, tone: Tone) -> slint::Color {
    let t = ui.global::<Theme>();
    match tone {
        Tone::Ok => t.get_ok(),
        Tone::Err => t.get_err(),
        Tone::Dim => t.get_sub(),
    }
}

/// `RowData` → `.slint` 的 `ResultRow`。
///
/// ⚠️ `title_markup` 在这里才从 markdown 串变成 `StyledText` ——
/// 转换必须发生在 UI 线程（`StyledText` 是 UI 侧的东西），
/// 而 markdown 串本身在 `ui_model` 里就能算、能单测。
fn to_slint_row(r: &RowData) -> ResultRow {
    ResultRow {
        idx: r.idx,
        title_markup: markup_to_styled(&r.title_markup),
        title_plain: r.title_plain.clone().into(),
        meta: r.meta.clone().into(),
        provider: r.provider.clone().into(),
        seeds: r.seeds,
        leechers: r.leechers,
        action_label: r.action.label().into(),
    }
}

fn to_slint_chip(ui: &AppWindow, c: &ChipData) -> ProviderChip {
    ProviderChip {
        id: c.id.clone().into(),
        name: c.name.clone().into(),
        glyph: c.glyph.into(),
        detail: c.detail.clone().into(),
        tone: tone_color(ui, c.tone),
    }
}

/// 把状态条与计数（**小**的两块）整块同步给 UI；行则增量 push。
/// ③ `.slint` 的 `SiteToggle` 要 color，而 `SiteData` 只给 bool —— 在这一层映射
/// （与 `to_slint_chip` 同一套做法：颜色只在 `.slint` 的 `Theme` 里定义一次）。
fn to_slint_site(ui: &AppWindow, s: &crate::ui_model::SiteData) -> SiteToggle {
    let g = ui.global::<Theme>();
    SiteToggle {
        id: s.id.clone().into(),
        label: s.label().into(),
        tone: if s.on { g.get_ok() } else { g.get_sub() },
    }
}

/// #1 `.slint` 的 `Chip`（排序键 / 分类）—— 文案已经在 `ui_model` 里算好了，这里只搬运。
fn to_slint_chip_row(c: &crate::ui_model::Chip) -> Chip {
    Chip {
        id: c.id.clone().into(),
        label: c.label.clone().into(),
    }
}

/// 把 `st.app` 的**视图**整块重建进 Slint 模型。
///
/// ⚠️ 这与搜索中的「增量 push」是两回事：那条规矩（别整块重建）是为了**保住用户的
/// 滚动位置**；而排序 / 筛选**本来就是重排列表**，滚动位置没有意义了。
/// 另外排序会**重编号 `idx`**（见 `ui_model::sort_rows`），所以必须整块重建 ——
/// 只更新现有行的内容会让 `idx` 和显示对不上，点一行会推错东西。
fn rebuild_rows_model(ui: &AppWindow, st: &mut UiState) {
    let view: Vec<ResultRow> = st.app.view().iter().map(|r| to_slint_row(r)).collect();
    st.rows_model = Rc::new(VecModel::from(view));
    ui.set_rows(ModelRc::from(st.rows_model.clone()));
}

fn sync_small(ui: &AppWindow, st: &UiState) {
    let chips: Vec<ProviderChip> = st.app.chips.iter().map(|c| to_slint_chip(ui, c)).collect();
    ui.set_providers(ModelRc::from(Rc::new(VecModel::from(chips))));
    // ③ 站点开关（只有 16 行，整块重建模型是安全的 —— 那条「别整块重建」的规矩
    //    是为 300 行的结果列表定的，重建立刻会重置用户的滚动位置，这里不会）。
    let sites: Vec<SiteToggle> = st.app.sites.iter().map(|s| to_slint_site(ui, s)).collect();
    ui.set_sites(ModelRc::from(Rc::new(VecModel::from(sites))));
    // #1 排序键与分类 chip（也都是小列表，整块重建安全）
    let sorts: Vec<Chip> = st.app.sort_chips().iter().map(to_slint_chip_row).collect();
    ui.set_sorts(ModelRc::from(Rc::new(VecModel::from(sorts))));
    let cats: Vec<Chip> = st.app.cat_chips().iter().map(to_slint_chip_row).collect();
    ui.set_cats(ModelRc::from(Rc::new(VecModel::from(cats))));
    // ⚠️ `total` 报的是**视图**条数（屏幕上真有几行），不是 `rows.len()` ——
    // 筛选状态下两者不同，显示后者会让用户以为筛坏了。
    ui.set_total(st.app.view().len() as i32);
    ui.set_searching(st.app.chips.iter().any(|c| c.tone == Tone::Dim));
    ui.set_status_text(st.status.clone().into());
    // ① 分页
    ui.set_page(st.app.page as i32);
    ui.set_has_more(st.app.has_more);
}

// ---------------------------------------------------------------------------
// 搜索
// ---------------------------------------------------------------------------

/// 开始一轮搜索。**在 UI 线程调用**。
///
/// ① `page` 由调用方给：首次搜索是 1，「下一页」是当前页 + 1。
/// ③ 参与搜索的站点来自 UI 的站点开关；**一个都没勾时直接不发搜索**。
fn start_search(ui: &AppWindow, query: String, page: u32) {
    let weak = ui.as_weak();
    let accent = accent_hex(ui);

    // ③ 先看用户勾了哪些。`None` = 全勾（不筛选）—— 与 JS 同口径。
    let (picked, none) = STATE.with(|s| {
        let st = s.borrow();
        (st.app.selected_providers(), st.app.none_selected())
    });
    if none {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "一个站点都没勾选 —— 至少勾一家再搜".to_string();
            sync_small(ui, &st);
        });
        return;
    }

    // ⚠️ **每次搜索现造一份** provider 集合（#4：这样 Torznab / demo / 测试注入才
    // 能并进来）。它之后要 **move 进后台任务**，所以筛选必须**先在本线程做完**。
    let providers = provider_set();

    // 目标 = 与聚合层**同一套**筛选逻辑（别在这儿重写一遍）
    let args = SearchArgs {
        providers: picked.as_deref(),
        page,
        browse: false,
    };
    let targets: Vec<(String, String)> = aggregate::resolve_targets(&providers, &args)
        .iter()
        .map(|p| (p.id().to_string(), p.name().to_string()))
        .collect();

    // ③ 站点开关列表要显示**全部**站点，不是筛选后的那些 ——
    // 否则用户取消勾选后，那一家就从列表里消失了、再也勾不回来。
    // 同样复用 `resolve_targets`（不另写一套"全部启用站点"的筛选）。
    let all: Vec<(String, String)> = aggregate::resolve_targets(
        &providers,
        &SearchArgs {
            providers: None,
            page: 1,
            browse: false,
        },
    )
    .iter()
    .map(|p| (p.id().to_string(), p.name().to_string()))
    .collect();

    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.app.begin_search(&targets);
        st.app.set_page(page);
        st.app.sync_sites(&all);
        // #1 ⚠️ 新搜索**清掉分类筛选**：旧筛选（比如「电影」）配上新结果很可能一条都不命中，
        // 于是用户看到「共 0 条」而其实搜到了一堆 —— 那比"筛选没延续"糟得多。
        // （排序键保留：那是用户的偏好，不是某次搜索的属性。）
        st.app.category = None;
        st.query = query.clone();
        st.accent = accent.clone();
        // 换一个新模型：整块重建一次是安全的（此时列表本来就是空的）
        st.rows_model = Rc::new(VecModel::from(Vec::new()));
        st.status = format!("搜索中…（第 {page} 页 · {} 家）", targets.len());

        ui.set_rows(ModelRc::from(st.rows_model.clone()));
        sync_small(ui, &st);
    });

    if targets.is_empty() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "没有可用的站点".to_string();
            sync_small(ui, &st);
        });
        return;
    }

    let q = query.clone();
    // ③ `picked` 是 String，异步块要自己持有一份（不能借用 STATE）
    let picked_async = picked.clone();
    spawn(async move {
        // ⚠️ #4 之后 `providers` 是**本地拥有**的一份（由 `provider_set()` 现造），
        // 跟着 `async move` 一起进来 —— 不再是"靠 'static 借"。`http()` 仍是 'static。
        // 真需要 move 的：query / 筛选项 / **provider 集合** / Weak 句柄（都是 Send）。
        let outcome = aggregate::search_stream(
            http(),
            &providers,
            &q,
            SearchArgs {
                providers: picked_async.as_deref(),
                page,
                browse: false,
            },
            |report| {
                let weak = weak.clone();
                let q = q.clone();
                let accent = accent.clone();
                // ⚠️ 关键一跳：数据回 UI 线程。**不能**在这儿直接碰任何 UI 属性。
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    STATE.with(|s| {
                        let mut st = s.borrow_mut();
                        st.app.apply_report(&report, &q, &accent);
                        // 增量 push：保住用户的滚动位置
                        for r in st.app.rows.iter().skip(st.rows_model.row_count()) {
                            st.rows_model.push(to_slint_row(r));
                        }
                        st.status = st.app.summary(true);
                        sync_small(&ui, &st);
                    });
                });
            },
        )
        .await;

        // 收尾：① `has_more` 决定「下一页」亮不亮（它来自**引擎层**，不是"这页满了"的猜测），
        // 再把 searching 关掉、状态行换成最终统计。
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|s| {
                let mut st = s.borrow_mut();

                // #6 把这一轮**每家**的结果落盘（引擎健康度历史）。
                // ⚠️ 数据不用另接管道：`outcome.providers` 里已经有每家的
                // count / error / ms —— 收尾这一刻是唯一能一次拿全的地方。
                // ⚠️ 写失败**静默** —— 一份统计文件不该影响搜索，也不该弹警告。
                // （16 行的小追加，放 UI 线程上可接受；真变慢再说。）
                let batch: Vec<health::Record> = outcome
                    .providers
                    .iter()
                    .map(|(id, ps)| health::Record {
                        id: id.clone(),
                        n: ps.count,
                        ok: ps.error.is_none(),
                        ms: ps.ms,
                    })
                    .collect();
                let _ = health::append(&batch);

                st.app.has_more = outcome.has_more;
                st.status = st.app.summary(false);
                sync_small(&ui, &st);
            });
        });
    });
}

/// ③ 点一个站点开关。**在 UI 线程调用** —— 只改状态、发一次同步，不触发搜索。
///
/// 不自动重搜是**有意的**：勾选是"下一次搜索的输入"，
/// 每点一下都重搜 16 家会把站点打爆，而且用户往往要连点好几家。
fn toggle_site(ui: &AppWindow, id: String) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let Some(on) = st.app.toggle_site(&id) else {
            return;
        };
        let picked = st.app.sites.iter().filter(|s| s.on).count();
        let total = st.app.sites.len();
        st.status = if picked == total {
            format!("已选全部 {total} 家")
        } else if picked == 0 {
            "一个站点都没勾选 —— 至少勾一家再搜".to_string()
        } else {
            format!(
                "已选 {picked}/{total} 家{}",
                if on { "" } else { "（取消了一家）" }
            )
        };
        sync_small(ui, &st);
    });
}

// ---------------------------------------------------------------------------
// #1 排序 / 分类（UI 层的视图）
// ---------------------------------------------------------------------------

/// #1 点一个排序键。**在 UI 线程调用** —— 同一个键再点一次翻方向。
///
/// ⚠️ 排序会**重编号 `idx`** ⇒ 必须整块重建模型（`rebuild_rows_model`）。
/// ⚠️ 搜索**进行中**新到的行是**追加在末尾**的（不插进已排好的序列）——
/// 所以这两个按钮在 `searching` 时是灰的（见 `.slint`）：排一个还在长的列表没有意义。
fn pick_sort(ui: &AppWindow, id: String) {
    let Some(key) = SortKey::parse(&id) else {
        return;
    };
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let q = st.query.clone();
        st.app.pick_sort(key, &q);
        rebuild_rows_model(ui, &mut st);
        sync_small(ui, &st);
    });
}

/// #1 点一个分类 chip（`id` 是空串 = 全部）。**不改 `rows`**，只改视图。
fn pick_category(ui: &AppWindow, id: String) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.app
            .set_category(if id.is_empty() { None } else { Some(id) });
        rebuild_rows_model(ui, &mut st);
        sync_small(ui, &st);
    });
}

// ---------------------------------------------------------------------------
// 推送（磁力 → 下载器）
// ---------------------------------------------------------------------------

/// 点一行的按钮。**在 UI 线程调用**。
///
/// 一次点击走完整条链：没有磁力就**先惰性解析**，再推给下载器
/// （对那 6 家 `needs_magnet` 的站点，用户不需要点两次）。
fn push_row(ui: &AppWindow, idx: i32) {
    // 先把要用的东西抄出来，别把 borrow 带进异步
    let plan = STATE.with(|s| {
        let mut st = s.borrow_mut();
        let Some(row) = st.app.row(idx).cloned() else {
            return None;
        };
        let Some((kind, cfg)) = st.downloader.clone() else {
            st.status = "还没配置下载器：填地址 → 保存（或先「探测」）".to_string();
            sync_small(ui, &st);
            return None;
        };

        match row.magnet() {
            Some(m) => Some(Plan {
                idx,
                kind,
                cfg,
                provider: row.provider.clone(),
                detail_url: row.detail_url.clone(),
                magnet: m.to_string(),
                need_resolve: false,
            }),
            None => {
                let Some(url) = row.detail_url.clone() else {
                    st.status = "这条既没有磁力也没有详情页，拿不到".to_string();
                    sync_small(ui, &st);
                    return None;
                };
                Some(Plan {
                    idx,
                    kind,
                    cfg,
                    provider: row.provider.clone(),
                    detail_url: Some(url),
                    magnet: String::new(),
                    need_resolve: true,
                })
            }
        }
    });

    let Some(plan) = plan else { return };

    // 立刻反馈：按钮变成「解析中…」/「已推送 ✓」由异步阶段写
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.app.set_action(
            plan.idx,
            if plan.need_resolve {
                ui_model::RowAction::Resolving
            } else {
                ui_model::RowAction::Done
            },
        );
        st.status = if plan.need_resolve {
            format!("正在解析磁力（{}）…", plan.provider)
        } else {
            format!("正在推送到 {}…", plan.kind.as_str())
        };
        sync_small(ui, &st);
        refresh_row(&st, plan.idx);
    });

    let weak = ui.as_weak();
    // ⚠️ **必须在 `spawn` 之前取**：`provider_set()` 读的是 `thread_local` 的 `INJECTED`
    // （#4），而 tokio 的后台线程是**另一个线程** —— 在那里调会读到 `None`，
    // 于是退回静态注册表、`--selftest` 的假 provider 就找不到了。
    // 旧代码用 `registry()`（`'static`）从哪个线程读都对，这是本次改动**引入的新风险**。
    let resolve_providers = provider_set();
    spawn(async move {
        let idx = plan.idx;

        // 第 1 步：需要就先惰性解析磁力
        let magnet = if plan.need_resolve {
            let url = plan.detail_url.clone().unwrap_or_default();
            // `Box<dyn Provider>` **不是 Clone**，所以这里只能借（集合已经 move 进来了）。
            let Some(p) = resolve_providers.iter().find(|p| p.id() == plan.provider) else {
                finish(&weak, idx, format!("找不到站点 {}", plan.provider));
                return;
            };

            // ⚠️ 只调**一次**。解析是网络请求，调两次就是两次请求
            //（而且第二次可能拿到不同的结果）。
            let out = p.resolve_magnet(http(), &url).await;
            match out.magnet.filter(|m| !m.is_empty()) {
                Some(m) => m,
                // 拿不到磁力：优先用 provider 给的错误串；没有就用 JS 的兜底文案
                // （`/api/magnet` 在找不到 resolver 时回的就是 `no resolver`）。
                None => {
                    finish(
                        &weak,
                        idx,
                        out.error.unwrap_or_else(|| "no resolver".to_string()),
                    );
                    return;
                }
            }
        } else {
            plan.magnet.clone()
        };

        // 第 2 步：推送
        let pushed = downloaders::push(plan.kind, &plan.cfg, &magnet).await;
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|s| {
                let mut st = s.borrow_mut();
                match pushed {
                    Ok(out) => {
                        st.app.set_magnet(idx, magnet);
                        st.app.set_action(idx, ui_model::RowAction::Done);
                        st.status =
                            format!("已推送到 {}（HTTP {}）", plan.kind.as_str(), out.status);
                    }
                    Err(e) => {
                        st.app.set_action(idx, ui_model::RowAction::Ready);
                        st.status = format!("推送失败：{e}");
                    }
                }
                sync_small(&ui, &st);
                refresh_row(&st, idx);
            });
        });
    });
}

struct Plan {
    idx: i32,
    kind: Kind,
    cfg: Config,
    provider: String,
    detail_url: Option<String>,
    magnet: String,
    need_resolve: bool,
}

/// 把某个失败原因写回界面（由后台线程调用）。
fn finish(weak: &slint::Weak<AppWindow>, idx: i32, msg: String) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.app.set_action(idx, ui_model::RowAction::Ready);
            st.status = format!("取磁力失败：{msg}");
            sync_small(&ui, &st);
            refresh_row(&st, idx);
        });
    });
}

/// 只刷新一行（按钮文案会变）—— 不必重建整个列表。
fn refresh_row(st: &UiState, idx: i32) {
    if idx < 0 {
        return;
    }
    let i = idx as usize;
    if i < st.rows_model.row_count() {
        if let Some(row) = st.app.row(idx) {
            st.rows_model.set_row_data(i, to_slint_row(row));
        }
    }
}

// ---------------------------------------------------------------------------
// 下载器配置
// ---------------------------------------------------------------------------

fn save_downloader(ui: &AppWindow) {
    let url = ui.get_downloader_url().to_string();
    if url.trim().is_empty() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "请先填下载器地址".to_string();
            sync_small(ui, &st);
        });
        return;
    }

    // ⚠️ 信任边界：只放行 http/https。JS 把这一步放在 server.js 的 safeHttpUrl；
    // Rust 侧没有那层服务器，所以放库里 —— 而且 `push` 内部还会再查一次。
    if downloaders::safe_http_url(&url).is_none() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "地址不合法：只接受 http:// 或 https://".to_string();
            sync_small(ui, &st);
        });
        return;
    }

    let kind = Kind::parse(&ui.get_downloader_kind()).unwrap_or(Kind::Qbittorrent);
    let secret = ui.get_downloader_secret().to_string();
    let (user, pass, token) = if downloaders::meta_of(kind).auth == downloaders::Auth::Token {
        (String::new(), String::new(), secret)
    } else {
        (ui.get_downloader_user().to_string(), secret, String::new())
    };

    let cfg = Config {
        url,
        user,
        pass,
        token,
    };
    // ② 落盘。⚠️ **失败不致命**：配置在本次会话里照常可用，只是下次要重填 ——
    // 所以把原因塞进状态栏，而不是弹窗 / 退出。
    let saved = match config_store::save(kind, &cfg) {
        Ok(()) => String::new(),
        Err(e) => format!("（{e}）"),
    };
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.downloader = Some((kind, cfg));
        st.status = format!("下载器已保存：{}{saved}", kind.as_str());
        sync_small(ui, &st);
    });
}

fn detect_downloader(ui: &AppWindow) {
    let weak = ui.as_weak();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.status = "正在探测本机下载器…".to_string();
        sync_small(ui, &st);
    });

    spawn(async move {
        let hit = downloaders::detect().await;
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|s| {
                let mut st = s.borrow_mut();
                match hit {
                    Some(d) => {
                        ui.set_downloader_kind(d.kind.as_str().into());
                        ui.set_downloader_url(d.config.url.clone().into());
                        ui.set_downloader_user(d.config.user.clone().into());
                        let secret = if d.config.token.is_empty() {
                            d.config.pass.clone()
                        } else {
                            d.config.token.clone()
                        };
                        ui.set_downloader_secret(secret.into());
                        ui.set_downloader_status(format!("已探测到 {}", d.kind.as_str()).into());
                        st.status = format!("探测到下载器：{}", d.kind.as_str());
                        st.downloader = Some((d.kind, d.config));
                    }
                    None => {
                        ui.set_downloader_status("没探测到（试试手填地址）".into());
                        st.status = "没探测到本机下载器".to_string();
                    }
                }
                sync_small(&ui, &st);
            });
        });
    });
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 把窗口的初始状态与**全部回调**接上。
///
/// ⚠️ 抽成函数是**必须的**，不是整洁癖：`--selftest` 必须走**同一条**接线，
/// 否则它验的就不是真程序（测了个平行的实现，等于没测）。
fn wire(ui: &AppWindow) {
    // 让 .slint 的初始模型指向 thread-local 里那个（空的）模型 ——
    // 否则 `rows` 是另一个空模型，后面 push 的行不会显示出来。
    STATE.with(|s| {
        let st = s.borrow();
        ui.set_rows(ModelRc::from(st.rows_model.clone()));
    });

    ui.set_downloader_status("未配置（点「探测」或手填地址后「保存」）".into());

    {
        let weak = ui.as_weak();
        ui.on_search(move |q| {
            if let Some(ui) = weak.upgrade() {
                // ① 新搜索一律从第 1 页开始
                start_search(&ui, q.to_string(), 1);
            }
        });
    }
    // ① 下一页：拿**当前** query 与页号 +1 重跑一轮。
    // ⚠️ 别在这儿自己筛"哪些站能翻页" —— 聚合层的 `resolve_targets` 在 page>1 时
    // 已经按 JS 的口径筛过了（只留 `paginated()` 的），重写一遍就是两套实现。
    {
        let weak = ui.as_weak();
        ui.on_next_page(move || {
            let Some(ui) = weak.upgrade() else { return };
            let (q, next) = STATE.with(|s| {
                let st = s.borrow();
                (st.query.clone(), st.app.page + 1)
            });
            if q.is_empty() {
                return;
            }
            start_search(&ui, q, next);
        });
    }
    // ③ 站点开关（只改状态，不自动重搜 —— 见 `toggle_site` 的注释）
    {
        let weak = ui.as_weak();
        ui.on_toggle_site(move |id| {
            if let Some(ui) = weak.upgrade() {
                toggle_site(&ui, id.to_string());
            }
        });
    }
    // #1 排序 / 分类（都是"只重排视图"，不触发搜索）
    {
        let weak = ui.as_weak();
        ui.on_pick_sort(move |id| {
            if let Some(ui) = weak.upgrade() {
                pick_sort(&ui, id.to_string());
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_pick_category(move |id| {
            if let Some(ui) = weak.upgrade() {
                pick_category(&ui, id.to_string());
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_push_row(move |idx| {
            if let Some(ui) = weak.upgrade() {
                push_row(&ui, idx);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_pick_kind(move |k| {
            // ⚠️ 回调里**不能**捕获 `ui.clone_strong()` —— 回调本身存在组件里，
            // 强引用会形成环，组件永不释放。M1 的 spike 用的就是 as_weak 这套。
            let Some(ui) = weak.upgrade() else { return };
            ui.set_downloader_kind(k.clone());
            STATE.with(|s| {
                let mut st = s.borrow_mut();
                st.status = format!("下载器类型：{k}");
                sync_small(&ui, &st);
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_save_downloader(move || {
            if let Some(ui) = weak.upgrade() {
                save_downloader(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_detect_downloader(move || {
            if let Some(ui) = weak.upgrade() {
                detect_downloader(&ui);
            }
        });
    }
}

/// #6 打印各引擎的健康度。**纯读**：不开窗口、不写文件。
///
/// 表格里**最近连空**那一列才是重点 —— "曾经空过"没有意义（关键词本来就没资源的
/// 时候也会空），"**最近连续**空手"才是"这家可能改版了"的信号。
fn print_health() {
    let records = health::read_all();
    let where_is = health::path().map_or_else(
        || "（取不到 %APPDATA%，所以没有记录）".to_string(),
        |p| p.display().to_string(),
    );

    if records.is_empty() {
        println!("还没有任何记录 —— 跑一次搜索就会开始记。");
        println!("记录位置：{where_is}");
        return;
    }

    let stats = health::summarize(&records);
    println!("共 {} 条记录 · {} 家引擎", records.len(), stats.len());
    println!("记录位置：{where_is}");
    println!();
    println!(
        "{:<18}{:>6}{:>8}{:>12}{:>10}",
        "引擎", "轮次", "空手", "最近连空", "平均条数"
    );
    for (id, s) in &stats {
        // ⚠️ 只按**最近连续**空手提示。早先空过、后来好了的不该被标记 ——
        // 那正是 `summarize` 里"有结果就打断计数"的理由。
        let flag = if s.tail_empty >= health::STALE_AFTER {
            "   ⚠ 可能已改版"
        } else {
            ""
        };
        println!(
            "{:<18}{:>6}{:>8}{:>12}{:>10}{flag}",
            id, s.runs, s.empty, s.tail_empty, s.avg_hits
        );
    }
}

fn main() -> Result<(), slint::PlatformError> {
    // `--selftest`：无网、无人、无窗口的**全链路**真跑（见 selftest.rs）。
    // 退出码 0 = 通过，1 = 失败 —— 这一步是 `rust.yml` 的硬门。
    if std::env::args().any(|a| a == "--selftest") {
        let ok = selftest::run()?;
        if !ok {
            std::process::exit(1);
        }
        return Ok(());
    }

    // `--health`：打印各引擎的健康度（#6），**不开窗口、纯读**。
    //
    // 为什么要有它：站点改版的失败形态是**静默返回 0 条** —— 在 UI 上和不存在的资源
    // 长得一样。这里把历史变成趋势："这家最近连续 8 次空手"是可据此行动的结论，
    // 而"这次没搜到"不是。
    if std::env::args().any(|a| a == "--health") {
        print_health();
        return Ok(());
    }

    let ui = AppWindow::new()?;
    wire(&ui);

    // ② 载入上次保存的下载器配置。⚠️ 这一步**绝不能拦启动** ——
    // 没有配置文件、文件被改坏、APPDATA 取不到，都只是"没配置过"。
    // 参数 / 环境变量一个都没有 ⇒ 用户重填一次就能继续用。
    if let Some((kind, cfg)) = config_store::load() {
        ui.set_downloader_kind(kind.as_str().into());
        ui.set_downloader_url(cfg.url.clone().into());
        // ⚠️ 回填时要把 `Config` 拆回**界面上的两个框**：
        // token 类客户端（aria2 / Gopeed）用"密码 / token"那一个框，用户名框留空 ——
        // 与 `save_downloader` 的写方向必须**对称**，否则来回存两次会把凭据搬家。
        let (user, secret) = if downloaders::meta_of(kind).auth == downloaders::Auth::Token {
            (String::new(), cfg.token.clone())
        } else {
            (cfg.user.clone(), cfg.pass.clone())
        };
        ui.set_downloader_user(user.into());
        ui.set_downloader_secret(secret.into());
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.downloader = Some((kind, cfg));
            st.status = format!("已载入上次的下载器配置：{}", kind.as_str());
            sync_small(&ui, &st);
        });
    }

    ui.run()
}
