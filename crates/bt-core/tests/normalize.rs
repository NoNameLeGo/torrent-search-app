//! 从 `test/normalize.test.js`（216 行）逐条搬过来的对照测试。
//!
//! 每条断言都能在 JS 版里找到对应行 —— 这是「语义等价」的验收依据。
//! 两处**有意**的差异在文件末尾单独成测试，写明了原因。

use bt_core::normalize::*;

fn ps(s: &str) -> Option<i64> {
    parse_size(Some(&NumOrText::Text(s.to_string())))
}
fn psn(v: i64) -> Option<i64> {
    parse_size(Some(&NumOrText::Num(v)))
}
fn pd(s: &str) -> Option<i64> {
    parse_date(Some(&NumOrText::Text(s.to_string())))
}
fn pdn(v: i64) -> Option<i64> {
    parse_date(Some(&NumOrText::Num(v)))
}
fn ti(s: &str) -> Option<i64> {
    to_int(Some(&NumOrText::Text(s.to_string())))
}
/// 当前时间（ms）。用 std 而不是 chrono —— 集成测试只能访问本 crate 的公开 API
/// 和 dev-dependencies，拿不到 bt-core 的普通依赖。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ============================================================================
// parseSize
// ============================================================================

#[test]
fn parse_size_units() {
    assert_eq!(ps("1.2 GB"), Some(1_288_490_189), "GB");
    assert_eq!(ps("800 MiB"), Some(800 * 1024_i64.pow(2)), "MiB");
    assert_eq!(ps("512 MB"), Some(512 * 1024_i64.pow(2)), "MB");
    assert_eq!(ps("1 TB"), Some(1024_i64.pow(4)), "TB");
    assert_eq!(ps("1 PB"), Some(1024_i64.pow(5)), "PB");
    assert_eq!(ps("1 KB"), Some(1024), "KB");
    assert_eq!(ps("1 K"), Some(1024), "K alias");
    assert_eq!(ps("1 KIB"), Some(1024), "KIB alias");
    assert_eq!(ps("2048"), Some(2048), "bare bytes");
    assert_eq!(ps("1.5G"), Some(1_610_612_736), "G shorthand");
    assert_eq!(ps("1.5G"), ps("1.5 GB"), "consistent with space");
    assert_eq!(ps("1,024 MB"), Some(1024 * 1024_i64.pow(2)), "comma separator");
}

#[test]
fn parse_size_none_cases() {
    assert_eq!(ps(""), None, "empty string");
    assert_eq!(ps("abc"), None, "invalid string");
    assert_eq!(parse_size(None), None, "null / undefined");
}

#[test]
fn parse_size_numeric_input() {
    assert_eq!(psn(12345), Some(12345), "numeric input passes through");
}

#[test]
fn parse_size_edge_cases() {
    assert_eq!(ps("0 B"), Some(0), "zero size");
    assert_eq!(ps("0"), Some(0), "zero bytes");
    assert!(ps("1 PB") > ps("1 TB"), "PB > TB");
}

// ============================================================================
// parseDate
// ============================================================================

#[test]
fn parse_date_unix_timestamps() {
    let secs = 1_652_877_231_i64;
    assert_eq!(pdn(secs), Some(secs * 1000), "unix seconds -> ms");
    assert_eq!(pdn(secs * 1000), Some(secs * 1000), "unix ms stays ms");
}

#[test]
fn parse_date_relative_ordering() {
    let now = now_ms();
    assert!(pd("2 hours ago").unwrap() <= now, "relative: 2 hours ago");
    assert!(pd("3 days ago") < pd("2 hours ago"), "days < hours");
    assert!(pd("yesterday") < pd("today"), "yesterday < today");
    assert!(pd("last month") < pd("yesterday"), "last month < yesterday");
}

#[test]
fn parse_date_special_phrases() {
    let now = now_ms();
    assert!(pd("a minute ago").unwrap() <= now, "a minute ago");
    assert!(pd("an hour ago").unwrap() <= now, "an hour ago");
    assert!(pd("a day ago").unwrap() <= now, "a day ago");
}

#[test]
fn parse_date_absolute() {
    // 2024-01-15T00:00:00Z
    assert_eq!(pd("2024-01-15"), Some(1_705_276_800_000), "ISO date");
    assert_eq!(pd("not a date"), None, "invalid string");
    assert_eq!(pd(""), None, "empty string");
    assert_eq!(parse_date(None), None, "null / undefined");
}

#[test]
fn parse_date_extra_ordering() {
    assert!(pd("1 year ago") < pd("1 month ago"), "year > month");
    assert!(pd("1 week ago") < pd("1 day ago"), "week > day");
}

#[test]
fn format_date_roundtrip() {
    assert_eq!(format_date(Some(1_705_276_800_000)), "2024-01-15");
    assert_eq!(format_date(None), "—");
}

// ============================================================================
// buildMagnet
// ============================================================================

#[test]
fn build_magnet_cases() {
    let with_name = build_magnet(Some("ABC123"), Some("Test Torrent")).unwrap();
    assert!(
        with_name.starts_with("magnet:?xt=urn:btih:ABC123&dn="),
        "with name: {with_name}"
    );
    // 关键：name 必须走 encodeURIComponent，空格 -> %20
    assert_eq!(with_name, "magnet:?xt=urn:btih:ABC123&dn=Test%20Torrent");

    assert!(
        build_magnet(Some("ABC123"), None)
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:ABC123"),
        "without name"
    );
    assert_eq!(build_magnet(None, Some("x")), None, "null hash");
    assert_eq!(build_magnet(Some(""), Some("x")), None, "empty hash");
    assert_eq!(build_magnet(None, None), None, "no args");
}

#[test]
fn build_magnet_long_hashes() {
    let a = build_magnet(Some(&"A".repeat(40)), Some("Test")).unwrap();
    assert!(a.len() > 50, "40-char hash");
    let b = build_magnet(Some(&"a".repeat(32)), Some("Test")).unwrap();
    assert!(b.len() > 50, "32-char hash");
}

// ============================================================================
// extractInfoHash
// ============================================================================

#[test]
fn extract_info_hash_cases() {
    let expected = Some("abc123def456789012345678901234567890".to_string());

    assert_eq!(
        extract_info_hash(Some(
            "magnet:?xt=urn:btih:ABC123DEF456789012345678901234567890&dn=test"
        )),
        expected,
        "valid hash normalized to lowercase"
    );
    assert_eq!(
        extract_info_hash(Some("btih:ABC123DEF456789012345678901234567890")),
        expected,
        "bare btih string"
    );
    assert_eq!(extract_info_hash(Some("not a magnet")), None, "no btih");
    assert_eq!(extract_info_hash(None), None, "null");
    assert_eq!(extract_info_hash(Some("")), None, "empty");
    assert_eq!(extract_info_hash(Some("short")), None, "too short");
}

// ============================================================================
// ruDate
// ============================================================================

#[test]
fn ru_date_cases() {
    assert!(ru_date("Сегодня").contains("Today"), "today translation");
    assert!(ru_date("Вчера").contains("Yesterday"), "yesterday translation");
    assert!(ru_date("мая").contains("May"), "genitive month -> May");
    assert!(ru_date("янв").contains("Jan"), "Russian month -> Jan");
}

// ============================================================================
// toInt / coercePage
// ============================================================================

#[test]
fn to_int_is_lenient_like_parse_int() {
    assert_eq!(ti("39"), Some(39));
    assert_eq!(ti("1,234"), Some(1234), "comma separator");
    assert_eq!(ti("-5"), Some(-5), "negative");
    assert_eq!(ti("12abc"), Some(12), "stops at first non-digit");
    assert_eq!(ti("  7  "), Some(7), "trims");
    assert_eq!(ti("abc"), None);
    assert_eq!(ti(""), None);
    assert_eq!(to_int(None), None);
    assert_eq!(to_int(Some(&NumOrText::Num(42))), Some(42));
}

#[test]
fn coerce_page_clamps_to_one() {
    assert_eq!(coerce_page(Some(&NumOrText::Num(3))), 3);
    assert_eq!(coerce_page(Some(&NumOrText::Num(0))), 1);
    assert_eq!(coerce_page(Some(&NumOrText::Num(-9))), 1);
    assert_eq!(coerce_page(Some(&NumOrText::Text("abc".into()))), 1);
    assert_eq!(coerce_page(None), 1);
}

// ============================================================================
// normalize()
// ============================================================================

#[test]
fn normalize_full_result() {
    let raw = RawResult {
        provider: "tpb".into(),
        name: Some("Ubuntu 22.04 LTS".into()),
        info_hash: Some("2C6B6858D61DA9543D4231A71DB4B1C9264B0685".into()),
        size: Some(NumOrText::Text("3.4 GB".into())),
        seeders: Some(NumOrText::Num(39)),
        leechers: Some(NumOrText::Num(1)),
        date: Some(NumOrText::Num(1_652_877_231)),
        category: Some("Apps".into()),
        detail_url: Some("https://thepiratebay.org/description.php?id=59191690".into()),
        files: Some(NumOrText::Num(1)),
        ..Default::default()
    };

    let r = normalize(&raw);

    assert_eq!(r.id, "tpb:2C6B6858D61DA9543D4231A71DB4B1C9264B0685", "id format");
    assert_eq!(r.name, "Ubuntu 22.04 LTS", "name");
    assert_eq!(r.provider, "tpb", "provider");
    assert_eq!(r.size, Some(3_650_722_202), "size in bytes");
    assert_eq!(r.size_text, "3.4 GB", "size text");
    assert_eq!(r.seeders, Some(39), "seeders");
    assert_eq!(r.leechers, Some(1), "leechers");
    assert_eq!(r.date, Some(1_652_877_231_000), "date as ms");
    assert_eq!(r.category.as_deref(), Some("Apps"), "category");
    assert_eq!(r.files, Some(1), "files");
    assert!(r.magnet.as_deref().unwrap().starts_with("magnet:?"), "magnet URI");
    assert!(!r.needs_magnet, "no need for magnet");
}

#[test]
fn normalize_minimal_result() {
    let raw = RawResult {
        provider: "demo".into(),
        name: Some("Test".into()),
        ..Default::default()
    };

    let r = normalize(&raw);

    assert_eq!(r.id, "demo:Test", "id from provider:name");
    assert_eq!(r.name, "Test", "name");
    assert_eq!(r.provider, "demo", "provider");
    assert_eq!(r.size, None, "size null");
    assert_eq!(r.size_text, "—", "size text placeholder");
    assert_eq!(r.seeders, None, "seeders null");
    assert_eq!(r.magnet, None, "magnet null");
    assert!(!r.needs_magnet, "no detailUrl so no needsMagnet");
}

#[test]
fn normalize_needs_magnet() {
    let raw = RawResult {
        provider: "1337x".into(),
        name: Some("Ubuntu ISO".into()),
        size: Some(NumOrText::Text("2 GB".into())),
        detail_url: Some("https://1337x.to/torrent/123/".into()),
        ..Default::default()
    };

    let r = normalize(&raw);

    assert!(r.needs_magnet, "needs magnet when no infoHash");
    assert_eq!(r.magnet, None, "magnet is null");
}

#[test]
fn normalize_prefers_explicit_magnet() {
    let raw = RawResult {
        provider: "demo".into(),
        name: Some("X".into()),
        magnet: Some("magnet:?xt=urn:btih:deadbeef".into()),
        ..Default::default()
    };
    let r = normalize(&raw);
    assert_eq!(r.magnet.as_deref(), Some("magnet:?xt=urn:btih:deadbeef"));
    assert!(!r.needs_magnet);
}

#[test]
fn normalize_falls_back_to_untitled() {
    let raw = RawResult {
        provider: "demo".into(),
        ..Default::default()
    };
    let r = normalize(&raw);
    assert_eq!(r.name, "(untitled)");
    assert_eq!(r.id, "demo:");
}

// ============================================================================
// 与 JS 版的**有意**差异 —— 每条都写明原因
// ============================================================================

/// JS 版 `parseDate` 的 "a X ago" 分支正则写的是 `(min|minute|hour)`，
/// 漏了 day/week/month/year，因此 `parseDate("a day ago")` 会掉到
/// `Date.parse` 并返回 `null`。Rust 版补全了这几个单位。
/// （JS 测试里那条 `assert.ok(parseDate('a day ago') <= refTime)` 之所以能过，
/// 只是因为 `null <= number` 在 JS 里为 true —— 是个假绿。）
#[test]
fn divergence_a_day_ago_now_resolves() {
    assert!(pd("a day ago").is_some(), "JS 版这里是 null，Rust 版修好了");
    assert!(pd("a week ago").is_some());
    assert!(pd("a month ago").is_some());
    assert!(pd("a year ago").is_some());
}

/// `parseFloat` 比 Rust 的 `f64::from_str` 宽松：`"1.2.3"` → `1.2`。
/// 站点偶尔会吐出这种脏数据，所以专门复刻了这个行为。
#[test]
fn divergence_js_parse_float_leniency() {
    assert_eq!(ps("1.2.3 GB"), Some(1_288_490_189), "取最长合法数字前缀");
    assert_eq!(ps(".5 GB"), Some(536_870_912), "前导小数点");
}

/// 兜底解析只认几种常见格式，其余明确返回 None；
/// JS 的 `Date.parse()` 宽松度无法完全复刻（它会认一堆连字符怪格式）。
#[test]
fn divergence_absolute_format_whitelist() {
    assert_eq!(pd("2024/01/15"), Some(1_705_276_800_000), "斜杠格式");
    assert_eq!(pd("2024-01-15 10:30:00"), Some(1_705_314_600_000), "带时间");
    assert_eq!(pd("2024-01-15T10:30:00Z"), Some(1_705_314_600_000), "RFC3339");
    assert_eq!(pd("totally bogus date"), None, "不认识就放弃，不瞎猜");
}
