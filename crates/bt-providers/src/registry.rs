//! M2：把 JS 的 `REGISTRY` 搬成 Rust 的 [`Provider`] 实现。
//!
//! ## 为什么 impl 集中放在这里，而不是散到 16 个 provider 文件里
//!
//! 1. **风险隔离**：本机没有 `cargo check`（编译只在 CI，一轮 ~2 分钟）。把 16 个
//!    新 impl 放进**新文件**，能把编译风险圈在「不会弄坏已有 16 个 provider 的测试」
//!    之外。
//! 2. **形状对齐 JS**：`src/providers/index.js:79-121` 的 `REGISTRY` 本来就是
//!    「一处集中声明 + 逐条旗标」；这里的形状正好对上它。
//!
//! ## 顺序 = UI 显示顺序
//!
//! 下面 17 条的顺序取自 JS `REGISTRY`（`src/providers/index.js:79-121`）里
//! **已搬的那些**的相对顺序：16 个站点按 JS 顺序排列，**`demo` 在最后一条**
//! （JS 里它也是第 121 行的最后一项）。未搬的（`1337x` / `anilibria` / …）跳过，
//! 但**不要重排**。对照表在 `AGENTS.md`「M2 开工前勘明」⑦。

use bt_core::http::HttpClient;

use crate::{BoxFuture, MagnetOutcome, Provider, SearchOutcome};

/// 声明一个站点：unit struct + [`Provider`] impl。
///
/// 站点自身**零状态**（站点名就是它的全部）—— `http` 由聚合层在调用时传进来。
/// 需要带状态的 provider（D17 的 Torznab：URL + API key）不走这个宏，另外写 impl。
///
/// ## 为什么是两条宏臂而不是「可选的 `magnet` 标记」
///
/// 本来想写成 `$(, magnet)? $(, $flag:ident)*`，但那是**可选重复紧跟重复**，
/// macro_rules 对这种形状有歧义风险。两条臂（各写一遍 impl 体）虽然重复 14 行，
/// 但匹配是确定的 —— 在没有本地编译器的前提下，确定性比省行数重要。
///
/// - 第 1 条臂：**有**惰性磁力解析（6/17 家有：`audiobookbay` / `megapeer` /
///   `oxtorrent` / `therarbg` / `torrent9` / `zeromagnet`），末位写 `magnet`。
/// - 第 2 条臂：**没有**，末位直接跟旗标（可为空）。
///
/// 旗标名必须是 trait 上的方法名：`paginated` / `browseable` / `demo`。
macro_rules! site {
    // ---- 有惰性磁力解析 ----
    ($ty:ident, $module:ident, $id:literal, $name:literal, magnet $(, $flag:ident)*) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $ty;

        impl Provider for $ty {
            fn id(&self) -> &'static str {
                $id
            }

            fn name(&self) -> &'static str {
                $name
            }

            $(
                fn $flag(&self) -> bool {
                    true
                }
            )*

            fn search<'a>(
                &'a self,
                http: &'a HttpClient,
                query: &'a str,
                page: u32,
            ) -> BoxFuture<'a, SearchOutcome> {
                Box::pin(crate::$module::search(http, query, page))
            }

            fn resolve_magnet<'a>(
                &'a self,
                http: &'a HttpClient,
                url: &'a str,
            ) -> BoxFuture<'a, MagnetOutcome> {
                Box::pin(crate::$module::resolve_magnet(http, url))
            }
        }
    };

    // ---- 没有惰性磁力解析（用 trait 的默认实现 → NO_RESOLVER）----
    ($ty:ident, $module:ident, $id:literal, $name:literal $(, $flag:ident)*) => {
        #[derive(Debug, Clone, Copy)]
        pub struct $ty;

        impl Provider for $ty {
            fn id(&self) -> &'static str {
                $id
            }

            fn name(&self) -> &'static str {
                $name
            }

            $(
                fn $flag(&self) -> bool {
                    true
                }
            )*

            fn search<'a>(
                &'a self,
                http: &'a HttpClient,
                query: &'a str,
                page: u32,
            ) -> BoxFuture<'a, SearchOutcome> {
                Box::pin(crate::$module::search(http, query, page))
            }
        }
    };
}

// ---------------------------------------------------------------------------
// 28 个站点（前 25 个 = JS REGISTRY 顺序 = UI 显示顺序；bdflixs / pomo / cilixiong 是 Rust 独有，追加在末尾）
// ---------------------------------------------------------------------------

site!(Tpb, tpb, "tpb", "The Pirate Bay");
site!(Nyaa, nyaa, "nyaa", "NYAA", paginated);
site!(Yts, yts, "yts", "YTS", browseable);
site!(Knaben, knaben, "knaben", "Knaben");
site!(TorrentsCsv, torrentscsv, "torrentscsv", "TorrentsCSV");
site!(AniRena, anirena, "anirena", "AniRena", magnet);
site!(AnimeTosho, animetosho, "animetosho", "AnimeTosho");
site!(BangumiMoe, bangumimoe, "bangumimoe", "Bangumi.Moe");
site!(Dmhy, dmhy, "dmhy", "DMHY", paginated);
site!(Mikan, mikan, "mikan", "Mikan");
site!(SubsPlease, subsplease, "subsplease", "SubsPlease");
// JS 里 sukebei 排在 subsplease 之后、tokyotoshokan 之前（`src/providers/index.js:93`）——
// tokyotoshokan 没搬，所以紧跟在 subsplease 后面就是正确的相对位置。
// ⚠️ JS 里这家**没有 paginated**（nyaa 有），照抄。
site!(Sukebei, sukebei, "sukebei", "Sukebei");
site!(NekoBt, nekobt, "nekobt", "NekoBT");
site!(
    LimeTorrents,
    limetorrents,
    "limetorrents",
    "LimeTorrents",
    paginated
);
site!(TheRarBg, therarbg, "therarbg", "TheRarBg", magnet);
site!(Rutor, rutor, "rutor", "Rutor", paginated);
site!(Torrent9, torrent9, "torrent9", "Torrent9", magnet);
site!(ZeroMagnet, zeromagnet, "zeromagnet", "ZeroMagnet", magnet);
site!(OxTorrent, oxtorrent, "oxtorrent", "OxTorrent", magnet);
site!(
    AudioBookBay,
    audiobookbay,
    "audiobookbay",
    "AudioBookBay",
    magnet
);
site!(FileMood, filemood, "filemood", "FileMood");
site!(
    InternetArchive,
    internetarchive,
    "internetarchive",
    "InternetArchive"
);
site!(LinuxTracker, linuxtracker, "linuxtracker", "LinuxTracker");
site!(MegaPeer, megapeer, "megapeer", "MegaPeer", magnet);
site!(Xxxtracker, xxxtracker, "xxxtracker", "XXXTracker");
site!(Bdflixs, bdflixs, "bdflixs", "BD电影首发", magnet);
site!(Pomo, pomo, "pomo", "POMO", magnet);
site!(Cilixiong, cilixiong, "cilixiong", "磁力熊", magnet);

/// 静态注册表。**顺序 = UI 显示顺序**（见文件头）。
///
/// ⚠️ 将来 Torznab 的动态 provider（D17）**不在这里** —— 调用方把
/// 「这个静态表 + 动态实例」拼起来再交给聚合层；聚合层只认 `&[Box<dyn Provider>]`，
/// 不关心某个 provider 是编译期固定的还是运行期来的。
///
/// ⚠️ `demo` **不用 [`site!`] 宏**：宏生成的是「调 `crate::<mod>::search_at(...)`」那种
/// 统一形状，而 demo 不碰网络（它自己造数据）。宏的注释里也写了这一条
/// （「需要带状态的 provider 不走这个宏，另外写 impl」）—— demo 是同类里的另一种。
pub fn registry() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(Tpb),
        Box::new(Nyaa),
        Box::new(Yts),
        Box::new(Knaben),
        Box::new(TorrentsCsv),
        Box::new(AniRena),
        Box::new(AnimeTosho),
        Box::new(BangumiMoe),
        Box::new(Dmhy),
        Box::new(Mikan),
        Box::new(SubsPlease),
        Box::new(NekoBt),
        Box::new(LimeTorrents),
        Box::new(TheRarBg),
        Box::new(Rutor),
        Box::new(Torrent9),
        Box::new(ZeroMagnet),
        Box::new(OxTorrent),
        Box::new(AudioBookBay),
        Box::new(FileMood),
        Box::new(InternetArchive),
        Box::new(LinuxTracker),
        Box::new(MegaPeer),
        Box::new(Xxxtracker),
        Box::new(Bdflixs),
        Box::new(Pomo),
        Box::new(Cilixiong),
        // 最后一条，与 JS REGISTRY 一致（那边的 demo 也是最后一项）
        Box::new(crate::demo::Demo),
    ]
}

/// [`list`] 的条目 —— 对齐 JS `list()` 的 `{ id, name, enabled, demo, paginated }`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub enabled: bool,
    pub demo: bool,
    pub paginated: bool,
}

/// 对齐 JS `list()`（`src/providers/index.js:123-131`）里**静态注册表那部分**。
///
/// ⚠️ JS 的 `list()` 还会 `concat` 动态 Torznab 的 `{ id, name, enabled: true }`
/// —— 那部分等 D17 真做时由调用方合并（与 [`crate::aggregate::resolve_targets`] 同口径）。
pub fn list() -> Vec<ProviderInfo> {
    registry()
        .iter()
        .map(|p| ProviderInfo {
            id: p.id(),
            name: p.name(),
            enabled: p.enabled(),
            demo: p.demo(),
            paginated: p.paginated(),
        })
        .collect()
}
