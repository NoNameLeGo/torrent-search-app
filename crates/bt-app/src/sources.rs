//! **数据源统一注册表** —— `%APPDATA%\bt-search\sources.json` 的读写与迁移。
//!
//! 设计见 `docs/SOURCES-DESIGN.md`。这里只做**批次 1**：schema + 读写 + 旧文件迁移 +
//! 内置项生成。**还没接线**：搜索仍然走编译期的 `registry()` / `video::registry()`，
//! 所以本模块目前不改变任何搜索行为。
//!
//! ## 为什么先落库再接 UI
//!
//! 与 `tvbox` / `cardigann` 同一个节奏：先把**数据形状**钉死并测住，再接 UI。
//! 反过来的话，schema 一改就要连带改界面与接线，返工成本高得多。
//!
//! ## `adapter` 是唯一分叉点
//!
//! 「怎么取数」全部收敛到一个枚举：`builtin` / `maccms_json` / `html_rule` /
//! `cardigann_catalog` / `torznab`。加一种取数方式 = 加一个枚举值 + 一个实现，
//! **不再动 registry**。
//!
//! ## 熔断状态不在这里
//!
//! `enabled` 是**用户意图**，熔断是**运行期事实**。后者复用 `health.jsonl`
//! （key 就用 `source.id`），配置文件保持「用户手改友好」—— 手改 `sources.json`
//! 不该跟一份程序频繁重写的状态打架。

// ⚠️ 批次 1 刻意 allow：`HtmlRule` / `CardigannCatalog` 这些适配器与
// `of_kind` / `disabled_ids` 要等批次 2/3 才有人调用（现在搜索仍走编译期 registry）。
// 这不是"忘了删"，是**先钉住数据形状**；等接线完成后这个 allow 应该能去掉。
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// 当前 schema 版本。将来不兼容改动时递增，并在 `from_text` 里做升级分支。
pub const SCHEMA_VERSION: u32 = 1;

/// 这个源进哪个模式（顶栏三模式里只有两个是「搜索源」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Video,
    Bt,
}

/// 「怎么取数」—— 唯一分叉点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Adapter {
    /// 编译期写死的实现（28 家 BT provider + 5 家 MacCMS 站）。
    /// 列表里能看到、能关，**不能删**。
    Builtin,
    /// 苹果CMS JSON 接口（`?ac=videolist&wd=`）。
    MaccmsJson,
    /// 用户自写规则（Cardigann 语法 + filter 白名单）。
    HtmlRule,
    /// 目录型：一条记录带 N 个子站（Prowlarr / TVBox 单仓展开）。
    CardigannCatalog,
    /// Torznab / Jackett / Prowlarr 的个人索引器（要 URL + API key）。
    Torznab,
}

/// 目录型源里的一个子站。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 站点语言标签（`zh-CN` / `ja` / `en-US` …）。导入时用来决定**默认开哪些**。
    #[serde(default)]
    pub language: String,
}

/// 一条数据源。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceEntry {
    /// 全局唯一。内置项用 `builtin:` 前缀，用户自建的用 `user:`，规则用 `rule:`。
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub adapter: Adapter,
    /// 内置项：不可删，`enabled` 可改。
    #[serde(default)]
    pub builtin: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub group: Option<String>,
    /// 取数地址（`maccms_json` 的 api、`cardigann_catalog` 的清单 URL、`torznab` 的地址）。
    #[serde(default)]
    pub url: Option<String>,
    /// 允许覆盖同 id 的手写实现（D5）。只有当 `adapter != Builtin` 时才有意义。
    #[serde(default)]
    pub r#override: bool,
    /// 用户规则体。批次 3 才会解析；现在原样保存，绝不因为看不懂就丢掉。
    #[serde(default)]
    pub rule: Option<serde_json::Value>,
    /// `cardigann_catalog` 的子站。
    #[serde(default)]
    pub members: Vec<Member>,
}

/// 一条订阅（返回一份规则/源清单的 URL）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    /// `tvbox` / `libretv` / `prowlarr` / `bare` —— 决定用哪个导入器。
    #[serde(default)]
    pub format: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 上次同步时间（unix 秒，0 = 没同步过）。批次 4 才用。
    #[serde(default)]
    pub last_sync: i64,
}

/// `sources.json` 的整体形状。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcesFile {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub sources: Vec<SourceEntry>,
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
}

impl Default for SourcesFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sources: Vec::new(),
            subscriptions: Vec::new(),
        }
    }
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

fn default_true() -> bool {
    true
}

impl SourceEntry {
    /// 熔断判定：这条源**最近连续空手**次数是否已达阈值。
    ///
    /// ⚠️ 阈值直接用 `health::STALE_AFTER`（3）—— 与磁力侧 chip 的「⚠连空 N 次」
    /// 是同一个数，别在两处各写一份（改一处、另一处悄悄不一致是最难查的）。
    ///
    /// ⚠️ 熔断是**运行期临时停用**，不改 `enabled`：用户没关它，是它自己坏了；
    /// 数据好转后会自动回来。
    pub fn triggers_breaker(&self, tail_empty: usize) -> bool {
        tail_empty >= crate::health::STALE_AFTER
    }
}

impl SourcesFile {
    /// 按 id 找一条。
    pub fn get(&self, id: &str) -> Option<&SourceEntry> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// 某个模式下的源（按 `kind` 过滤，保持原顺序 = UI 显示顺序）。
    pub fn of_kind(&self, kind: Kind) -> Vec<&SourceEntry> {
        self.sources.iter().filter(|s| s.kind == kind).collect()
    }

    /// 用户关掉的源 id（搜索路径要用的视图）。
    pub fn disabled_ids(&self) -> Vec<&str> {
        self.sources
            .iter()
            .filter(|s| !s.enabled)
            .map(|s| s.id.as_str())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 解析与序列化
// ---------------------------------------------------------------------------

/// 从文本解析。**纯函数**，所以能离线单测。
///
/// 失败的两种形态都给机器码（`bad_json:` / `unsupported_schema:`），
/// 与 `tvbox::parse` 同一套口径：调用方把它们翻成人话。
pub fn from_text(text: &str) -> Result<SourcesFile, String> {
    let file: SourcesFile = serde_json::from_str(text).map_err(|e| format!("bad_json: {e}"))?;
    if file.schema_version > SCHEMA_VERSION {
        // 未来的版本可能有不认识的字段语义 —— 不猜，直接拒绝（和"没配置过"区分开）
        return Err(format!(
            "unsupported_schema: {} > {}",
            file.schema_version, SCHEMA_VERSION
        ));
    }
    Ok(file)
}

/// 序列化（pretty，便于用户手改）。
pub fn to_text(file: &SourcesFile) -> Result<String, String> {
    serde_json::to_string_pretty(file).map_err(|e| format!("serialize_failed: {e}"))
}

// ---------------------------------------------------------------------------
// 内置项
// ---------------------------------------------------------------------------

/// 由编译期注册表生成内置条目。
///
/// ⚠️ 这里是**唯一**知道「哪些是内置的」的地方 —— 将来某家 provider 被删掉，
/// [`ensure_builtins`] 会把对应条目一并清掉，不会留一条永远搜不到的幽灵。
pub fn builtin_entries() -> Vec<SourceEntry> {
    let mut out = Vec::new();

    // ⚠️ `bt_providers::registry` 既是**模块名**也是里面有同名函数 ——
    // 必须写全 `registry::registry()`，否则解析成模块，编译报 "expected function"。
    for p in bt_providers::registry::registry() {
        out.push(SourceEntry {
            id: format!("builtin:{}", p.id()),
            name: p.name().to_string(),
            kind: Kind::Bt,
            adapter: Adapter::Builtin,
            builtin: true,
            enabled: true,
            group: Some("内置引擎".to_string()),
            url: None,
            r#override: false,
            rule: None,
            members: Vec::new(),
        });
    }

    for v in bt_providers::video::registry() {
        out.push(SourceEntry {
            id: format!("builtin:{}", v.id()),
            name: v.name().to_string(),
            kind: Kind::Video,
            adapter: Adapter::Builtin,
            builtin: true,
            enabled: true,
            group: Some("内置影视站".to_string()),
            url: None,
            r#override: false,
            rule: None,
            members: Vec::new(),
        });
    }

    out
}

/// 把内置项与文件内容对齐：
///
/// - 缺的补上（默认 `enabled = true`）；
/// - 文件里 `builtin: true` 但**注册表里已不存在**的删掉（provider 被移除/改名）；
/// - 已存在的**保留用户的 `enabled`**（这是用户唯一的可改项）。
///
/// 返回是否发生了改动（调用方据此决定要不要落盘）。
pub fn ensure_builtins(file: &mut SourcesFile) -> bool {
    let builtins = builtin_entries();
    let live: Vec<&str> = builtins.iter().map(|b| b.id.as_str()).collect();
    let mut changed = false;

    // 1) 清掉已消失的内置项
    let before = file.sources.len();
    file.sources
        .retain(|s| !s.builtin || live.contains(&s.id.as_str()));
    if file.sources.len() != before {
        changed = true;
    }

    // 2) 补上缺的
    for b in builtins {
        if file.get(&b.id).is_none() {
            file.sources.push(b);
            changed = true;
        }
    }

    changed
}

// ---------------------------------------------------------------------------
// 旧配置迁移（D10）
// ---------------------------------------------------------------------------

/// 把 `videosources.json`（TVBox 导入的影视源）与 `indexers.json`（Torznab）并进文件。
///
/// 规则：
/// - **同 id 已存在就不动**（用户可能已经改过；迁移只补缺，不覆盖）；
/// - 已存在的 `builtin: true` 条目**绝不碰**；
/// - 返回新增条数（0 = 没什么可迁的）。
pub fn merge_legacy(
    file: &mut SourcesFile,
    videos: &[bt_providers::maccms_json::JsonApiSite],
    indexers: &[bt_torznab::IndexerConfig],
) -> usize {
    let mut added = 0;

    for v in videos {
        if file.get(&v.id).is_some() {
            continue;
        }
        file.sources.push(SourceEntry {
            id: v.id.clone(),
            name: v.name.clone(),
            kind: Kind::Video,
            adapter: Adapter::MaccmsJson,
            builtin: false,
            enabled: true,
            group: Some("导入".to_string()),
            url: Some(v.api.clone()),
            r#override: false,
            rule: None,
            members: Vec::new(),
        });
        added += 1;
    }

    for ix in indexers {
        // 索引器的 id 可能是空串（由 `TorznabProvider::new` 兜底成 `torznab:<name>`），
        // 这里沿用同一个兜底，保证与搜索路径上的 id 对得上。
        let id = if ix.id.trim().is_empty() {
            format!("torznab:{}", ix.name)
        } else {
            ix.id.clone()
        };
        if file.get(&id).is_some() {
            continue;
        }
        file.sources.push(SourceEntry {
            id,
            name: ix.name.clone(),
            kind: Kind::Bt,
            adapter: Adapter::Torznab,
            builtin: false,
            // `indexers.json` 里有 `enabled` 字段，照搬（关掉的索引器不该在迁移时被打开）
            enabled: ix.enabled,
            group: Some("索引器".to_string()),
            url: Some(ix.url.clone()),
            r#override: false,
            rule: None,
            members: Vec::new(),
        });
        added += 1;
    }

    added
}

// ---------------------------------------------------------------------------
// 摘要
// ---------------------------------------------------------------------------

/// 设置面板那一行文字。**纯函数**，有单测。
pub fn describe(file: &SourcesFile) -> String {
    if file.sources.is_empty() {
        return "数据源：空（点「导入」加源）".to_string();
    }

    let total = file.sources.len();
    let builtin = file.sources.iter().filter(|s| s.builtin).count();
    let imported = total - builtin;
    let off = file.sources.iter().filter(|s| !s.enabled).count();
    let subs = file.subscriptions.len();

    let mut parts = vec![format!("内置 {builtin}")];
    if imported > 0 {
        parts.push(format!("导入 {imported}"));
    }
    if subs > 0 {
        parts.push(format!("订阅 {subs}"));
    }
    let mut text = format!("数据源 {total} 条（{}）", parts.join(" · "));
    if off > 0 {
        text.push_str(&format!("，{off} 条已关"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, adapter: Adapter, enabled: bool) -> SourceEntry {
        SourceEntry {
            id: id.to_string(),
            name: id.to_string(),
            kind: Kind::Video,
            adapter,
            builtin: false,
            enabled,
            group: None,
            url: None,
            r#override: false,
            rule: None,
            members: Vec::new(),
        }
    }

    fn builtin(id: &str, enabled: bool) -> SourceEntry {
        SourceEntry {
            builtin: true,
            ..entry(id, Adapter::Builtin, enabled)
        }
    }

    #[test]
    fn round_trips_through_json() {
        let file = SourcesFile {
            schema_version: SCHEMA_VERSION,
            sources: vec![entry("user:a", Adapter::MaccmsJson, true)],
            subscriptions: vec![Subscription {
                id: "sub:a".to_string(),
                name: "A".to_string(),
                url: "https://a/sources.json".to_string(),
                format: "libretv".to_string(),
                enabled: true,
                last_sync: 0,
            }],
        };
        let text = to_text(&file).unwrap();
        assert_eq!(from_text(&text).unwrap(), file);
    }

    #[test]
    fn rejects_a_future_schema_instead_of_guessing() {
        let err = from_text(r#"{"schema_version": 99, "sources": []}"#).unwrap_err();
        assert!(err.starts_with("unsupported_schema"), "{err}");
        assert!(from_text("not json").unwrap_err().starts_with("bad_json"));
    }

    #[test]
    fn missing_fields_get_safe_defaults() {
        // 手写文件常常只写最少的字段 —— 不能因此整份作废
        let file =
            from_text(r#"{"sources":[{"id":"x","name":"X","kind":"bt","adapter":"builtin"}]}"#)
                .unwrap();
        assert_eq!(file.schema_version, SCHEMA_VERSION);
        assert!(file.sources[0].enabled, "默认启用");
        assert!(!file.sources[0].builtin);
        assert!(file.subscriptions.is_empty());
    }

    #[test]
    fn ensure_builtins_adds_missing_and_keeps_user_choice() {
        let mut file = SourcesFile {
            sources: vec![
                // 用户关掉了一个真实存在的内置项
                builtin("builtin:dmhy", false),
                // 一个已经被删掉的 provider
                builtin("builtin:gone-forever", true),
            ],
            ..Default::default()
        };

        assert!(ensure_builtins(&mut file));

        // 关掉的仍然关着（用户意图不能被"补齐"覆盖）
        let dmhy = file.get("builtin:dmhy").expect("内置项应存在");
        assert!(!dmhy.enabled);
        // 幽灵项被清掉
        assert!(file.get("builtin:gone-forever").is_none());
        // 其余内置项补了进来
        assert!(file.sources.len() > 2);

        // 再跑一次不该有改动（幂等）
        assert!(!ensure_builtins(&mut file));
    }

    #[test]
    fn merge_legacy_adds_only_missing_and_never_touches_builtins() {
        let mut file = SourcesFile {
            sources: vec![
                builtin("builtin:dmhy", true),
                entry("user:keep", Adapter::MaccmsJson, true),
            ],
            ..Default::default()
        };

        let videos = vec![
            bt_providers::maccms_json::JsonApiSite {
                id: "user:keep".to_string(),
                name: "改过名字".to_string(),
                api: "https://keep/vod/".to_string(),
            },
            bt_providers::maccms_json::JsonApiSite {
                id: "tvbox:new".to_string(),
                name: "新导入".to_string(),
                api: "https://new/api.php/provide/vod/".to_string(),
            },
        ];
        let indexers = vec![bt_torznab::IndexerConfig {
            id: String::new(),
            name: "Jackett".to_string(),
            url: "http://localhost:9117".to_string(),
            api_key: "k".to_string(),
            enabled: false,
        }];

        let added = merge_legacy(&mut file, &videos, &indexers);
        assert_eq!(added, 2, "user:keep 已存在不该重复加");

        // 已存在的条目一个字段都没被改
        let keep = file.get("user:keep").unwrap();
        assert_eq!(keep.name, "user:keep");

        let new = file.get("tvbox:new").unwrap();
        assert_eq!(new.adapter, Adapter::MaccmsJson);
        assert_eq!(new.kind, Kind::Video);
        assert_eq!(new.url.as_deref(), Some("https://new/api.php/provide/vod/"));

        // 索引器 id 兜底成 torznab:<name>，且 `enabled: false` 要照搬
        let ix = file.get("torznab:Jackett").unwrap();
        assert_eq!(ix.adapter, Adapter::Torznab);
        assert_eq!(ix.kind, Kind::Bt);
        assert!(!ix.enabled, "关掉的索引器不该在迁移时被打开");

        // 幂等
        assert_eq!(merge_legacy(&mut file, &videos, &indexers), 0);
    }

    #[test]
    fn describe_counts_builtin_imported_and_subscriptions() {
        assert_eq!(
            describe(&SourcesFile::default()),
            "数据源：空（点「导入」加源）"
        );

        let file = SourcesFile {
            sources: vec![
                builtin("builtin:a", true),
                builtin("builtin:b", true),
                entry("user:c", Adapter::MaccmsJson, false),
            ],
            subscriptions: vec![Subscription {
                id: "sub:x".to_string(),
                name: "X".to_string(),
                url: "https://x/".to_string(),
                format: "tvbox".to_string(),
                enabled: true,
                last_sync: 0,
            }],
            ..Default::default()
        };
        let text = describe(&file);
        assert!(text.contains("内置 2"), "{text}");
        assert!(text.contains("导入 1"), "{text}");
        assert!(text.contains("订阅 1"), "{text}");
        assert!(text.contains("1 条已关"), "{text}");
    }

    #[test]
    fn breaker_trips_at_the_same_threshold_as_the_chip() {
        let e = entry("rule:x", Adapter::HtmlRule, true);
        // 阈值来自 health::STALE_AFTER —— 2 次不算、3 次算
        assert!(!e.triggers_breaker(0));
        assert!(!e.triggers_breaker(crate::health::STALE_AFTER - 1));
        assert!(e.triggers_breaker(crate::health::STALE_AFTER));
        assert!(e.triggers_breaker(99));
    }

    #[test]
    fn of_kind_and_disabled_views() {
        let file = SourcesFile {
            sources: vec![
                builtin("builtin:a", true),
                entry("user:b", Adapter::MaccmsJson, false),
            ],
            ..Default::default()
        };
        assert_eq!(file.of_kind(Kind::Video).len(), 2, "两条都是 video");
        assert_eq!(file.of_kind(Kind::Bt).len(), 0);
        assert_eq!(file.disabled_ids(), vec!["user:b"]);
    }
}

// ---------------------------------------------------------------------------
// 读写（IO 层，放最后 —— 上面全是纯函数，单测不碰盘）
// ---------------------------------------------------------------------------

/// 从目录 URL 里取 schema 版本号（`…/master/12` → `12`）。
///
/// 取不到就退回 `"12"` —— 这个值只在拼「单个定义的 URL」时用，
/// 万一 Prowlarr 升到 13 而我们读不出，最坏情况是那一轮全部 404，状态里会显示出来。
pub fn catalog_version(url: &str) -> String {
    let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
        last.to_string()
    } else {
        "12".to_string()
    }
}

/// 目录里**已经启用**的成员。
pub fn enabled_members(entry: &SourceEntry) -> Vec<&Member> {
    entry.members.iter().filter(|m| m.enabled).collect()
}

/// 订阅那一行的摘要。
pub fn describe_subs(file: &SourcesFile) -> String {
    match file.subscriptions.len() {
        0 => "无订阅".to_string(),
        1 => format!("1 个订阅：{}", file.subscriptions[0].name),
        n => format!("{n} 个订阅"),
    }
}

/// 把「刷新回来的一批条目」并进文件（D8：**只换定义，保留用户的开关**）。
///
/// 规则：
/// - 同一 `id` 已存在 ⇒ 只更新 `name` / `url`（以及**原先没有**时的 `rule`），
///   `enabled` 一个字不动 —— 用户关掉的源不能因为一次刷新又自己打开；
/// - 用户手改过的 `rule` **不覆盖**（订阅里带了也不覆盖，只在原本为空时补上）；
/// - 不存在的 ⇒ 新增。
///
/// 返回 `(新增, 更新)`。
pub fn merge_refreshed(file: &mut SourcesFile, entries: &[SourceEntry]) -> (usize, usize) {
    let mut added = 0;
    let mut updated = 0;

    for e in entries {
        match file.sources.iter_mut().find(|s| s.id == e.id) {
            Some(existing) => {
                let mut changed = false;
                if existing.url != e.url {
                    existing.url = e.url.clone();
                    changed = true;
                }
                if existing.name != e.name {
                    existing.name = e.name.clone();
                    changed = true;
                }
                // 只在「原本没有」时补规则/成员：有内容就是用户的，不动
                if existing.rule.is_none() && e.rule.is_some() {
                    existing.rule = e.rule.clone();
                    changed = true;
                }
                if existing.members.is_empty() && !e.members.is_empty() {
                    existing.members = e.members.clone();
                    changed = true;
                }
                if changed {
                    updated += 1;
                }
            }
            None => {
                file.sources.push(e.clone());
                added += 1;
            }
        }
    }

    (added, updated)
}

/// `%APPDATA%\bt-search\sources.json`（与其它配置文件同目录）。
pub fn path() -> Option<std::path::PathBuf> {
    Some(crate::config_store::data_dir()?.join("sources.json"))
}

/// 读盘 → 补齐内置项 → 迁移旧配置。返回 `(文件, 是否需要落盘)`。
///
/// **任何一步失败都只是「按默认继续」**：配置文件坏了不该让应用起不来，
/// 也不该拦搜索（与 `config_store` / `health` 同一条规矩）。
pub fn load() -> (SourcesFile, bool) {
    let mut changed = false;

    let mut file = match path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => match from_text(&text) {
            Ok(f) => f,
            // 坏文件当「没配置过」—— 但**要落盘一次**把内置项写回去，
            // 否则下次启动还是同一个坏文件（用户看不到任何可改的东西）
            Err(_) => {
                changed = true;
                SourcesFile::default()
            }
        },
        None => {
            changed = true;
            SourcesFile::default()
        }
    };

    if ensure_builtins(&mut file) {
        changed = true;
    }

    // 旧配置迁移（D10）：`videosources.json` 与 `indexers.json` 只补缺，不覆盖
    let videos = crate::config_store::load_video_sources();
    let indexers = crate::config_store::load_indexers();
    if merge_legacy(&mut file, &videos, &indexers) > 0 {
        changed = true;
    }

    (file, changed)
}

/// 落盘（原子写，同 `config_store`）。
// ---------------------------------------------------------------------------
// Cardigann 定义缓存（目录成员按需下载，落到 %APPDATA%/bt-search/cardigann/）
// ---------------------------------------------------------------------------

/// 成员 id 是否安全（**来自远程 JSON，必须校验**）。
///
/// 不做这件事，一个 `"id": "../../evil"` 就能把文件写到配置目录之外。
pub fn safe_member_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !id.contains("..")
}

fn definition_path(ver: &str, id: &str) -> Option<std::path::PathBuf> {
    if !safe_member_id(id) || !safe_member_id(ver) {
        return None;
    }
    Some(
        crate::config_store::data_dir()?
            .join("cardigann")
            .join(format!("{ver}__{id}.yml")),
    )
}

/// 读缓存里的定义文本（没有就是 `None`）。
pub fn cached_definition(ver: &str, id: &str) -> Option<String> {
    let text = std::fs::read_to_string(definition_path(ver, id)?).ok()?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 写入缓存（原子写）。
pub fn save_definition(ver: &str, id: &str, text: &str) -> Result<(), String> {
    let p =
        definition_path(ver, id).ok_or_else(|| "member id unsafe, refuse to write".to_string())?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir failed: {e}"))?;
    }
    crate::config_store::write_atomic(&p, text).map_err(|e| format!("write failed: {e}"))
}

pub fn save(file: &SourcesFile) -> Result<(), String> {
    let p = path().ok_or_else(|| "取不到 %APPDATA%，数据源没保存".to_string())?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let text = to_text(file)?;
    crate::config_store::write_atomic(&p, &text).map_err(|e| format!("写入失败：{e}"))
}

/// 从注册表里取出**可搜索的** JSON 影视源（`enabled` 且 `adapter == maccms_json`）。
///
/// 这是批次 2 的接点：视频搜索不再直接读 `videosources.json`，而是读这份统一注册表
/// —— 于是「关掉一个源」**真的**会让它不参与搜索。
///
/// 非 http(s) 的地址一律丢掉（手改文件不该把 `file://` 带进搜索路径）。
pub fn video_json_sites(file: &SourcesFile) -> Vec<bt_providers::maccms_json::JsonApiSite> {
    file.sources
        .iter()
        .filter(|s| s.enabled && s.kind == Kind::Video && s.adapter == Adapter::MaccmsJson)
        .filter_map(|s| {
            let api = s.url.as_deref()?.trim();
            if !(api.starts_with("http://") || api.starts_with("https://")) {
                return None;
            }
            Some(bt_providers::maccms_json::JsonApiSite {
                id: s.id.clone(),
                name: s.name.clone(),
                api: api.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod video_view_tests {
    use super::*;

    /// 本地版（不能借 `mod tests` 里的同名 helper —— 兄弟模块之间互相看不见）
    fn builtin(id: &str, enabled: bool) -> SourceEntry {
        SourceEntry {
            builtin: true,
            adapter: Adapter::Builtin,
            ..json_src(id, enabled, None)
        }
    }

    fn json_src(id: &str, enabled: bool, url: Option<&str>) -> SourceEntry {
        SourceEntry {
            id: id.to_string(),
            name: id.to_string(),
            kind: Kind::Video,
            adapter: Adapter::MaccmsJson,
            builtin: false,
            enabled,
            group: None,
            url: url.map(str::to_string),
            r#override: false,
            rule: None,
            members: Vec::new(),
        }
    }

    #[test]
    fn video_json_sites_respects_enabled_kind_and_scheme() {
        let file = SourcesFile {
            sources: vec![
                json_src("user:a", true, Some("https://a/vod/")),
                json_src("user:off", false, Some("https://off/vod/")),
                json_src("user:ftp", true, Some("ftp://bad/")),
                json_src("user:nourl", true, None),
                builtin("builtin:appmovie", true),
                SourceEntry {
                    // BT 源不该混进视频视图
                    kind: Kind::Bt,
                    ..json_src("catalog:prowlarr", true, Some("https://p/indexers"))
                },
            ],
            ..Default::default()
        };

        let sites = video_json_sites(&file);
        assert_eq!(sites.len(), 1, "{sites:?}");
        assert_eq!(sites[0].id, "user:a");
        assert_eq!(sites[0].api, "https://a/vod/");
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    fn e(id: &str, name: &str, url: &str, enabled: bool) -> SourceEntry {
        SourceEntry {
            id: id.to_string(),
            name: name.to_string(),
            kind: Kind::Video,
            adapter: Adapter::MaccmsJson,
            builtin: false,
            enabled,
            group: None,
            url: Some(url.to_string()),
            r#override: false,
            rule: None,
            members: Vec::new(),
        }
    }

    #[test]
    fn refresh_updates_definitions_but_never_the_users_switch() {
        let mut file = SourcesFile {
            sources: vec![e("url:a.example", "旧名", "https://a/old/", false)],
            ..Default::default()
        };

        let (added, updated) = merge_refreshed(
            &mut file,
            &[
                e("url:a.example", "新名", "https://a/new/", true),
                e("url:b.example", "B", "https://b/", true),
            ],
        );

        assert_eq!((added, updated), (1, 1));
        let a = file.get("url:a.example").unwrap();
        assert_eq!(a.name, "新名", "订阅说了算的是定义");
        assert_eq!(a.url.as_deref(), Some("https://a/new/"));
        // 关键：用户关掉的仍然是关着的（订阅里写 enabled:true 也不算数）
        assert!(!a.enabled);
    }

    #[test]
    fn refresh_does_not_clobber_a_hand_edited_rule() {
        let mut mine = e("rule:x", "规则", "https://x/", true);
        mine.rule = Some(serde_json::json!({"base": "https://mine/"}));
        let mut file = SourcesFile {
            sources: vec![mine],
            ..Default::default()
        };

        let mut incoming = e("rule:x", "规则", "https://x/", true);
        incoming.rule = Some(serde_json::json!({"base": "https://theirs/"}));
        let (_, updated) = merge_refreshed(&mut file, &[incoming]);

        assert_eq!(updated, 0, "没变化就不算更新");
        assert_eq!(
            file.get("rule:x").unwrap().rule.as_ref().unwrap()["base"],
            "https://mine/"
        );
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;

    #[test]
    fn catalog_version_reads_the_trailing_number() {
        assert_eq!(
            catalog_version("https://indexers.prowlarr.com/master/12"),
            "12"
        );
        assert_eq!(
            catalog_version("https://indexers.prowlarr.com/master/11/"),
            "11"
        );
        // 读不出来时退回 12（拼 URL 用，最坏是 404，会在状态里暴露）
        assert_eq!(catalog_version("https://indexers.prowlarr.com/"), "12");
        assert_eq!(catalog_version(""), "12");
    }

    #[test]
    fn only_enabled_members_are_returned() {
        let e = SourceEntry {
            id: "catalog:prowlarr".to_string(),
            name: "目录".to_string(),
            kind: Kind::Bt,
            adapter: Adapter::CardigannCatalog,
            builtin: false,
            enabled: true,
            group: None,
            url: Some("https://indexers.prowlarr.com/master/12".to_string()),
            r#override: false,
            rule: None,
            members: vec![
                Member {
                    id: "a".into(),
                    name: "A".into(),
                    enabled: true,
                    language: "zh-CN".into(),
                },
                Member {
                    id: "b".into(),
                    name: "B".into(),
                    enabled: false,
                    language: "en-US".into(),
                },
            ],
        };
        assert_eq!(enabled_members(&e).len(), 1);
        assert_eq!(enabled_members(&e)[0].id, "a");
    }
}
