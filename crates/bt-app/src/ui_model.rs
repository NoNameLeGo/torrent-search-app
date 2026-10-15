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
#[derive(Debug, Default)]
pub struct AppState {
    /// 权威的行集合。`.slint` 的模型永远是它的镜像；行号 = 下标。
    pub rows: Vec<RowData>,
    pub chips: Vec<ChipData>,
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
        self.chips = providers
            .iter()
            .map(|(id, name)| pending_chip(id, name))
            .collect();
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
}
