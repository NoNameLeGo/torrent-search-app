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

/// 读 PanSou 配置。**任何一步失败都只是 `None`** —— 调用方用默认地址兜底。
pub fn load_pansou() -> Option<StoredPansou> {
    let text = std::fs::read_to_string(pansou_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// 保存 PanSou 配置。返回 `Err(人话)` 供状态栏显示。
pub fn save_pansou(url: &str, token: &str) -> Result<(), String> {
    let path = pansou_path().ok_or_else(|| "取不到 %APPDATA%，配置没保存".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    let text = serde_json::to_string_pretty(&StoredPansou {
        url: url.trim().to_string(),
        token: token.trim().to_string(),
    })
    .map_err(|e| format!("序列化失败：{e}"))?;
    write_atomic(&path, &text).map_err(|e| format!("写入失败：{e}"))
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
