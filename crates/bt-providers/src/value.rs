//! `serde_json::Value` → `bt-core` 宽松类型的公共转换。
//!
//! 抽出来的原因：站点发来的 JSON 类型不固定（apibay 全是字符串、knaben 是数字），
//! 每个 provider 都要做同一件事。原来内联在 `tpb.rs` 里，第二个 provider 落地时提为公共。

use bt_core::normalize::NumOrText;
use serde_json::Value;

/// `Value` → `NumOrText`。空字符串视作"没有值"（JS 里 `"" || null` 也是 falsy）。
pub fn v2nt(v: &Value) -> Option<NumOrText> {
    match v {
        Value::Number(n) => n.as_i64().map(NumOrText::Num),
        Value::String(s) if !s.is_empty() => Some(NumOrText::Text(s.clone())),
        _ => None,
    }
}

/// `Value` → 字符串。数字也收（JS 的模板字符串会隐式转）。
pub fn v2string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// 同 [`v2nt`]，但**数字 0 也算"没有值"**。
///
/// 对齐 JS 里 `x ? Number(x) : null` 这种写法 —— `0` 是 falsy，
/// 于是 `created_unix: 0` 会变成 `null`（而不是 1970-01-01）。
/// torrentscsv / yts 的日期字段都用了这个写法。
pub fn v2nt_nonzero(v: &Value) -> Option<NumOrText> {
    v2nt(v).filter(|nt| !matches!(nt, NumOrText::Num(0)))
}

/// 取数组里的**最小**数字（用于 knaben 的 `categoryId: [4000000, 4004000]` 这种）。
///
/// 复刻 JS：`it.categoryId.map(Number).filter(n => !isNaN(n))` 后取 `Math.min(...)` ——
/// 所以非数字项要被丢掉，而不是让整数组变成 `NaN`。
pub fn min_number(v: &Value) -> Option<i64> {
    v.as_array()?
        .iter()
        .filter_map(|x| match x {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            Value::String(s) => s
                .trim()
                .parse::<i64>()
                .ok()
                .or_else(|| s.trim().parse::<f64>().ok().map(|f| f as i64)),
            _ => None,
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn v2nt_treats_empty_string_as_absent() {
        assert_eq!(v2nt(&json!(39)), Some(NumOrText::Num(39)));
        assert_eq!(v2nt(&json!("39")), Some(NumOrText::Text("39".into())));
        assert_eq!(v2nt(&json!("")), None, "空串在 JS 里是 falsy");
        assert_eq!(v2nt(&json!(null)), None);
        assert_eq!(v2nt(&json!([1, 2])), None);
        assert_eq!(v2nt(&json!(true)), None);
    }

    #[test]
    fn v2nt_nonzero_mirrors_js_falsy_zero() {
        assert_eq!(v2nt_nonzero(&json!(0)), None, "JS: x ? ... : null");
        assert_eq!(v2nt_nonzero(&json!(1)), Some(NumOrText::Num(1)));
        // 字符串 "0" 在 JS 里是 truthy
        assert_eq!(v2nt_nonzero(&json!("0")), Some(NumOrText::Text("0".into())));
    }

    #[test]
    fn v2string_accepts_numbers() {
        assert_eq!(v2string(&json!("x")), Some("x".to_string()));
        assert_eq!(v2string(&json!(42)), Some("42".to_string()));
        assert_eq!(v2string(&json!("")), None);
        assert_eq!(v2string(&json!(null)), None);
    }

    #[test]
    fn min_number_picks_the_minimum_and_drops_junk() {
        assert_eq!(min_number(&json!([4000000, 4004000])), Some(4_000_000));
        assert_eq!(min_number(&json!([10000000, 9001000])), Some(9_001_000));
        // 非数字项被丢掉，和 JS 的 `.filter(n => !isNaN(n))` 一致
        assert_eq!(min_number(&json!(["5", "abc", 3])), Some(3));
        assert_eq!(min_number(&json!([])), None);
        assert_eq!(min_number(&json!("nope")), None, "不是数组就是 None");
    }
}
