//! M1 —— Slint #13548 风险验证（**可自动判定**版）
//!
//! 为什么要单独一个 `--selftest` 模式：旧版 spike 只被 CI **编译**过，
//! 从来没被**运行**过，所以「中文标题 + 关键词高亮 + 自动换行会不会 panic」
//! 一直是「未实测」。编译通过 ≠ 文本布局跑过。
//!
//! 这个模块把 `docs/AGENT_HANDBOOK.md` §1.2 的六条及格线变成 CI 里真跑的东西：
//!
//! | # | 及格线 | 这里怎么测 |
//! |---|---|---|
//! | 1 | 200 条中文标题 + 高亮 + 自动换行全部渲染、不 panic | `ListView` 里逐条滚过去，**去重统计实际被实例化（= 走过布局）的卡片序号** |
//! | 2 | 关键词落在第 2 行也 OK | 前缀长度扫描 50..=109 个汉字，关键词水平落点逐字扫过整行并越过软换行边界（**构造**保证覆盖） |
//! | 3 | 关键词正好落在换行边界也 OK | 同上，扫描步长是 **1 个汉字** |
//! | 4 | 西里尔也过一遍 | 50 条真实俄语标题（提取自 `test/fixtures/megapeer-1080p.html`，windows-1251） |
//! | 5 | 报出实测体积 | 由 `slint-spike.yml` 的「报告体积」步骤量自包含 exe |
//! | 6 | `ListView` 虚拟化生效 | 2000 条只读不滚 → 实测实例化数（真虚拟化应 ≪ 2000） |
//!
//! ⚠️ **及格线 2/3 的口径要说清**：Rust 侧拿不到字体度量，无法反查「关键词落在第几行」，
//! 所以这两条是靠**构造**保证覆盖（扫描必然经过边界），真正**被实测**的是
//! 「这些情形会不会让 parley panic」。**不许**把它写成「实测关键词在第 2 行」。
//!
//! 判定方式：**进程跑到底 = 通过**。一旦 parley panic，进程直接死，
//! CI 变红，日志里留下 `[m1][PANIC]` + 栈 + 出事时的那行步进日志。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::WindowEvent;
use slint::{ComponentHandle, LogicalPosition, ModelRc, Timer, TimerMode, VecModel};

use crate::{highlight, hl_hex, md, MainWindow, TorrentCard};

/// 合成中文卡片数（照当年 spike 的规模）
const CJK_CARDS: usize = 200;
/// 虚拟化基线：条数故意开到 2000
const VIRT_CARDS: usize = 2000;
/// 每步滚动的逻辑像素。取半张卡高（卡片 80px），保证每条都会进视野、不会被跳过去
const SCROLL_PX: f32 = 40.0;
/// 安全超时：无论如何都别让 CI 挂死
const SAFETY_SECS: u64 = 150;

/// 关键词。必须出现在合成标题里（高亮需要命中）
const KEYWORD: &str = "ubuntu";

/// 真实俄语标题，提取自 `test/fixtures/megapeer-1080p.html`（**windows-1251**）。
///
/// 提取口径与 `crates/bt-providers/src/megapeer.rs` 的 `NAME_LINK`
/// （`td:nth-child(2) > a:nth-child(2)`，即 `class="url"` 的 `<a>`）一致：
/// 50 条，长度 39..=148 字符 —— 正好是会触发换行的真实素材。
const CYRILLIC: &[&str] = &[
    "30 ночей с бывшим / 30 notti con il mio ex (2025) BDRip 1080p | Р | Кирилица",
    "История его служанки [2 сезон: 1-3 серии из 30] (2026) WEB-DL 1080p",
    "Тёмная материя / Dark Matter [2 сезон: 1-7 серия из 10] (2026) WEB-DL 1080p | LostFilm, HDRezka Studio, NewComers, ViruseProject, ColdFilm",
    "Matchbox. Фильм / Matchbox the Movie (2026) WEB-DL 1080p | D, Р",
    "МастерШеф: Профессионалы / МастерШеф: Професіонали [5 сезон: 1-8 выпуск из 18] (2026) IPTV 1080p | UKR",
    "Голубой свет / Curse of the Blue Lights (1988) Blu-Ray Remux 1080p | P | ОРТ",
    "Медленные лошади / Хромые лошади / Slow Horses [6 сезон: 1-4 серия из 6] (2026) WEB-DL 1080p | LostFilm, HDRezka Studio, Red Head Sound",
    "Гангстерленд / MobLand [2 сезон: 1-4 серия из 10] (2026) WEB-DL 1080p | Red Head Sound, LostFilm, HDRezka Studio, NewComers, ViruseProject, ColdFilm",
    "Второй номер [1-4 серии из 4] (2026) WEB-DL 1080p от ExKinoRay",
    "Захват / Поймать Эль Чапо / La Captura (2026) WEB-DL-HEVC 1080p | P, P2",
    "Выживалити. Наследники. Жизнь после острова [2 сезон: 12 выпуск из 13] [Эфир от 09.10] (2026) HDTV 1080p от Files-x",
    "Выживалити. Наследники [2 сезон: 11 выпуск из 13] [Эфир от 09.10] (2026) HDTV 1080p от Files-x",
    "Кэрри / Carrie [1 сезон] (2026) WEB-DL 1080p | D, P, L | HDrezka Studio, LE-production",
    "Условный мент: Охта [7 сезон: 1-5 серия из 120] (2023-2027) WEB-DL 1080p от Files-х",
    "Идеальное ограбление [1 сезон: 2 выпуск из 10] [Эфир от 09.10] (2026) HDTV 1080p от Files-x",
    "Ну-ка, все вместе! [8 сезон: 5 выпуск] [Эфир от 09.10] (2026) HDTV 1080p от Files-x",
    "Выживалити. Легенда Альфа [4 сезон: 2 выпуск] [Эфир от 08.10] (2026) HDTV 1080p от Files-x",
    "Астрал 6: Они уже здесь / Insidious: Out of the Further (2026) WEB-DL 1080p | D, Р",
    "Что я наделала?! [2 сезон: 3 выпуск из 12] [Эфир от 06.10] (2026) HDTV 1080p от Files-x",
    "Далеко до Москвы [1 сезон: 1-28 серии из 32] (2026) WEB-DL 1080p от ExKinoRay",
    "Гангстерленд / MobLand [2 сезон: 1-4 серии из 10] (2026) WEB-DL 1080p | D, P, P2",
    "Великолепная пятёрка [8 сезон: 1-65 серия из 100] (2026) WEB-DL 1080p от Files-x",
    "Превращения / Transformations (1988) BDRip 1080p от ExKinoRay | P2, A",
    "Кстати [116 выпуск] (2026) WEBRip 1080p",
    "Человек-паук: Новый день / Spider-Man: Brand New Day (2026) WEB-Rip 1080p от New-Team | D | IMAX",
    "Балтийское море [1-2 серии из 8] (2026) WEB-DL 1080p от ExKinoRay",
    "Смешанные единоборства. BKFC Fight Night 47: Bethlehem. Full Event [09.10] (2026) WEBRip 1080p | 50fps | EN",
    "Клюквенный щербет / Кизиловый щербет / Kizilcik Serbeti [5 сезон: 1-3 серии] (2026) WEB-DLRip 1080p | P | Moygolos",
    "Новый Comedy Club [22 сезон: 15 выпуск] [эфир от 09.10] (2026) HDTV 1080p",
    "Клиника / Scrubs [2 сезон: 1-3 серии из 10] (2026) WEB-DL 1080p | WinMedia, TVShows, Red Head Sound",
    "И сыновья / & Sons / And Sons (2025) WEBRip 1080p | L",
    "Истории большой страны [3 сезон: 1-10 серии] (2026) WEBRip 1080p",
    "З/Л/О: Сборник / V/H/S/Mixtape (2026) WEB-DL 1080p | Sub",
    "Несколько дней из жизни И.И. Обломова (1979) BDRip-HEVC 1080p | Крупный план",
    "Людвиг / Ludwig [2 сезон] (2026) WEBRip 1080p | P, L",
    "Лунтик. Обратная сторона Луны (2026) WEB-DL 1080p",
    "Мой пёс Гохан / Gohan (2026) WEB-DL 1080p | D | Pride Production",
    "Тысяча и одна ложь [1-4 серии из 4] (2026) WEB-DL 1080p",
    "Новый Comedy Club [22 сезон: 1-15 выпуски] (2026) WEBRip 1080p от Files-x",
    "Невский: Наследство архитектора [8 сезон: 1-20 серии из 30] (2025) WEB-DL 1080p",
    "Ронин [3 сезон: 1-12 серия из 24] (2025-2026) WEB-DL 1080p от Files-x",
    "Университеты с привидениями 4 / Thoem 4 (2026) WEBRip 1080p | Sub",
    "Поздняя слава / Late Fame (2025) WEB-DL 1080p | Р",
    "Искусство падения [1 сезон: 1-4 серии из 8] (2025) WEB-DL 1080p от ExKinoRay",
    "Предатели / Зрадники [2 сезон: 1-5 выпуски из 10] (2026) WEBRip 1080p | UKR",
    "Книготорговцы / The Booksellers (2019) WEB-DL 1080p | L1",
    "Братья / Brothers [1 сезон: 1-4 серии из 8] (2026) WEBRip-HEVC 1080p | D, P | HDrezka Studio",
    "Животные / Animals (2026) WEB-DL 1080p | D | Videofilm Int",
    "Тёмный ангел / Восхождение темного ангела / Dark Angel: The Ascent (1994) Blu-Ray Remux 1080p | P, A",
    "Месть Чарли Шоу / Charlie Shaw’s Revenge (2026) WEBRip 1080p | Sub",
];

// ---------------------------------------------------------------------------
// 素材
// ---------------------------------------------------------------------------

/// 造一条合成中文标题（及格线 1/2/3 的素材）。
///
/// `sweep` 是中文前缀长度，从 50 逐字加到 109：关键词的水平落点因此逐字扫过
/// 整行、越过软换行边界，并在第 2 行上继续扫 —— 于是
/// 「关键词在第 2 行」「关键词正好压在边界上」**必然**被构造出来。
fn cjk_title(i: usize) -> String {
    let sweep = 50 + (i / 4) % 60;
    let pad = "完".repeat(sweep);
    let tail = "全集 中字 内嵌简繁英多语字幕 HDR10 10bit x265-GROUP 完整版";
    match i % 4 {
        // 关键词在开头（第一行行首边界）
        0 => format!("{KEYWORD} 电影 {pad} {tail}"),
        // 关键词被长前缀推着走：会跨过软换行落到第 2 行
        1 => format!("{pad} {KEYWORD} 电影 2160p {tail}"),
        // 关键词在很靠后的位置
        2 => format!("{pad} {tail} {KEYWORD}"),
        // 关键词被长前缀夹在中间；ASCII / 中文混排
        _ => format!("第{:02}集 {pad}{KEYWORD}{pad} 未删减 {tail}", i % 24 + 1),
    }
}

fn meta_for(i: usize) -> String {
    const SIZES: &[&str] = &["1.42 GB", "8.71 GB", "24.3 GB", "712.4 MB", "58.1 GB"];
    format!(
        "{}  ·  2026-{:02}-{:02}",
        SIZES[i % SIZES.len()],
        1 + i % 9,
        1 + i % 28
    )
}

fn build_cards(titles: &[String], query: &str, accent: &str) -> Vec<TorrentCard> {
    const SOURCES: &[&str] = &[
        "1337x",
        "The Pirate Bay",
        "Knaben",
        "NYAA",
        "Rutor",
        "Megapeer",
        "YTS",
        "dmhy",
    ];
    const CATS: &[&str] = &["movies", "series", "anime", "games", "apps", "books"];

    titles
        .iter()
        .enumerate()
        .map(|(i, title)| TorrentCard {
            idx: i as i32,
            title_markup: md(&highlight(title, query, accent)),
            title_plain: title.as_str().into(),
            meta: meta_for(i).into(),
            source: SOURCES[i % SOURCES.len()].into(),
            cat: CATS[i % CATS.len()].into(),
            seeds: (i % 4000) as i32 + 3,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检驱动
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// 及格线 6：2000 条只读不滚，量虚拟化基线
    Virt,
    /// 及格线 1/2/3：200 条中文，逐条滚过去
    CjkScroll,
    /// 及格线 4：50 条真实西里尔，逐条滚过去
    CyrScroll,
    Done,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Phase::Virt => "6a 虚拟化基线",
            Phase::CjkScroll => "1/2/3 中文",
            Phase::CyrScroll => "4 西里尔",
            Phase::Done => "收尾",
        }
    }
}

struct Runner {
    ui: MainWindow,
    phase: Phase,
    /// 当前相位内已走的步数
    step: usize,
    /// 当前相位预算步数（条数 × 2 + 余量：每步滚半张卡）
    budget: usize,
    /// 滚动方向（+1 / -1）；万一约定反了就自动翻
    dir: f32,
    /// 连续多少步实例化数没有增长
    stall: usize,
    /// 上一次看到的累计实例化数
    last_live: usize,
    /// 当前相位累计实例化次数
    live: Rc<Cell<usize>>,
    /// 当前相位「被实例化过的卡片序号」去重集合
    seen: Rc<RefCell<Vec<bool>>>,
    /// 已记录结果
    virt_live: usize,
    cjk_covered: usize,
    cyr_covered: usize,
    scroll_worked: bool,
    /// 每步强制同步重绘成功的次数（`Window::take_snapshot` 返回 Ok 即证明真的重绘过）
    snaps_ok: usize,
    /// 快照尺寸，用来证明拿到的是一张真缓冲，不是 1×1 占位
    snap_size: (u32, u32),
    /// 首次失败原因（后端不支持 take_snapshot 时记下来）
    snaps_err: Option<String>,
}

impl Runner {
    /// 换一批卡片，并把计数清零、按条数重建去重集合
    fn load(&mut self, titles: &[String], budget: usize) {
        let accent = hl_hex(&self.ui);
        let cards = build_cards(titles, KEYWORD, &accent);
        self.ui.set_total(cards.len() as i32);
        self.ui
            .set_cards(ModelRc::from(Rc::new(VecModel::from(cards))));
        self.live.set(0);
        *self.seen.borrow_mut() = vec![false; titles.len()];
        self.step = 0;
        self.budget = budget;
        self.dir = -1.0;
        self.stall = 0;
        self.last_live = 0;
    }

    fn covered(&self) -> usize {
        self.seen.borrow().iter().filter(|b| **b).count()
    }

    /// 发一次滚动事件，然后**强制一次同步重绘**。
    ///
    /// ⚠️ 为什么必须强制：第一版只靠定时器 + 事件循环重绘，结果 440 步只花了 0.38s
    /// （≈0.86ms/步）—— 全窗口软件重绘不可能这么快，说明**根本没重绘**。
    /// 实例化确实发生了（`ListView` 行高固定 80px，建实例不需要量文本），
    /// 但 parley 的文本布局是在**绘制**时才算的 —— 所以那一轮的「绿」其实没测到
    /// #13548。`take_snapshot()` 会重新渲染整个场景，于是每步都真走一次文本布局。
    fn scroll_and_paint(&mut self) {
        let ev = WindowEvent::PointerScrolled {
            position: LogicalPosition::new(600.0_f32, 420.0_f32),
            delta_x: 0.0,
            delta_y: self.dir * SCROLL_PX,
        };
        if let Err(e) = self.ui.window().dispatch_event_with_result(ev) {
            eprintln!("[m1][WARN] 滚动事件派发失败：{e}");
        }

        match self.ui.window().take_snapshot() {
            Ok(buf) => {
                self.snaps_ok += 1;
                self.snap_size = (buf.width(), buf.height());
            }
            Err(e) => {
                if self.snaps_err.is_none() {
                    eprintln!(
                        "[m1][WARN] take_snapshot 不可用（{e}）→ 退回 request_redraw()；\
                         「每步都真重绘」的保证不成立，本轮结论只能当参考"
                    );
                    self.snaps_err = Some(e.to_string());
                }
                self.ui.window().request_redraw();
            }
        }
    }

    fn tick(&mut self) {
        match self.phase {
            Phase::Virt => {
                if self.step == 0 {
                    let titles: Vec<String> = (0..VIRT_CARDS)
                        .map(|i| format!("{} 第{:04}条 {KEYWORD} 测试", "完".repeat(i % 40), i))
                        .collect();
                    self.load(&titles, 0);
                    println!("[m1][6a] 已装入 {VIRT_CARDS} 条（**不滚动**），等一帧后读实例化数……");
                    self.step = 1;
                } else {
                    self.virt_live = self.live.get();
                    let ok = self.virt_live * 4 < VIRT_CARDS;
                    println!(
                        "[m1][6a] 2000 条 → 实测实例化 {} 个 → {}",
                        self.virt_live,
                        if ok {
                            "虚拟化生效"
                        } else {
                            "⚠️ 疑似未虚拟化（实例化数接近总条数）"
                        }
                    );
                    self.phase = Phase::CjkScroll;
                    let titles: Vec<String> = (0..CJK_CARDS).map(cjk_title).collect();
                    self.load(&titles, CJK_CARDS * 2 + 40);
                    println!(
                        "[m1][1-3] 装入 {CJK_CARDS} 条合成中文标题（前缀 50..=109 逐字扫换行边界），开始逐条滚过……"
                    );
                }
            }
            Phase::CjkScroll => self.scroll_step(Phase::CyrScroll),
            Phase::CyrScroll => self.scroll_step(Phase::Done),
            Phase::Done => {}
        }
    }

    /// 滚动相位：每 tick 滚半张卡，并打印覆盖率。
    fn scroll_step(&mut self, next: Phase) {
        let total = self.seen.borrow().len();

        self.scroll_and_paint();

        let live = self.live.get();
        if live == self.last_live {
            self.stall += 1;
        } else {
            self.stall = 0;
            self.scroll_worked = true;
            self.last_live = live;
        }

        // 头几步就卡住 → 说明方向约定反了，翻过来
        if self.stall == 3 && self.step < 8 {
            self.dir = -self.dir;
            self.stall = 0;
            println!("[m1] 滚动方向无反馈，翻转方向重试（dir = {}）", self.dir);
        }

        // 每步一行：出 panic 时，上一行就是「滚到哪、覆盖到哪、重绘了几次」
        println!(
            "[m1][{}] step {}/{}  dir {:+.0}  覆盖 {}/{}  累计实例 {}  强制重绘 {}",
            self.phase.label(),
            self.step + 1,
            self.budget,
            self.dir,
            self.covered(),
            total,
            live,
            self.snaps_ok
        );

        self.step += 1;
        if self.step >= self.budget {
            let cov = self.covered();
            match self.phase {
                Phase::CjkScroll => {
                    self.cjk_covered = cov;
                    println!("[m1][1-3] 完成：合成中文标题布局覆盖 {cov}/{total}");
                    let titles: Vec<String> = CYRILLIC.iter().map(|s| (*s).to_string()).collect();
                    self.phase = next;
                    self.load(&titles, titles.len() * 2 + 40);
                    println!(
                        "[m1][4] 装入 {} 条真实西里尔标题（megapeer fixture / windows-1251），开始逐条滚过……",
                        CYRILLIC.len()
                    );
                }
                _ => {
                    self.cyr_covered = cov;
                    println!("[m1][4] 完成：西里尔布局覆盖 {cov}/{total}");
                    self.phase = next;
                    self.finish();
                }
            }
        }
    }

    fn finish(&mut self) {
        let cjk_ok = self.cjk_covered >= CJK_CARDS;
        let cyr_ok = self.cyr_covered >= CYRILLIC.len();
        let virt_ok = self.virt_live * 4 < VIRT_CARDS;

        println!("[m1] ================= 结果 ==================");
        println!(
            "[m1][1] 200 条中文标题 + 高亮 + 自动换行：布局覆盖 {}/{}，全程未 panic → {}",
            self.cjk_covered,
            CJK_CARDS,
            if cjk_ok {
                "PASS"
            } else {
                "⚠️ 覆盖不全"
            }
        );
        println!(
            "[m1][2] 关键词落在第 2 行：由前缀 50..=109 **构造覆盖**（保证跨过软换行边界），未 panic"
        );
        println!("[m1][3] 关键词压在换行边界：同上，扫描步长 1 个汉字（构造覆盖），未 panic");
        println!(
            "[m1][4] 西里尔（真实素材）布局覆盖 {}/{}，未 panic → {}",
            self.cyr_covered,
            CYRILLIC.len(),
            if cyr_ok {
                "PASS"
            } else {
                "⚠️ 覆盖不全"
            }
        );
        println!("[m1][5] 自包含 exe 体积：见 CI 的「报告体积」步骤（本进程无法自量）");
        println!(
            "[m1][6] ListView 虚拟化：2000 条 → 实测实例化 {} 个 → {}",
            self.virt_live,
            if virt_ok {
                "PASS（≪ 总条数）"
            } else {
                "FAIL / 存疑"
            }
        );
        if !self.scroll_worked {
            println!(
                "[m1][WARN] ⚠️ 滚动全程没有产生新的实例化 —— 滚动事件没生效，\
                 及格线 1/4 的覆盖是**不完整**的，别当成通过。"
            );
        }
        // 仪器有效性：没强制重绘成功 = 这轮根本没走到 parley 的绘制期文本布局，
        // 那么「没 panic」什么也证明不了。这种情况必须**红**，不许绿着骗过去。
        let instrument_ok = self.snaps_ok > 0;
        println!(
            "[m1][paint] 每步强制同步重绘（take_snapshot）成功 {} 次，快照 {}x{}{}",
            self.snaps_ok,
            self.snap_size.0,
            self.snap_size.1,
            match &self.snaps_err {
                Some(e) => format!("（失败原因：{e}）"),
                None => String::new(),
            }
        );
        let all = cjk_ok && cyr_ok && self.scroll_worked && instrument_ok;
        println!(
            "[m1] VERDICT: {}",
            if all {
                "未复现 #13548 —— 中文标题 + 高亮 + 自动换行在本版本下没有 panic"
            } else {
                "有测项未覆盖完整，结论需人工确认（见上面的 WARN）"
            }
        );
        println!("[m1] ==========================================");

        let _ = slint::quit_event_loop();

        // 仪器失效要红：没走到绘制期就没测到 #13548，绿着出去等于骗人。
        // 退出码 2 = 「测了但没测到」，与 101（panic，= 复现了 bug）区分开。
        if !instrument_ok {
            eprintln!(
                "[m1][FATAL] 强制重绘 0 次 —— 本轮没有真正走到 parley 的绘制期文本布局，\
                 M1 结论**无效**。请改用 request_redraw + 等待事件循环，或换后端。"
            );
            std::process::exit(2);
        }
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[m1][PANIC] ==============================================");
        eprintln!("[m1][PANIC] 命中 panic —— 若正文是 'is not a char boundary'，");
        eprintln!("[m1][PANIC] 那就是 Slint #13548 被复现了（Plan B 见 AGENT_HANDBOOK §1.4）");
        eprintln!("[m1][PANIC] location: {:?}", info.location());
        eprintln!("[m1][PANIC] payload : {info}");
        eprintln!(
            "[m1][PANIC] backtrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        eprintln!("[m1][PANIC] 出事位置 = 上面最后一行 [m1][...] 步进日志的 step / 覆盖");
        eprintln!("[m1][PANIC] ==============================================");
    }));
}

/// `--selftest` 入口。跑完六条及格线里的四条 + 虚拟化基线，然后自己退出。
pub fn run() -> Result<(), slint::PlatformError> {
    install_panic_hook();

    println!("[m1] ========== M1：Slint #13548 风险验证 ==========");
    println!(
        "[m1] 后端 = {}（CI 用 SLINT_BACKEND=winit-software 避开 GPU）",
        std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "<默认>".into())
    );
    println!("[m1] 窗口 1180x800；每步滚 {SCROLL_PX}px（半张卡高）");

    let ui = MainWindow::new()?;
    // 自检要的是覆盖率，不是好看 —— 关掉 StyledText 就等于不测 #13548，强制打开
    ui.set_styled(true);
    ui.set_query(KEYWORD.into());

    let live = Rc::new(Cell::new(0usize));
    let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let live = live.clone();
        let seen = seen.clone();
        // 只累加计数，**不写任何 UI 属性** —— 写全局属性会形成属性依赖环
        // （旧版「共 200 条 / 已实例化 2628 个」就是这么来的）
        ui.on_card_instantiated(move |idx: i32| {
            live.set(live.get() + 1);
            let i = idx as usize;
            let mut s = seen.borrow_mut();
            if i < s.len() {
                s[i] = true;
            }
        });
    }

    let runner = Rc::new(RefCell::new(Runner {
        ui,
        phase: Phase::Virt,
        step: 0,
        budget: 0,
        dir: -1.0,
        stall: 0,
        last_live: 0,
        live,
        seen,
        virt_live: 0,
        cjk_covered: 0,
        cyr_covered: 0,
        scroll_worked: false,
        snaps_ok: 0,
        snap_size: (0, 0),
        snaps_err: None,
    }));

    // 事件循环跑在 ui 上；Runner 也用同一个强引用，两边看的是同一个组件
    let ui = runner.borrow().ui.clone_strong();
    let ui_weak = ui.as_weak();

    let ticker = Timer::default();
    {
        let ui_weak = ui_weak.clone();
        let runner = runner.clone();
        ticker.start(TimerMode::Repeated, Duration::from_millis(25), move || {
            // UI 已经被销毁就别再跑
            if ui_weak.upgrade().is_none() {
                return;
            }
            let mut r = runner.borrow_mut();
            if r.phase == Phase::Done {
                return;
            }
            r.tick();
        });
    }

    // 安全网：无论发生什么，150s 后强制收工，别让 CI 挂死
    let safety = Timer::default();
    safety.start(
        TimerMode::SingleShot,
        Duration::from_secs(SAFETY_SECS),
        || {
            eprintln!("[m1][WARN] 安全超时 {SAFETY_SECS}s 触发，强制退出（可能卡在渲染）");
            let _ = slint::quit_event_loop();
        },
    );

    ui.run()?;

    // 保持 timer 存活到事件循环结束（drop 会取消定时器）
    drop(ticker);
    drop(safety);
    drop(runner);
    Ok(())
}
