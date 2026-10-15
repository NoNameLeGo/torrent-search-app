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

mod highlight;
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
use ui_model::{AppState, ChipData, RowData, Tone};

// ---------------------------------------------------------------------------
// 进程级共享（都是 Send + Sync，可以安全地被后台任务借）
// ---------------------------------------------------------------------------

/// 静态注册表（16 家）。⚠️ 将来 Torznab 的动态 provider 要并进这里 ——
/// `aggregate` 只认 `&[Box<dyn Provider>]`，不关心编译期还是运行期来的。
fn registry() -> &'static [Box<dyn Provider>] {
    static REGISTRY: OnceLock<Vec<Box<dyn Provider>>> = OnceLock::new();
    REGISTRY.get_or_init(provider_registry::registry)
}

fn http() -> &'static HttpClient {
    static HTTP: OnceLock<HttpClient> = OnceLock::new();
    HTTP.get_or_init(HttpClient::new)
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
fn sync_small(ui: &AppWindow, st: &UiState) {
    let chips: Vec<ProviderChip> = st.app.chips.iter().map(|c| to_slint_chip(ui, c)).collect();
    ui.set_providers(ModelRc::from(Rc::new(VecModel::from(chips))));
    ui.set_total(st.app.total() as i32);
    ui.set_searching(st.app.chips.iter().any(|c| c.tone == Tone::Dim));
    ui.set_status_text(st.status.clone().into());
}

// ---------------------------------------------------------------------------
// 搜索
// ---------------------------------------------------------------------------

/// 开始一轮搜索。**在 UI 线程调用**。
fn start_search(ui: &AppWindow, query: String) {
    let weak = ui.as_weak();
    let accent = accent_hex(ui);

    // 目标 = 与聚合层**同一套**筛选逻辑（别在这儿重写一遍）
    let args = SearchArgs {
        providers: None,
        page: 1,
        browse: false,
    };
    let targets: Vec<(String, String)> = aggregate::resolve_targets(registry(), &args)
        .iter()
        .map(|p| (p.id().to_string(), p.name().to_string()))
        .collect();

    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.app.begin_search(&targets);
        st.query = query.clone();
        st.accent = accent.clone();
        // 换一个新模型：整块重建一次是安全的（此时列表本来就是空的）
        st.rows_model = Rc::new(VecModel::from(Vec::new()));
        st.status = format!("搜索中…（{} 家）", targets.len());

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
    tokio::spawn(async move {
        // ⚠️ `registry()` / `http()` 都是 'static，所以这里不用把任何东西 move 进来；
        // 真需要 move 的只有 query 与 Weak 句柄（都是 Send）。
        let _ = aggregate::search_stream(
            http(),
            registry(),
            &q,
            SearchArgs {
                providers: None,
                page: 1,
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

        // 收尾：把 searching 关掉、状态行换成最终统计
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            STATE.with(|s| {
                let mut st = s.borrow_mut();
                st.status = st.app.summary(false);
                sync_small(&ui, &st);
            });
        });
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
    tokio::spawn(async move {
        let idx = plan.idx;

        // 第 1 步：需要就先惰性解析磁力
        let magnet = if plan.need_resolve {
            let url = plan.detail_url.clone().unwrap_or_default();
            // ⚠️ `Box<dyn Provider>` **不是 Clone**，所以只能借 ——
            // 借得到是因为 `registry()` 是 'static。
            let Some(p) = registry().iter().find(|p| p.id() == plan.provider) else {
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
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        st.downloader = Some((kind, cfg));
        st.status = format!("下载器已保存：{}", kind.as_str());
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

    tokio::spawn(async move {
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

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

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
                start_search(&ui, q.to_string());
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

    ui.run()
}
