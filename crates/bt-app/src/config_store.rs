//! 下载器配置的持久化（2026-10-16 补，遗留清单里的 ②）。
//!
//! ## 为什么需要它
//!
//! M4 的下载器配置原先只活在内存里（`STATE.downloader`），**重启就没了** ——
//! 每次开应用都要重填地址 / 用户名 / 密码。Node 版有配置文件，Rust 版一直缺这一块。
//!
//! ## 放在哪
//!
//! `%APPDATA%\bt-search\config.json`。**刻意不放在 exe 旁边**：
//! exe 可能被放进 `Program Files`（不可写）或从 U 盘 / 只读目录运行，
//! 而「单文件自包含、拷到哪都能跑」正是这个产品的卖点（D7 / S2 / M7）。
//!
//! ## ⚠️ 明文存密码 / token
//!
//! 与 Node 版同口径（`data/torznab.json` 里也是明文 API key）：这是用户自己机器上的
//! 自用工具（D10）。加一层"加密"只会给出**虚假的安全感** —— 密钥还是得放在同一台
//! 机器上。写在这里是为了**别让人以为它是加密的**。
//!
//! ## 失败一律不致命
//!
//! 读不到 / 解析不了 ⇒ 当作"没配置过"（用户重填一次即可）；写不进去 ⇒ 返回一句人话，
//! 调用方显示在状态栏，**本次会话照常可用**。绝不 panic ——
//! 一个配置文件不该让应用起不来。所以这里**通篇没有 `unwrap`**：
//! 文件可能被手改坏、目录可能没权限。
//!
//! ## 为什么 `Stored` 不复用 `bt_downloaders::Config`
//!
//! 那是**引擎层**的类型，不该知道"怎么存"。这里是**应用层的文件格式** ——
//! 将来格式要加字段 / 做迁移，不该去动引擎层（也不该动引擎层的公开 API）。

use std::path::{Path, PathBuf};

use bt_downloaders::{Config, Kind};
use serde::{Deserialize, Serialize};

/// 落盘的结构。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    /// `Kind::as_str()` 的值（`qbittorrent` / `transmission` / `aria2` / `gopeed`）
    pub kind: String,
    pub url: String,
    // ⚠️ `default`：手写 / 旧版配置文件少字段时**不该整份作废**。
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub pass: String,
    #[serde(default)]
    pub token: String,
}

impl Stored {
    pub fn from_parts(kind: Kind, c: &Config) -> Self {
        Self {
            kind: kind.as_str().to_string(),
            url: c.url.clone(),
            user: c.user.clone(),
            pass: c.pass.clone(),
            token: c.token.clone(),
        }
    }

    /// id 认不出来 ⇒ `None`（**不猜**一个默认下载器：配置文件被改坏时，
    /// 假装"没配置过"比悄悄改成 qBittorrent 更安全 —— 后者会把磁力推给一个用户没选过的客户端）。
    pub fn into_parts(self) -> Option<(Kind, Config)> {
        let kind = Kind::parse(&self.kind)?;
        Some((
            kind,
            Config {
                url: self.url,
                user: self.user,
                pass: self.pass,
                token: self.token,
            },
        ))
    }
}

/// 数据目录：`BT_SEARCH_HOME` 优先，否则 `%APPDATA%\bt-search`。
///
/// ⚠️ **只有测试和 `--selftest` 会用那个环境变量** —— 它存在的理由很具体：
/// `--selftest` 走的是**生产路径**（`save_downloader` / `health::append`），
/// 于是它会把桩下载器地址（`http://127.0.0.1:<随机端口>`）写进**用户真实的配置**，
/// 并把 alpha/beta/gamma 那三家假 provider 写进健康历史。
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("BT_SEARCH_HOME") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("bt-search"))
}

/// 写文件：**先写临时文件再 rename**。
///
/// ⚠️ `fs::write` 是**就地截断**的 —— 写入中途崩溃/断电就留下半截文件，
/// 而这里写的全是**用户配置**（丢了就得重填）。同一卷上的 `rename` 是原子的：
/// 要么旧内容、要么新内容；失败时旧文件原封不动。
///
/// 临时文件名带 pid 与序号：`--selftest` 与正常保存可能并发写同一个目标。
pub(crate) fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut tmp = path.to_path_buf().into_os_string();
    tmp.push(format!(
        ".tmp{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// `%APPDATA%\bt-search\config.json`。取不到 `APPDATA` 就 `None`（**不猜别的路径** ——
/// 猜出来的位置可能正是只读的那个）。
pub fn config_path() -> Option<PathBuf> {
    Some(data_dir()?.join("config.json"))
}

/// 读上次保存的配置。**任何一步失败都只是 `None`**，不区分"没保存过"与"读坏了" ——
/// 对调用方来说两者要做的事完全一样（让用户填）。
pub fn load() -> Option<(Kind, Config)> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    let stored: Stored = serde_json::from_str(&text).ok()?;
    stored.into_parts()
}

/// 保存。返回 `Err(人话)` 供状态栏显示 —— 调用方**不该**把它当成致命错误。
pub fn save(kind: Kind, c: &Config) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "取不到 %APPDATA%，配置没保存".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let text = serde_json::to_string_pretty(&Stored::from_parts(kind, c))
        .map_err(|e| format!("序列化失败：{e}"))?;
    write_atomic(&path, &text).map_err(|e| format!("写入失败：{e}"))
}

/// PanSou 的落盘形状（方案 A：外部服务）。
///
/// 单独一个 `pansou.json`，不塞进 `config.json` —— 与索引器同一个理由：
/// 下载器 / 搜索源是两件事，一个文件坏了不该连累另一个。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredPansou {
    /// 空 ⇒ 用默认 `http://127.0.0.1:8888`（调用方兜底）。
    #[serde(default)]
    pub url: String,
    /// 开了认证时的 Bearer token；**明文**，与下载器/索引器同口径。
    #[serde(default)]
    pub token: String,
}

/// `%APPDATA%\bt-search\pansou.json`。
pub fn pansou_path() -> Option<PathBuf> {
    Some(data_dir()?.join("pansou.json"))
}

// ---------------------------------------------------------------------------
// PanSou 多实例（2026-10-27）
// ---------------------------------------------------------------------------

/// 最多记几个实例。
///
/// ⚠️ 4 是拍的：PanSou 是**外部依赖**，多配几个是为了「主挂了还有备」，
/// 不是为了「配一堆然后并行搜」—— 并行会让同一个关键词在多个实例上重复搜，
/// 结果重复率高、还会放大对方的负载。
pub const MAX_PANSOU_INSTANCES: usize = 4;

/// 读**全部** PanSou 实例。兼容老格式（单个对象）与新格式（数组）。
///
/// **任何一步失败都只是空列表** —— 与其它配置文件同一口径，绝不 panic。
pub fn load_pansou_all() -> Vec<StoredPansou> {
    let Some(text) = pansou_path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Vec::new();
    };
    let head = text.trim_start();
    let raw: Vec<StoredPansou> = if head.starts_with('[') {
        serde_json::from_str(&text).unwrap_or_default()
    } else {
        // 老格式：单对象 `{"url":…,"token":…}`
        serde_json::from_str::<StoredPansou>(&text)
            .map(|one| vec![one])
            .unwrap_or_default()
    };
    raw.into_iter().filter_map(clean_pansou).collect()
}

/// 保存**全部**实例（整体覆盖）。
pub fn save_pansou_all(list: &[StoredPansou]) -> Result<(), String> {
    let path = pansou_path().ok_or_else(|| "取不到 %APPDATA%，配置没保存".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let text = serde_json::to_string_pretty(list).map_err(|e| format!("序列化失败：{e}"))?;
    write_atomic(&path, &text).map_err(|e| format!("写入失败：{e}"))
}

/// 把「设置里当前填的那一对」并进列表。
///
/// - 同 URL ⇒ **就地覆盖** token（用户多半是在改 token）；
/// - 新 URL ⇒ **插到最前**，成为主实例；
/// - 超过 [`MAX_PANSOU_INSTANCES`] 就丢尾部的（备用的备用意义不大）。
///
/// 地址不合法时**原样返回旧列表**（不让一次手滑把配置清空）。
pub fn upsert_pansou(list: &[StoredPansou], url: &str, token: &str) -> Vec<StoredPansou> {
    let Some(one) = clean_pansou(StoredPansou {
        url: url.to_string(),
        token: token.to_string(),
    }) else {
        return list.to_vec();
    };

    let mut out: Vec<StoredPansou> = list.to_vec();
    match out.iter().position(|p| p.url == one.url) {
        Some(pos) => out[pos] = one,
        None => out.insert(0, one),
    }
    out.truncate(MAX_PANSOU_INSTANCES);
    out
}

/// 地址归一 + 过滤：不是 http(s) 的整条丢掉（同 `indexers.json` 那条信任边界）。
fn clean_pansou(mut p: StoredPansou) -> Option<StoredPansou> {
    let url = p.url.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    p.url = url;
    p.token = p.token.trim().to_string();
    Some(p)
}

/// 状态栏的人话摘要。
pub fn describe_pansou(list: &[StoredPansou]) -> String {
    match list.len() {
        0 => "未配置网盘搜索（默认 127.0.0.1:8888）".to_string(),
        1 => format!("1 个实例：{}", list[0].url),
        n => format!("{n} 个实例（主：{}）", list[0].url),
    }
}

/// 一个 Torznab 索引器的落盘形状（⑤）。
///
/// ⚠️ 字段与 `bt_torznab::IndexerConfig` 一一对应，但**不直接复用**那个类型 ——
/// 与 `Stored` 不复用 `bt_downloaders::Config` 同一个理由：文件格式属于**应用层**，
/// 将来要加字段 / 做迁移，不该去动引擎层。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredIndexer {
    /// 空 ⇒ 由 `bt_torznab` 兜底成 `torznab:<name>`
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    /// ⚠️ **明文** API key —— 与 Node 版 `data/torznab.json` 同口径（见文件头）
    #[serde(default)]
    pub api_key: String,
    /// ⚠️ 默认 **true**：JS 的语义是 `enabled !== false`（"不是显式关掉"）。
    /// 手写配置时漏了这个字段，索引器**不该**因此变哑巴。
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// `%APPDATA%\bt-search\indexers.json` —— **单独一个文件**。
///
/// ⚠️ 为什么不塞进 `config.json`：那会把现有格式从"一个对象"改成"一个对象 + 一个数组"，
/// 而**旧文件是平铺的** ⇒ 得写兼容分支。单独一个文件零风险，而且
/// "下载器"与"搜索源"本来就是两件事（一个文件坏了不该连累另一个）。
pub fn indexers_path() -> Option<PathBuf> {
    Some(data_dir()?.join("indexers.json"))
}

impl StoredIndexer {
    pub fn from_config(c: &bt_torznab::IndexerConfig) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            url: c.url.clone(),
            api_key: c.api_key.clone(),
            enabled: c.enabled,
        }
    }

    /// ⚠️ **url 为空的条目直接丢掉**：没有 URL 的索引器没法搜，
    /// 留着只会在每次搜索时发一个必然失败的请求（JS 那边会是 `?apikey=…` 这种残 URL）。
    pub fn into_config(self) -> Option<bt_torznab::IndexerConfig> {
        if self.url.trim().is_empty() {
            return None;
        }
        Some(bt_torznab::IndexerConfig {
            id: self.id,
            name: self.name,
            url: self.url,
            api_key: self.api_key,
            enabled: self.enabled,
        })
    }
}

/// 读索引器配置。**任何一步失败都只是空列表**（"没配过"与"读坏了"对调用方是一回事）。
pub fn load_indexers() -> Vec<bt_torznab::IndexerConfig> {
    let Some(path) = indexers_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<StoredIndexer>>(&text) else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(StoredIndexer::into_config)
        .collect()
}

/// 一行人类可读的索引器摘要（UI 状态栏用）。**纯函数**，有单测。
///
/// 名字留空就用地址当代名 —— 这只是**给人看的**；id 由 `TorznabProvider::new`
/// 的兜底（`torznab:<name>`）决定，两者不是一回事。
pub fn describe(list: &[bt_torznab::IndexerConfig]) -> String {
    if list.is_empty() {
        return "未配置索引器（Torznab）".to_string();
    }
    let names: Vec<&str> = list
        .iter()
        .map(|c| {
            if c.name.is_empty() {
                c.url.as_str()
            } else {
                c.name.as_str()
            }
        })
        .collect();
    let off = list.iter().filter(|c| !c.enabled).count();
    if off == 0 {
        format!("已配置 {} 个：{}", names.len(), names.join("、"))
    } else {
        format!(
            "已配置 {} 个（{} 个已停用）：{}",
            names.len(),
            off,
            names.join("、")
        )
    }
}

/// 保存索引器配置。返回 `Err(人话)`，调用方**不该**当成致命错误。
pub fn save_indexers(list: &[bt_torznab::IndexerConfig]) -> Result<(), String> {
    let path = indexers_path().ok_or_else(|| "取不到 %APPDATA%，索引器没保存".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let stored: Vec<StoredIndexer> = list.iter().map(StoredIndexer::from_config).collect();
    let text = serde_json::to_string_pretty(&stored).map_err(|e| format!("序列化失败：{e}"))?;
    write_atomic(&path, &text).map_err(|e| format!("写入失败：{e}"))
}

// ---------------------------------------------------------------------------
// 影视源（TVBox 单仓导入，2026-10-27）
// ---------------------------------------------------------------------------

/// `%APPDATA%\bt-search\videosources.json` —— 用户导入的**在线影视 JSON 源**。
///
/// ## 为什么不塞进 `config.json` / `indexers.json`
///
/// 同 [`indexers_path`] 的理由：下载器、BT 索引器、影视源是三件独立的事，
/// 一个文件坏了不该连累另外两个。而且这份文件被**整体覆盖**（重新导入即刷新），
/// 单独放更好理解。
///
/// ## 为什么要存「解析结果」而不是「原始 TVBox 配置」
///
/// 原始配置动辄几百 KB、里面几百个站、地址天天变；而用户真正要的是
/// **那一刻筛出来的可用 JSON 源**。想刷新就再导入一次 —— 新结果整体覆盖旧的，
/// 不做增量合并（合并会让「这条是哪次导入来的」变得无从追溯）。
pub fn video_sources_path() -> Option<PathBuf> {
    Some(data_dir()?.join("videosources.json"))
}

/// 落盘结构。与 `bt_providers::maccms_json::JsonApiSite` 字段一一对应，
/// 但**刻意不复用它做序列化** —— 那是引擎层类型，文件格式的演进不该被它绑住
/// （同 [`Stored`] 与 `bt_downloaders::Config` 的关系）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVideoSource {
    pub id: String,
    pub name: String,
    pub api: String,
}

impl StoredVideoSource {
    fn from_site(s: &bt_providers::maccms_json::JsonApiSite) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            api: s.api.clone(),
        }
    }

    /// 只放行 http(s) 的 api，且 id 不能空 —— 手改坏的文件不该把
    /// `file://` / `ftp://` 带进搜索路径（同 `indexers.json` 那条信任边界）。
    fn into_site(self) -> Option<bt_providers::maccms_json::JsonApiSite> {
        let api = self.api.trim();
        if !(api.starts_with("http://") || api.starts_with("https://")) {
            return None;
        }
        let id = self.id.trim();
        if id.is_empty() {
            return None;
        }
        let name = if self.name.trim().is_empty() {
            api.to_string()
        } else {
            self.name.trim().to_string()
        };
        Some(bt_providers::maccms_json::JsonApiSite {
            id: id.to_string(),
            name,
            api: api.to_string(),
        })
    }
}

/// 读影视源。**任何一步失败都只是空列表**（"没导入过"与"读坏了"对调用方是一回事）。
pub fn load_video_sources() -> Vec<bt_providers::maccms_json::JsonApiSite> {
    let Some(path) = video_sources_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<StoredVideoSource>>(&text) else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(StoredVideoSource::into_site)
        .collect()
}

/// 保存影视源（整体覆盖）。返回 `Err(人话)`，调用方不该当成致命错误。
pub fn save_video_sources(list: &[bt_providers::maccms_json::JsonApiSite]) -> Result<(), String> {
    let path = video_sources_path().ok_or_else(|| "取不到 %APPDATA%，影视源没保存".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let stored: Vec<StoredVideoSource> = list.iter().map(StoredVideoSource::from_site).collect();
    let text = serde_json::to_string_pretty(&stored).map_err(|e| format!("序列化失败：{e}"))?;
    write_atomic(&path, &text).map_err(|e| format!("写入失败：{e}"))
}

/// 一行人类可读的影视源摘要（UI 状态文字）。**纯函数**，有单测。
pub fn describe_video_sources(list: &[bt_providers::maccms_json::JsonApiSite]) -> String {
    if list.is_empty() {
        return "未导入影视源".to_string();
    }
    let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
    let shown = if names.len() > 6 {
        format!("{} 等 {}", names[..6].join("、"), names.len())
    } else {
        names.join("、")
    };
    format!("已导入 {} 个：{shown}", list.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            url: "http://localhost:8080".to_string(),
            user: "admin".to_string(),
            pass: "p@ss=\"w/rd\"".to_string(),
            token: String::new(),
        }
    }

    #[test]
    fn round_trips_through_json() {
        let s = Stored::from_parts(Kind::Qbittorrent, &cfg());
        let text = serde_json::to_string(&s).unwrap();
        let back: Stored = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);

        let (kind, c) = back.into_parts().unwrap();
        assert_eq!(kind, Kind::Qbittorrent);
        assert_eq!(c, cfg(), "密码里的引号 / 等号 / 斜杠都不能丢");
    }

    /// 用 serde 而不是手写 `key=value` 的理由 —— 这一条就是那个理由。
    #[test]
    fn awkward_secrets_survive_the_round_trip() {
        for secret in ["a=b\nc", "中文密码", "{\"json\":\"看起来像配置\"}", ""] {
            let c = Config {
                url: "http://x:1".to_string(),
                user: "u".to_string(),
                pass: secret.to_string(),
                token: String::new(),
            };
            let text = serde_json::to_string(&Stored::from_parts(Kind::Aria2, &c)).unwrap();
            let back: Stored = serde_json::from_str(&text).unwrap();
            assert_eq!(back.into_parts().unwrap().1, c, "secret={secret:?}");
        }
    }

    /// 认不出的 kind **不能**猜 —— 猜错会把磁力推给用户没选过的客户端。
    #[test]
    fn unknown_kind_is_rejected_not_guessed() {
        let s = Stored {
            kind: "utorrent".to_string(),
            url: "http://x:1".to_string(),
            user: String::new(),
            pass: String::new(),
            token: String::new(),
        };
        assert!(s.into_parts().is_none());
    }

    /// 手写 / 旧版配置少字段时不该整份作废。
    #[test]
    fn missing_optional_fields_default_to_empty() {
        let text = r#"{"kind":"gopeed","url":"http://x:1"}"#;
        let s: Stored = serde_json::from_str(text).unwrap();
        let (kind, c) = s.into_parts().unwrap();
        assert_eq!(kind, Kind::Gopeed);
        assert_eq!(c.user, "");
        assert_eq!(c.pass, "");
        assert_eq!(c.token, "");
    }

    /// 坏 JSON ⇒ `into_parts` 之前就该失败；这里钉住"解析不出来不是 panic"。
    #[test]
    fn broken_json_is_an_error_not_a_panic() {
        let r: Result<Stored, _> = serde_json::from_str("{ 这不是 json");
        assert!(r.is_err());
    }

    // ---- ⑤ 索引器持久化 ----

    fn indexer() -> bt_torznab::IndexerConfig {
        bt_torznab::IndexerConfig {
            id: "torznab:jackett".to_string(),
            name: "Jackett".to_string(),
            url: "http://localhost:9117".to_string(),
            api_key: "k3y=\"with quotes\"".to_string(),
            enabled: true,
        }
    }

    #[test]
    fn indexers_round_trip_through_json() {
        let s = StoredIndexer::from_config(&indexer());
        let text = serde_json::to_string(&s).unwrap();
        let back: StoredIndexer = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
        assert_eq!(
            back.into_config().unwrap(),
            indexer(),
            "API key 里的引号不能丢"
        );
    }

    /// ⚠️ **漏了 `enabled` 字段 ⇒ 默认 true**（JS 的语义是 `enabled !== false`）。
    /// 用 `#[serde(default)]` 的话默认是 `false` —— 手写的配置会静默变哑巴。
    #[test]
    fn a_hand_written_indexer_without_enabled_is_enabled() {
        let text = r#"{"name":"X","url":"http://x:9117"}"#;
        let s: StoredIndexer = serde_json::from_str(text).unwrap();
        assert!(s.enabled, "⚠️ 默认必须是 true，不是 false");
        assert!(s.into_config().unwrap().enabled);
    }

    /// ⚠️ **没有 URL 的条目直接丢掉** —— 留着只会每轮发一个必然失败的请求。
    #[test]
    fn an_indexer_without_a_url_is_dropped() {
        let text = r#"[{"name":"no url"},{"name":"ok","url":"http://x:9117"}]"#;
        let list: Vec<StoredIndexer> = serde_json::from_str(text).unwrap();
        let cfgs: Vec<_> = list
            .into_iter()
            .filter_map(StoredIndexer::into_config)
            .collect();
        assert_eq!(cfgs.len(), 1, "只有带 url 的那条活下来");
        assert_eq!(cfgs[0].name, "ok");
    }

    #[test]
    fn a_broken_indexers_file_is_an_empty_list_not_a_panic() {
        let r: Result<Vec<StoredIndexer>, _> = serde_json::from_str("[ 这不是 json");
        assert!(
            r.is_err(),
            "解析失败 ⇒ load_indexers 返回空列表（已在上层吞掉）"
        );
        // 而且空文件 / 空数组都是合法的「没配过」
        let empty: Vec<StoredIndexer> = serde_json::from_str("[]").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn describe_says_what_is_configured() {
        assert_eq!(describe(&[]), "未配置索引器（Torznab）");

        assert_eq!(describe(&[indexer()]), "已配置 1 个：Jackett");

        // 名字留空 ⇒ 用地址当代名（不然状态栏上是一片空白）
        let nameless = vec![bt_torznab::IndexerConfig {
            name: String::new(),
            ..indexer()
        }];
        assert_eq!(describe(&nameless), "已配置 1 个：http://localhost:9117");

        // 多个用顿号连起来；**停用的要说出来**（静默少一家比报错更迷惑）
        let two = vec![
            indexer(),
            bt_torznab::IndexerConfig {
                name: "Prowlarr".to_string(),
                enabled: false,
                ..indexer()
            },
        ];
        assert_eq!(
            describe(&two),
            "已配置 2 个（1 个已停用）：Jackett、Prowlarr"
        );
    }
}

#[cfg(test)]
mod pansou_tests {
    use super::*;

    fn ep(url: &str) -> StoredPansou {
        StoredPansou {
            url: url.to_string(),
            token: String::new(),
        }
    }

    #[test]
    fn upsert_keeps_existing_position_and_overwrites_token() {
        let list = vec![ep("http://a:8888"), ep("http://b:8888")];
        let out = upsert_pansou(&list, "http://b:8888/", "tok");
        // 尾斜杠被归一 ⇒ 认成同一个实例，就地覆盖（不是新增）
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].url, "http://a:8888");
        assert_eq!(out[1].url, "http://b:8888");
        assert_eq!(out[1].token, "tok");
    }

    #[test]
    fn upsert_puts_a_new_endpoint_first_and_caps_the_list() {
        let mut list = vec![ep("http://a:8888")];
        let out = upsert_pansou(&list, "http://new:8888", "");
        assert_eq!(out[0].url, "http://new:8888", "新地址应成为主实例");

        for i in 0..10 {
            list = upsert_pansou(&list, &format!("http://n{i}:8888"), "");
        }
        assert_eq!(list.len(), MAX_PANSOU_INSTANCES);
    }

    #[test]
    fn upsert_ignores_illegal_urls_instead_of_clearing_the_list() {
        let list = vec![ep("http://a:8888")];
        let out = upsert_pansou(&list, "file:///etc/passwd", "");
        assert_eq!(out, list, "非法地址不能让配置变空");
        let out = upsert_pansou(&list, "  ", "");
        assert_eq!(out, list);
    }

    #[test]
    fn describe_counts_instances() {
        assert_eq!(
            describe_pansou(&[]),
            "未配置网盘搜索（默认 127.0.0.1:8888）"
        );
        assert_eq!(
            describe_pansou(&[ep("http://a:8888")]),
            "1 个实例：http://a:8888"
        );
        assert_eq!(
            describe_pansou(&[ep("http://a:8888"), ep("http://b:8888")]),
            "2 个实例（主：http://a:8888）"
        );
    }
}
