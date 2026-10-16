//! 引擎健康度（#6）：把每次搜索里**每家返回了几条**落盘，用来发现"某个引擎悄悄不产出了"。
//!
//! ## 为什么需要它
//!
//! 站点改版的失败形态是**静默返回 0 条** —— 在 UI 上它和"这个关键词本来就没资源"
//! 长得一模一样。没有历史数据的话，用户只能猜；有了历史，就能说
//! "这家最近 8 次都是 0 条，八成是改版了"。
//!
//! ## 存哪 / 什么格式
//!
//! `%APPDATA%\bt-search\health.jsonl`（与 `config_store` 同一个目录）。
//! **一行一条 JSON**（JSONL）—— 追加写不必读整个文件，坏了一行也只丢一行。
//!
//! ## ⚠️ 有上限，不是只追加
//!
//! 超过 [`MAX_LINES`] 行就重写成最后 [`KEEP_LINES`] 行。
//! 一个只追加的文件迟早变成几 MB，而"最近几百次"足够回答"这家还活着吗"。
//!
//! ## 失败一律不致命
//!
//! 与 `config_store` 同一条规矩：读不到 / 写不进去都只是"没有数据"，
//! **绝不 panic、绝不拦搜索**。所以这里也没有 `unwrap`。

use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 超过这么多行就压缩。
pub const MAX_LINES: usize = 5000;
/// 压缩后保留最后这么多行。
pub const KEEP_LINES: usize = 2000;
/// **最近连续空手**达到几次就提示"可能已改版"。
///
/// ⚠️ 3 次是有意的：站点偶尔抽风、关键词偶尔真没资源都很正常，
/// 阈值太低会把噪声报成故障；太高则要等到用户自己发现搜不到了才知道。
pub const STALE_AFTER: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    /// 这一次返回了几条
    pub n: usize,
    /// 没报错（`false` = 连不上 / 超时 / 站点拒绝）
    pub ok: bool,
    pub ms: u64,
}

/// 一家的概览。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stat {
    pub runs: usize,
    /// "空手而归"的次数（0 条，**或**报错 —— 对用户来说都是"没拿到东西"）
    pub empty: usize,
    /// **最近连续**空手几次 —— 这个才是"是不是坏了"的信号
    pub tail_empty: usize,
    /// 有结果的轮次里平均返回几条（空手的轮次**不算进平均** ——
    /// 否则一家彻底挂掉的站会因为平均数被拉低而显得"只是变少了"）
    pub avg_hits: usize,
}

/// 从记录算各家概览。**纯函数**，所以能离线单测。
///
/// 顺序 = **首次出现的顺序**（不是字母序）—— 保持与注册表/UI 一致的观感。
pub fn summarize(records: &[Record]) -> Vec<(String, Stat)> {
    let mut out: Vec<(String, Stat)> = Vec::new();

    for r in records {
        let slot = match out.iter_mut().find(|(id, _)| id == &r.id) {
            Some(s) => s,
            None => {
                out.push((r.id.clone(), Stat::default()));
                let last = out.len() - 1;
                &mut out[last]
            }
        };
        let s = &mut slot.1;
        s.runs += 1;
        if r.ok && r.n > 0 {
            // 有结果：连续空手计数器归零
            s.tail_empty = 0;
        } else {
            s.empty += 1;
            s.tail_empty += 1;
        }
    }

    // 平均要等遍历完再算（需要"有结果的轮次"总数）
    for (id, s) in out.iter_mut() {
        let hits: Vec<usize> = records
            .iter()
            .filter(|r| &r.id == id && r.ok && r.n > 0)
            .map(|r| r.n)
            .collect();
        s.avg_hits = if hits.is_empty() {
            0
        } else {
            hits.iter().sum::<usize>() / hits.len()
        };
    }

    out
}

/// `%APPDATA%\bt-search\health.jsonl`。取不到 `APPDATA` 就 `None`（不猜别的路径）。
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("bt-search").join("health.jsonl"))
}

/// 读全部记录。**坏行直接跳过**（手改过 / 写一半断电都不该让整个文件作废）。
pub fn read_all() -> Vec<Record> {
    let Some(p) = path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Record>(l).ok())
        .collect()
}

/// 追加一批记录。返回 `Err(人话)` —— 调用方**不该**当成致命错误（连警告都不一定要弹）。
pub fn append(batch: &[Record]) -> Result<(), String> {
    if batch.is_empty() {
        return Ok(());
    }
    let path = path().ok_or_else(|| "取不到 %APPDATA%".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }

    let mut buf = String::new();
    for r in batch {
        let line = serde_json::to_string(r).map_err(|e| format!("序列化失败：{e}"))?;
        buf.push_str(&line);
        buf.push('\n');
    }

    // 追加写：`OpenOptions::append` 不用先读文件
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("打开失败：{e}"))?;
    f.write_all(buf.as_bytes())
        .map_err(|e| format!("写入失败：{e}"))?;

    trim_if_needed(&path);
    Ok(())
}

/// ⚠️ **只追加的文件迟早变成几 MB**，而"最近几百次"就够回答问题。
/// 超限就重写成最后 [`KEEP_LINES`] 行。**失败静默** —— 这只是清理，不该影响搜索。
fn trim_if_needed(path: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= MAX_LINES {
        return;
    }
    let keep = &lines[lines.len() - KEEP_LINES..];
    let mut out = keep.join("\n");
    out.push('\n');
    let _ = std::fs::write(path, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, n: usize, ok: bool) -> Record {
        Record {
            id: id.to_string(),
            n,
            ok,
            ms: 100,
        }
    }

    #[test]
    fn summarize_counts_runs_and_averages() {
        let rs = vec![
            rec("tpb", 10, true),
            rec("tpb", 20, true),
            rec("nyaa", 4, true),
        ];
        let s = summarize(&rs);
        assert_eq!(s.len(), 2);
        let (id, st) = &s[0];
        assert_eq!(id, "tpb");
        assert_eq!((st.runs, st.empty, st.tail_empty), (3 - 1, 0, 0));
        assert_eq!(st.avg_hits, 15, "(10+20)/2");
    }

    /// **这条是 #6 的核心**：最近连续空手才是"是不是坏了"的信号。
    #[test]
    fn tail_empty_tracks_only_the_trailing_run() {
        let rs = vec![
            rec("tpb", 0, true),  // 空（早先的一次）
            rec("tpb", 50, true), // 又好了
            rec("tpb", 0, true),  // 连续空 1
            rec("tpb", 0, true),  // 连续空 2
            rec("tpb", 0, true),  // 连续空 3 ⇒ 该报警了
        ];
        let (_, st) = &summarize(&rs)[0];
        assert_eq!(st.runs, 5);
        assert_eq!(st.empty, 4, "一共空过 4 次");
        assert_eq!(st.tail_empty, 3, "但**最近连续**只有 3 次");
        assert!(st.tail_empty >= STALE_AFTER, "达到阈值");
        assert_eq!(st.avg_hits, 50, "只有一次有结果，平均就是它");
    }

    /// 报错也算"空手" —— 对用户来说它和"0 条"都是没拿到东西。
    #[test]
    fn errors_count_as_empty_for_the_tail_signal() {
        let rs = vec![
            rec("tpb", 0, false), // 连不上
            rec("tpb", 0, true),  // 连上了但 0 条
        ];
        let (_, st) = &summarize(&rs)[0];
        assert_eq!(st.empty, 2);
        assert_eq!(st.tail_empty, 2);
    }

    /// 有结果就**打断**连续计数 —— 否则一个偶尔抽风的站会被永久标记。
    #[test]
    fn a_single_hit_resets_the_tail_counter() {
        let rs = vec![
            rec("tpb", 0, true),
            rec("tpb", 0, true),
            rec("tpb", 7, true), // ← 这一下就清空
            rec("tpb", 0, true),
        ];
        let (_, st) = &summarize(&rs)[0];
        assert_eq!(st.tail_empty, 1, "只看最后一次之后");
        assert!(!(st.tail_empty >= STALE_AFTER));
    }

    /// 顺序 = 首次出现顺序，不是字母序。
    #[test]
    fn providers_keep_their_first_seen_order() {
        let rs = vec![rec("zelda", 1, true), rec("alpha", 1, true)];
        let ids: Vec<String> = summarize(&rs).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["zelda", "alpha"]);
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert!(summarize(&[]).is_empty());
    }
}
