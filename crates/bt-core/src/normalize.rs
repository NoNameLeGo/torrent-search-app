//! 归一化层 —— 从 `src/lib/normalize.js`（161 行）逐条移植。
//!
//! 目标是**语义等价**，不是"写个差不多的"。`test/normalize.test.js` 的每条断言
//! 都搬到了 `tests/normalize.rs`，改这里必须让那些测试继续绿。
//!
//! ## 与 JS 版的两处**有意的**差异（都在下方就地注明）
//! 1. `parse_date` 的 "a day ago" —— JS 版的 `(min|minute|hour)` 漏了 day/week/month/year，
//!    导致 "a day ago" 返回 null。这里补全了。
//! 2. `parse_date` 的兜底解析 —— JS 用 `Date.parse()`，宽松度无法完全复刻。
//!    这里只认几种常见格式，其余返回 None（原来是"可能解析出奇怪结果"，现在是"明确放弃"）。

use std::sync::LazyLock;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use regex::{Captures, Regex};

// ---- 输入类型 -------------------------------------------------------------

/// 抓来的原始值可能是数字（JSON API）也可能是文本（HTML 抓取）。
///
/// JS 版靠 `typeof input === 'number'` 在运行时区分；Rust 里把这个区分提到类型层面，
/// 避免"数字被当字符串解析"这类静默错位（例如 `parseDate(1652877231)` 是秒，
/// 而 `parseDate("1652877231")` 在 JS 里是 `NaN`）。
#[derive(Debug, Clone, PartialEq)]
pub enum NumOrText {
    Num(i64),
    Text(String),
}

macro_rules! from_num {
    ($($t:ty),*) => {
        $(impl From<$t> for NumOrText {
            fn from(v: $t) -> Self { NumOrText::Num(v as i64) }
        })*
    };
}
from_num!(i32, u32, i64, u64, usize);

impl From<&str> for NumOrText {
    fn from(v: &str) -> Self {
        NumOrText::Text(v.to_string())
    }
}
impl From<String> for NumOrText {
    fn from(v: String) -> Self {
        NumOrText::Text(v)
    }
}
impl From<&String> for NumOrText {
    fn from(v: &String) -> Self {
        NumOrText::Text(v.clone())
    }
}

// ---- 正则（编译一次，全局复用）---------------------------------------------

static SIZE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^([\d.]+)\s*([A-Za-z]+)?$").unwrap());
static BTIH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)btih:([a-f0-9]{32,40})").unwrap());
static AN_AGO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^an?\s+(min|minute|hour|day|week|month|year)s?\s+ago$").unwrap()
});
static REL_AGO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(\d+)\s*(min|minute|hour|day|week|month|year)s?\s*ago$").unwrap()
});
static YESTERDAY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^yesterday$").unwrap());
static TODAY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^today$").unwrap());
static LAST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^last\s+(month|year)$").unwrap());
static MAY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)мая").unwrap());
static CYR_WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[а-яё]+").unwrap());

const MS_MIN: i64 = 60_000;
const MS_HOUR: i64 = 3_600_000;
const MS_DAY: i64 = 86_400_000;
const MS_WEEK: i64 = 604_800_000;
const MS_MONTH: i64 = 2_629_800_000;
const MS_YEAR: i64 = 31_536_000_000;

fn unit_ms(unit: &str) -> Option<i64> {
    Some(match unit {
        "min" | "minute" => MS_MIN,
        "hour" => MS_HOUR,
        "day" => MS_DAY,
        "week" => MS_WEEK,
        "month" => MS_MONTH,
        "year" => MS_YEAR,
        _ => return None,
    })
}

// ---- Size -----------------------------------------------------------------

fn size_unit_mult(unit_upper: &str) -> Option<i64> {
    Some(match unit_upper {
        "B" => 1,
        "KB" | "K" | "KIB" => 1024,
        "MB" | "M" | "MIB" => 1024_i64.pow(2),
        "GB" | "G" | "GIB" => 1024_i64.pow(3),
        "TB" | "T" | "TIB" => 1024_i64.pow(4),
        "PB" | "P" | "PIB" => 1024_i64.pow(5),
        _ => return None,
    })
}

/// 复刻 JS `parseFloat` 的宽松语义：取最长的合法数字前缀。
///
/// 直接 `.parse::<f64>()` 会拒绝 `"1.2.3"`，而 JS 的 `parseFloat("1.2.3")` 给 `1.2`。
fn js_parse_float(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0usize;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0usize;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = i - frac_start;
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    s[..i].parse::<f64>().ok()
}

/// `"1.2 GB"` / `"800 MiB"` / `"2048"` → 字节数。
pub fn parse_size(input: Option<&NumOrText>) -> Option<i64> {
    match input? {
        NumOrText::Num(n) => Some(*n),
        NumOrText::Text(raw) => {
            let cleaned = raw.trim().replace(',', "");
            let caps = SIZE_RE.captures(&cleaned)?;
            let num = js_parse_float(caps.get(1)?.as_str())?;
            if num.is_nan() {
                return None;
            }
            let unit = caps
                .get(2)
                .map(|m| m.as_str().to_ascii_uppercase())
                .unwrap_or_else(|| "B".to_string());
            let mult = size_unit_mult(&unit)?;
            Some((num * mult as f64).round() as i64)
        }
    }
}

/// 字节数 → `"3.4 GB"`；拿不到就给 `"—"`。
pub fn format_size(bytes: Option<i64>) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let Some(bytes) = bytes else {
        return "—".to_string();
    };
    let mut v = bytes as f64;
    let mut i = 0usize;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let body = if v >= 100.0 || i == 0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    };
    format!("{} {}", body, UNITS[i])
}

// ---- Date -----------------------------------------------------------------

/// 绝对日期文本 → 毫秒时间戳（UTC）。只认常见格式，认不出就 None。
fn parse_absolute_date(s: &str) -> Option<i64> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp_millis());
    }
    for f in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, f) {
            return Some(Utc.from_utc_datetime(&naive).timestamp_millis());
        }
    }
    for f in ["%Y-%m-%d", "%Y/%m/%d", "%d.%m.%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(s, f) {
            let naive = d.and_hms_opt(0, 0, 0)?;
            return Some(Utc.from_utc_datetime(&naive).timestamp_millis());
        }
    }
    // JS 在这一步之后还有 `Date.parse(s)` 兜底，它能认「日 月缩写 年」这类写法。
    // 少了这一支，`"30 Jun 26"`（rutor 的 `ru_date` 正好产出这个形状）在 Rust 里
    // 会是 None —— 2026-10-09 移植 rutor 时才暴露出来。这里按 JS 的语义补上：
    // **按本地时区当天零点**（不是 UTC，这点与上面 `%Y-%m-%d` 那一支不同，与 JS 一致）。
    if let Some(ms) = parse_named_month_date(s) {
        return Some(ms);
    }
    None
}

/// 解析 `Date.parse` 认识、但上面那些格式没覆盖的「月名」写法：
///
/// ```text
/// 30 Jun 26     30 Jun 2026     Jun 30 26     Jun 30, 2026
/// ```
///
/// 月份名用英文三字母缩写（大小写不敏感，`ru_date` 产出的就是这个）。
/// **按本地时区当天零点**，与 JS 的 `Date.parse` 一致。
fn parse_named_month_date(s: &str) -> Option<i64> {
    let cleaned = s.replace(',', " ");
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() != 3 {
        return None;
    }

    let (month_idx, month) = parts
        .iter()
        .enumerate()
        .find_map(|(i, p)| month_from_name(p).map(|m| (i, m)))?;

    // 月名之外的两个数字：前一个是「日」，后一个是「年」
    // （`30 Jun 26` 与 `Jun 30 26` 都是这个顺序）
    let nums: Vec<&str> = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != month_idx)
        .map(|(_, p)| *p)
        .collect();
    if nums.len() != 2 {
        return None;
    }

    let day: u32 = nums[0].parse().ok()?;
    let year_raw: i32 = nums[1].parse().ok()?;
    // JS 的规则：两位年份 0-49 归 20xx，50-99 归 19xx
    let year = if (0..100).contains(&year_raw) {
        if year_raw < 50 {
            2000 + year_raw
        } else {
            1900 + year_raw
        }
    } else {
        year_raw
    };

    let naive = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(0, 0, 0)?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

/// 英文三字母月份缩写 → 月号。只看前三个字符，所以 `June` / `JUN` 也认。
fn month_from_name(s: &str) -> Option<u32> {
    let l = s.to_ascii_lowercase();
    Some(match l.get(0..3)? {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// `"2 hours ago"` / `"Yesterday"` / `"2024-01-02"` / unix 秒或毫秒 → 毫秒时间戳。
pub fn parse_date(input: Option<&NumOrText>) -> Option<i64> {
    match input? {
        // unix 秒 / 毫秒自动判别：10 位是秒，13 位是毫秒
        NumOrText::Num(n) => Some(if *n > 100_000_000_000 {
            *n
        } else {
            n.saturating_mul(1000)
        }),
        NumOrText::Text(raw) => {
            let s = raw.trim();
            if s.is_empty() {
                return None;
            }
            let now = Utc::now().timestamp_millis();

            // "a minute ago" / "an hour ago" —— 注意：这里比 JS 版多认了
            // day/week/month/year。JS 的 `(min|minute|hour)` 漏了这几个，
            // 导致 "a day ago" 会掉到 Date.parse 并返回 NaN。
            if let Some(c) = AN_AGO_RE.captures(s) {
                return Some(now - unit_ms(&c[1].to_ascii_lowercase())?);
            }

            // "5 minutes ago" / "3 days ago"
            if let Some(c) = REL_AGO_RE.captures(s) {
                let n: i64 = c[1].parse().ok()?;
                return Some(now - n * unit_ms(&c[2].to_ascii_lowercase())?);
            }

            if YESTERDAY_RE.is_match(s) {
                return Some(now - MS_DAY);
            }
            if TODAY_RE.is_match(s) {
                // "今天"按**本地**午夜算，和 JS `d.setHours(0,0,0,0)` 一致
                let d = Local::now().date_naive();
                let naive = d.and_hms_opt(0, 0, 0)?;
                return Local
                    .from_local_datetime(&naive)
                    .earliest()
                    .map(|dt| dt.timestamp_millis());
            }
            // "last month" / "last year" → 约 30 / 365 天前
            if let Some(c) = LAST_RE.captures(s) {
                let mult = if c[1].eq_ignore_ascii_case("month") {
                    MS_MONTH
                } else {
                    MS_YEAR
                };
                return Some(now - mult);
            }

            parse_absolute_date(s)
        }
    }
}

/// 毫秒时间戳 → `"YYYY-MM-DD"`（UTC，与 JS `toISOString().slice(0,10)` 一致）。
pub fn format_date(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return "—".to_string();
    };
    match Utc.timestamp_millis_opt(ts).single() {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => "—".to_string(),
    }
}

// ---- Magnet ---------------------------------------------------------------

/// 复刻 JS `encodeURIComponent`：只有 `A-Za-z0-9-_.!~*'()` 保持原样。
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 组磁力链。infoHash 为空（或 None）时返回 None —— 调用方据此走 `resolveMagnet`。
pub fn build_magnet(info_hash: Option<&str>, name: Option<&str>) -> Option<String> {
    let hash = info_hash.filter(|h| !h.is_empty())?;
    let dn = match name.filter(|n| !n.is_empty()) {
        Some(n) => format!("&dn={}", encode_uri_component(n)),
        None => String::new(),
    };
    Some(format!("magnet:?xt=urn:btih:{hash}{dn}"))
}

/// 从磁力链（或任何含 `btih:` 的字符串）里抠出 hex infoHash，统一小写。
pub fn extract_info_hash(s: Option<&str>) -> Option<String> {
    let s = s.filter(|s| !s.is_empty())?;
    BTIH_RE.captures(s).map(|c| c[1].to_ascii_lowercase())
}

// ---- Russian date helpers -------------------------------------------------

fn ru_month(word_lower: &str) -> Option<&'static str> {
    Some(match word_lower {
        "янв" => "Jan",
        "фев" => "Feb",
        "мар" => "Mar",
        "апр" => "Apr",
        "май" | "мая" => "May",
        "июн" => "Jun",
        "июл" => "Jul",
        "авг" => "Aug",
        "сен" => "Sep",
        "окт" => "Oct",
        "ноя" => "Nov",
        "дек" => "Dec",
        _ => return None,
    })
}

/// 俄语日期尽量转成英文，好让 `parse_date` 认得（rutor / megapeer 用）。
pub fn ru_date(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let s = s.replace("Сегодня", "Today").replace("Вчера", "Yesterday");
    // JS: .replace(/Мая/gi, 'Май') —— 属格转主格
    let s = MAY_RE.replace_all(&s, "Май").into_owned();
    CYR_WORD_RE
        .replace_all(&s, |caps: &Captures| {
            let w = caps[0].to_lowercase();
            match ru_month(&w) {
                Some(m) => m.to_string(),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

// ---- 通用小工具 ------------------------------------------------------------

/// `"1,234 seeds"` → `Some(1234)`；`"abc"` → `None`。
/// 复刻 JS `parseInt`：跳过前导空白、认一个正负号、取最长的数字前缀。
pub fn to_int(input: Option<&NumOrText>) -> Option<i64> {
    match input? {
        NumOrText::Num(n) => Some(*n),
        NumOrText::Text(raw) => {
            let cleaned = raw.replace(',', "");
            let t = cleaned.trim();
            let b = t.as_bytes();
            let mut i = 0usize;
            let neg = match b.first() {
                Some(b'-') => {
                    i = 1;
                    true
                }
                Some(b'+') => {
                    i = 1;
                    false
                }
                _ => false,
            };
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i == start {
                return None;
            }
            let n: i64 = t[start..i].parse().ok()?;
            Some(if neg { -n } else { n })
        }
    }
}

/// 页码收敛成 ≥ 1 的整数。
pub fn coerce_page(input: Option<&NumOrText>) -> u32 {
    match to_int(input) {
        Some(n) if n > 0 => n.min(u32::MAX as i64) as u32,
        _ => 1,
    }
}

// ---- 归一化主函数 ----------------------------------------------------------

/// provider 抓到的原始结果。除 `provider` 外都可缺省 —— 站点能给多少算多少。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawResult {
    pub provider: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub info_hash: Option<String>,
    pub magnet: Option<String>,
    pub size: Option<NumOrText>,
    pub seeders: Option<NumOrText>,
    pub leechers: Option<NumOrText>,
    pub date: Option<NumOrText>,
    pub category: Option<String>,
    pub detail_url: Option<String>,
    pub files: Option<NumOrText>,
}

/// 全项目统一的搜索结果形状。
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentResult {
    pub id: String,
    pub name: String,
    pub size: Option<i64>,
    pub size_text: String,
    pub seeders: Option<i64>,
    pub leechers: Option<i64>,
    pub date: Option<i64>,
    pub date_text: String,
    pub magnet: Option<String>,
    pub info_hash: Option<String>,
    pub provider: String,
    pub category: Option<String>,
    pub detail_url: Option<String>,
    pub files: Option<i64>,
    /// 没有磁力但有详情页 → 前端点击时再惰性解析
    pub needs_magnet: bool,
}

fn non_empty(o: &Option<String>) -> Option<&str> {
    o.as_deref().filter(|s| !s.is_empty())
}

/// 把 provider 的原始结果规整成 `TorrentResult`。
pub fn normalize(raw: &RawResult) -> TorrentResult {
    let size = parse_size(raw.size.as_ref());
    let date_ts = parse_date(raw.date.as_ref());

    let magnet = non_empty(&raw.magnet)
        .map(|s| s.to_string())
        .or_else(|| build_magnet(non_empty(&raw.info_hash), non_empty(&raw.name)));

    let info_hash = non_empty(&raw.info_hash).map(|s| s.to_string());
    let name = non_empty(&raw.name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "(untitled)".to_string());

    let id = non_empty(&raw.id)
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let tail = info_hash
                .clone()
                .or_else(|| non_empty(&raw.name).map(|s| s.to_string()))
                .unwrap_or_default();
            format!("{}:{}", raw.provider, tail)
        });

    let detail_url = non_empty(&raw.detail_url).map(|s| s.to_string());
    // 没有磁力、但有详情页 —— 说明磁力要等用户点击时再去详情页捞
    let needs_magnet = magnet.is_none() && detail_url.is_some();

    TorrentResult {
        id,
        name,
        size,
        size_text: format_size(size),
        seeders: to_int(raw.seeders.as_ref()),
        leechers: to_int(raw.leechers.as_ref()),
        date: date_ts,
        date_text: format_date(date_ts),
        magnet,
        info_hash,
        provider: raw.provider.clone(),
        category: non_empty(&raw.category).map(|s| s.to_string()),
        detail_url,
        files: to_int(raw.files.as_ref()),
        needs_magnet,
    }
}
