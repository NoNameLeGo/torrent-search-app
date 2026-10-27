# 可接入的外部数据源（分级 + 待办）

> 2026-10-27 立项。调研对象是「能不能给本项目**整段接进来一个章节**」——
> 只看数据形态与接入成本，不看它是不是同类项目。
> 与 `docs/BACKLOG-ZHUIJU.md`（追剧清单评估）是两件事：那边评「要不要抄功能」，这边评「能不能当数据源」。

## 分级口径

| 级 | 判定 |
|---|---|
| **A** | 有**结构化数据文件** + 稳定 URL，能直接拉来当源清单 |
| **B** | 有公开 API / RSS，**不要登录**，一次 GET 出结构化结果 |
| **C** | 只能 HTML 抓，但页面结构单一、解析器可复用 |
| **D** | 不行（JS 渲染 / 要登录 / 二进制托管 / 是播放器） |

---

## 1. 结论

- **A 级只有一条：Prowlarr 的公共索引器清单**（577 条，其中 80 条 public 免登录）。
- **网盘方向没有 A 级源**（见 §4）。
- 字幕方向没有「不要 token 的公开 API」（assrt 要 token、OpenSubtitles 无 key 直接 403）。

---

## 2. A 级：Prowlarr 索引器清单（首选）

### 2.1 数据在哪

```text
清单：https://indexers.prowlarr.com/master/<VER>        ← 1.34MB JSON，一次 GET
单站：https://indexers.prowlarr.com/master/<VER>/<id>   ← 该站的 Cardigann YAML
整包：https://indexers.prowlarr.com/master/<VER>/package.zip
版本：https://raw.githubusercontent.com/Prowlarr/Indexers/master/VERSIONS
      → 内含 CURRENT_VERSION / MAX_VERSION
源码：Prowlarr/Indexers 的 definitions/v11/*.yml（577 个）
```

⚠️ **`<VER>` 不要硬编码**。调研时是 `12`，Prowlarr 源码里写死
`DEFINITION_VERSION = 12`，但 `/master/11` 目前仍返回 200 —— 直接从 `VERSIONS` 读。

### 2.2 清单 JSON 的真实字段（`dmhy` 那条原文）

```json
{"id":"dmhy","file":"dmhy","implementation":"cardigann","name":"dmhy",
 "protocol":"torrent","type":"public","language":"zh-TW",
 "links":["https://share.dmhy.org/"],
 "settings":[{"name":"info_flaresolverr","type":"info_flaresolverr"}],
 "caps":{"categorymappings":[{"id":"2","cat":"TV/Anime"}]},
 "legacylinks":["..."],"sha":"..."}
```

实测分布：`type` = private **439** / public **80** / semi-private **58**；
`language` 含 `zh-CN` **142**，但其中 public 只有 6 个：
`0magnet, acgrip, anibt, dmhy, mikan, u3c3`。

### 2.3 单个定义的抓取规则（`dmhy.yml` 原文，最小可跑样例）

```yaml
type: public
links: [https://share.dmhy.org/]
search:
  paths: [{ path: topics/list }]
  inputs: { keyword: "{{ .Keywords }}", sort_id: 0, order: date-desc, team_id: 0 }
  rows: { selector: 'table tbody tr:has(a[href^="magnet:?"])' }
  fields:
    title:    { selector: 'a[href^="/topics/view/"]' }
    download: { selector: 'a[href^="magnet:?"]', attribute: href }
    size:     { selector: 'td:nth-child(5)' }
    seeders:  { selector: 'td:nth-child(6)' }
```

接入五步：① 拉清单 → 按 `type=="public"` 过滤；② 按 `id` 取单站 YAML；
③ `search.paths` + `inputs`（`.Keywords` 做 URL encode）拼 URL；
④ `rows.selector` 取行；⑤ `fields.*` 取列。

### 2.4 规模与工作量（实测）

- 80 个 public 里 **78 个纯 GET**、仅 2 个 POST；**67 个是单 path**。
- `fields` 用到的 filter 只有 12 种，按出现次数：
  `re_replace`(241) / `replace`(151) / `regexp`(69) / `append`(47) / `dateparse`(46) /
  `andmatch`(26) / `sort`(26) / `split`(14) / `trim`(10) / `type`(17)。
- 80 个里只有 **6 个走 JSON**（bangumi-moe / internetarchive / magnetdownload /
  thepiratebay / yts / zamundalife），其余全是 HTML。

⇒ 一个 Cardigann 解析器能把本项目磁力侧从 **28 家扩到 100+**，且不用一站点一文件。

### 2.5 五个坑（都实测过）

| 坑 | 说明 | 应对 |
|---|---|---|
| `:has()` 选择器 | `rows.selector` 用了 `tr:has(a[href^="magnet:?"])`；`bt_core::dom` 底层是 `scraper`（Servo selectors），`:nth-child` 可用，**`:has` 要实测** | 降级：`tr:has(X)` 改写成「取 `tr` + 行内再查 X」 |
| schema 版本 | URL 里带 `<VER>` | 读 `VERSIONS` 再拼，别硬编码 |
| 境内可达性 | 实测 `share.dmhy.org`、`1337x.to` 都 `000`；80 个 public 里 **24 个**要过 FlareSolverr | 逐站健康检查 + 自动禁用，**不能假设拉下来就能用** |
| **无许可证** | `Prowlarr/Indexers` 仓里**没有 LICENSE 文件** | 只能**运行时拉取 + 本地缓存**，绝不把 YAML 打进安装包 |
| private 439 条 | 要账号 / API key | `settings[]` 已列出字段名（`username`/`password`/`cookie`/`apikey`），可做「填了就启用」 |

### 2.6 Jakeett（备选，同构）

`Jackett/Jackett` 的 `src/Jackett.Common/Definitions/*.yml` 是**同一套 Cardigann schema**，
**583 个**，与 Prowlarr 交集 576，Jackett 独有 7 个（`anirena`/`ildragonero`/`magnetz`/
`rutracker-ru`/`therarbg`/`torrentgalaxyclone`/`uatorrent`），Prowlarr 独有 1 个。
缺点：**没有清单 URL**（只能整仓下 tarball）+ **GPL-2.0**（捆绑进闭源安装包有传染性争议）。
⇒ 定位为「Prowlarr 某站失效时的人工兜底」，不做自动接入。

---

## 3. B 级：可直接 GET 的单点

| 源 | 形态 | 备注 |
|---|---|---|
| `https://apibay.org/q.php?q=<kw>` | TPB 公共 API（JSON） | 本项目已有 `tpb` provider，价值有限 |
| `bitmagnet-io/bitmagnet` | 自建 DHT 索引 + GraphQL | 要用户跑一套 Docker（PG + Meilisearch），重 |
| `fish2018/pansou` | 自建，`POST /api/search`、`GET /api/health` | 已在用（见 §4） |

---

## 4. 网盘方向：**没有 A 级源**（结论）

把 pansou（113 插件 / 14 种网盘：baidu aliyun quark guangya tianyi uc mobile 115 pikpak
xunlei 123 magnet ed2k others）、PanHub、CloudSaver、懒盘、混合盘、以及 pansou 全部 fork
（最高 3 star）都查了：**没有任何仓库把「N 个网盘搜索端点」整理成 JSON / YAML**。
现状就是「一个 PanSou 实例 + 一家家爬」。

| 候选 | 为什么不行 |
|---|---|
| `wu529778790/panhub.shenzjd.com` | `src/config.ts` 里 `API_BASE` 硬编码官方后端，README 自述「纯静态前台」，插件全在闭源服务端 |
| `jiangrui1994/CloudSaver`（9.4k★） | 资源来自 TG 频道 + 用户自配频道，仓里只有 `sponsors.json`，无源清单 |
| `759039446/OnePanSearchApi` | 52★、2025-08 停更、无 license，每站一个 Python 类 = 又写一遍 HTML 解析器 |
| PanSou 的 113 个插件 | 是 **Go 代码**不是声明式数据（`plugin/plugin.go` 里 `RegisterGlobalPlugin` 编译期注册），移植等于用 Rust 重写 113 个解析器 |

**⇒ 维持现有 PanSou 接入，只补两件低成本的事：**

1. **多实例**：支持配置多个 PanSou 地址，主备切换 / 一键健康检查。
   理由：PanSou 是唯一外部依赖，一个地址挂了整个网盘模式就废了（`docs/BACKLOG-ZHUIJU.md` §3
   已把「不要依赖中心化服务」列为不抄项，这里是同一个风险的落地缓解）。
2. **插件枚举**：`GET /api/health` 返回 `{"plugins":["pansearch","panta",...],"plugin_count":N}`。
   白拿的能力：UI 里显示「这个实例支持哪些源」，让用户知道为什么搜不到某网盘。

---

## 5. 排除清单（明确不做）

| 项 | 原因 |
|---|---|
| OpenSubtitles REST | 无 key 直接 **403**（实测），不是「不要 token」 |
| assrt API | 要 token，且需要账号体系；L1 的「拼搜索页」已经够用 |
| 任何需要 JS 渲染的前台 | 与「不引 WebView」的硬约束冲突 |
| 内置 Jackett 定义 | GPL-2.0 + 体积 |
| 把 Prowlarr YAML 打进安装包 | 无 LICENSE；且定义天天变，打进包就过期 |

---

## 6. 待办（按顺序）

| # | 项 | 交付物 | 状态 |
|---|---|---|---|
| 1 | **Cardigann 解析器**：清单 → 定义 → 搜索（只做 `type=public`） | `bt-providers/src/cardigann.rs` | ✅ **已合入**（PR #6，9 条单测；`Cargo.lock` 由 `lockfile.yml` 重生成） |
| 2 | 接线：磁力模式把公共索引器并入聚合层 | `registry` / `aggregate` 扩展 + 设置里开关 | ⬜ 未做（见下「接线的三个真问题」） |
| 3 | **PanSou 多实例 + 插件枚举** | `pan::search_failover` / `plugins`、`config_store::*_pansou_all`、设置里「探活」 | ✅ **已合入**（PR #7，4 条单测） |

### 接线（#2）的三个真问题：动手前先答

1. **清单与定义从哪来、什么时候拉？** 每次搜索都拉 577 条清单显然不行；
   缓存多久、缓存放哪（`%APPDATA%\bt-search\` 还是内存）、过期怎么刷 —— 要定。
2. **80 个 public 里能用几个？** 调研实测量：`share.dmhy.org`、`1337x.to` 本机 `000`，
   24 个还要过 FlareSolverr ⇒ **不能一次性全塞进聚合层**（每次搜索 80 个并发请求 = 长时间等待 + 大面积失败）。
   合理做法：先做一次「体检」把连不上的标掉，只把活着的接进搜索。
3. **和现有 28 家怎么共存？** `dmhy` / `mikan` / `anirena` / `therarbg` 等已经手写过解析器，
   接 Cardigann 后会出现**同一站点两条路径**（重复结果、双倍请求）。
   ⇒ 要么按 id 去重（手写优先），要么干脆让 Cardigann 只补「没有手写实现」的那些站。

⚠️ 依赖变更代价：`cardigann` 需要 `serde_yaml` 与 `regex`（**已加，lock 已重生成**）。
以后再加依赖同样要跑一次：

```bash
scripts/gh-retry.sh workflow run lockfile.yml --ref <branch>
git pull --rebase     # CI 把新 lock 提交回分支
```
