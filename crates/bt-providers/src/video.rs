//! 在线影视搜索（Video 模式）—— **通用 MacCMS 适配器**。
//!
//! 这个模块与 [`crate::Provider`]（BT/磁力）**刻意分开**：
//!
//! - BT provider 返回 [`bt_core::TorrentResult`]（磁力、做种数、下载器推送）；
//! - Video provider 只返回「标题 + 详情页 URL」，点击后由 UI 打开系统浏览器。
//!
//! 两者混在一个 trait 里会让「行操作」变成条件分支（推送下载器 / 打开网页），
//! 所以这里单独定义 [`VideoProvider`] / [`VideoItem`] / [`VideoSearchOutcome`]。
//!
//! ## 为什么是「通用 MacCMS 适配器」而不是一站一文件
//!
//! 实测的在线影视站大量使用 MacCMS / 苹果CMS 的搜索页，结果列表只有两类模板：
//!
//! ```text
//! ① stui：ul.stui-vodlist > li.stui-vodlist__item > h4.stui-vodlist__title > a
//! ② myui：ul#searchList > li > div.detail > h4.title > a
//! ```
//!
//! 所以先用一组选择器链解析，再按站点配置拼搜索 URL。新增同模板站点只需要
//! 在 [`registry`] 里加一条 [`MacCmsSite`]，不用写新解析器。
//!
//! ## 契约
//!
//! 与 BT provider 一致：**永不 panic、永不返回 Err**；失败落在
//! [`VideoSearchOutcome::error`]，没有结果则是 `ok(vec![])`（不是错误）。
//!
//! ⚠️ 本模块**不解析播放地址**（m3u8 / iframe / VIP 线路）—— 那是播放器的活。
//! 这里只负责「找到条目」，点开之后能不能看由目标站自己决定。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::encode_uri_component;

use crate::BoxFuture;

/// 一条在线影视搜索结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoItem {
    /// 片名（站点搜索结果里的标题）。
    pub title: String,
    /// 详情页绝对 URL。UI 的「打开网页」直接用它。
    pub detail_url: String,
    /// provider id（`appmovie` / `dbku` / `didahd`），用于来源标签与状态栏。
    pub provider: String,
    /// 年份（可选；当前 MacCMS 适配器不从列表页猜，留 `None`）。
    pub year: Option<String>,
    /// 分类（可选；同上）。
    pub category: Option<String>,
}

/// Video 模式的搜索结果。字段名对齐 [`crate::SearchOutcome`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSearchOutcome {
    pub results: Vec<VideoItem>,
    pub error: Option<String>,
}

impl VideoSearchOutcome {
    /// 正常返回（**无结果也算正常**）。
    pub fn ok(results: Vec<VideoItem>) -> Self {
        Self {
            results,
            error: None,
        }
    }

    /// 出错了。`results` 一律为空。
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            results: Vec::new(),
            error: Some(msg.into()),
        }
    }
}

/// 一个 MacCMS 站点的配置。
///
/// `search_path` 里的 `{q}` 会被替换成 URL 编码后的关键词；
/// `page > 1` 时自动追加 `&page=N`（MacCMS 的通用翻页参数）。
#[derive(Debug, Clone, Copy)]
pub struct MacCmsSite {
    pub id: &'static str,
    pub name: &'static str,
    pub base: &'static str,
    pub search_path: &'static str,
}

/// APP影院（stui 模板）。实测 2026-10-25：`/index.php/vod/search.html?wd=`。
pub static APPMOVIE: MacCmsSite = MacCmsSite {
    id: "appmovie",
    name: "APP影院",
    base: "https://www.appmovie.art",
    search_path: "/index.php/vod/search.html?wd={q}",
};

/// 独播库（myui 模板）。实测 2026-10-25：`/vodsearch/-------------.html?wd=`。
pub static DBKU: MacCmsSite = MacCmsSite {
    id: "dbku",
    name: "独播库",
    base: "https://www.dbku.tv",
    search_path: "/vodsearch/-------------.html?wd={q}",
};

/// 嘀嗒影视（myui 模板）。实测 2026-10-25：`/search/-------------.html?wd=`。
pub static DIDAHd: MacCmsSite = MacCmsSite {
    id: "didahd",
    name: "嘀嗒影视",
    base: "https://www.didahd.xyz",
    search_path: "/search/-------------.html?wd={q}",
};

/// 追影（`zhuiying3.cc`）。实测 2026-10-25：`/index.php/vod/search.html?wd=`。
pub static ZHUIYING: MacCmsSite = MacCmsSite {
    id: "zhuiying",
    name: "追影",
    base: "https://zhuiying3.cc",
    search_path: "/index.php/vod/search.html?wd={q}",
};

/// 宅男影视（`zndy.top`）。实测 2026-10-25：`/index.php/vod/search.html?wd=`。
pub static ZNDY: MacCmsSite = MacCmsSite {
    id: "zndy",
    name: "宅男影视",
    base: "https://zndy.top",
    search_path: "/index.php/vod/search.html?wd={q}",
};

/// 结果列表的选择器链 —— **按顺序尝试，取第一个非空的**。
///
/// 只列实测过的两种模板；新增模板时在这里加一条，并补一份 fixture 测试。
const SELECTOR_CHAINS: &[&str] = &[
    // stui（APP影院）
    "ul.stui-vodlist > li.stui-vodlist__item > h4.stui-vodlist__title > a",
    // myui（独播库 / 嘀嗒影视）
    "ul#searchList > li > div.detail > h4.title > a",
    // myui 的另一种写法（没有 id 的列表）
    "ul.myui-vodlist__media > li > div.detail > h4.title > a",
    // 追影：结果列表里的 /video/ 链接（缩略图锚点只有 title 属性，靠 anchor_title 兜底）
    "div.search-list a[href^=\"/video/\"]",
    // 宅男影视：结果区里的 a.card（标题在子 h2.card-title 里）
    "section#search-result-section a.card[href^=\"play.php\"]",
];

/// 一个在线影视搜索源。
pub trait VideoProvider: Send + Sync {
    /// 稳定 id（与站点配置里的 `id` 一致）。
    fn id(&self) -> &'static str;

    /// 展示名（UI 的来源标签 / 状态栏）。
    fn name(&self) -> &'static str;

    /// 查一页。**永不 panic、永不返回 Err**。
    fn search<'a>(
        &'a self,
        http: &'a HttpClient,
        query: &'a str,
        page: u32,
    ) -> BoxFuture<'a, VideoSearchOutcome>;
}

/// 用默认站点配置搜索（生产路径）。
pub async fn search_site(
    http: &HttpClient,
    site: &MacCmsSite,
    query: &str,
    page: u32,
) -> VideoSearchOutcome {
    search_at(http, site.base, site, query, page).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(
    http: &HttpClient,
    base: &str,
    site: &MacCmsSite,
    query: &str,
    page: u32,
) -> VideoSearchOutcome {
    let url = search_url(base, site, query, page);

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return VideoSearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return VideoSearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let anchors = collect_anchors(&dom);
    if anchors.is_empty() {
        // 没有结果列表不是错误 —— 与 BT provider 的约定一致。
        return VideoSearchOutcome::ok(Vec::new());
    }

    let base = base.trim_end_matches('/');
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();

    for a in anchors {
        let Some(href) = attr(a, "href").filter(|h| !h.is_empty()) else {
            continue;
        };
        // 跳过 JS 伪链接 / 锚点
        if href.starts_with("javascript:") || href.starts_with('#') {
            continue;
        }

        let title = anchor_title(&dom, a);
        if title.is_empty() {
            continue;
        }

        let detail_url = absolute_url(base, &href);
        // 同一个条目在列表里可能出现两次（海报 + 标题），按 URL 去重。
        if !seen.insert(detail_url.clone()) {
            continue;
        }

        results.push(VideoItem {
            title,
            detail_url,
            provider: site.id.to_string(),
            year: None,
            category: None,
        });
    }

    VideoSearchOutcome::ok(results)
}

/// 取一条结果的标题：优先卡片里的 `h2/h3/h4`，其次锚文本，最后 `title` 属性。
///
/// 三种真实形态：
/// - 宅男：`<a class="card"><h2 class="card-title">流浪地球</h2></a>`（锚文本含「官采 超清」徽章）
/// - 追影缩略图：`<a title="流浪地球"><img></a>`（锚文本为空）
/// - APP/独播库：`<h4><a>流浪地球</a></h4>`（锚文本就是标题）
fn anchor_title(dom: &Dom, a: ElementRef<'_>) -> String {
    // 1) 卡片里的标题元素（宅男的 h2.card-title）
    for sel in ["h2", "h3", "h4"] {
        if let Some(h) = dom.find(a, sel).into_iter().next() {
            let t = text_trim(h);
            if !t.is_empty() {
                return t;
            }
        }
    }
    // 2) 海报锚点（追影缩略图）：锚文本是「电影 正片」这类徽章，`title` 才是片名
    if !dom.find(a, "img").is_empty() {
        if let Some(t) = attr(a, "title").filter(|t| !t.trim().is_empty()) {
            return t.trim().to_string();
        }
    }
    // 3) 普通标题锚点（APP / 独播库 / 追影标题）
    let t = text_trim(a);
    if !t.is_empty() {
        return t;
    }
    // 4) 最后兜底
    attr(a, "title").unwrap_or_default().trim().to_string()
}

/// 按选择器链收集候选锚点；第一条非空即停。
fn collect_anchors<'a>(dom: &'a Dom) -> Vec<ElementRef<'a>> {
    for chain in SELECTOR_CHAINS {
        let found = dom.select(chain);
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// `{base}{search_path}`，`{q}` 做 URL 编码；`page > 1` 时追加 `&page=N`。
fn search_url(base: &str, site: &MacCmsSite, query: &str, page: u32) -> String {
    let mut url = format!(
        "{}{}",
        base.trim_end_matches('/'),
        site.search_path
            .replace("{q}", &encode_uri_component(query))
    );
    if page > 1 {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str("page=");
        url.push_str(&page.to_string());
    }
    url
}

/// 相对链接转绝对。`base` 已经去掉尾部 `/`。
fn absolute_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with('/') {
        format!("{base}{href}")
    } else {
        format!("{base}/{href}")
    }
}

/// 声明一个站点 provider：unit struct + [`VideoProvider`] impl。
macro_rules! video_site {
    ($ty:ident, $site:ident) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $ty;

        impl VideoProvider for $ty {
            fn id(&self) -> &'static str {
                $site.id
            }

            fn name(&self) -> &'static str {
                $site.name
            }

            fn search<'a>(
                &'a self,
                http: &'a HttpClient,
                query: &'a str,
                page: u32,
            ) -> BoxFuture<'a, VideoSearchOutcome> {
                Box::pin(search_site(http, &$site, query, page))
            }
        }
    };
}

video_site!(AppMovie, APPMOVIE);
video_site!(Dbku, DBKU);
video_site!(DidaHd, DIDAHd);
video_site!(Zhuiying, ZHUIYING);
video_site!(Zndy, ZNDY);

/// 静态注册表。**顺序 = UI 显示顺序**（与 BT 的 [`crate::registry`] 同口径）。
pub fn registry() -> Vec<Box<dyn VideoProvider>> {
    vec![
        Box::new(AppMovie),
        Box::new(Dbku),
        Box::new(DidaHd),
        Box::new(Zhuiying),
        Box::new(Zndy),
    ]
}
