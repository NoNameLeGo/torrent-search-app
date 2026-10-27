# 数据源统一管理（sources.json）设计草案

> 2026-10-27 经过三轮追问定稿的**方向**（尚未实现）。目标：把「视频源 / BT 站点」从**写死**变成
> **一份 JSON + 导入器 + 列表管理**，加源不再发版。
> 现状：BT 28 家是编译期 `registry()`，视频 5 家是编译期 `MacCmsSite` 表，
> 用户能加的只有 `indexers.json`（Torznab）与 `videosources.json`（TVBox 导入）。

## 1. 已定的决策

| # | 决策 | 说明 |
|---|---|---|
| D1 | **范围 = 视频 + BT 站点** | 网盘（PanSou 实例）**不进**这份清单：它是「服务端点」不是「搜索源」，塞进来就要为它开特例 |
| D2 | **内部自定义 schema + 兼容导入** | 内部 `sources.json`；导入器认 TVBox 单仓、LibreTV SourceList v2、Prowlarr 定义、裸 URL 列表 |
| D3 | **规则语言 = 我们自己的完整规则** | 语法抄 Cardigann（`base` / `search.paths` / `rows.selector` / `fields`），**filter 只认白名单**（现有 7 个）。白名单外的规则**拒绝导入并指出是哪个 filter** —— 失败归因要清楚 |
| D4 | **规则来源 = 本地导入 + URL 订阅** | 订阅的规则文件**不自动启用**（见 §5 风险） |
| D5 | **手写优先，JSON 可显式覆盖** | 同 id 命中手写实现时用手的；JSON 里写 `"override": true` 才允许盖 |
| D6 | **完整列表 + 编辑表单** | 设置里能看到全部源、开关、删除、进表单改 |
| D7 | **Cardigann 目录 = 1 条记录 + 可展开子站** | 避免 80 行刷屏；子站级开关存 `members[].enabled` |
| D8 | **订阅刷新保留本地修改** | 刷新只替换「定义」（`url` / `rule`），用户的 `enabled` 与自定义规则存 `overrides`，不被冲掉 |
| D9 | **内置 28 家也进列表** | 标 `"builtin": true`、**不可删、可关**，与导入的源一样被统一管理 |
| D10 | **自动迁移 + 兼容读** | 启动时把 `videosources.json` / `indexers.json` 并进 `sources.json`；旧文件**保留不删**，仍可读 |
| D11 | **启动刷新 + 自动熔断** | 启动拉一次订阅；单条源**连续 3 次**失败自动置停用（可手动重开） |
| D12 | **熔断状态不进 `sources.json`** | 复用现有 `health.jsonl`（key 就用 `source.id`），配置文件保持「用户手改友好」 |

## 2. schema 草案（`%APPDATA%\bt-search\sources.json`）

```json
{
  "schema_version": 1,
  "sources": [
    {
      "id": "builtin:dmhy",
      "name": "dmhy",
      "kind": "bt",
      "adapter": "builtin",
      "builtin": true,
      "enabled": true,
      "group": "动漫"
    },
    {
      "id": "builtin:appmovie",
      "name": "APP影院",
      "kind": "video",
      "adapter": "builtin",
      "builtin": true,
      "enabled": true
    },
    {
      "id": "user:lunatv-3",
      "name": "某 MacCMS 站",
      "kind": "video",
      "adapter": "maccms_json",
      "url": "https://x/api.php/provide/vod/",
      "enabled": true
    },
    {
      "id": "rule:my-site",
      "name": "我自己接的站",
      "kind": "bt",
      "adapter": "html_rule",
      "enabled": true,
      "rule": {
        "base": "https://share.dmhy.org",
        "search": {
          "path": "/topics/list",
          "method": "GET",
          "inputs": { "keyword": "{{ .Keywords }}" }
        },
        "rows": "table tbody tr:has(a[href^=\"magnet:?\"])",
        "fields": {
          "title":    { "selector": "a", "filters": [{ "name": "trim" }] },
          "download": { "selector": "a[href^=\"magnet:?\"]", "attribute": "href" },
          "size":     { "selector": "td:nth-child(5)" }
        }
      }
    },
    {
      "id": "catalog:prowlarr",
      "name": "Prowlarr 公共目录",
      "kind": "bt",
      "adapter": "cardigann_catalog",
      "url": "https://indexers.prowlarr.com/master/12",
      "enabled": true,
      "members": [
        { "id": "dmhy", "name": "dmhy", "enabled": true, "overrides": {} },
        { "id": "1337x", "name": "1337x", "enabled": false, "overrides": {} }
      ]
    }
  ],
  "subscriptions": [
    {
      "id": "sub:lunatv",
      "name": "LunaTV 源",
      "url": "https://example.com/sources.json",
      "format": "libretv",
      "enabled": true,
      "last_sync": 0
    }
  ]
}
```

字段约定：

| 字段 | 取值 | 说明 |
|---|---|---|
| `kind` | `video` / `bt` | 决定进哪个模式（与顶栏三模式对齐，网盘不在内） |
| `adapter` | `builtin` / `maccms_json` / `html_rule` / `cardigann_catalog` | 「怎么取数」的唯一分叉点；新增适配器 = 一个 enum 值 + 一个实现 |
| `builtin` | bool | 内置项的标记：不可删，`enabled` 可改 |
| `override` | bool | 允许覆盖同 id 的手写实现（D5） |
| `overrides` | object | 订阅成员的**本地覆盖**（D8）：只存与订阅定义不同的字段 |
| `members[]` | array | 目录型源的子站；`enabled` / `overrides` 是本地态 |

## 3. 导入器（一个入口，四种格式）

| 输入 | 判据 | 变成什么 |
|---|---|---|
| TVBox 单仓 | JSON 有 `sites[]` | 每条 `type==1` → `adapter: maccms_json`；`type==3` 计数跳过 |
| TVBox 多仓 | JSON 有 `urls[]` | 只列子仓地址，让用户挑（**不递归**，同 `tvbox.rs` 的口径） |
| LibreTV SourceList v2 | JSON 有 `sources[]` | 每条 → `adapter: maccms_json` |
| Prowlarr 清单 | JSON 有 `indexers[]` | 建一条 `cardigann_catalog` + N 个 `members`（只留 `type=public`） |
| 裸 URL 列表 | 一行一个 `http(s)` | 每条 → `adapter: maccms_json`（用户可在表单里改） |
| 单条规则 | YAML / JSON，有 `search` + `rows` + `fields` | `adapter: html_rule`；filter 不在白名单 ⇒ **拒绝并报错** |

## 4. 实施分批（每批一个 PR，各自跑 CI）

| 批次 | 内容 | 状态 |
|---|---|---|
| **1** | `sources.rs`：schema + `load/save` + 旧文件迁移 + 内置项生成；设置里**只读摘要** | ✅ PR #8 |
| **2** | 导入器四格式 → 统一写入；`maccms_json` 改成从 `sources.json` 取源 | ✅ 本批（`source_import.rs`） |
| **3** | `html_rule` 适配器（把 `cardigann.rs` 从「Prowlarr 专用」提升为「通用规则执行器」）+ 表单编辑 | ⬜ |
| **4** | 订阅刷新（启动 + 手动）+ 熔断（复用 `health.jsonl`）+ 成员展开 UI | ⬜ |

## 5. 已知风险（不解决就要写进文档）

| 风险 | 处置 |
|---|---|
| **订阅 = 远程内容** | 订阅回来的规则**默认不启用**（或只启用体检通过的）。否则等于允许别人往你的搜索链路里塞任意 URL |
| **规则里能写 URL 模板** | 只放行 `http(s)`；**不禁内网地址**（本机 PanSou / 局域网索引器要用） |
| **正则 ReDoS** | 用户自己的规则，风险自担；但单站请求仍走现有 10s 超时，且熔断兜底 |
| **80 个 public 里能用的很少** | 导入后**不自动全开**；先跑一次体检，只启用活着的（见 `docs/DATA-SOURCES.md` §6） |
| **`sources.json` 手改坏** | 与其它配置文件同口径：解析失败 ⇒ 当「没配置过」，绝不 panic、绝不拦搜索 |

## 6. 与本文件邻居的关系

- `docs/DATA-SOURCES.md` —— 源从哪来（分级与调研结论）
- 本文件 —— 源进来之后**怎么存、怎么管**
- `docs/BACKLOG-ZHUIJU.md` —— 哪些功能值得抄、哪些明确不做
