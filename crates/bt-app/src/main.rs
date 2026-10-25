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
//!    17 家并发、慢站不拖累快站，用户先看到先回来的那些。
//!    ⚠️ 而且列表模型要**增量 push**（[`UiState::rows_model`]），
//!    否则每来一站就重建模型会把用户的滚动位置顶回去。
//!
//! ## 状态放在哪
//!
//! - [`AppState`]（`ui_model`）：**权威**的行/状态条数据，纯逻辑、有单测。
//! - `ui.rows` / `ui.providers`：它的**镜像**，喂给 Slint 渲染。
//! - 行号 = `AppState.rows` 的下标；`.slint` 的按钮把行号回传（`push-row(int)`）。

// ⚠️ **Windows 子系统** —— 不加这一行，exe 启动时会**带一个控制台窗口**
// （Rust 默认编成 console 子系统，哪怕它是 GUI 程序）。
//
// 只在 **release** 里加（`not(debug_assertions)`）：
//   · 开发与 CI **需要**那个控制台 —— `--selftest` 的 `[m4]` 输出、
//     `--health` 的表格、panic 的回溯，全靠它（`rust.yml` 的门就是
//     `cargo run -p bt-app --bin bt-search -- --selftest`）。
//   · 交付给用户的 release 是 Slint GUI，双击不该弹黑框。
//
// ⚠️ 它必须是 **crate 级 inner attribute**，而且只能待在文件最前面
// （内部 doc 注释之后、任何 item 之前）。挪下去就编译不过。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config_store;
mod health;
mod highlight;
mod selftest;
mod ui_model;

slint::include_modules!();

use std::cell::RefCell;
use std::collections::HashSet;
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
/// 要把「静态 17 家 + 运行期动态的」（Torznab / demo / 测试注入）并进**同一份**集合，
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

    /// **追加**进来的动态 provider 工厂（Torznab 索引器走这条）。
    ///
    /// ⚠️ 与 [`INJECTED`] 分开是有意的：
    ///   · `INJECTED` 是**替换**语义 —— `--selftest` 要的是"完全离线、只有假 provider"；
    ///   · 这里是**追加**语义 —— 静态 17 家照旧，另加用户配的索引器。
    /// 合成一条会让 `--selftest` 变成"假 provider + 真 Torznab"，那就不是离线测试了。
    static DYNAMIC: RefCell<Vec<ProviderFactory>> = const { RefCell::new(Vec::new()) };
}

/// **本次搜索要用的 provider 集合** —— 每次现造一份。
///
/// 注入了就用注入的（`--selftest` 要完全离线、确定性），否则用静态注册表那 17 家；
/// 最后**追加**动态的那些（Torznab 索引器）。
///
/// ⚠️ **只能在 UI 线程调**（它读 `thread_local`），拿到之后 move 进后台任务 ——
/// 在 `spawn` 的闭包里调会读到空的一份（那条规矩见 `start_search` 里的注释）。
fn provider_set() -> Vec<Box<dyn Provider>> {
    let mut all: Vec<Box<dyn Provider>> = INJECTED.with(|slot| match slot.borrow().as_ref() {
        Some(factories) => factories.iter().map(|f| f()).collect(),
        None => provider_registry::registry(),
    });
    // 追加：动态 provider 在静态那 17 家**之后**（JS 的 `list()` 也是这个顺序 ——
    // `[...REGISTRY, ...dynamicTorznabProviders()]`）
    DYNAMIC.with(|slot| {
        for f in slot.borrow().iter() {
            all.push(f());
        }
    });
    all
}

/// **运行期追加**一个 provider 工厂（Torznab 的索引器走这条）。
///
/// 可重复调用：每个配置好的索引器调一次。⚠️ 同样**只能在 UI 线程**调。
fn add_dynamic_factory(factory: ProviderFactory) {
    DYNAMIC.with(|slot| slot.borrow_mut().push(factory));
}

/// 清空所有动态工厂（`--selftest` 与"索引器列表变了 ⇒ 重建"都用）。
fn clear_dynamic_factories() {
    DYNAMIC.with(|slot| slot.borrow_mut().clear());
}

/// **按列表重建**动态工厂：先清空、再逐个加。
///
/// ⚠️ 每次**整套重建**，而不是"增删单个工厂"：`DYNAMIC` 里的工厂没有身份标识，
/// 增量维护迟早会留下**重复 id** —— 而重复 id 会让每站状态条互相覆盖
/// （`apply_report` 是按 id 找槽位的）。重建的代价是每次几个 `Box::new`，可以忽略。
fn rebuild_dynamic_factories(list: &[bt_torznab::IndexerConfig]) {
    clear_dynamic_factories();
    for cfg in list {
        let cfg = cfg.clone();
        add_dynamic_factory(Box::new(move || -> Box<dyn Provider> {
            Box::new(bt_torznab::TorznabProvider::new(cfg.clone()))
        }));
    }
}

/// **只有 `--selftest` 用**：把 provider 集合换成假的（离线、确定性）。
///
/// 传的是**工厂**：下次 `provider_set()` 现造一套全新的假 provider ——
/// 状态天然重置，也不会跨用例串味。
///
/// ⚠️ 同时清空 [`DYNAMIC`]：自检要的是**完全离线**，任何残留的动态索引器
/// 都会让它去发真请求（那就不再是自检了）。
fn install_registry(factories: Vec<ProviderFactory>) {
    DYNAMIC.with(|slot| slot.borrow_mut().clear());
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
    /// ⑤ 用户配的 Torznab 索引器。**权威副本**在这里（UI 只是它的镜像）——
    /// 加/删都走 [`apply_indexers`]，它同时负责"重建动态工厂 + 落盘 + 更新状态栏"。
    indexers: Vec<bt_torznab::IndexerConfig>,
    status: String,
    /// **搜索代数围栏**。每开一轮 +1；回报/收尾的闭包各自带一份开轮时的值，
    /// 不匹配就整个丢掉。
    ///
    /// ⚠️ 为什么要它：`accepted` 不受 `searching` 拦（用户可以回车再搜一次），
    /// 而 `begin_search` 会**清空** rows/chips。没有围栏时，**上一轮**迟到的回报会把旧结果
    /// 追加进已清空的列表，而且是用**新关键词**去高亮；它的收尾还会把新搜索的状态行与
    /// `has_more` 盖掉。
    search_gen: u64,
    /// 批量勾选的结果 —— **存 key 不存 idx**：排序/筛选会重编号 idx（同 push_row 的教训），
    /// key 才是这一行一生不变的标识。
    selected: HashSet<u64>,
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
        accent: "#fab387".to_string(),
        downloader: None,
        indexers: Vec::new(),
        status: "就绪".to_string(),
        search_gen: 0,
        selected: HashSet::new(),
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
fn to_slint_row(r: &RowData, selected: &HashSet<u64>) -> ResultRow {
    ResultRow {
        idx: r.idx,
        selected: selected.contains(&r.key),
        title_markup: markup_to_styled(&r.title_markup),
        title_plain: r.title_plain.clone().into(),
        meta: r.meta.clone().into(),
        provider: r.provider.clone().into(),
        seeds: r.seeds,
        leechers: r.leechers,
        action_label: r.action.label().into(),
    }
}

/// `RowData` → 详情面板（屏 03）的扁平属性。
///
/// ⚠️ 只取 `RowData` 里**已有的原始字段**（`magnet` / `info_hash` / `detail_url`），
/// 不从 `meta` 反解 —— `meta` 是给人看的格式化文本（见 ui_model 的注释）。
/// 磁力可能为空：那 6 家要惰性解析，空串就照空串显示，不编造。
fn show_detail(ui: &AppWindow, r: &crate::ui_model::RowData) {
    ui.set_detail_open(true);
    ui.set_detail_idx(r.idx);
    ui.set_detail_title(r.title_plain.clone().into());
    ui.set_detail_meta(r.meta.clone().into());
    ui.set_detail_provider(r.provider.clone().into());
    ui.set_detail_seeds(r.seeds);
    ui.set_detail_leechers(r.leechers);
    ui.set_detail_infohash(r.info_hash.clone().unwrap_or_default().into());
    ui.set_detail_magnet(r.magnet.clone().unwrap_or_default().into());
    ui.set_detail_url(r.detail_url.clone().unwrap_or_default().into());
    ui.set_detail_action(r.action.label().into());
}

/// 关详情面板。新搜索 / 重排 / 关按钮都要调 —— 否则详情还指着旧 `idx`，
/// 而重排会重编号 `idx`（同 `push_row` 里那条 key vs idx 的教训）。
fn close_detail(ui: &AppWindow) {
    ui.set_detail_open(false);
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
    let view: Vec<ResultRow> = st
        .app
        .view()
        .iter()
        .map(|r| to_slint_row(r, &st.selected))
        .collect();
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
    // 屏 03：新搜索一律收起详情（旧行可能已经不在结果里了）
    close_detail(ui);
    // 新搜索 = 新的一批行（key 重新发号），勾选没有继承的意义
    STATE.with(|s| s.borrow_mut().selected.clear());
    ui.set_selected_count(0);

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

    // ⚠️ 空目标要在 `begin_search` **之前**拦：`begin_search` 会清空 rows ——
    // 先清再报错的话，「第 1 页有结果 + 取消勾选全部 paginated 站 + 点下一页」会把
    // 屏幕上的第 1 页结果清光，只留一句「没有可用的站点」（用户的东西被自己弄没了）。
    if targets.is_empty() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "没有可用的站点（可能都取消勾选了）—— 结果保持不变".to_string();
            sync_small(ui, &st);
        });
        return;
    }

    let my_gen = STATE.with(|s| {
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
        // 发号 —— 这一轮的所有回报都要对得上它
        st.search_gen += 1;
        let gen = st.search_gen;

        ui.set_rows(ModelRc::from(st.rows_model.clone()));
        sync_small(ui, &st);
        gen
    });

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
                        // ⚠️ 代数围栏：不是这一轮了 ⇒ 这轮已被取代（用户又搜了一次），
                        // 结果**整个丢掉** —— 否则旧结果会被追加进新列表、并按新关键词高亮。
                        if st.search_gen != my_gen {
                            return;
                        }
                        st.app.apply_report(&report, &q, &accent);
                        // 增量 push：保住用户的滚动位置。
                        // ⚠️ 这条依赖"模型 == 权威 rows"这个不变式：排序/分类 chip 在
                        // `searching` 时是灰的（见 `.slint`），搜索中没人能把模型重排成
                        // `view()`（那样长度就对不上，`skip` 会推开重复行）。
                        for r in st.app.rows.iter().skip(st.rows_model.row_count()) {
                            st.rows_model.push(to_slint_row(r, &st.selected));
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

                // ⚠️ 同一道代数围栏：旧搜索的收尾**不能**写状态行与 `has_more`，
                // 否则它会把新一轮刚点亮的「下一页」和统计数字盖回上一轮的。
                if st.search_gen != my_gen {
                    return;
                }

                // #6 把这一轮**每家**的结果落盘（引擎健康度历史）。
                // ⚠️ 数据不用另接管道：`outcome.providers` 里已经有每家的
                // count / error / ms —— 收尾这一刻是唯一能一次拿全的地方。
                // ⚠️ 写失败**静默** —— 一份统计文件不该影响搜索，也不该弹警告。
                // ⚠️ **但不能占着 UI 线程**：`health::append` 超过 5000 行时会
                // `read_to_string` **整文件**一遍再重写（`trim_if_needed`）。
                // 丢给一个独立线程 —— 结果本来就没人在等，失败也是静默的。
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
                std::thread::spawn(move || {
                    let _ = health::append(&batch);
                });

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
/// 每点一下都重搜 17 家会把站点打爆，而且用户往往要连点好几家。
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
    // 重排会重编号 idx ⇒ 详情必须收起（否则按钮会把磁力推到别的行上）
    close_detail(ui);
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
    // 视图会换成子集、idx 重新编号 ⇒ 同上，收起详情
    close_detail(ui);
    // 被筛掉的行还留在「勾选」里会让批量操作作用到看不见的行上 ⇒ 换分类就清空
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if !st.selected.is_empty() {
            st.selected.clear();
            ui.set_selected_count(0);
        }
    });
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
                key: row.key,
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
                    key: row.key,
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
        st.app.set_action_by_key(
            plan.key,
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
        refresh_row(&st, plan.key);
        refresh_detail(ui, &st);
    });

    let weak = ui.as_weak();
    // ⚠️ **必须在 `spawn` 之前取**：`provider_set()` 读的是 `thread_local` 的 `INJECTED`
    // （#4），而 tokio 的后台线程是**另一个线程** —— 在那里调会读到 `None`，
    // 于是退回静态注册表、`--selftest` 的假 provider 就找不到了。
    // 旧代码用 `registry()`（`'static`）从哪个线程读都对，这是本次改动**引入的新风险**。
    let resolve_providers = provider_set();
    spawn(async move {
        let key = plan.key;

        // 第 1 步：需要就先惰性解析磁力
        let magnet = if plan.need_resolve {
            let url = plan.detail_url.clone().unwrap_or_default();
            // `Box<dyn Provider>` **不是 Clone**，所以这里只能借（集合已经 move 进来了）。
            let Some(p) = resolve_providers.iter().find(|p| p.id() == plan.provider) else {
                finish(&weak, key, format!("找不到站点 {}", plan.provider));
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
                        key,
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
                        st.app.set_magnet_by_key(key, magnet);
                        st.app.set_action_by_key(key, ui_model::RowAction::Done);
                        st.status =
                            format!("已推送到 {}（HTTP {}）", plan.kind.as_str(), out.status);
                    }
                    Err(e) => {
                        st.app.set_action_by_key(key, ui_model::RowAction::Ready);
                        st.status = format!("推送失败：{e}");
                    }
                }
                sync_small(&ui, &st);
                refresh_row(&st, key);
                refresh_detail(&ui, &st);
            });
        });
    });
}

struct Plan {
    /// **稳定 key**（不是下标）—— 这个计划要跨越一次网络往返，
    /// 期间用户可能点排序 chip 而把 `idx` 重编号（见 `RowData::key`）。
    key: u64,
    kind: Kind,
    cfg: Config,
    provider: String,
    detail_url: Option<String>,
    magnet: String,
    need_resolve: bool,
}

/// 把某个失败原因写回界面（由后台线程调用）。
fn finish(weak: &slint::Weak<AppWindow>, key: u64, msg: String) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.app.set_action_by_key(key, ui_model::RowAction::Ready);
            st.status = format!("取磁力失败：{msg}");
            sync_small(&ui, &st);
            refresh_row(&st, key);
            refresh_detail(&ui, &st);
        });
    });
}

/// 只刷新一行（按钮文案会变）—— 不必重建整个列表（那会把滚动位置顶回去）。
///
/// ⚠️ 收的是**稳定 key**，不是下标 —— 模型里装的是 [`AppState::view`]（筛选后的子集），
/// 而下标是"权威 `rows` 里的位置"，筛选状态下两者不同 ⇒ 用下标会把**别的行**的内容
/// 画到这张卡上。行已经不在视图里（被筛掉 / 被新搜索清掉）就什么也不做。
fn refresh_row(st: &UiState, key: u64) {
    let found = {
        let view = st.app.view();
        view.iter()
            .position(|r| r.key == key)
            .map(|pos| (pos, to_slint_row(view[pos], &st.selected)))
    };
    let Some((pos, row)) = found else {
        return;
    };
    st.rows_model.set_row_data(pos, row);
}

// ---------------------------------------------------------------------------
// 批量选择（勾选 → 批量推送 / 导出 CSV）
// ---------------------------------------------------------------------------

/// 当前**视图里**被勾选的条数（被分类筛掉的旧勾选不算 —— 否则「已选 N 项」会
/// 和屏幕上能看见的勾对不上）。
fn visible_selected(st: &UiState) -> usize {
    let view = st.app.view();
    view.iter().filter(|r| st.selected.contains(&r.key)).count()
}

/// 勾选 / 取消一行。**按 key 存**（排序会重编号 idx），模型里就地更新一行，
/// 不整块重建 —— 重建会把滚动位置顶回去。
fn toggle_row(ui: &AppWindow, idx: i32) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let Some(key) = st.app.row(idx).map(|r| r.key) else {
            return;
        };
        if st.selected.contains(&key) {
            st.selected.remove(&key);
        } else {
            st.selected.insert(key);
        }
        let pos = st.app.view().iter().position(|r| r.idx == idx);
        if let Some(pos) = pos {
            let row = st
                .app
                .view()
                .get(pos)
                .map(|r| to_slint_row(*r, &st.selected));
            if let Some(row) = row {
                st.rows_model.set_row_data(pos, row);
            }
        }
        let n = visible_selected(&st) as i32;
        ui.set_selected_count(n);
    });
}

/// 全选当前视图。
fn select_all(ui: &AppWindow) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let keys: Vec<u64> = st.app.view().iter().map(|r| r.key).collect();
        if keys.is_empty() {
            return;
        }
        st.selected.extend(keys);
        rebuild_rows_model(ui, &mut st);
        let n = visible_selected(&st) as i32;
        ui.set_selected_count(n);
    });
}

/// 清空勾选。
fn clear_selection(ui: &AppWindow) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        if st.selected.is_empty() {
            return;
        }
        st.selected.clear();
        rebuild_rows_model(ui, &mut st);
        ui.set_selected_count(0);
    });
}

/// 批量推送：逐条走 [`push_row`]（没有磁力的仍然先惰性解析）。
///
/// ⚠️ 顺序执行、**不并发**：每条的解析/推送仍由 `push_row` 内部 `spawn` 到运行时，
/// 但这里不额外并发，避免同一批把下载器打满。
fn batch_push(ui: &AppWindow) {
    let idxs: Vec<i32> = STATE.with(|s| {
        let st = s.borrow();
        st.app
            .view()
            .iter()
            .filter(|r| st.selected.contains(&r.key))
            .map(|r| r.idx)
            .collect()
    });
    if idxs.is_empty() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "先勾选要推送的结果".to_string();
            sync_small(ui, &st);
        });
        return;
    }
    for idx in idxs {
        push_row(ui, idx);
    }
}

/// 一个 CSV 单元格：转义引号、整体加引号，并防 Excel/WPS 公式注入
/// （`=` `+` `-` `@` 开头先加一个单引号 —— 与 Node 版同口径）。
fn csv_cell(s: &str) -> String {
    let guarded =
        if s.starts_with('=') || s.starts_with('+') || s.starts_with('-') || s.starts_with('@') {
            format!("'{s}")
        } else {
            s.to_string()
        };
    format!("\"{}\"", guarded.replace('"', "\"\""))
}

/// 导出勾选结果为 CSV（带 UTF-8 BOM，Excel 直接认中文）。
///
/// ⚠️ 没有文件对话框（没引新依赖）：固定写到数据目录 `selection.csv`，
/// 路径会显示在状态栏里。要"另存为"再谈，别为它加一个 GUI 依赖。
fn export_csv(ui: &AppWindow) {
    let rows: Vec<RowData> = STATE.with(|s| {
        let st = s.borrow();
        st.app
            .view()
            .iter()
            .filter(|r| st.selected.contains(&r.key))
            .map(|r| (*r).clone())
            .collect()
    });
    if rows.is_empty() {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "先勾选要导出的结果".to_string();
            sync_small(ui, &st);
        });
        return;
    }

    let mut out = String::from("标题,来源,做种,大小(字节),infoHash,磁力\r\n");
    for r in &rows {
        let cells = [
            r.title_plain.clone(),
            r.provider.clone(),
            r.seeds.to_string(),
            r.size.map(|v| v.to_string()).unwrap_or_default(),
            r.info_hash.clone().unwrap_or_default(),
            r.magnet.clone().unwrap_or_default(),
        ];
        let line: Vec<String> = cells.iter().map(|c| csv_cell(c)).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }

    let Some(dir) = config_store::data_dir() else {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "取不到 %APPDATA%，没导出".to_string();
            sync_small(ui, &st);
        });
        return;
    };
    let path = dir.join("selection.csv");
    let result =
        std::fs::create_dir_all(&dir).and_then(|()| config_store::write_atomic(&path, &out));
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.status = match result {
            Ok(()) => format!("已导出 {} 条 → {}", rows.len(), path.display()),
            Err(e) => format!("导出失败：{e}"),
        };
        sync_small(ui, &st);
    });
}

/// 写一行状态（避免每处都重复 borrow + sync）。
fn set_status(ui: &AppWindow, msg: String) {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.status = msg;
        sync_small(ui, &st);
    });
}

/// 复制文本到剪贴板。
///
/// `arboard` 已经是 Slint winit 后端的传递依赖（见 Cargo.toml 注释），
/// 这里直接声明同一个包 —— 不新增下载，也不引第三方进程。
fn copy_text(ui: &AppWindow, text: &str, ok_msg: String) {
    let msg = match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
        Ok(()) => ok_msg,
        Err(e) => format!("复制失败：{e}"),
    };
    set_status(ui, msg);
}

/// 复制一条的磁力。没有磁力（那 6 家要惰性解析）就说清楚下一步怎么做。
fn copy_magnet(ui: &AppWindow, idx: i32) {
    let found = STATE.with(|s| {
        let st = s.borrow();
        st.app.row(idx).map(|r| r.magnet.clone())
    });
    match found {
        Some(Some(m)) if !m.is_empty() => copy_text(ui, &m, "已复制磁力".to_string()),
        Some(_) => set_status(
            ui,
            "这条还没有磁力：先点一次推送（会惰性解析），解析完再复制".to_string(),
        ),
        None => {}
    }
}

/// 批量复制勾选行的磁力（一行一条，跳过没有磁力的）。
fn copy_selected_magnets(ui: &AppWindow) {
    let (magnets, missing) = STATE.with(|s| {
        let st = s.borrow();
        let sel: Vec<&RowData> = st
            .app
            .view()
            .iter()
            .filter(|r| st.selected.contains(&r.key))
            .map(|r| *r)
            .collect();
        let magnets: Vec<String> = sel
            .iter()
            .filter_map(|r| r.magnet.clone())
            .filter(|m| !m.is_empty())
            .collect();
        let missing = sel.len() - magnets.len();
        (magnets, missing)
    });
    if magnets.is_empty() {
        set_status(
            ui,
            "没有可复制的磁力（没勾选，或选中的都还没解析）".to_string(),
        );
        return;
    }
    let suffix = if missing > 0 {
        format!("（{missing} 条无磁力已跳过）")
    } else {
        String::new()
    };
    let msg = format!("已复制 {} 条磁力{}", magnets.len(), suffix);
    copy_text(ui, &magnets.join("\n"), msg);
}

/// 详情面板跟着行一起刷新。
///
/// ⚠️ 收 `&UiState` 而不是自己 `STATE.with` —— 调用点已经在 `borrow_mut()` 里了，
/// 再进一次会 **RefCell 双重借用 panic**。
fn refresh_detail(ui: &AppWindow, st: &UiState) {
    if !ui.get_detail_open() {
        return;
    }
    let idx = ui.get_detail_idx();
    match st.app.row(idx) {
        Some(r) => show_detail(ui, r),
        None => close_detail(ui),
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
    // ⚠️ 这个函数**返回归一化后的 URL**，别只拿它当"合法吗"的谓词：
    // 手输的串可能带空格/大写，归一化结果才是下游该用的那份。
    let Some(url) = downloaders::safe_http_url(&url) else {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "地址不合法：只接受 http:// 或 https://".to_string();
            sync_small(ui, &st);
        });
        return;
    };

    // ⚠️ 认不出来**不能默认成 qBittorrent**：那会把用户填的凭据发到这台主机的
    // qB 登录端点，并把报错误导成"qB 登录失败"。认不出就拒绝保存。
    let Some(kind) = Kind::parse(&ui.get_downloader_kind()) else {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.status = "无法识别的下载器类型（界面状态异常）—— 请重开设置面板再试".to_string();
            sync_small(ui, &st);
        });
        return;
    };
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

/// ⑤ 索引器变了 ⇒ 三件事一起做：重建动态工厂、落盘、更新状态栏。
///
/// ⚠️ 三件事**必须一起做**，否则会出现"界面说配了、实际搜不到"（工厂没重建）
/// 或"这次能用、重启就没了"（没落盘）这类难查的错位。
fn apply_indexers(ui: &AppWindow, list: Vec<bt_torznab::IndexerConfig>) {
    rebuild_dynamic_factories(&list);

    // ⚠️ 落盘失败**不致命**：本次会话照常能用（工厂已经重建），只是重启要重填 ——
    // 所以把原因塞进状态栏，而不是弹窗 / 退出（与下载器同口径）。
    let note = match config_store::save_indexers(&list) {
        Ok(()) => String::new(),
        Err(e) => format!("（{e}）"),
    };
    ui.set_indexer_status(format!("{}{note}", config_store::describe(&list)).into());

    // 已配清单也要跟上 —— 加/清空/停用都走这里，所以**只在这一处同步**，
    // 不会出现"状态改了、界面没改"的错位。
    sync_indexers(ui, &list);

    // 加完就把输入框清掉 —— 留着容易让人以为"还没加进去"
    ui.set_indexer_name("".into());
    ui.set_indexer_url("".into());
    ui.set_indexer_key("".into());

    STATE.with(|s| s.borrow_mut().indexers = list);
}

/// 索引器清单 → Slint 模型（每行一个「停用 / 启用」）。
///
/// ⚠️ 文案（含 ✓/✗ 与"停用"/"启用"）**在这里算好** —— `.slint` 里不写条件表达式
/// （那条约定见 `app.slint` 里 `Chip` 的注释，是从一次语法风险里学来的）。
fn sync_indexers(ui: &AppWindow, list: &[bt_torznab::IndexerConfig]) {
    let rows: Vec<IndexerRow> = list
        .iter()
        .map(|c| {
            let name = if c.name.is_empty() {
                c.url.as_str()
            } else {
                c.name.as_str()
            };
            IndexerRow {
                // 身份 = URL（读配置时没 URL 的条目就被丢了 ⇒ 必然非空）。
                // 不用下标：`for ix[i] in …` 那种带索引的写法本项目还没用过，
                // 而"只能靠 CI 验证"的语法不该在没编译器时当首例。
                id: c.url.clone().into(),
                label: format!("{} {name}", if c.enabled { "✓" } else { "✗" }).into(),
                action: if c.enabled { "停用" } else { "启用" }.into(),
            }
        })
        .collect();
    ui.set_indexers(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// 停用 / 启用某个索引器（按 URL 认身份）。
///
/// ⚠️ 找不到就返回：界面与状态不同步时（例如刚点完「清空」又点到旧的按钮）
/// 宁可什么都不做 —— 不能 panic，也不能改错一行。
fn toggle_indexer(ui: &AppWindow, id: &str) {
    let mut list = STATE.with(|s| s.borrow().indexers.clone());
    let Some(i) = list.iter().position(|c| c.url == id) else {
        return;
    };
    list[i].enabled = !list[i].enabled;
    apply_indexers(ui, list);
}

/// 加一个索引器（同名**覆盖** —— 改完 key 再点一次是常见动作，不该变成两条）。
fn add_indexer(ui: &AppWindow) {
    let name = ui.get_indexer_name().to_string();
    let url = ui.get_indexer_url().to_string();
    let key = ui.get_indexer_key().to_string();

    // ⑤ 用户多半是直接粘贴 Jackett/Prowlarr 的整条 "Copy Torznab Feed" 链接，
    // 那条链接**带着** `?apikey=…` —— 而 `normalize_api_url` 会刻意丢掉整个 query
    // （忠实复刻 JS），所以只能在**这一层**把 key 抠出来省用户一步。
    // ⚠️ 在原始串上抠（下面 `url` 会被归一化结果覆盖）。
    // ⚠️ key 框非空时以用户填的为准 —— 那是显式输入，优先级更高。
    let key_from_url = bt_torznab::extract_api_key(&url);

    if url.trim().is_empty() {
        ui.set_indexer_status("请先填 Torznab 地址".into());
        return;
    }
    // 与下载器同一个信任边界：只放行 http/https（不合法就别存进配置文件了），
    // 并且**把归一化结果留下来**（别丢掉返回值、把未校验的原文存进去）。
    let Some(url) = downloaders::safe_http_url(&url) else {
        ui.set_indexer_status("地址不合法：只接受 http:// 或 https://".into());
        return;
    };

    let name = if name.trim().is_empty() {
        // 名字留空 ⇒ 用地址当代名（比让所有索引器都叫 "Torznab" 有用）
        url.trim().to_string()
    } else {
        name.trim().to_string()
    };
    let api_key = if key.trim().is_empty() {
        key_from_url.unwrap_or_default()
    } else {
        key.trim().to_string()
    };
    let cfg = bt_torznab::IndexerConfig {
        // 空 id ⇒ `TorznabProvider::new` 兜底成 `torznab:<name>`
        id: String::new(),
        name: name.clone(),
        url: url.trim().to_string(),
        api_key,
        enabled: true,
    };

    let mut list = STATE.with(|s| s.borrow().indexers.clone());
    match list.iter().position(|c| c.name == name) {
        Some(i) => list[i] = cfg,
        None => list.push(cfg),
    }
    apply_indexers(ui, list);
}

/// 清空索引器（含文件）。
///
/// ⚠️ 没有"单项删除"：那要给每行一个 Button + 一个行模型；而"同名再添加一次即覆盖"
/// 已经够管理了。真要单个删，改一次 `list.retain(..)` 即可 —— 这里不做是因为
/// 每多一个控件就多一份"只能靠 CI 验证"的风险（见文件头那条 Slint 教训）。
fn clear_indexers(ui: &AppWindow) {
    apply_indexers(ui, Vec::new());
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
    // ⑤ 索引器：加 / 清空。⚠️ 两个回调都是"改了状态就同步"，没有后台任务 ——
    // 落盘是毫秒级的小写入，不值得为它开一个 spawn。
    {
        let weak = ui.as_weak();
        ui.on_add_indexer(move || {
            if let Some(ui) = weak.upgrade() {
                add_indexer(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_clear_indexers(move || {
            if let Some(ui) = weak.upgrade() {
                clear_indexers(&ui);
            }
        });
    }
    // ⑤ 每个索引器一个「停用 / 启用」—— 之前某个索引器坏了只能把全部清掉重加。
    // ⚠️ 传进来的是**身份（URL）**：找不到（界面与状态不同步）由 `toggle_indexer` 静默忽略。
    {
        let weak = ui.as_weak();
        ui.on_toggle_indexer(move |id| {
            if let Some(ui) = weak.upgrade() {
                toggle_indexer(&ui, id.as_str());
            }
        });
    }
    // 屏 03：打开详情（按 idx 取权威 Vec 的那一行）/ 关闭。
    {
        let weak = ui.as_weak();
        ui.on_open_row(move |idx| {
            if let Some(ui) = weak.upgrade() {
                STATE.with(|s| {
                    let st = s.borrow();
                    if let Some(row) = st.app.row(idx) {
                        show_detail(&ui, row);
                    }
                });
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_close_detail(move || {
            if let Some(ui) = weak.upgrade() {
                close_detail(&ui);
            }
        });
    }
    // 批量：勾选一行 / 全选 / 清空 / 推送 / 导出
    {
        let weak = ui.as_weak();
        ui.on_toggle_row(move |idx| {
            if let Some(ui) = weak.upgrade() {
                toggle_row(&ui, idx);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_select_all(move || {
            if let Some(ui) = weak.upgrade() {
                select_all(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_clear_selection(move || {
            if let Some(ui) = weak.upgrade() {
                clear_selection(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_batch_push(move || {
            if let Some(ui) = weak.upgrade() {
                batch_push(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_export_csv(move || {
            if let Some(ui) = weak.upgrade() {
                export_csv(&ui);
            }
        });
    }
    // 复制磁力（单条 / 批量）
    {
        let weak = ui.as_weak();
        ui.on_copy_magnet(move |idx| {
            if let Some(ui) = weak.upgrade() {
                copy_magnet(&ui, idx);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_copy_selected_magnets(move || {
            if let Some(ui) = weak.upgrade() {
                copy_selected_magnets(&ui);
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

/// 把**父进程的控制台**接回来。
///
/// ## 为什么需要它
///
/// release 版是 **windows 子系统**（见文件头的 `#![cfg_attr(...)]`）—— 好处是双击
/// 不弹黑框，代价是**它没有控制台**：从 cmd/PowerShell 里跑 `bt-search.exe --health`，
/// `println!` 的输出会**不知去向**。
///
/// 所以命令行模式在打印任何东西**之前**，先主动附到父进程的控制台。
/// 这是 Win32 的标准做法；只为一个函数引 `windows-sys` 不值，直接声明它。
///
/// ⚠️ **双击启动时没有父控制台** ⇒ `AttachConsole` 返回 0，什么都不发生 ✅ ——
/// 那正是我们要的：GUI 模式不该凭空造出一个黑框。
/// ⚠️ 已知的观赏性小毛病：附上去之后 shell 的提示符可能**先**出现，命令的输出跟在后面。
/// ⚠️ **`cfg(windows)` 而不是 `cfg(not(debug_assertions))`** —— 这是有意的：
/// CI 的门（`rust.yml`）跑的是 **debug** 构建，如果这段只在 release 里编译，
/// **它永远不会被编译检查**，绿灯也就不代表它能编过。
/// 让它进每一次 Windows 构建，`cargo test`/`clippy` 就都会看它。
///
/// debug 下它**会被调用**（`--selftest` 也算 CLI 模式）但什么都不发生：
/// 进程本来就有控制台，`AttachConsole` 会以 ERROR_ACCESS_DENIED 失败，无害。
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
    }
    /// Win32 的 `ATTACH_PARENT_PROCESS`
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;

    // SAFETY: 只调一个 Win32 函数，没有指针参数、没有所有权问题；
    // 失败（没有父控制台）返回 0，我们刻意忽略 —— 那是最常见的正常情况
    // （双击启动时就没有父控制台，正是我们要的效果）。
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// 非 Windows：没有这回事。
#[cfg(not(windows))]
fn attach_parent_console() {}

fn main() -> Result<(), slint::PlatformError> {
    // ⚠️ **必须在任何 println! 之前**：Rust 的 stdout 会缓存句柄，
    // 先打印过再 attach 就晚了（句柄已经定死）。
    let cli_mode = std::env::args().any(|a| a == "--selftest" || a == "--health");
    if cli_mode {
        attach_parent_console();
    }

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

    // #6 载入各家的健康度（最近连续空手次数）—— 启动读一次，之后每轮搜索在
    // `ui_model::apply_report` 里增量更新（那里才建 `chips`，见那边的说明）。
    // ⚠️ 读失败（没记录 / 文件坏了 / 取不到 %APPDATA%）都只是"没有历史"，不是错误。
    let stale: std::collections::HashMap<String, usize> = health::summarize(&health::read_all())
        .into_iter()
        .map(|(id, s)| (id, s.tail_empty))
        .collect();
    STATE.with(|s| s.borrow_mut().app.stale = stale);

    // ⑤ 载入用户配的 Torznab 索引器，**追加**成动态 provider（排在静态 17 家之后，
    // 与 JS 的 `[...REGISTRY, ...dynamicTorznabProviders()]` 同序）。
    //
    // ⚠️ 必须在这里做（UI 线程、第一次搜索之前）：`provider_set()` 读的是
    // `thread_local`，在 `spawn` 出来的后台线程里读会读到空的一份。
    // ⚠️ 文件坏了 / 一个都没配 ⇒ 空列表，**不影响启动**（与 config_store 同口径）。
    // ⚠️ **启动这条路也得过 `safe_http_url`** —— 此前只有 UI 的「添加索引器」过了，
    // 手改过 `indexers.json` 的人能让 `file://` / `ftp://` 进搜索路径（UI 那条就白拦了）。
    // 顺手把归一化结果写回去，下游就不必反复解析未校验的原文。
    let indexers: Vec<_> = config_store::load_indexers()
        .into_iter()
        .filter_map(|mut i| {
            downloaders::safe_http_url(&i.url).map(|u| {
                i.url = u;
                i
            })
        })
        .collect();
    // ⚠️ 启动这条路**不落盘**：只是把已有的读回来。从没配过索引器的机器不该因为
    // 启动一次就多出一个 indexers.json —— 落盘只发生在用户点「添加 / 清空」时。
    ui.set_indexer_status(config_store::describe(&indexers).into());
    rebuild_dynamic_factories(&indexers);
    // ⚠️ 必须在 `indexers` 被 move 进 STATE **之前**同步（下面那行会吃掉它）
    sync_indexers(&ui, &indexers);
    STATE.with(|s| s.borrow_mut().indexers = indexers);

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
