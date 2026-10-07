//! 共享 HTTP 层 —— 从 `src/lib/http.js`（71 行）逐条移植。
//!
//! 契约完全一致：**永不抛异常**。在 Rust 里就是永不返回 `Err` ——
//! 失败一律塞进 `error` 字段。provider 统一这样写：
//!
//! ```ignore
//! let r = http.get_text(url, None).await;
//! if let Some(e) = &r.error {
//!     return Err(format!("XXX unreachable: {e}"));
//! }
//! let html = r.html.unwrap_or_default();
//! ```
//!
//! 注意字段名刻意沿用 JS：文本用 `html`，JSON 用 `data`。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, USER_AGENT};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// 与 JS 版一字不差的 UA 池。
pub const USER_AGENTS: [&str; 4] = [
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0",
];

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;

static UA_CURSOR: AtomicUsize = AtomicUsize::new(0);

/// 取一个 UA。
///
/// 与 JS 版的差异（**有意**）：JS 用 `Math.random()`，这里用原子游标轮询。
/// 目的（同一站点在跨请求间换 UA）一致，但结果可复现 —— 否则测试没法断言。
pub fn pick_ua() -> &'static str {
    let i = UA_CURSOR.fetch_add(1, Ordering::Relaxed) % USER_AGENTS.len();
    USER_AGENTS[i]
}

/// 单次请求的可选项，对应 JS 的 `opts`（`{ headers, timeout }`）。
#[derive(Debug, Clone, Default)]
pub struct ReqOpts {
    headers: Vec<(String, String)>,
    timeout: Option<Duration>,
}

impl ReqOpts {
    /// 追加/覆盖一个请求头（同名后者胜，与 JS 的对象展开顺序一致）。
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// 覆盖这一条请求的超时（`scraper.js` 用到）。
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// `get_text` 的返回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextResponse {
    pub html: Option<String>,
    /// 与 JS 版保持一致：**只要进了 error 路径，status 一律是 None**。
    /// JS 的 catch 分支把 status 硬编码成 `null`，即便 axios 的 `e.response` 里其实带着状态码。
    /// 保留这个行为是为了两边可逐字对照；所有 provider 也都不在 error 路径上读 status。
    pub status: Option<u16>,
    pub error: Option<String>,
}

impl TextResponse {
    fn fail(error: String) -> Self {
        Self {
            html: None,
            status: None,
            error: Some(error),
        }
    }

    /// `error.is_none()` 的糖。
    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }
}

/// `get_json` / `post_json` 的返回。
#[derive(Debug, Clone, PartialEq)]
pub struct JsonResponse<T> {
    pub data: Option<T>,
    pub status: Option<u16>,
    pub error: Option<String>,
}

impl<T> JsonResponse<T> {
    fn fail(error: String) -> Self {
        Self {
            data: None,
            status: None,
            error: Some(error),
        }
    }

    /// `error.is_none()` 的糖。
    pub fn is_ok(&self) -> bool {
        self.error.is_none()
    }
}

/// 共享的 HTTP 客户端。对应 JS 那个模块级 `axios.create({...})`。
///
/// `reqwest::Client` 内部自带连接池，所以**整个应用共用一个实例**是对的；
/// 需要单独超时的场景（测试、或某个慢站点）再 `with_timeout` 造一个。
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: reqwest::Client,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    fn build(timeout: Duration) -> reqwest::Client {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .default_headers(headers)
            .build()
            // 只有在 TLS 后端初始化失败时才会到这里，属于启动期硬故障
            .expect("reqwest::Client 构建失败")
    }

    /// 默认 10 秒超时 / 最多 5 次跳转 / 浏览器式 Accept 头 —— 与 JS 版一致。
    pub fn new() -> Self {
        Self {
            client: Self::build(DEFAULT_TIMEOUT),
        }
    }

    /// 换一个默认超时（测试验证超时路径时用）。
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            client: Self::build(timeout),
        }
    }

    /// 拿到底层 `reqwest::Client`，供 provider 做特殊请求（如自定义 body）。
    pub fn raw(&self) -> &reqwest::Client {
        &self.client
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        opts: Option<&ReqOpts>,
    ) -> reqwest::RequestBuilder {
        let mut hm = HeaderMap::new();
        // 每个请求换一个 UA（和 JS 一致）；opts 里的同名头随后覆盖它
        if let Ok(v) = HeaderValue::from_str(pick_ua()) {
            hm.insert(USER_AGENT, v);
        }
        if let Some(o) = opts {
            for (k, v) in &o.headers {
                // 名字或值不合法就跳过，不让一个错头把整个请求带崩
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    HeaderValue::from_str(v),
                ) {
                    hm.insert(name, value);
                }
            }
        }
        let mut req = self.client.request(method, url).headers(hm);
        if let Some(t) = opts.and_then(|o| o.timeout) {
            req = req.timeout(t);
        }
        req
    }

    /// GET 一段文本（HTML）。永不失败返回 —— 失败看 `error`。
    pub async fn get_text(&self, url: &str, opts: Option<&ReqOpts>) -> TextResponse {
        let req = self.request(reqwest::Method::GET, url, opts);
        match req.send().await {
            Ok(res) => {
                let status = res.status();
                if !status.is_success() {
                    return TextResponse::fail(http_status_error(status));
                }
                match res.text().await {
                    Ok(t) => TextResponse {
                        html: Some(t),
                        status: Some(status.as_u16()),
                        error: None,
                    },
                    Err(e) => TextResponse::fail(map_err(&e)),
                }
            }
            Err(e) => TextResponse::fail(map_err(&e)),
        }
    }

    /// GET 并解析 JSON。
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        opts: Option<&ReqOpts>,
    ) -> JsonResponse<T> {
        let req = self.request(reqwest::Method::GET, url, opts);
        send_json(req).await
    }

    /// POST 一个 JSON body 并解析 JSON 响应。
    pub async fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        url: &str,
        body: &B,
        opts: Option<&ReqOpts>,
    ) -> JsonResponse<T> {
        let req = self.request(reqwest::Method::POST, url, opts).json(body);
        send_json(req).await
    }
}

async fn send_json<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> JsonResponse<T> {
    match req.send().await {
        Ok(res) => {
            let status = res.status();
            if !status.is_success() {
                return JsonResponse::fail(http_status_error(status));
            }
            // 先取文本再自己解析，这样"响应根本不是 JSON"能给出可读错误，
            // 而不是 reqwest 那句含糊的 decode error。
            //
            // 与 JS 版的差异（**有意**）：axios 解析失败时会**静默退回原始字符串**，
            // provider 因此拿到一个 string，直到下一行 `data.results.map` 才炸，
            // 报错位置离病因很远。这里直接报错。
            let text = match res.text().await {
                Ok(t) => t,
                Err(e) => return JsonResponse::fail(map_err(&e)),
            };
            match serde_json::from_str::<T>(&text) {
                Ok(v) => JsonResponse {
                    data: Some(v),
                    status: Some(status.as_u16()),
                    error: None,
                },
                Err(e) => JsonResponse::fail(format!("invalid json: {e}")),
            }
        }
        Err(e) => JsonResponse::fail(map_err(&e)),
    }
}

/// 复刻 axios 的 HTTP 状态错误文案（provider 会把它拼进自己的错误串）。
fn http_status_error(status: reqwest::StatusCode) -> String {
    format!("Request failed with status code {}", status.as_u16())
}

/// 把 reqwest 的错误映射成 axios 风格的错误码，方便两边对照。
fn map_err(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        // axios 用 ECONNABORTED 表示超时
        "ECONNABORTED".to_string()
    } else if e.is_connect() {
        // 连接失败。JS 那边 DNS 解析不了会给 ENOTFOUND、端口不通给 ECONNREFUSED，
        // reqwest 不区分，这里统一成 ECONNREFUSED（够用，且不会误导成"站点不存在"）
        "ECONNREFUSED".to_string()
    } else {
        e.to_string()
    }
}

static DEFAULT_CLIENT: OnceLock<HttpClient> = OnceLock::new();

/// 全局共享实例 —— 对应 JS 里那个模块级 `axios.create({...})`。
pub fn default_client() -> &'static HttpClient {
    DEFAULT_CLIENT.get_or_init(HttpClient::new)
}
