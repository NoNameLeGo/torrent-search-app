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
}

impl RowData {
    /// 从一条 `TorrentResult` 建行。`idx` 由调用方给（= 它在权威 Vec 里的下标）。
    pub fn from_result(r: &TorrentResult, idx: i32, query: &str, accent: &str) -> Self {
        let has_magnet = r.magnet.as_deref().is_some_and(|m| !m.is_empty());
        Self {
            idx,
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
pub fn chip_for(id: &str, name: &str, status: &ProviderStatus) -> ChipData {
    match status.status {
        ProviderState::Ok => ChipData {
            id: id.to_string(),
            name: name.to_string(),
            glyph: "✓",
            detail: format!("{} 条", status.count),
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

#[derive(Debug, Default)]
pub struct AppState {
    /// 权威的行集合。`.slint` 的模型永远是它的镜像；行号 = 下标。
    pub rows: Vec<RowData>,
    pub chips: Vec<ChipData>,
    /// ① 当前页码（1 起）
    pub page: u32,
    /// ① 引擎层回的 `has_more` —— 「下一页」按钮的 enabled 用它
    pub has_more: bool,
    /// ③ 站点开关。顺序 = 注册表顺序 = UI 显示顺序
    pub sites: Vec<SiteData>,
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
    /// 「没筛选」⇒ 打出全部 16 家（那是 M2 按 JS 复制并测过的**引擎**口径，别去改它）。
    /// 但 UI 层不该让「我把站点全取消勾选」变成「偷偷搜全部」—— 那是反直觉的。
    /// 所以这一层拦下来，由调用方给一句提示。
    pub fn none_selected(&self) -> bool {
        !self.sites.is_empty() && !self.sites.iter().any(|s| s.on)
    }

    /// 一站回来了：**追加**它的结果（所以「谁先回来谁先显示」），并更新它的状态条。
    ///
    /// ⚠️ 别写成「全部回来后一次性 fill」—— 那正是 `search_stream` 要避免的体验问题。
    pub fn apply_report(&mut self, report: &ProviderReport, query: &str, accent: &str) {
        for r in &report.results {
            let idx = self.rows.len() as i32;
            self.rows.push(RowData::from_result(r, idx, query, accent));
        }

        let chip = chip_for(&report.id, &report.name, &report.status);
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
        format!(
            "共 {N} 条 · {ok} 家成功 / {err} 家失败",
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
        let row = RowData::from_result(&r, 0, "q", "#fff");
        assert_eq!(row.seeds, 0);
        assert_eq!(row.leechers, 0);

        r.seeders = Some(i64::MAX);
        let row = RowData::from_result(&r, 0, "q", "#fff");
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
    /// 会变成"偷偷搜全部 16 家"，反直觉。
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
}
