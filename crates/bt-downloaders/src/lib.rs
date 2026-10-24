//! # bt-downloaders
//!
//! 把磁力推送到**本机**运行的下载器 WebUI / RPC —— `src/lib/downloaders.js`（214 行，
//! **唯一真值来源**）的移植。四个客户端：
//!
//! | kind | label | 认证 | 默认地址 |
//! |---|---|---|---|
//! | `qbittorrent` | qBittorrent | 用户名密码 | `http://localhost:8080` |
//! | `transmission` | Transmission | 用户名密码 | `http://localhost:9091` |
//! | `aria2` | aria2 / Motrix | token | `http://localhost:16800/jsonrpc` |
//! | `gopeed` | Gopeed | token | `http://localhost:9999` |
//!
//! ## ⚠️ 与 provider 层**相反**的约定：失败靠 `Err`，不是靠字段
//!
//! provider 层的契约是「永不返回 `Err`」，失败塞进 `SearchOutcome.error`。
//! 这一层**刻意反过来**（JS 注释原话：「这里刻意用抛异常表达失败」）：
//! 每个客户端的错误细节（登录失败 / RPC 报错 / 连不上）都要能透传给前端提示，
//! 所以对外统一是 `Result<_, String>` —— `Err` 里就是给用户看的那句话。
//!
//! ## `safeHttpUrl` 在这一层，不在 JS 那一层
//!
//! JS 把 URL 协议校验放在 `server.js` 的 `safeHttpUrl()`（只放行 `http:` / `https:`）。
//! Rust 侧**没有那层 HTTP 服务器**，所以它落在库里 —— 而且 [`push`] / [`test`]
//! **内部**会再校验一次。这是**信任边界**上的校验（用户填的 url 会被拿去发请求），
//! 手册明确要求不许省；放在库里等于调用方不可能"忘了"。

mod aria2;
mod gopeed;
mod qbittorrent;
mod transmission;

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::HeaderMap;
use reqwest::{Client, Response, StatusCode};
use serde_json::Value;

/// 每个下载器都在本机跑，超时给短一点，连不上尽快失败（JS `TIMEOUT = 10000`）。
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// JS `server.js` 里 `unknown_client` 的文案 —— 由 [`Kind::parse`] 的调用方使用。
pub const UNKNOWN_CLIENT: &str = "unknown_client";

/// JS `server.js` 里 `invalid_url_scheme` 的文案。
pub const INVALID_URL_SCHEME: &str = "invalid_url_scheme";

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 客户端种类。JS 里是字符串 key（`'qbittorrent'` …）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Qbittorrent,
    Transmission,
    Aria2,
    Gopeed,
}

impl Kind {
    /// 与 JS 逐字一致的字符串 id（`META` 的 key、`push(kind, …)` 的 kind 都是它）。
    /// M6 差分要用，别改大小写。
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Qbittorrent => "qbittorrent",
            Kind::Transmission => "transmission",
            Kind::Aria2 => "aria2",
            Kind::Gopeed => "gopeed",
        }
    }

    /// 从 JS 那边的字符串解析。**未知 kind 返回 `None`** —— 调用方配
    /// [`UNKNOWN_CLIENT`] 使用，与 JS 的 `!CLIENTS[kind]` 等价。
    ///
    /// （叫 `parse` 而不是 `from_str`，免得和 `std::str::FromStr::from_str` 撞上
    /// 触发 clippy 的 `should_implement_trait`。）
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "qbittorrent" => Some(Kind::Qbittorrent),
            "transmission" => Some(Kind::Transmission),
            "aria2" => Some(Kind::Aria2),
            "gopeed" => Some(Kind::Gopeed),
            _ => None,
        }
    }
}

/// 认证方式 —— 决定设置面板显示哪些字段（JS `auth: 'userpass' | 'token'`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    UserPass,
    Token,
}

impl Auth {
    pub fn as_str(self) -> &'static str {
        match self {
            Auth::UserPass => "userpass",
            Auth::Token => "token",
        }
    }
}

/// 客户端元信息 —— 照 JS 的 `META`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    pub kind: Kind,
    pub label: &'static str,
    pub auth: Auth,
    pub default_url: &'static str,
}

/// 照 JS `META` 的四条，**顺序照 JS 对象插入顺序**（前端下拉按它排）。
///
/// 用 `static`（不是 fn 里的 `const`）—— 形式更明确地保证返回的引用是 `'static`。
static META: [Meta; 4] = [
    Meta {
        kind: Kind::Qbittorrent,
        label: "qBittorrent",
        auth: Auth::UserPass,
        default_url: "http://localhost:8080",
    },
    Meta {
        kind: Kind::Transmission,
        label: "Transmission",
        auth: Auth::UserPass,
        default_url: "http://localhost:9091",
    },
    Meta {
        kind: Kind::Aria2,
        label: "aria2 / Motrix",
        auth: Auth::Token,
        default_url: "http://localhost:16800/jsonrpc",
    },
    Meta {
        kind: Kind::Gopeed,
        label: "Gopeed",
        auth: Auth::Token,
        default_url: "http://localhost:9999",
    },
];

/// 四个客户端的元信息表（顺序 = UI 下拉顺序）。
pub fn meta() -> &'static [Meta] {
    &META
}

/// 某个 kind 的元信息。
///
/// 这里的 `expect` 是**不变量断言**、不是运行期失败路径：`Kind` 只有四个变体，
/// 而 `META` 覆盖全部四个 —— `tests/clients.rs::meta_matches_the_js_meta_table`
/// 把这个不变量钉住了。所以它不该、也不会在真实路径上触发。
pub fn meta_of(kind: Kind) -> Meta {
    *meta()
        .iter()
        .find(|m| m.kind == kind)
        .expect("meta() 覆盖了全部四个 Kind（由 meta_matches_the_js_meta_table 钉住）")
}

/// 连接配置。字段名照 JS 的 `{ url, user, pass, token }`。
///
/// JS 里这些字段可能是 `undefined`，而 JS 到处用 `cfg.user || ''` 兜 —— 所以
/// Rust 侧用 `String`（默认空串）恰好等价，**不需要 `Option<String>`**。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub url: String,
    pub user: String,
    pub pass: String,
    pub token: String,
}

/// `push` 成功时的返回。JS 是 `{ status }`（qB 取响应状态、gopeed 取响应状态、
/// Transmission / aria2 硬编码 200）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushOutcome {
    pub status: u16,
}

/// 自动探测的候选 —— 照 JS 的 `DETECT_CANDIDATES` 条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectCandidate {
    pub kind: Kind,
    pub url: String,
    pub user: String,
    pub pass: String,
}

/// [`detect`] 命中时的返回。
///
/// JS 的 `detect()` 直接把候选对象整个交出去（`{ kind, url, user, pass }`），
/// server 层 `res.json({ ok: true, ...hit })` 把它摊平 —— 所以这里带上 `kind`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub kind: Kind,
    pub config: Config,
}

// ---------------------------------------------------------------------------
// 对外 API
// ---------------------------------------------------------------------------

/// 只放行 `http:` / `https:` —— 复刻 `server.js` 的 `safeHttpUrl()`。
///
/// 返回**归一化**后的 URL（JS 用 `u.toString()`）；解析不了、或协议不是
/// `http` / `https`，一律 `None`。
///
/// ⚠️ 这是**信任边界**上的校验，不许省 —— 用户填的 url 会被拿去发请求。
/// JS 把它放在 server 层；Rust 侧放库里，并且 [`push`] / [`test`] 内部各再查一次。
pub fn safe_http_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let url = reqwest::Url::parse(trimmed).ok()?;
    match url.scheme() {
        "http" | "https" => Some(url.to_string()),
        _ => None,
    }
}

/// 本机自动探测候选 —— 照 JS `DETECT_CANDIDATES` 的 **9 条与顺序**（顺序即优先级）。
///
/// 只覆盖「开箱默认」情形；设了非默认端口 / 密码的用户仍可手动填写。
pub fn detect_candidates() -> Vec<DetectCandidate> {
    fn c(kind: Kind, url: &str, user: &str, pass: &str) -> DetectCandidate {
        DetectCandidate {
            kind,
            url: url.to_string(),
            user: user.to_string(),
            pass: pass.to_string(),
        }
    }
    let qb = Kind::Qbittorrent;
    vec![
        c(qb, "http://localhost:8080", "admin", "adminadmin"),
        c(qb, "http://localhost:8080", "admin", ""),
        c(qb, "http://127.0.0.1:8080", "admin", "adminadmin"),
        c(qb, "http://localhost:8081", "admin", "adminadmin"),
        c(Kind::Transmission, "http://localhost:9091", "", ""),
        c(Kind::Transmission, "http://127.0.0.1:9091", "", ""),
        // Motrix 默认
        c(Kind::Aria2, "http://localhost:16800/jsonrpc", "", ""),
        // aria2 默认
        c(Kind::Aria2, "http://localhost:6800/jsonrpc", "", ""),
        c(Kind::Gopeed, "http://localhost:9999", "", ""),
    ]
}

/// 逐个候选试疎通，**命中第一个即返回**；全不中返回 `None`。
pub async fn detect() -> Option<Detected> {
    detect_from(&detect_candidates()).await
}

/// [`detect`] 的实现体，候选表可注入 —— 测试用它钉「命中第一个即返回」。
pub async fn detect_from(candidates: &[DetectCandidate]) -> Option<Detected> {
    for c in candidates {
        let config = Config {
            url: c.url.clone(),
            user: c.user.clone(),
            pass: c.pass.clone(),
            token: String::new(),
        };
        if test(c.kind, &config).await.is_ok() {
            return Some(Detected {
                kind: c.kind,
                config,
            });
        }
    }
    None
}

/// 推送磁力 —— 等价于 JS 的 `push(kind, cfg, magnet)`。
///
/// `Err` 里就是给用户看的文案；协议层那几种（登录失败 / RPC 报错）与 JS **逐字一致**，
/// 网络层那几种见 [`crate::net_err`] 的说明（有意 divergence）。
pub async fn push(kind: Kind, cfg: &Config, magnet: &str) -> Result<PushOutcome, String> {
    ensure_http_url(cfg)?;
    match kind {
        Kind::Qbittorrent => qbittorrent::push(cfg, magnet).await,
        Kind::Transmission => transmission::push(cfg, magnet).await,
        Kind::Aria2 => aria2::push(cfg, magnet).await,
        Kind::Gopeed => gopeed::push(cfg, magnet).await,
    }
}

/// 测试连通性，**不产生任何下载副作用** —— 等价于 JS 的 `test(kind, cfg)`。
///
/// JS 成功时返回 `{ ok: true }`；失败靠抛。Rust 侧同理：`Ok(())` 即
/// 「可达 + 认证通过」，`Err` 里是文案。
pub async fn test(kind: Kind, cfg: &Config) -> Result<(), String> {
    ensure_http_url(cfg)?;
    match kind {
        Kind::Qbittorrent => qbittorrent::test(cfg).await,
        Kind::Transmission => transmission::test(cfg).await,
        Kind::Aria2 => aria2::test(cfg).await,
        Kind::Gopeed => gopeed::test(cfg).await,
    }
}

fn ensure_http_url(cfg: &Config) -> Result<(), String> {
    if safe_http_url(&cfg.url).is_none() {
        return Err(INVALID_URL_SCHEME.to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四个客户端共用的管道（各模块 `use crate::…` 取用）
// ---------------------------------------------------------------------------

/// 共用一个 `reqwest::Client`（连接池 + 默认超时）。JS 每次 `axios.post` 也走同一个
/// 全局 agent，语义上等价。
pub(crate) fn http() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(TIMEOUT)
            .build()
            // 自定义 TLS 配置都没有，构建失败几乎不可能；真失败就退回默认 client，
            // 而不是 panic —— 「永不 panic」是本项目的硬约束。
            .unwrap_or_else(|_| Client::new())
    })
}

/// 去掉结尾的全部斜杠 —— 复刻 JS 的 `String(x).replace(/\/+$/, '')`。
pub(crate) fn strip_trailing_slashes(s: &str) -> String {
    s.trim_end_matches('/').to_string()
}

/// 把客户端固定路径补到用户填的基址上 —— 复刻 JS 的 `withPath(base, path)`。
///
/// 用户已经把完整路径填进去时（大小写不敏感地以 `path` 结尾）原样用，不加第二次。
/// 例：`http://localhost:9091` + `/transmission/rpc` → `http://localhost:9091/transmission/rpc`。
pub(crate) fn with_path(base: &str, path: &str) -> String {
    let b = strip_trailing_slashes(base);
    if b.to_lowercase().ends_with(&path.to_lowercase()) {
        b
    } else {
        format!("{b}{path}")
    }
}

/// 复刻 JS 的 `encodeURIComponent`。
///
/// 不保留的字符（含所有非 ASCII）按 **UTF-8 字节**逐个 `%XX` —— 与 JS 一致。
/// 保留集就是 ECMA-262 规定的那几个：`A-Za-z0-9 - _ . ! ~ * ' ( )`。
///
/// 为什么不用 `form` 编码（`reqwest` 的 `.form()`）：那套把空格编成 `+`、
/// 且不转义 `! ~ ' ( )`，与 `encodeURIComponent` 逐字节不同。这里要的是逐字节一致。
pub(crate) fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 读响应体并按 **axios 的方式**处理：
///
/// - 能 `JSON.parse` → 就用那个值；
/// - 不能 → **当成字符串**，而不是报错。
///
/// ⚠️ 这条很重要，别"顺手改成严格解析"：JS 里几个客户端的成功判定是
/// `data && data.result && …` / `typeof data.code === 'number'` 这类**软检查** ——
/// 非 JSON 正文时 `data` 是个字符串，那些检查**全部为假**，于是**判定为成功**。
/// 若这里改成解析失败就 `Err`，行为就和 JS 分叉了。
/// 反过来，JS 用 `resp.json()` 严格解析的只有 provider 层，不是这一层。
pub(crate) async fn json_or_text(resp: Response) -> Result<Value, String> {
    let text = resp.text().await.map_err(net_err)?;
    Ok(serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text)))
}

/// JS 的**真值**语义（`if (x)`），用来照抄那些 `data && data.error && …` 判断。
///
/// ⚠️ 别用 Rust 的 `bool` 直觉代替：JS 里 `0` / `""` / `null` / `false` 都是假，
/// 而 `{}` / `[]` 是**真**。Rust 侧 `Value::Object` 没有"空即假"这回事，
/// 但 `0` 和 `""` 必须判假 —— 否则 `aria2` 的 `data.error` 恰好是 `0` 时会误报错。
pub(crate) fn js_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// 网络层失败（连不上 / 超时 / DNS）的文案。
///
/// ⚠️ **有意 divergence**：JS 用 axios，文案是它自己的
/// （`connect ECONNREFUSED 127.0.0.1:8080`、`timeout of 10000ms exceeded`），
/// reqwest 给的是另一套措辞。**逐字对齐没有意义** —— 两套都是给用户看的「连不上」，
/// 而且 JS 那句也不是我们写的。所以这里统一成中文短句，测试钉的是**我们自己的措辞**
/// （钉「可读」，不是钉「与 axios 相同」）。
///
/// 与 JS **逐字对齐**的是协议层的文案：`登录失败（用户名或密码错误）` /
/// `Transmission: xxx` / `aria2: xxx` / `Gopeed: xxx` —— 那些才是我们自己写的。
pub(crate) fn net_err(e: reqwest::Error) -> String {
    // ⚠️ **不能直接 `format!("{e}")`** —— reqwest 的 `Display` 会带上**完整 URL**，
    // 而下载器地址常把凭据放在 query 里（`?token=…`）或内嵌 basic-auth；
    // 这段文案会进 UI 状态行、`--health` 落盘历史和日志（2026-10-16 审计 #14）。
    // `without_url()` 只剥掉 URL，底层原因（DNS / 连接 / TLS / 超时）原样保留。
    format!("请求下载器失败 ({})", e.without_url())
}

/// 非 2xx 的文案。⚠️ 见 [`net_err`]：axios 会说 `Request failed with status code N`，
/// 这里换成中文短句，属于有意的文案 divergence。
///
/// **为什么要显式判非 2xx**：axios **默认对非 2xx 抛异常**，而 reqwest **不抛**
/// （它只把状态码交给你）。所以每个"axios 会抛"的地方，Rust 侧都要自己查状态码 ——
/// 漏了就会把 500 当成功。
pub(crate) fn status_err(status: StatusCode) -> String {
    format!("下载器返回 HTTP {}", status.as_u16())
}

/// 取某个响应头的**全部**值。
///
/// 存在的理由：qB 的登录判定必须遍历**所有** `set-cookie` 条目去找 `SID=` ——
/// 只看第一个是旧实现的 bug（axios 会把多条塞成数组）。
/// `HeaderMap::get_all` 本身对**头名**就是大小写不敏感的，值的顺序即响应里的顺序。
pub(crate) fn header_values(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .collect()
}
