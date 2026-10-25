//! 网盘搜索（Pan 模式）—— **PanSou 客户端**。
//!
//! 方案 A：App 只当客户端，PanSou 由用户自己起（默认 `http://127.0.0.1:8888`）。
//! 不在进程内嵌 Go 二进制、不管理 sidecar —— 主 exe 的体积与「单文件」承诺不变。
//!
//! ## 上游 API（`fish2018/pansou`，MIT）
//!
//! ```text
//! POST /api/search
//! {"kw":"流浪地球","res":"merge"}
//! → {"total":…, "merged_by_type": {
//!      "quark": [{"url":"…","password":"…","note":"…","datetime":"…","source":"…"}],
//!      "baidu": […]
//!   }}
//! ```
//!
//! - `GET /api/health` 可探活（`status=ok`）；
//! - 开了认证时加 `Authorization: Bearer <token>`；
//! - 公开实例 `pansou.de` **不暴露** `/api/search`（404），所以只支持自建/远程的 PanSou。
//!
//! ## 契约
//!
//! 与 BT/Video provider 一致：永不 panic、永不返回 Err；失败落在
//! [`PanSearchOutcome::error`]，没有结果则是 `ok(vec![])`。

use bt_core::http::{HttpClient, JsonResponse, ReqOpts};
use serde_json::Value;

/// 一条网盘搜索结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanItem {
    /// 资源标题（PanSou 的 `note`，缺了退回 `work_title`）。
    pub title: String,
    /// 网盘类型：`quark` / `baidu` / `aliyun` / `uc` / `115` / `pikpak` …
    pub pan_type: String,
    /// 网盘链接。
    pub url: String,
    /// 提取码（空串当没有）。
    pub code: Option<String>,
    /// 来源（`tg:频道` / `plugin:插件`）。
    pub source: Option<String>,
    /// 链接时间（原样字符串，不做时区换算）。
    pub date: Option<String>,
}

/// Pan 模式的搜索结果。字段名对齐 [`crate::SearchOutcome`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanSearchOutcome {
    pub results: Vec<PanItem>,
    pub error: Option<String>,
}

impl PanSearchOutcome {
    pub fn ok(results: Vec<PanItem>) -> Self {
        Self {
            results,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            results: Vec::new(),
            error: Some(msg.into()),
        }
    }
}

/// PanSou 客户端。`base` 是用户配的地址（例如 `http://127.0.0.1:8888`）。
#[derive(Debug, Clone)]
pub struct PanSouClient {
    base: String,
    token: Option<String>,
}

impl PanSouClient {
    pub fn new(base: &str, token: Option<&str>) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            token: token
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string),
        }
    }

    /// 搜索。`limit` 是**返回条数上限**（PanSou 一次可能回几百条，UI 不需要全量）。
    pub async fn search(&self, http: &HttpClient, query: &str, limit: usize) -> PanSearchOutcome {
        if self.base.is_empty() {
            return PanSearchOutcome::err("pansou_url_empty");
        }
        let url = format!("{}/api/search", self.base);
        let body = serde_json::json!({ "kw": query, "res": "merge" });

        let opts = self
            .token
            .as_deref()
            .map(|t| ReqOpts::default().header("Authorization", &format!("Bearer {t}")));

        let resp: JsonResponse<Value> = http.post_json(&url, &body, opts.as_ref()).await;
        if let Some(e) = resp.error {
            return PanSearchOutcome::err(e);
        }
        let Some(data) = resp.data else {
            return PanSearchOutcome::err("no_json");
        };

        let Some(merged) = data.get("merged_by_type").and_then(Value::as_object) else {
            // 合法 JSON 但没有 `merged_by_type`：当作 0 条，不报错（PanSou 的空结果也长这样）。
            return PanSearchOutcome::ok(Vec::new());
        };

        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (pan_type, arr) in merged {
            let Some(items) = arr.as_array() else {
                continue;
            };
            for item in items {
                let Some(link) = item.get("url").and_then(Value::as_str) else {
                    continue;
                };
                if link.is_empty() || !seen.insert(link.to_string()) {
                    continue;
                }

                let title = item
                    .get("note")
                    .or_else(|| item.get("work_title"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();

                out.push(PanItem {
                    title,
                    pan_type: pan_type.clone(),
                    url: link.to_string(),
                    code: item
                        .get("password")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .map(str::to_string),
                    source: item
                        .get("source")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    date: item
                        .get("datetime")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                });

                if out.len() >= limit {
                    return PanSearchOutcome::ok(out);
                }
            }
        }

        PanSearchOutcome::ok(out)
    }

    /// 探活：`GET /api/health`，只看 HTTP 是否成功（不解析 body）。
    pub async fn health(&self, http: &HttpClient) -> bool {
        if self.base.is_empty() {
            return false;
        }
        let url = format!("{}/api/health", self.base);
        let resp = http.get_text(&url, None).await;
        resp.error.is_none()
    }
}
