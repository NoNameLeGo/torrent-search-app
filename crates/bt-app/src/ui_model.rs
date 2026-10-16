//! UI 数据映射层 —— **纯函数**，不碰 Slint、不碰网络。
//!
//! 为什么单独一层：M4 是 GUI，渲染路径在 CI 上只能靠 `--selftest` 粗验；
//! 而「搜索结果怎么变成行、每站状态怎么变成状态条、按钮该显示什么」这些**逻辑**
//! 是能离线单测的。把它们从 `main.rs` 的接线里拎出来，就等于把 M4 的大部分
//! 行为变成了 CI 可回归的东西。
//!
//! 这一层**不 import slint**：`.slint` 那边的 `ResultRow` / `ProviderChip` 由
//! `main.rs` 负责搬运。色彩也只给 [`Tone`] 枚举 —— 具体 RGB 在 `.slint` 的
//! `Theme` global 里（单一来源），`main.rs` 读它做映射。

use bt_core::TorrentResult;
use bt_providers::{ProviderReport, ProviderState, ProviderStatus};

use crate::highlight::highlight_markup;

/// 一条结果行右侧按钮的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowAction {
    /// 结果里没有磁力（`needs_magnet` 或 magnet 为空）→ 点了先解析再推
    NeedsMagnet,
    /// 已经有磁力 → 点了直接推
    Ready,
    /// 正在惰性解析磁力
    Resolving,
    /// 已经推送成功
    Done,
}

impl RowAction {
    /// 按钮文案（`.slint` 直接显示它）。
    pub fn label(self) -> &'static str {
        match self {
            RowAction::NeedsMagnet => "取磁力并推送",
            RowAction::Ready => "推送到下载器",
            RowAction::Resolving => "解析中…",
            RowAction::Done => "已推送 ✓",
        }
    }
}

/// 状态条的色调 —— 具体颜色由 `.slint` 的 `Theme` 决定（这里只给语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Err,
    Dim,
}

/// 状态条上的一站。字段与 `.slint` 的 `ProviderChip` 一一对应（`tone` 在那边是 color）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipData {
    pub id: String,
    pub name: String,
    pub glyph: &'static str,
    pub detail: String,
    pub tone: Tone,
}

/// 一行结果。字段与 `.slint` 的 `ResultRow` 一一对应
/// （`title_markup` 那边是 `styled-text`，由 `main.rs` 用 `StyledText::from_markdown` 转换）。
#[derive(Debug, Clone, PartialEq)]
pub struct RowData {
    pub idx: i32,
    /// **稳定 key** —— 这一行一生不变（`idx` 会被排序重编号）。
    ///
    /// ⚠️ 为什么要它：`push_row` 中间有个**网络往返**（惰性解析磁力），回来时用户
    /// 可能已经点过排序 chip（[`sort_rows`] 会重编号 `idx`）⇒ 拿旧 `idx` 写回会把
    /// 解析出的磁力和「已推送 ✓」落到**别的行**上 —— 之后复制/推送用的是**错磁力**。
    /// 所以凡是**跨 await 的写回**，一律用 key 定位，不用 `idx`。
    pub key: u64,
    /// markdown 串（关键词高亮已经注入）—— **不是** `StyledText`，
    /// 因为本层要能离线单测，而 `StyledText` 属于 UI 线程的东西。
    pub title_markup: String,
    pub title_plain: String,
    pub meta: String,
    pub provider: String,
    pub seeds: i32,
    pub leechers: i32,
    /// 结果自带的磁力（可能为空 —— 那 6 家要惰性解析）
    pub magnet: Option<String>,
    pub info_hash: Option<String>,
    pub detail_url: Option<String>,
    pub action: RowAction,
    // ---- 排序 / 筛选要用的原始值 ----
    // ⚠️ 这三项**原先没进 `RowData`**（当时 UI 只显示 `meta` 那串格式化文本）。
    // 排序必须比**数值**、不能比 `"3.2 GB"` 这种字符串（`"10 MB" < "3 GB"` 是错的），
    // 所以从 `TorrentResult` 直接留一份 —— **别从 `meta` 里反解**。
    pub size: Option<i64>,
    pub date: Option<i64>,
    pub category: Option<String>,
}

impl RowData {
    /// 从一条 `TorrentResult` 建行。`idx` = 它在权威 Vec 里的下标，`key` = 稳定标识
    /// （由 [`AppState::apply_report`] 发号，别自己编）。
    pub fn from_result(r: &TorrentResult, idx: i32, key: u64, query: &str, accent: &str) -> Self {
        let has_magnet = r.magnet.as_deref().is_some_and(|m| !m.is_empty());
        Self {
            idx,
            key,
            title_markup: highlight_markup(&r.name, query, accent),
            title_plain: r.name.clone(),
            meta: meta_text(r),
            provider: r.provider.clone(),
            seeds: clamp_i32(r.seeders),
            leechers: clamp_i32(r.leechers),
            magnet: r.magnet.clone(),
            info_hash: r.info_hash.clone(),
            detail_url: r.detail_url.clone(),
            action: if has_magnet {
                RowAction::Ready
            } else {
                RowAction::NeedsMagnet
            },
            size: r.size,
            date: r.date,
            category: r.category.clone(),
        }
    }

    /// 拿得到磁力吗（直接给或解析出来后）。
    pub fn magnet(&self) -> Option<&str> {
        self.magnet.as_deref().filter(|m| !m.is_empty())
    }
}

/// `i64 -> i32` 并且夹到非负 —— UI 上显示负数 seeder 没有意义。
fn clamp_i32(v: Option<i64>) -> i32 {
    v.unwrap_or(0).clamp(0, i32::MAX as i64) as i32
}

/// 卡片上那行元信息：`大小 · 日期 · 分类 · N 个文件`。
///
/// ⚠️ 大小/日期直接用 `TorrentResult` 里已经格式化好的 `size_text` / `date_text`
/// （codebase 约定：`date_text` 缺失时是 `"—"`，不是空串）—— 别在这儿重新格式化，
/// 那会和 provider 层算出来的东西分叉。
pub fn meta_text(r: &TorrentResult) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(non_empty_or_dash(&r.size_text));
    parts.push(non_empty_or_dash(&r.date_text));
    if let Some(c) = r.category.as_deref().filter(|c| !c.is_empty()) {
        parts.push(c.to_string());
    }
    if let Some(f) = r.files {
        parts.push(format!("{f} 个文件"));
    }
    parts.join(" · ")
}

fn non_empty_or_dash(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

/// 一站上报之后的状态条。
///
/// `stale_runs` = 这家**最近连续空手**了几次（来自 `health`，#6）。
/// ⚠️ 为什么要它：站点改版的表现是**"每次都成功、但都没有结果"** ——
/// 光看这一次上报（`Ok` + 0 条）和"这个关键词本来就没资源"**完全分不出来**，
/// 只有历史能分。
pub fn chip_for(id: &str, name: &str, status: &ProviderStatus, stale_runs: usize) -> ChipData {
    // ⚠️ **两个条件都要**：`count == 0` 让不变式在**函数内部**成立 ——
    // "有结果"就不可能是"连空"。（`apply_report` 有结果时也会把计数归零，
    // 但纯函数不该依赖调用方做对。）
    let stale = status.count == 0 && stale_runs >= crate::health::STALE_AFTER;
    match status.status {
        ProviderState::Ok => ChipData {
            id: id.to_string(),
            name: name.to_string(),
            glyph: "✓",
            detail: if stale {
                // 状态条窄，所以文案短（比错误串那 18 字还短）
                format!("{} 条 ⚠连空 {stale_runs} 次", status.count)
            } else {
                format!("{} 条", status.count)
            },
            tone: Tone::Ok,
        },
        ProviderState::Error => ChipData {
            id: id.to_string(),
            name: name.to_string(),
            glyph: "✗",
            // 状态条很窄，错误串截断显示；完整信息在状态行
            detail: truncate(status.error.as_deref().unwrap_or("错误"), 18),
            tone: Tone::Err,
        },
    }
}

/// 搜索刚开始时的「进行中」状态条。
pub fn pending_chip(id: &str, name: &str) -> ChipData {
    ChipData {
        id: id.to_string(),
        name: name.to_string(),
        glyph: "…",
        detail: "进行中".to_string(),
        tone: Tone::Dim,
    }
}

/// 按 **char** 截断（别按 byte 切中文）。
pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// M4 的界面状态机 —— **这一层就是 M4 可被 CI 回归的部分**。
/// 站点开关的一行（③）。字段与 `.slint` 的 `SiteToggle` 对应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteData {
    pub id: String,
    pub name: String,
    /// 参与搜索吗
    pub on: bool,
}

impl SiteData {
    /// 按钮文案。**字形在 Rust 侧算好** —— `.slint` 里不写条件表达式（少一处语法风险）。
    pub fn label(&self) -> String {
        format!("{} {}", if self.on { "✓" } else { "○" }, self.name)
    }
}

/// 排序键。**不是从 JS 抄的** —— 见 [`relevance`] 的说明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Seeds,
    Relevance,
    Size,
    Date,
}

impl SortKey {
    /// 4 个键，顺序 = UI 显示顺序。
    /// ⚠️ **做种数排第一个**是有意的：对"能不能下得动"它是最直接的信号。
    pub const ALL: [SortKey; 4] = [
        SortKey::Seeds,
        SortKey::Relevance,
        SortKey::Size,
        SortKey::Date,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SortKey::Seeds => "seeds",
            SortKey::Relevance => "relevance",
            SortKey::Size => "size",
            SortKey::Date => "date",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Seeds => "做种数",
            SortKey::Relevance => "相关度",
            SortKey::Size => "大小",
            SortKey::Date => "时间",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == s)
    }
}

impl Default for SortKey {
    /// ⚠️ 默认**按做种数降序** —— 这是 Rust 版的选择，**不是** JS 的默认值。
    /// 理由：D7 说"搜索是载体、成品才是目的"，而做种数是"这条能不能下"最直接的信号。
    fn default() -> Self {
        SortKey::Seeds
    }
}

/// 相关度打分（**Rust 版自定义**，没照抄 JS 的打分函数）。
///
/// ## 为什么可以不逐字对齐 JS
///
/// M6 差分比的是**聚合层输出的 `results` 数组**（那 15 个字段）——
/// 排序是 **UI 层**的事、不进差分，所以这里有自由。
/// 但**规则必须写清**，免得以后有人以为它是从 JS 抄来却"抄错了"。
///
/// 规则（1 命中即停，2/3 不叠加）：
///   1. 标题**就是**查询词（忽略大小写）→ 1000
///   2. 否则**以**查询词开头 → 500
///   3. 否则标题**含有**查询词 → 300
///   4. 多词查询再按**词元覆盖率**加分（每个命中 +200）
///   5. 全不命中 → 0
///
/// 空 / 纯空白查询词 → 0（此时排序等于没排，稳定排序保持到达顺序）。
pub fn relevance(name: &str, query: &str) -> i32 {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return 0;
    }
    let n = name.to_lowercase();

    let mut score = if n == q {
        1000
    } else if n.starts_with(&q) {
        500
    } else if n.contains(&q) {
        300
    } else {
        0
    };

    // 多词查询靠词元覆盖率拉开差距（"ubuntu server" 中两个词的该排在只中一个的前面）
    let toks: Vec<&str> = q.split_whitespace().collect();
    if toks.len() > 1 {
        score += toks.iter().filter(|t| n.contains(**t)).count() as i32 * 200;
    }
    score
}

/// **就地**排序，排完**重新编号 `idx`**。`sort_by` 是稳定排序。
///
/// ⚠️ **重编号是必须的、不是顺手做的**：`idx` 是"回指权威 `rows` 的下标"，
/// UI 的 `push-row(idx)` 靠它取那一条的磁力。排完不重编号 ⇒ 点第 3 行推的是
/// 第 3 个**到达**的结果 —— 错得很隐蔽：能推，只是推错东西。
///
/// ⚠️ **缺值一律排最后**（不是当 0 排最前）："没有做种数信息"和"0 个做种"是两回事，
/// 把前者混进最前面会让它看起来像热门资源。
pub fn sort_rows(rows: &mut [RowData], key: SortKey, asc: bool, query: &str) {
    /// 方向只翻转**有值**那一档 —— 否则升序会把缺值翻到最前面来。
    fn ord_opt(a: Option<i64>, b: Option<i64>, asc: bool) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => {
                if asc {
                    x.cmp(&y)
                } else {
                    y.cmp(&x)
                }
            }
        }
    }

    rows.sort_by(|a, b| match key {
        SortKey::Relevance => {
            let (x, y) = (
                relevance(&a.title_plain, query),
                relevance(&b.title_plain, query),
            );
            if asc {
                x.cmp(&y)
            } else {
                y.cmp(&x)
            }
        }
        SortKey::Seeds => ord_opt(Some(a.seeds as i64), Some(b.seeds as i64), asc),
        SortKey::Size => ord_opt(a.size, b.size, asc),
        SortKey::Date => ord_opt(a.date, b.date, asc),
    });

    for (i, r) in rows.iter_mut().enumerate() {
        r.idx = i as i32;
    }
}

/// 分类筛选要用的 `(分类名, 条数)`，**按条数降序** —— 排在前面的才是用户真搜到的东西。
pub fn categories(rows: &[RowData]) -> Vec<(String, usize)> {
    let mut acc: Vec<(String, usize)> = Vec::new();
    for r in rows {
        let Some(c) = r.category.as_deref().filter(|c| !c.is_empty()) else {
            continue;
        };
        match acc.iter_mut().find(|(name, _)| name == c) {
            Some((_, n)) => *n += 1,
            None => acc.push((c.to_string(), 1)),
        }
    }
    // 同票数时按名字排 —— 否则顺序取决于插入次序，UI 上的 chip 会跳来跳去
    acc.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    acc
}

/// 排序按钮 / 分类 chip 的一行（`.slint` 的 `Chip` 对应）。
/// **文案与"选中"标记都由 Rust 算好** —— `.slint` 里不写条件表达式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Default)]
pub struct AppState {
    /// 权威的行集合。`.slint` 的模型是它的**视图**（可能被排序 / 筛选过）；
    /// `idx` 永远是它在**这个 Vec** 里的下标。
    pub rows: Vec<RowData>,
    pub chips: Vec<ChipData>,
    /// ① 当前页码（1 起）
    pub page: u32,
    /// ① 引擎层回的 `has_more` —— 「下一页」按钮的 enabled 用它
    pub has_more: bool,
    /// ③ 站点开关。顺序 = 注册表顺序 = UI 显示顺序
    pub sites: Vec<SiteData>,
    /// 排序键 + 方向（`asc = false` 是常用的那一档）
    pub sort: SortKey,
    pub sort_asc: bool,
    /// 分类筛选。`None` = 全部
    pub category: Option<String>,
    /// #2 这次搜索里**连上了但返回 0 条**的家数。
    ///
    /// ⚠️ 为什么要单独记：`ChipData` 只有 `tone`（`Ok`/`Err`/`Dim`），
    /// 而"HTTP 200 + 0 条"的 tone 也是 `Ok` —— 从 chips 里**看不出**它和
    /// "成功返回了 50 条"的区别。而这两者对用户的意义完全不同：
    /// 前者很可能是**站点改版了**（选择器失效），后者是真搜到了。
    pub zero_hits: usize,
    /// #2 失败再分一层：**网络层**（超时 / 连不上）与**站点层**（HTTP 状态 / 认不出来）。
    ///
    /// ⚠️ 这个区分是有意义的：它决定用户该**换网**还是**换站** —— 那是他唯一能做的动作。
    /// 判据来自 `bt_core::http::classify_error`（依据是 providers 复刻的 axios 错误码，
    /// **不是**对文案的猜测）。
    pub net_failures: usize,
    pub site_failures: usize,
    /// #6 每家的**最近连续空手次数**（`health::summarize` 的 `tail_empty`）。
    ///
    /// ⚠️ 用 `HashMap` 而不是 Vec：这里只按 id 查，不需要顺序
    /// —— 顺序由 `chips` 保证，那才是 UI 的显示顺序。
    pub stale: std::collections::HashMap<String, usize>,
    /// 发号器（见 [`RowData::key`]）。只增不减 —— **不随 `begin_search` 归零**，
    /// 否则上一轮迟到的异步写回可能命中新搜索里的同号行。
    next_key: u64,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 开始一轮搜索：**清空结果**，把每个目标站点置成「进行中」。
    ///
    /// `providers` 是 `(id, name)` 列表，顺序 = UI 显示顺序（来自注册表）。
    pub fn begin_search(&mut self, providers: &[(String, String)]) {
        self.rows.clear();
        self.has_more = false;
        self.zero_hits = 0;
        self.net_failures = 0;
        self.site_failures = 0;
        self.chips = providers
            .iter()
            .map(|(id, name)| pending_chip(id, name))
            .collect();
    }

    /// ① 换页。页码是 UI 的，`has_more` 要等这一页跑完才知道 ⇒ 先归零
    /// （否则用户点「下一页」的瞬间按钮还会亮着，能连点）。
    pub fn set_page(&mut self, page: u32) {
        self.page = page;
        self.has_more = false;
    }

    /// ③ 用注册表补齐站点开关列表：**已知的保留勾选状态**，新出现的默认开。
    ///
    /// 「保留已知的」是有意的：注册表将来可能变（加 provider / 动态 Torznab），
    /// 不该因为一次搜索就把用户已经取消掉的站点又勾回来。
    pub fn sync_sites(&mut self, providers: &[(String, String)]) {
        let next = providers
            .iter()
            .map(|(id, name)| SiteData {
                id: id.clone(),
                name: name.clone(),
                on: self
                    .sites
                    .iter()
                    .find(|s| &s.id == id)
                    .map_or(true, |s| s.on),
            })
            .collect();
        self.sites = next;
    }

    /// ③ 翻转一个站点，返回它现在是否参与。`None` = 没有这个 id。
    pub fn toggle_site(&mut self, id: &str) -> Option<bool> {
        let s = self.sites.iter_mut().find(|s| s.id == id)?;
        s.on = !s.on;
        Some(s.on)
    }

    /// ③ 勾选中的站点，逗号分隔 —— 直接喂给 `SearchArgs::providers`。
    ///
    /// **全选时返回 `None`**（= 不筛选）：与 JS 同口径，而且少一次逐家比对。
    pub fn selected_providers(&self) -> Option<String> {
        let picked: Vec<&str> = self
            .sites
            .iter()
            .filter(|s| s.on)
            .map(|s| s.id.as_str())
            .collect();
        if self.sites.is_empty() || picked.len() == self.sites.len() {
            None
        } else {
            Some(picked.join(","))
        }
    }

    /// ③ 一个都没勾 —— **这种情况不发搜索**。
    ///
    /// ⚠️ 这里**有意**不沿用引擎层的语义：`SearchArgs::providers` 收到 `""` 会被当成
    /// 「没筛选」⇒ 打出全部 17 家（那是 M2 按 JS 复制并测过的**引擎**口径，别去改它）。
    /// 但 UI 层不该让「我把站点全取消勾选」变成「偷偷搜全部」—— 那是反直觉的。
    /// 所以这一层拦下来，由调用方给一句提示。
    pub fn none_selected(&self) -> bool {
        !self.sites.is_empty() && !self.sites.iter().any(|s| s.on)
    }

    // -----------------------------------------------------------------------
    // 排序 / 筛选（UI 层的视图，不动权威 rows 的语义）
    // -----------------------------------------------------------------------

    /// 点一个排序键：**同一个键再点一次就翻方向**，换键则回到默认方向（降序）。
    ///
    /// ⚠️ 排序会**重编号 `idx`**（见 [`sort_rows`]）—— 所以调用方必须**重建模型**，
    /// 不能只更新现有行。
    pub fn pick_sort(&mut self, key: SortKey, query: &str) {
        if self.sort == key {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort = key;
            self.sort_asc = false;
        }
        sort_rows(&mut self.rows, self.sort, self.sort_asc, query);
    }

    /// 设分类筛选（`None` = 全部）。**不改 `rows`** —— 只影响 [`AppState::view`]。
    pub fn set_category(&mut self, cat: Option<String>) {
        self.category = cat;
    }

    /// **当前视图** = `rows` 里符合筛选的那些，顺序即 `rows` 的顺序（已排过序）。
    ///
    /// ⚠️ 视图下标**不是** `idx`，别混：模型的第 N 行显示 `view()[N]`，
    /// 但点它要推的是 `view()[N].idx` —— `to_slint_row` 已经把它填进 `ResultRow.idx` 了。
    pub fn view(&self) -> Vec<&RowData> {
        self.rows
            .iter()
            .filter(|r| match &self.category {
                None => true,
                Some(c) => r.category.as_deref() == Some(c.as_str()),
            })
            .collect()
    }

    /// 排序按钮的文案：选中的那个带方向箭头（`↓` 降序 / `↑` 升序）。
    pub fn sort_chips(&self) -> Vec<Chip> {
        SortKey::ALL
            .into_iter()
            .map(|k| {
                let label = if k == self.sort {
                    format!("{} {}", k.label(), if self.sort_asc { "↑" } else { "↓" })
                } else {
                    k.label().to_string()
                };
                Chip {
                    id: k.id().to_string(),
                    label,
                }
            })
            .collect()
    }

    /// 分类 chip。**第一个恒为「全部」**（`id` 是空串 = 不筛），选中的那个带方括号。
    pub fn cat_chips(&self) -> Vec<Chip> {
        let total = self.rows.len();
        let mut out = vec![Chip {
            id: String::new(),
            label: if self.category.is_none() {
                format!("[全部] {total}")
            } else {
                format!("全部 {total}")
            },
        }];
        for (name, n) in categories(&self.rows) {
            let on = self.category.as_deref() == Some(name.as_str());
            out.push(Chip {
                id: name.clone(),
                label: if on {
                    format!("[{name}] {n}")
                } else {
                    format!("{name} {n}")
                },
            });
        }
        out
    }

    /// 一站回来了：**追加**它的结果（所以「谁先回来谁先显示」），并更新它的状态条。
    ///
    /// ⚠️ 别写成「全部回来后一次性 fill」—— 那正是 `search_stream` 要避免的体验问题。
    pub fn apply_report(&mut self, report: &ProviderReport, query: &str, accent: &str) {
        for r in &report.results {
            let idx = self.rows.len() as i32;
            self.next_key += 1;
            let key = self.next_key;
            self.rows
                .push(RowData::from_result(r, idx, key, query, accent));
        }

        // #2 「连上了但 0 条」单独计数 —— 它的 tone 也是 Ok，从 chips 里看不出来。
        // ⚠️ 只在**没有错误**时算：报错的家 count 也是 0，但那是"失败"不是"返回空"。
        if report.status.error.is_none() && report.status.count == 0 {
            self.zero_hits += 1;
        }

        // #2 失败再分一层：网络层（该换网）vs 站点层（该换站）。
        // 判据是 `bt_core::http` 的 axios 兼容错误码 —— 纯函数，不猜文案。
        if let Some(err) = report.status.error.as_deref() {
            if bt_core::http::classify_error(err) == bt_core::http::ErrorKind::Network {
                self.net_failures += 1;
            } else {
                self.site_failures += 1;
            }
        }

        // #6 **就地**更新健康度：有结果 ⇒ 归零；空手/报错 ⇒ +1。
        //
        // ⚠️ 必须在这里更新、不能挪到"搜索收尾"去更新：`chips` 是在这里**逐个**建起来的，
        // 收尾再更新的话，状态条上的 ⚠ 会比 `--health` 的数字**差一轮** ——
        // 两个视图对不上，用户就没法用它们互相印证了。
        // ⚠️ 语义要与 `health::summarize` 一致（它也是"有结果就打断连续计数"）。
        let empty = report.status.error.is_some() || report.status.count == 0;
        let stale_runs = {
            let cur = self.stale.get(&report.id).copied().unwrap_or(0);
            if empty {
                cur + 1
            } else {
                0
            }
        };
        self.stale.insert(report.id.clone(), stale_runs);
        let chip = chip_for(&report.id, &report.name, &report.status, stale_runs);
        match self.chips.iter_mut().find(|c| c.id == report.id) {
            Some(slot) => *slot = chip,
            // 注册表里没有它（将来 Torznab 的动态 provider 就会走这条）→ 追加
            None => self.chips.push(chip),
        }
    }

    pub fn total(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, idx: i32) -> Option<&RowData> {
        if idx < 0 {
            return None;
        }
        self.rows.get(idx as usize)
    }

    /// 按**稳定 key** 找行 —— 跨 await 的写回必须用它（见 [`RowData::key`]）。
    pub fn row_by_key(&self, key: u64) -> Option<&RowData> {
        self.rows.iter().find(|r| r.key == key)
    }

    /// 按 key 改按钮状态。返回 `false` = 那行已经不在了（被新搜索清掉）—— 静默即可。
    pub fn set_action_by_key(&mut self, key: u64, action: RowAction) -> bool {
        match self.rows.iter_mut().find(|r| r.key == key) {
            Some(r) => {
                r.action = action;
                true
            }
            None => false,
        }
    }

    /// 按 key 写回解析出的磁力。
    pub fn set_magnet_by_key(&mut self, key: u64, magnet: String) -> bool {
        match self.rows.iter_mut().find(|r| r.key == key) {
            Some(r) => {
                r.magnet = Some(magnet);
                true
            }
            None => false,
        }
    }

    pub fn set_action(&mut self, idx: i32, action: RowAction) -> bool {
        match self.row_mut(idx) {
            Some(r) => {
                r.action = action;
                true
            }
            None => false,
        }
    }

    pub fn set_magnet(&mut self, idx: i32, magnet: String) -> bool {
        match self.row_mut(idx) {
            Some(r) => {
                r.magnet = Some(magnet);
                true
            }
            None => false,
        }
    }

    fn row_mut(&mut self, idx: i32) -> Option<&mut RowData> {
        if idx < 0 {
            return None;
        }
        self.rows.get_mut(idx as usize)
    }

    /// 搜索结束后的状态行：`共 N 条 · X 家成功 / Y 家失败`。
    pub fn summary(&self, searching: bool) -> String {
        if searching {
            let done = self.chips.iter().filter(|c| c.tone != Tone::Dim).count();
            return format!(
                "搜索中… {done}/{} 家已返回 · 已 {N} 条",
                self.chips.len(),
                N = self.rows.len()
            );
        }
        let ok = self.chips.iter().filter(|c| c.tone == Tone::Ok).count();
        let err = self.chips.iter().filter(|c| c.tone == Tone::Err).count();
        let zero = self.zero_hits;
        let total = self.chips.len();
        // ⚠️ `pending == 0` 这个守卫不能少：`summary(false)` 的语义是"**这轮结束了**"，
        // 但调用方也可能在还没人回来时调它（现有用例就是这么测的）。
        // 没有它的话，"全都还在等"会被说成"一条都没有，多半是网络问题" —— 胡说八道。
        let pending = self.chips.iter().filter(|c| c.tone == Tone::Dim).count();

        // #2 **一条都没有的时候必须把原因说细**，因为那是用户唯一想知道的事。
        // 这三种情况的应对完全不同：换网 / 换站 / 换关键词。
        if self.rows.is_empty() && total > 0 && pending == 0 {
            if err == total {
                // #2 再分一层：**网络层**（换网有用）还是**站点层**（换站有用）。
                // 全都失败时这一天差就是用户唯一能采取的动作，所以要说准。
                let hint = if self.net_failures == err {
                    "**网络层不通**（超时 / 连不上）—— 多半是网络问题，先查网络或代理"
                } else if self.site_failures == err {
                    "**站点层拒绝**（403 / 限流 / 返回内容不认识）—— 站点还在，是它们挡了这次请求"
                } else {
                    "网络层与站点层都有 —— 先把网络理顺，再看站点"
                };
                return format!("一条都没有：{total} 家**全部失败** —— {hint}");
            }
            if zero == total {
                return format!(
                    "一条都没有：{total} 家都连上了但**返回 0 条** —— 更像是站点改版了（选择器失效）"
                );
            }
            return format!("一条都没有：{ok} 家成功但没结果 / {err} 家失败 / {zero} 家返回空");
        }

        let tail = if zero > 0 {
            format!(" / {zero} 家返回 0 条")
        } else {
            String::new()
        };
        format!(
            "共 {N} 条 · {ok} 家成功 / {err} 家失败{tail}",
            N = self.rows.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::TorrentResult;

    fn hit(name: &str, provider: &str, magnet: Option<&str>) -> TorrentResult {
        TorrentResult {
            id: name.to_string(),
            name: name.to_string(),
            size: Some(1024),
            size_text: "1.0 GB".to_string(),
            seeders: Some(12),
            leechers: Some(3),
            date: None,
            date_text: "—".to_string(),
            magnet: magnet.map(|m| m.to_string()),
            info_hash: None,
            provider: provider.to_string(),
            category: Some("movies".to_string()),
            detail_url: Some("https://example.invalid/t/1".to_string()),
            files: Some(2),
            needs_magnet: magnet.is_none(),
        }
    }

    fn report(
        id: &str,
        name: &str,
        results: Vec<TorrentResult>,
        error: Option<&str>,
    ) -> ProviderReport {
        let status = ProviderStatus {
            status: if error.is_some() {
                ProviderState::Error
            } else {
                ProviderState::Ok
            },
            count: results.len(),
            error: error.map(|e| e.to_string()),
            has_more: None,
            ms: 7,
        };
        ProviderReport {
            id: id.to_string(),
            name: name.to_string(),
            results,
            status,
        }
    }

    fn two_providers() -> Vec<(String, String)> {
        vec![
            ("tpb".to_string(), "The Pirate Bay".to_string()),
            ("nyaa".to_string(), "NYAA".to_string()),
        ]
    }

    #[test]
    fn begin_search_resets_rows_and_marks_every_provider_pending() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());

        assert!(st.rows.is_empty());
        assert_eq!(st.chips.len(), 2);
        assert!(st
            .chips
            .iter()
            .all(|c| c.glyph == "…" && c.tone == Tone::Dim));
        assert_eq!(st.chips[0].name, "The Pirate Bay");
    }

    #[test]
    fn reports_append_in_arrival_order_and_update_their_own_chip() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());

        // 后发起的 nyaa 先回来 —— 它就该先出现在列表里
        st.apply_report(
            &report("nyaa", "NYAA", vec![hit("nyaa-a", "nyaa", None)], None),
            "a",
            "#fff",
        );
        st.apply_report(
            &report(
                "tpb",
                "The Pirate Bay",
                vec![hit("tpb-a", "tpb", None), hit("tpb-b", "tpb", None)],
                None,
            ),
            "a",
            "#fff",
        );

        assert_eq!(st.total(), 3);
        let providers: Vec<&str> = st.rows.iter().map(|r| r.provider.as_str()).collect();
        assert_eq!(
            providers,
            ["nyaa", "tpb", "tpb"],
            "行顺序 = 到站顺序（这就是「谁先回来先显示谁」）"
        );

        // 行号连续且等于下标
        assert_eq!(st.rows.iter().map(|r| r.idx).collect::<Vec<_>>(), [0, 1, 2]);

        // 状态条各自更新，顺序仍是注册表顺序
        assert_eq!(st.chips[0].id, "tpb");
        assert_eq!(st.chips[0].glyph, "✓");
        assert_eq!(st.chips[0].detail, "2 条");
        assert_eq!(st.chips[1].id, "nyaa");
        assert_eq!(st.chips[1].detail, "1 条");
    }

    #[test]
    fn a_failing_provider_shows_an_error_chip_without_killing_the_others() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());

        st.apply_report(
            &report(
                "tpb",
                "The Pirate Bay",
                vec![],
                Some("tpb unreachable (boom)"),
            ),
            "q",
            "#fff",
        );
        st.apply_report(
            &report("nyaa", "NYAA", vec![hit("n", "nyaa", None)], None),
            "q",
            "#fff",
        );

        assert_eq!(st.total(), 1, "失败的那家不产生行，但也不影响别家");
        assert_eq!(st.chips[0].glyph, "✗");
        assert_eq!(st.chips[0].tone, Tone::Err);
        assert!(
            st.chips[0].detail.ends_with('…'),
            "窄条里要截断：{}",
            st.chips[0].detail
        );
        assert_eq!(st.chips[1].glyph, "✓");
    }

    #[test]
    fn action_depends_on_whether_a_magnet_came_with_the_result() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report(
                "tpb",
                "The Pirate Bay",
                vec![
                    hit("with", "tpb", Some("magnet:?xt=urn:btih:AAA")),
                    hit("without", "tpb", None),
                ],
                None,
            ),
            "q",
            "#fff",
        );

        assert_eq!(st.rows[0].action, RowAction::Ready);
        assert_eq!(st.rows[0].action.label(), "推送到下载器");
        assert_eq!(st.rows[1].action, RowAction::NeedsMagnet);
        assert_eq!(st.rows[1].action.label(), "取磁力并推送");

        // 惰性解析成功后：动作切回「推送」，磁力也补上了
        assert!(st.set_magnet(1, "magnet:?xt=urn:btih:BBB".to_string()));
        assert!(st.set_action(1, RowAction::Ready));
        assert_eq!(st.rows[1].magnet(), Some("magnet:?xt=urn:btih:BBB"));
        assert_eq!(st.rows[1].action, RowAction::Ready);
    }

    #[test]
    fn out_of_range_row_indices_are_ignored_not_panicked() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report("tpb", "TPB", vec![hit("a", "tpb", None)], None),
            "q",
            "#fff",
        );

        assert!(st.row(-1).is_none());
        assert!(st.row(99).is_none());
        assert!(!st.set_action(99, RowAction::Ready));
        assert!(!st.set_magnet(-1, "x".into()));
        // 权威状态没被弄坏
        assert_eq!(st.rows[0].action, RowAction::NeedsMagnet);
    }

    #[test]
    fn title_carries_the_keyword_highlight() {
        let mut st = AppState::new();
        st.begin_search(&[(("tpb").to_string(), "TPB".to_string())]);
        st.apply_report(
            &report(
                "tpb",
                "TPB",
                vec![hit("Ubuntu 24.04 desktop", "tpb", None)],
                None,
            ),
            "ubuntu",
            "#f9e2af",
        );

        assert_eq!(
            st.rows[0].title_markup,
            "<font color=\"#f9e2af\">Ubuntu</font> 24.04 desktop"
        );
        assert_eq!(st.rows[0].title_plain, "Ubuntu 24.04 desktop");
    }

    #[test]
    fn meta_text_uses_the_preformatted_fields_and_missing_ones_become_a_dash() {
        let r = hit("x", "tpb", None);
        assert_eq!(meta_text(&r), "1.0 GB · — · movies · 2 个文件");

        let mut empty = r.clone();
        empty.size_text = String::new();
        empty.category = None;
        empty.files = None;
        assert_eq!(meta_text(&empty), "— · —");
    }

    #[test]
    fn seeds_are_clamped_to_non_negative_i32() {
        let mut r = hit("x", "tpb", None);
        r.seeders = Some(-5);
        r.leechers = None;
        let row = RowData::from_result(&r, 0, 0, "q", "#fff");
        assert_eq!(row.seeds, 0);
        assert_eq!(row.leechers, 0);

        r.seeders = Some(i64::MAX);
        let row = RowData::from_result(&r, 0, 0, "q", "#fff");
        assert_eq!(row.seeds, i32::MAX);
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        assert_eq!(truncate("abcdef", 3), "ab…");
        assert_eq!(truncate("abc", 3), "abc");
        // 中文按字算（若按 byte 切会切出半个字）
        assert_eq!(truncate("一二三四五", 3), "一二…");
    }

    #[test]
    fn summary_reflects_searching_and_counts() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        assert_eq!(st.summary(true), "搜索中… 0/2 家已返回 · 已 0 条");
        assert_eq!(st.summary(false), "共 0 条 · 0 家成功 / 0 家失败");

        st.apply_report(
            &report("tpb", "TPB", vec![hit("a", "tpb", None)], None),
            "q",
            "#fff",
        );
        st.apply_report(&report("nyaa", "NYAA", vec![], Some("boom")), "q", "#fff");
        assert_eq!(st.summary(true), "搜索中… 2/2 家已返回 · 已 1 条");
        assert_eq!(st.summary(false), "共 1 条 · 1 家成功 / 1 家失败");
    }

    // -----------------------------------------------------------------------
    // ③ 站点开关
    // -----------------------------------------------------------------------

    fn three_sites() -> Vec<(String, String)> {
        vec![
            ("tpb".to_string(), "The Pirate Bay".to_string()),
            ("nyaa".to_string(), "NYAA".to_string()),
            ("yts".to_string(), "YTS".to_string()),
        ]
    }

    #[test]
    fn sites_default_to_all_on_and_labels_carry_the_glyph() {
        let mut st = AppState::new();
        st.sync_sites(&three_sites());

        assert_eq!(st.sites.len(), 3);
        assert!(st.sites.iter().all(|s| s.on));
        assert_eq!(st.sites[0].label(), "✓ The Pirate Bay");
        // 全勾 ⇒ `None` = 不筛选（与 JS 同口径，少一次逐家比对）
        assert_eq!(st.selected_providers(), None);
        assert!(!st.none_selected());
    }

    #[test]
    fn unchecking_one_site_narrows_the_provider_filter() {
        let mut st = AppState::new();
        st.sync_sites(&three_sites());

        assert_eq!(
            st.toggle_site("nyaa"),
            Some(false),
            "返回的是**翻转后**的状态"
        );
        assert_eq!(st.selected_providers().as_deref(), Some("tpb,yts"));
        assert!(!st.none_selected());
        assert_eq!(
            st.sites.iter().find(|s| s.id == "nyaa").unwrap().label(),
            "○ NYAA"
        );
    }

    /// **这条钉住一个分工**：全不勾时 `selected_providers()` 给出的是 `""`，
    /// 而引擎层收到 `""` 会当成"没筛选"⇒ 搜全部（那是 M2 按 JS 复制的**引擎**口径）。
    /// 所以 UI 层**必须**先用 `none_selected()` 拦住 —— 否则用户"全部取消勾选"
    /// 会变成"偷偷搜全部 17 家"，反直觉。
    #[test]
    fn unchecking_every_site_is_not_a_search_for_everything() {
        let mut st = AppState::new();
        st.sync_sites(&three_sites());
        for id in ["tpb", "nyaa", "yts"] {
            st.toggle_site(id);
        }

        assert!(st.none_selected(), "UI 层要拦住");
        assert_eq!(
            st.selected_providers().as_deref(),
            Some(""),
            "引擎层口径：\"\" 等于不筛选 ⇒ 搜全部。所以拦在前面的必须是 none_selected()"
        );
    }

    #[test]
    fn sync_sites_keeps_the_users_choices() {
        let mut st = AppState::new();
        st.sync_sites(&three_sites());
        st.toggle_site("nyaa");

        // 注册表变了：nyaa 还在、多了一家 dmhy
        st.sync_sites(&vec![
            ("tpb".to_string(), "The Pirate Bay".to_string()),
            ("nyaa".to_string(), "NYAA".to_string()),
            ("dmhy".to_string(), "DMHY".to_string()),
        ]);

        assert!(
            !st.sites.iter().find(|s| s.id == "nyaa").unwrap().on,
            "用户取消过的，不能被下一次搜索重新勾回来"
        );
        assert!(
            st.sites.iter().find(|s| s.id == "dmhy").unwrap().on,
            "新出现的站点默认勾上"
        );
    }

    #[test]
    fn toggle_unknown_id_is_a_noop() {
        let mut st = AppState::new();
        st.sync_sites(&three_sites());
        assert_eq!(st.toggle_site("nope"), None);
        assert!(st.sites.iter().all(|s| s.on));
    }

    // -----------------------------------------------------------------------
    // ① 分页
    // -----------------------------------------------------------------------

    #[test]
    fn set_page_and_begin_search_both_clear_has_more() {
        let mut st = AppState::new();

        st.has_more = true;
        st.set_page(2);
        assert_eq!(st.page, 2);
        assert!(!st.has_more, "换页瞬间先按灭「下一页」，否则能连点");

        // 换页时得先知道"这一页还有没有下一页" —— 所以 begin_search 也要归零，
        // 结果等这一轮跑完由 `has_more` 重新点亮。
        st.has_more = true;
        st.begin_search(&three_sites());
        assert!(!st.has_more);
    }

    // -----------------------------------------------------------------------
    // #1 排序 / 分类
    // -----------------------------------------------------------------------

    /// 造一行：`seeds` / `size` / `date` / `category` 都能给。
    fn row_at(idx: i32, name: &str, seeds: i32, size: Option<i64>, cat: Option<&str>) -> RowData {
        let mut r = RowData::from_result(&hit(name, "tpb", None), idx, idx as u64, "q", "#fff");
        r.seeds = seeds;
        r.size = size;
        r.category = cat.map(|c| c.to_string());
        r
    }

    /// **这条最重要**：排序会重编号 `idx`，而 `idx` 是 push-row 取磁力的下标 ——
    /// 不重编号就会"点第 3 行推第 3 个**到达**的结果"，错得很隐蔽（能推、推错）。
    #[test]
    fn sort_renumbers_idx_so_push_row_still_points_at_the_right_row() {
        let mut rows = vec![
            row_at(0, "a", 1, None, None),
            row_at(1, "b", 50, None, None),
            row_at(2, "c", 9, None, None),
        ];
        sort_rows(&mut rows, SortKey::Seeds, false, "q");

        assert_eq!(
            rows.iter()
                .map(|r| r.title_plain.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c", "a"],
            "按做种数降序"
        );
        assert_eq!(
            rows.iter().map(|r| r.idx).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "idx 必须按**新顺序**重编号"
        );
    }

    /// ⚠️ **这条钉的是上面那条注释没说到的那一半**：`idx` 重编号只保证了
    /// "**点击那一刻**按下标取到的是对的行"；而 `push_row` 中间隔着一次网络往返，
    /// 回来时 `idx` 可能已经指到别的行了 ⇒ 那条路必须用 `key`。
    /// 去掉 `set_action_by_key` / `set_magnet_by_key` 而改用 `set_action(idx, …)`，
    /// 这条测试会红（磁力落到 "b" 上）。
    #[test]
    fn deferred_writes_survive_a_resort_because_they_use_the_key() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report(
                "tpb",
                "TPB",
                vec![
                    TorrentResult {
                        name: "a".to_string(),
                        size: Some(100),
                        ..hit("a", "tpb", None)
                    },
                    TorrentResult {
                        name: "b".to_string(),
                        size: Some(900),
                        ..hit("b", "tpb", None)
                    },
                ],
                None,
            ),
            "q",
            "#fff",
        );
        // 用户点的是 "a"（下标 0），把自己的 key 交给异步阶段
        let key = st.rows[0].key;
        assert_eq!(st.rows[0].title_plain, "a");

        // 用户接着点了排序（换一个键 ⇒ 方向默认降序）⇒ "b" 跑到下标 0
        st.pick_sort(SortKey::Size, "q");
        assert_eq!(st.rows[0].title_plain, "b");

        // 异步阶段回来了，按 key 写回
        assert!(st.set_magnet_by_key(key, "magnet:?xt=urn:btih:AAA".to_string()));
        assert!(st.set_action_by_key(key, RowAction::Done));

        assert_eq!(st.rows[1].title_plain, "a");
        assert_eq!(
            st.rows[1].magnet.as_deref(),
            Some("magnet:?xt=urn:btih:AAA"),
            "磁力必须落在用户点的那一行上"
        );
        assert_eq!(st.rows[1].action, RowAction::Done);
        assert_eq!(st.rows[0].magnet, None, "不该落到排到前面的那一行");
        // ⚠️ 这一行本来就等着取磁力（`hit(.., None)` 没有磁力 ⇒ `NeedsMagnet`），
        // 所以“没被动过”的表现是它**停在原来的动作上**，不是 `Ready`。
        assert_eq!(
            st.rows[0].action,
            RowAction::NeedsMagnet,
            "排到前面那一行不该被写成 Done"
        );

        // 那行被新搜索清掉之后，写回只是静默失效（不 panic、不命中间号）
        st.begin_search(&two_providers());
        assert!(!st.set_action_by_key(key, RowAction::Done));
    }

    /// key 只增不减（否则上一轮迟到的写回会命中新一轮的同号行）。
    #[test]
    fn keys_are_never_reused_across_searches() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report("tpb", "TPB", vec![hit("a", "tpb", None)], None),
            "q",
            "#fff",
        );
        let first = st.rows[0].key;

        st.begin_search(&two_providers());
        st.apply_report(
            &report("tpb", "TPB", vec![hit("a", "tpb", None)], None),
            "q",
            "#fff",
        );
        assert_ne!(st.rows[0].key, first, "新一轮的同一行必须换一个 key");
    }

    /// 缺值**恒排最后**，而且方向只翻转有值那一档 ——
    /// 否则升序会把"没有大小信息"的行翻到最前面，看着像最大的资源。
    #[test]
    fn missing_size_is_always_last_in_both_directions() {
        for asc in [false, true] {
            let mut rows = vec![
                row_at(0, "has", 0, Some(100), None),
                row_at(1, "none", 0, None, None),
                row_at(2, "big", 0, Some(900), None),
            ];
            sort_rows(&mut rows, SortKey::Size, asc, "q");
            let names: Vec<&str> = rows.iter().map(|r| r.title_plain.as_str()).collect();
            assert_eq!(names[2], "none", "asc={asc} 时缺值仍该在最后");
            assert_eq!(names[0], if asc { "has" } else { "big" }, "asc={asc}");
        }
    }

    #[test]
    fn relevance_ranks_exact_over_prefix_over_contains() {
        assert_eq!(relevance("Ubuntu", "ubuntu"), 1000, "完全相等");
        assert_eq!(relevance("Ubuntu 24.04 LTS", "ubuntu"), 500, "以查询词开头");
        assert_eq!(relevance("xubuntu desktop", "ubuntu"), 300, "含有查询词");
        assert_eq!(relevance("debian netinst", "ubuntu"), 0, "不命中");
        // 大小写不敏感 + 查询词两侧空白要 trim
        assert_eq!(relevance("ubuntu", "  UBUNTU  "), 1000);
    }

    #[test]
    fn relevance_scores_multiple_tokens_and_ignores_empty_query() {
        // 开头命中(500) + 两个词元都命中(2×200)
        assert_eq!(relevance("ubuntu server iso", "ubuntu server"), 900);
        // 只是含有(300) + 两个词元(2×200)
        assert_eq!(relevance("xubuntu server", "ubuntu server"), 700);
        // 基础分 0（既不等、也不以它开头、也不含有）+ 一个词元(200)
        assert_eq!(relevance("ubuntu desktop", "ubuntu server"), 200);
        assert_eq!(relevance("anything", ""), 0, "空查询词 → 0");
        assert_eq!(relevance("anything", "   "), 0, "纯空白 → 0");
    }

    /// 分类筛选是**视图**，不能改 `rows` —— 否则取消筛选后行就回不来了。
    #[test]
    fn category_filter_is_a_view_not_a_mutation() {
        let mut st = AppState::new();
        st.rows = vec![
            row_at(0, "m1", 0, None, Some("电影")),
            row_at(1, "s1", 0, None, Some("剧集")),
            row_at(2, "m2", 0, None, Some("电影")),
            row_at(3, "x", 0, None, None),
        ];

        assert_eq!(st.view().len(), 4, "默认全部");
        st.set_category(Some("电影".to_string()));
        assert_eq!(st.view().len(), 2);
        assert_eq!(st.rows.len(), 4, "⚠️ rows 不能被筛掉");
        st.set_category(None);
        assert_eq!(st.view().len(), 4, "取消筛选后要能全回来");
    }

    #[test]
    fn categories_are_counted_and_sorted_by_count() {
        let rows = vec![
            row_at(0, "a", 0, None, Some("电影")),
            row_at(1, "b", 0, None, Some("剧集")),
            row_at(2, "c", 0, None, Some("电影")),
            row_at(3, "d", 0, None, Some("")),
            row_at(4, "e", 0, None, None),
        ];
        // 空分类与 None 都不进列表（有些 provider 根本不给 category）
        assert_eq!(
            categories(&rows),
            vec![("电影".to_string(), 2), ("剧集".to_string(), 1)]
        );
    }

    #[test]
    fn cat_chips_start_with_all_and_mark_the_selection() {
        let mut st = AppState::new();
        st.rows = vec![
            row_at(0, "a", 0, None, Some("电影")),
            row_at(1, "b", 0, None, Some("剧集")),
        ];

        /// ⚠️ **按 id 查，别按下标断言分类顺序**：`categories()` 是「条数降序、同票按名字」，
        /// 而中文的"名字序"是**码位序**（剧 U+5267 < 电 U+7535），不是拼音 ——
        /// 所以「电影」并不在「剧集」前面。按下标写会在换一批数据时莫名其妙地红。
        fn label_of(chips: &[Chip], id: &str) -> String {
            chips
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.label.clone())
                .unwrap_or_else(|| panic!("没有 id={id} 的 chip"))
        }

        let chips = st.cat_chips();
        assert_eq!(chips[0].id, "", "第一个恒为「全部」");
        assert_eq!(chips[0].label, "[全部] 2");
        assert_eq!(label_of(&chips, "电影"), "电影 1");
        assert_eq!(label_of(&chips, "剧集"), "剧集 1");

        st.set_category(Some("电影".to_string()));
        let chips = st.cat_chips();
        assert_eq!(chips[0].label, "全部 2", "不再是选中态");
        assert_eq!(label_of(&chips, "电影"), "[电影] 1", "选中的带方括号");
        assert_eq!(label_of(&chips, "剧集"), "剧集 1", "没选中的不带");
    }

    #[test]
    fn sort_chips_show_a_direction_arrow_only_on_the_active_key() {
        let mut st = AppState::new();
        assert_eq!(st.sort, SortKey::Seeds, "默认做种数");
        assert_eq!(
            st.sort_chips()
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            vec!["做种数 ↓", "相关度", "大小", "时间"]
        );

        // 同一个键再点一次 ⇒ 翻方向
        st.pick_sort(SortKey::Seeds, "q");
        assert!(st.sort_asc);
        assert_eq!(st.sort_chips()[0].label, "做种数 ↑");

        // 换键 ⇒ 回到默认降序
        st.pick_sort(SortKey::Size, "q");
        assert_eq!(st.sort, SortKey::Size);
        assert!(!st.sort_asc);
        assert_eq!(st.sort_chips()[2].label, "大小 ↓");
        assert_eq!(st.sort_chips()[0].label, "做种数", "旧的键不再带箭头");
    }

    #[test]
    fn sort_key_ids_round_trip() {
        for k in SortKey::ALL {
            assert_eq!(SortKey::parse(k.id()), Some(k));
        }
        assert_eq!(SortKey::parse("nope"), None);
    }

    // -----------------------------------------------------------------------
    // #2 「一条都没有」要能自证原因
    // -----------------------------------------------------------------------

    /// 全家失败、且都是**网络层**（超时/连不上）⇒ 让用户去查网络。
    #[test]
    fn all_network_failures_point_at_the_network() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report("tpb", "TPB", vec![], Some("tpb unreachable (ECONNABORTED)")),
            "q",
            "#fff",
        );
        st.apply_report(
            &report(
                "nyaa",
                "NYAA",
                vec![],
                Some("nyaa unreachable (ECONNREFUSED)"),
            ),
            "q",
            "#fff",
        );

        assert_eq!(
            (st.net_failures, st.site_failures),
            (2, 0),
            "两家都归网络层"
        );
        let s = st.summary(false);
        assert!(s.contains("全部失败"), "{s}");
        assert!(s.contains("网络层不通"), "{s}");
        assert!(s.contains("网络问题"), "{s}");
    }

    /// 全家失败、且都是**站点层**（403 / 认不出来）⇒ **不能说成网络问题** ——
    /// 那会让人白折腾网络，而真正该做的是换站。
    #[test]
    fn all_site_failures_do_not_blame_the_network() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report(
                "tpb",
                "TPB",
                vec![],
                Some("tpb unreachable (Request failed with status code 403)"),
            ),
            "q",
            "#fff",
        );
        // 认不出来的串（解析失败那种）也算站点层 —— 但**不硬说**是网络
        st.apply_report(&report("nyaa", "NYAA", vec![], Some("boom")), "q", "#fff");

        assert_eq!((st.net_failures, st.site_failures), (0, 2));
        let s = st.summary(false);
        assert!(s.contains("站点层拒绝"), "{s}");
        assert!(!s.contains("网络问题"), "站点层失败不该甩锅给网络：{s}");
    }

    #[test]
    fn mixed_failures_are_reported_as_mixed() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report("tpb", "TPB", vec![], Some("ECONNABORTED")),
            "q",
            "#fff",
        );
        st.apply_report(
            &report(
                "nyaa",
                "NYAA",
                vec![],
                Some("Request failed with status code 403"),
            ),
            "q",
            "#fff",
        );

        assert_eq!((st.net_failures, st.site_failures), (1, 1));
        let s = st.summary(false);
        assert!(s.contains("都有"), "两层混合要说清：{s}");
    }

    /// 全家都连上了但 0 条 ⇒ 说"更像站点改版"（**不是**网络问题 —— 那会让人白折腾网络）。
    #[test]
    fn all_providers_returning_zero_hits_says_the_sites_may_have_changed() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(&report("tpb", "TPB", vec![], None), "q", "#fff");
        st.apply_report(&report("nyaa", "NYAA", vec![], None), "q", "#fff");

        let s = st.summary(false);
        assert!(s.contains("返回 0 条"), "{s}");
        assert!(s.contains("改版"), "{s}");
    }

    /// ⚠️ `zero_hits` 不能把"失败"也算进去：失败的家 count 也是 0，
    /// 但它和"成功返回了 0 条"是两回事。这条用例就是钉这个。
    #[test]
    fn failed_providers_are_not_counted_as_zero_hits() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(&report("tpb", "TPB", vec![], Some("boom")), "q", "#fff");
        st.apply_report(&report("nyaa", "NYAA", vec![], None), "q", "#fff");

        assert_eq!(st.zero_hits, 1, "只有没报错的那家算'返回 0 条'");
        let s = st.summary(false);
        assert!(s.contains("1 家失败"), "{s}");
        assert!(s.contains("1 家返回空"), "{s}");
    }

    /// 有结果时**不要**啰嗦 —— 只在真有"返回 0 条"的家时才多说一句。
    #[test]
    fn summary_mentions_zero_hits_only_when_there_are_any() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.apply_report(
            &report("tpb", "TPB", vec![hit("a", "tpb", None)], None),
            "q",
            "#fff",
        );
        st.apply_report(
            &report("nyaa", "NYAA", vec![hit("b", "nyaa", None)], None),
            "q",
            "#fff",
        );
        assert_eq!(st.summary(false), "共 2 条 · 2 家成功 / 0 家失败");

        let mut st2 = AppState::new();
        st2.begin_search(&two_providers());
        st2.apply_report(
            &report("tpb", "TPB", vec![hit("a", "tpb", None)], None),
            "q",
            "#fff",
        );
        st2.apply_report(&report("nyaa", "NYAA", vec![], None), "q", "#fff");
        assert!(
            st2.summary(false).contains("1 家返回 0 条"),
            "{}",
            st2.summary(false)
        );
    }

    /// ⚠️ 还在等的时候**不能**说"一条都没有" —— 那是胡说八道（现有用例也钉着这条）。
    #[test]
    fn pending_providers_are_not_reported_as_no_results() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        assert_eq!(st.summary(false), "共 0 条 · 0 家成功 / 0 家失败");
    }

    // -----------------------------------------------------------------------
    // #6 健康度标记
    // -----------------------------------------------------------------------

    /// 连续空手够多次的家，**即使这一轮也算成功（0 条）**，状态条上要带 ⚠ ——
    /// 因为"每次都成功但都没有结果"正是站点改版的形状，光看这一轮分不出来。
    #[test]
    fn a_stale_engine_gets_a_warning_marker() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        // 预先标上「tpb 之前连续空手 4 次」。
        // ⚠️ `apply_report` 会**先把计数 +1 再用**（本轮也算进去），所以芯片上显示的是 5 ——
        // 这样它才与 `--health`（把本轮记录算进去）对得上。第一版测试写的是 4，我算错了。
        st.stale.insert("tpb".to_string(), 4);

        st.apply_report(&report("tpb", "TPB", vec![], None), "q", "#fff");
        st.apply_report(
            &report("nyaa", "NYAA", vec![hit("a", "nyaa", None)], None),
            "q",
            "#fff",
        );

        let tpb = st.chips.iter().find(|c| c.id == "tpb").unwrap();
        assert!(tpb.detail.contains('⚠'), "该带警告：{}", tpb.detail);
        assert!(
            tpb.detail.contains('5'),
            "次数要含本轮（4+1=5）：{}",
            tpb.detail
        );

        let nyaa = st.chips.iter().find(|c| c.id == "nyaa").unwrap();
        assert!(
            !nyaa.detail.contains('⚠'),
            "没标记的不该带：{}",
            nyaa.detail
        );
    }

    /// 没到阈值就不提示 —— 站点偶尔抽风不该被报成故障。
    ///
    /// ⚠️ 只插 `STALE_AFTER - 2`：本轮 +1 之后是 `STALE_AFTER - 1`，**仍在阈值下**。
    /// 插 `STALE_AFTER - 1` 会被本轮顶到正好等于阈值 —— 第一版就是栽在这上面。
    #[test]
    fn a_slightly_stale_engine_is_not_marked() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.stale
            .insert("tpb".to_string(), crate::health::STALE_AFTER - 2);

        st.apply_report(&report("tpb", "TPB", vec![], None), "q", "#fff");
        let tpb = st.chips.iter().find(|c| c.id == "tpb").unwrap();
        assert!(!tpb.detail.contains('⚠'), "没到阈值：{}", tpb.detail);
    }

    /// 报错的形态是 ✗ + 错误串，**不该**被「连空」文案盖掉 ——
    /// 那会把「连不上」说成「没结果」，指错方向。
    ///
    /// ⚠️ 错误串用**短**的：`chip_for` 会 `truncate(.., 18)`，
    /// 长串（如 `tpb unreachable (ECONNABORTED)`）根本留不下尾部的错误码。
    #[test]
    fn an_erroring_stale_engine_still_shows_its_error() {
        let mut st = AppState::new();
        st.begin_search(&two_providers());
        st.stale.insert("tpb".to_string(), 9);

        st.apply_report(
            &report("tpb", "TPB", vec![], Some("ECONNABORTED")),
            "q",
            "#fff",
        );
        let tpb = st.chips.iter().find(|c| c.id == "tpb").unwrap();
        assert_eq!(tpb.tone, Tone::Err);
        assert!(tpb.detail.contains("ECONNABORTED"), "{}", tpb.detail);
        assert!(
            !tpb.detail.contains("连空"),
            "别把错误改写成没结果：{}",
            tpb.detail
        );
    }
}
