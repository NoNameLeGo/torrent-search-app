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

use std::path::PathBuf;

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

/// `%APPDATA%\bt-search\config.json`。取不到 `APPDATA` 就 `None`（**不猜别的路径** ——
/// 猜出来的位置可能正是只读的那个）。
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("bt-search").join("config.json"))
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
    std::fs::write(&path, text).map_err(|e| format!("写入失败：{e}"))
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
}
