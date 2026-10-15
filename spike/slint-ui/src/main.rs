//! Slint spike — BT 聚合搜索 UI 可行性验证
//!
//! 要回答的四个问题：
//!   1. 中文关键词高亮：`styled-text` 属性 + Rust `StyledText::from_markdown()` 能不能跑通？
//!      （注意：`@markdown()` 会把插值内容整体转义，不能用来注入 markup）
//!   2. `ListView` 在 2000 条中文卡片下是否真虚拟化？按「重置计数 → 滚动 → 刷新计数」量。
//!   3. CJK 字体能不能靠 `Window.default-font-family` 解决？
//!   4. ⚠️ Slint #13548：内联样式跨换行 + 换行后是多字节字符时 parley 会 panic。
//!      中文标题 + `max-lines: 2` 正是最坏情况，专门来压。
//!
//! 运行：
//!   `cargo run --release`              —— 交互式 GUI（人看的）
//!   `cargo run --release -- --selftest` —— **M1 自动自检**（CI 跑的），见 `src/m1.rs`
//!
//! ⚠️ 旧版 spike 只被 CI「编译」过，从来没有**运行**过，所以上面第 4 条（#13548）
//! 一直是「未实测」。`--selftest` 就是补上这一刀：真的把 200 条中文标题
//! 逐条滚过去、真的走 parley 的文本布局，panic 就红。

mod m1;

slint::include_modules!();

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use slint::{ModelRc, StyledText, VecModel};

const BASES: &[&str] = &[
    "沙丘2",
    "流浪地球2",
    "三体",
    "奥本海默",
    "繁花",
    "Spirited Away",
    "进击的巨人",
    "Cyberpunk Edgerunners",
    "Interstellar",
    "ubuntu-24.04.1-desktop-amd64",
];

const SOURCES: &[&str] = &[
    "1337x",
    "The Pirate Bay",
    "Knaben",
    "NYAA",
    "Rutor",
    "YTS",
    "EZTV",
    "dmhy",
    "Mikan",
    "BT4G",
];

const CATS: &[&str] = &[
    "movies", "series", "anime", "games", "apps", "books", "music", "other",
];

const SIZES: &[&str] = &["1.42 GB", "8.71 GB", "24.3 GB", "712.4 MB", "58.1 GB"];

const QUALS: &[&str] = &["2160p", "1080p", "1080p", "720p"];

/// 逐字符转义：先处理 HTML 实体，再把 CommonMark 的行内特殊字符反斜杠转义。
/// 顺序很重要 —— 如果先转义再替换 `&`，会把自己注入的 `&amp;` 二次转义。
fn esc_seg(seg: &[char], out: &mut String) {
    for &ch in seg {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '!' | '~' | '|'
            | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
}

/// 把 `query` 在 `title` 中的所有命中包成 `<font color="...">...</font>`。
/// 先转义整条标题，再注入裸标签 —— 只有我们自己的标签是「活的」。
/// 按 char 而不是 byte 处理，避免中文被切断。
pub(crate) fn highlight(title: &str, query: &str, accent: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let q: Vec<char> = query.trim().chars().collect();

    let mut out = String::with_capacity(title.len() + 64);

    if q.is_empty() {
        esc_seg(&chars, &mut out);
        return out;
    }

    let n = chars.len();
    let m = q.len();
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);

    let mut pos = 0usize;
    let mut i = 0usize;
    while i + m <= n {
        let hit = (0..m).all(|k| lower(chars[i + k]) == lower(q[k]));
        if hit {
            esc_seg(&chars[pos..i], &mut out);
            out.push_str("<font color=\"");
            out.push_str(accent);
            out.push_str("\">");
            esc_seg(&chars[i..i + m], &mut out);
            out.push_str("</font>");
            i += m;
            pos = i;
        } else {
            i += 1;
        }
    }
    esc_seg(&chars[pos..], &mut out);
    out
}

/// 把 markup 解析成 styled-text。**解析失败就退回纯文本** ——
/// `from_markdown` 遇到 Slint 不支持的语法会返回 Err，绝不能让整条标题消失。
pub(crate) fn md(markup: &str) -> StyledText {
    StyledText::from_markdown(markup).unwrap_or_else(|_| StyledText::from_plain_text(markup))
}

/// 造一条拟真结果。`i % 5 == 4` 那批刻意塞满 Markdown 特殊字符，
/// 用来暴露转义坑；`i % 3 == 2` 那批标题长，用来压 #13548 的换行 panic。
fn make_card(i: usize, query: &str, accent: &str) -> TorrentCard {
    let base = BASES[i % BASES.len()];
    let qual = QUALS[i % QUALS.len()];

    let title = match i % 5 {
        0 => format!("{} (2024) {} WEB-DL DDP5.1 H.264-iNT", base, qual),
        1 => format!(
            "{} S0{}E{:02} {} WEB-DL AAC H.264",
            base,
            1 + i % 3,
            1 + i % 12,
            qual
        ),
        // 长标题 + 中文 + 高亮词靠后：专门用来压 parley 的换行 char-boundary panic
        2 => format!(
            "{} 全集 中字 内嵌简繁英多语字幕 {} HDR10 10bit x265-GROUP 完整版",
            base, qual
        ),
        3 => format!("{} {} BluRay REMUX TrueHD 7.1", base, qual),
        _ => format!(
            "{} *特别版* [{}] #{} _test_ 100% 完整 {}",
            base, qual, i, qual
        ),
    };

    let meta = format!(
        "{}  ·  2026-0{}-{:02}",
        SIZES[i % SIZES.len()],
        1 + i % 9,
        1 + i % 28
    );

    TorrentCard {
        idx: i as i32,
        title_markup: md(&highlight(&title, query, accent)),
        title_plain: title.into(),
        meta: meta.into(),
        source: SOURCES[i % SOURCES.len()].into(),
        cat: CATS[i % CATS.len()].into(),
        seeds: (i % 4000) as i32 + 3,
    }
}

fn make_cards(n: usize, query: &str, accent: &str) -> Vec<TorrentCard> {
    (0..n).map(|i| make_card(i, query, accent)).collect()
}

/// 高亮色从 `Theme.hl` 读，再转成 `#rrggbb` 拼进 markup。
/// 单一数据源 —— 避免 Rust 与 .slint 各写一份色值。
/// （顺带暴露一个真实代价：高亮色是**烘进 markup** 的，主题切换必须整体重建模型。）
pub(crate) fn hl_hex(ui: &MainWindow) -> String {
    let c = ui.global::<Theme>().get_hl();
    format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
}

fn set_cards(ui: &MainWindow, n: usize, query: &str) {
    let hex = hl_hex(ui);
    let cards = make_cards(n, query, &hex);
    ui.set_total(n as i32);
    ui.set_cards(ModelRc::from(Rc::new(VecModel::from(cards))));
}

fn main() -> Result<(), slint::PlatformError> {
    // M1 自动自检：CI 用这条路径。必须是**先于**任何 Slint 组件创建判断，
    // 因为自检要自己控制平台的初始化时机与退出。
    if std::env::args().any(|a| a == "--selftest") {
        return m1::run();
    }

    let ui = MainWindow::new()?;

    let count = Rc::new(Cell::new(200usize));
    // 实例化计数留在 Rust 侧：.slint 里写全局属性会形成依赖环
    // （上一版「共 200 条 / 已实例化 2628 个」的元凶）。
    let inst = Rc::new(Cell::new(0usize));

    let t0 = Instant::now();
    let query0 = ui.get_query().to_string();
    set_cards(&ui, count.get(), &query0);
    println!(
        "[spike] 首次构建 {} 条卡片 + 首帧 {:?}",
        count.get(),
        t0.elapsed()
    );

    // 每个 CardWidget 实例化时回调一次（带回卡片 idx，自检用它统计覆盖率）
    {
        let inst = inst.clone();
        ui.on_card_instantiated(move |_idx: i32| inst.set(inst.get() + 1));
    }

    // 重置 / 刷新计数：只在点击时写 count-shown，所以不会形成属性环
    {
        let weak = ui.as_weak();
        let inst = inst.clone();
        ui.on_reset_count(move || {
            inst.set(0);
            if let Some(ui) = weak.upgrade() {
                ui.set_count_shown(0);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let inst = inst.clone();
        ui.on_refresh_count(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_count_shown(inst.get() as i32);
            }
        });
    }

    // 关键词变化 -> 重算高亮
    {
        let weak = ui.as_weak();
        let count = count.clone();
        ui.on_requery(move |q: slint::SharedString| {
            let Some(ui) = weak.upgrade() else { return };
            set_cards(&ui, count.get(), &q);
        });
    }

    // 切换条数
    {
        let weak = ui.as_weak();
        let count = count.clone();
        let inst = inst.clone();
        ui.on_resize_list(move |n: i32| {
            let n = n.max(0) as usize;
            count.set(n);
            let Some(ui) = weak.upgrade() else { return };
            let t = Instant::now();
            let q = ui.get_query().to_string();
            inst.set(0);
            ui.set_count_shown(0);
            set_cards(&ui, n, &q);
            println!("[spike] 切到 {} 条，重建耗时 {:?}", n, t.elapsed());
        });
    }

    // 明暗切换。注意：高亮色是烘进 markup 的，所以必须整体重建模型 ——
    // 这是 StyledText 方案的真实代价，记下来。
    {
        let weak = ui.as_weak();
        let count = count.clone();
        ui.on_toggle_theme(move || {
            let Some(ui) = weak.upgrade() else { return };
            let g = ui.global::<Theme>();
            let dark = !g.get_dark();
            g.set_dark(dark);
            let q = ui.get_query().to_string();
            set_cards(&ui, count.get(), &q);
        });
    }

    ui.run()
}
