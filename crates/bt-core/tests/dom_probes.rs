//! 选择器语义等价性验证 —— **scraper 对照 cheerio**。
//!
//! 移植 37 个 HTML provider 之前，必须先回答一个问题：
//! **`scraper` 的 CSS 选择器与取值语义，和 cheerio 是不是一致？**
//! 不一致的地方不会报错，只会静默少几个字段 —— 那是最难查的一类问题。
//!
//! 做法：
//! 1. probe 清单 `test/fixtures/html-probes.json`（两份实现读同一份）
//! 2. cheerio 侧：`scripts/html-probes.cjs`，输出存成 `html-probes.expected.json`
//! 3. Rust 侧：本文件，用 `bt_core::dom` 跑同样的 probe，与期望值逐条比对
//!
//! 期望值必须在**有 cheerio 的环境**生成（本机 node_modules 已删，所以走 CI 的
//! `.github/workflows/html-probes.yml`）。期望文件不存在时本测试会**跳过并打警告**，
//! 这样离线 CI 不会因为「还没生成真值」而红。
//!
//! 失败时不是只报一句 assert，而是逐条打印差异 —— 因为比对失败通常意味着
//! 某条选择器语义真有分歧，需要看清是 `text` 拼接、实体解码还是 combinater 的差别。

use std::path::{Path, PathBuf};

use bt_core::dom::{attr, closest_tag, text_trim, Dom};
use serde_json::{json, Map, Value};

fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()))
}

/// 与 `scripts/html-probes.cjs` 的 `runProbe()` 一一对应。改一边必须改另一边。
fn run_probe(dom: &Dom, p: &Value) -> Value {
    let op = p["op"].as_str().unwrap_or("");
    let selector = p["selector"].as_str().unwrap_or("");
    let attr_name = p["attr"].as_str().unwrap_or("");
    let limit = p["limit"].as_u64().unwrap_or(3) as usize;

    let els = dom.select(selector);

    match op {
        "count" => json!(els.len()),

        "text_first" => json!(els.first().map(|e| text_trim(*e)).unwrap_or_default()),

        "attr_first" => match els.first() {
            Some(e) => opt_str(attr(*e, attr_name)),
            None => Value::Null,
        },

        "attrs" => Value::Array(
            els.iter()
                .take(limit)
                .map(|e| opt_str(attr(*e, attr_name)))
                .collect(),
        ),

        "texts" => Value::Array(
            els.iter()
                .take(limit)
                .map(|e| Value::String(text_trim(*e)))
                .collect(),
        ),

        "row_cells" => {
            let closest = p["closest"].as_str().unwrap_or("tr");
            let within = p["within"].as_str().unwrap_or("td");
            Value::Array(
                els.iter()
                    .take(limit)
                    .map(|e| match closest_tag(*e, closest) {
                        Some(row) => Value::Array(
                            dom.find(row, within)
                                .iter()
                                .map(|c| Value::String(text_trim(*c)))
                                .collect(),
                        ),
                        None => Value::Array(Vec::new()),
                    })
                    .collect(),
            )
        }

        other => json!({ "error": format!("unknown op {other}") }),
    }
}

fn opt_str(v: Option<String>) -> Value {
    v.map(Value::String).unwrap_or(Value::Null)
}

/// 用 scraper 把整份 probe 清单跑一遍，产出与 cheerio 侧同构的 JSON。
fn run_all(spec: &Value, fixtures_dir: &Path) -> Value {
    let mut out = Map::new();

    for fixture in spec["fixtures"].as_array().expect("fixtures 该是数组") {
        let file = fixture["file"].as_str().expect("fixture 缺少 file");
        let html = read(&fixtures_dir.join(file));
        let dom = Dom::parse(&html);

        let mut probes = Map::new();
        for probe in fixture["probes"].as_array().expect("probes 该是数组") {
            let id = probe["id"].as_str().expect("probe 缺少 id");
            probes.insert(id.to_string(), run_probe(&dom, probe));
        }
        out.insert(file.to_string(), Value::Object(probes));
    }

    Value::Object(out)
}

#[test]
fn selectors_and_accessors_match_cheerio_on_real_fixtures() {
    let spec: Value = serde_json::from_str(&read(&repo_path("test/fixtures/html-probes.json")))
        .expect("probe 清单该是合法 JSON");
    let expected_path = repo_path("test/fixtures/html-probes.expected.json");

    let actual = run_all(&spec, &repo_path("test/fixtures"));

    if !expected_path.exists() {
        // 期望值要在有 cheerio 的环境生成（本机没有 node_modules，走 CI 的 html-probes.yml）。
        // 这里刻意「跳过 + 醒目警告」而不是失败，避免离线 CI 因为还没生成真值而红。
        // 但它不是长期状态：生成后应把 html-probes.expected.json 提交进仓库。
        eprintln!(
            "⚠️ 跳过选择器等价性验证：{} 还不存在。\n\
             生成方式：在 CI 上跑 `.github/workflows/html-probes.yml`（手动触发），\n\
             把日志里的 JSON 存成该文件并提交。\n\
             本次 Rust 侧实际跑了 {} 个 fixture，仅打印摘要：{}",
            expected_path.display(),
            actual.as_object().map_or(0, Map::len),
            summarize(&actual)
        );
        return;
    }

    let expected: Value = serde_json::from_str(&read(&expected_path)).expect("期望值该是合法 JSON");

    if actual != expected {
        let diffs = diff(&expected, &actual);
        eprintln!(
            "scraper 与 cheerio 的差异（共 {} 条）：\n{}",
            diffs.len(),
            diffs.join("\n")
        );
        panic!(
            "scraper 的选择器/取值语义与 cheerio 不一致 —— 移植 HTML provider 之前必须搞清楚这 {} 条差异",
            diffs.len()
        );
    }

    let total: usize = actual
        .as_object()
        .map(|m| m.values().filter_map(Value::as_object).map(Map::len).sum())
        .unwrap_or(0);
    println!("✅ {} 条 probe 与 cheerio 完全一致", total);
}

fn summarize(v: &Value) -> String {
    let mut parts = Vec::new();
    for (file, probes) in v.as_object().into_iter().flatten() {
        let n = probes.as_object().map_or(0, Map::len);
        parts.push(format!("{file}({n})"));
    }
    parts.join(", ")
}

/// 逐条找出差异，返回可读的 "文件 / probe / 期望 / 实际" 列表。
fn diff(expected: &Value, actual: &Value) -> Vec<String> {
    let mut out = Vec::new();

    let empty = Map::new();
    let exp_files = expected.as_object().unwrap_or(&empty);
    let act_files = actual.as_object().unwrap_or(&empty);

    for (file, exp_probes) in exp_files {
        let Some(act_probes) = act_files.get(file).and_then(Value::as_object) else {
            out.push(format!("{file}: Rust 侧完全没有这个 fixture"));
            continue;
        };
        let exp_probes = exp_probes.as_object().unwrap_or(&empty);
        for (id, exp) in exp_probes {
            match act_probes.get(id) {
                None => out.push(format!("{file} / {id}: Rust 侧没有这个 probe")),
                Some(act) if act != exp => out.push(format!(
                    "{file} / {id}:\n    期望(cheerio) = {exp}\n    实际(scraper) = {act}"
                )),
                Some(_) => {}
            }
        }
        for id in act_probes.keys() {
            if !exp_probes.contains_key(id) {
                out.push(format!("{file} / {id}: 期望值里没有这个 probe"));
            }
        }
    }

    out
}
