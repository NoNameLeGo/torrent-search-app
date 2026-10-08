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
