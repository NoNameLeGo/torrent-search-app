//! 关键词高亮 —— 从 M1 的 `spike/slint-ui/src/main.rs` **原样搬过来**。
//!
//! M1 已经实测过这条路：200 条中文标题 + 高亮 + 自动换行，580 次强制同步重绘，
//! **Slint #13548 未复现**。所以这段逻辑是被验证过的，别顺手重写。
//!
//! 为什么标注放在这里而不是 `bt-core`：它产出的是 **Slint 的 markdown markup**，
//! 是纯展示层的东西；`bt-core` 是数据层（`TorrentResult` 里没有 markup 字段）。
//!
//! ## 两条不能忘的约束
//!
//! 1. 富文本属性在 `.slint` 里必须是 **`styled-text`**（不是 `string`）；
//!    markup 只能由 Rust 侧的 `StyledText::from_markdown()` 构造。
//!    `@markdown()` 会把插值**整体转义**（官方原文："Any text passed as an argument
//!    to the macro will be escaped"）—— 想动态注入高亮就必须走这条 Rust 路。
//! 2. 高亮色是**烘进 markup** 的 ⇒ 换主题必须**整体重建**模型（见 `ui_model`）。

/// 逐字符转义：先处理 HTML 实体，再把 CommonMark 的行内特殊字符反斜杠转义。
///
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

/// 把 `query` 在 `title` 里的**所有**命中包成 `<font color="...">…</font>`。
///
/// 先转义整条标题，再注入裸标签 —— 于是只有我们自己的标签是「活的」，
/// 标题里原本带 `<` 之类不会被当成标记。
///
/// 按 **char** 而不是 byte 处理，避免中文被切断（这也是 #13548 那一类 bug 的根源：
/// 按 byte 切多字节字符）。
pub fn highlight_markup(title: &str, query: &str, accent: &str) -> String {
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

/// 把 markup 解析成 `StyledText`。**解析失败就退回纯文本** ——
/// `from_markdown` 遇到 Slint 不支持的语法会返回 `Err`，
/// 绝不能让整条标题消失（这是 spike 当年就定下的兜底）。
pub fn markup_to_styled(markup: &str) -> slint::StyledText {
    slint::StyledText::from_markdown(markup)
        .unwrap_or_else(|_| slint::StyledText::from_plain_text(markup))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_every_hit_and_escapes_the_rest() {
        let out = highlight_markup("ubuntu ubuntu 22.04", "ubuntu", "#f9e2af");
        assert_eq!(
            out,
            "<font color=\"#f9e2af\">ubuntu</font> <font color=\"#f9e2af\">ubuntu</font> 22.04"
        );
    }

    #[test]
    fn escapes_markdown_and_html_metacharacters_in_the_title() {
        // 标题里的 * 和 < 必须被转义，否则会被当成 markup
        let out = highlight_markup("a*b <c> [d]", "zzz", "#fff");
        assert_eq!(out, "a\\*b &lt;c&gt; \\[d\\]");
    }

    #[test]
    fn matching_is_case_insensitive_and_handles_cjk() {
        let out = highlight_markup("电影 Ubuntu 2024", "ubuntu", "#fff");
        assert_eq!(out, "电影 <font color=\"#fff\">Ubuntu</font> 2024");

        // 中文关键词也要能高亮（按 char 切，不是按 byte）
        let out2 = highlight_markup("流浪地球 2 蓝光原盘", "流浪地球", "#fff");
        assert_eq!(out2, "<font color=\"#fff\">流浪地球</font> 2 蓝光原盘");
    }

    #[test]
    fn empty_query_yields_escaped_plain_title() {
        assert_eq!(highlight_markup("a*b", "", "#fff"), "a\\*b");
        assert_eq!(highlight_markup("a*b", "   ", "#fff"), "a\\*b");
    }

    #[test]
    fn markup_survives_the_styled_text_conversion() {
        // 能解析成富文本（而不是走纯文本兜底）—— 直接断言解析本身成功，
        // 别用 `StyledText` 上的辅助方法（那类方法的可用性没在本机验证过）。
        let markup = highlight_markup("ubuntu", "ubuntu", "#f9e2af");
        assert!(
            slint::StyledText::from_markdown(&markup).is_ok(),
            "注入的 <font color> 必须是 Slint 认的语法：{markup}"
        );
    }
}
