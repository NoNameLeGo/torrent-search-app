# 外部资源池评估：`awesome-zhuiju-free`（2026-10-27）

> 对象：<https://github.com/laoma2053/awesome-zhuiju-free>（网站 `zhuiju.me`，126 条收录，CC BY 4.0）。
> 评估口径：**只看「功能上能不能做」，不看主题是否适配**。判定的是**能力增量**，不是「能不能多抄几行清单」。
> 代码基线：`main` @ 2026-10-27（BT 28 家 / 视频 5 家 / PanSou）。

---

## 0. 一句话结论

126 条里 **约 90% 是「更多同型站点」** —— 能扩量，不增能力。真正的**能力缺口只有 3 处**：
字幕、JSON 源（结构化 API 而非 HTML 抓取）、站点存活信号。其余按「不做」处理。

---

## 1. 逐类判定

| 仓库分类 | 条数 | 本项目现状 | 功能可行性 | 成本 | 判定 |
|---|---|---|---|---|---|
| 在线影视 | 46 | 已覆盖 5 家（全在该表内） | ✅ 架构已就位（`MacCmsSite` 表） | 极低 | **做，扩数据** |
| 磁力 & BT | 16 | 已覆盖 3 家（`bdflixs`/`pomo`/`cilixiong`） | ⚠️ 需逐站验「列表页是否直出磁力」 | 低-中 | **做，先筛** |
| 字幕资源 | 3 | **零**（三模式无字幕维度） | ✅ 能 | 中 | **真缺口 → §2.1** |
| TVBox 接口 | 17 | 零 | ✅ 能（`type=1` 是 JSON API，比 HTML 稳） | 中 | **技术增量 → §2.2** |
| 网盘搜索 | 4 | 已有 PanSou（同源） | ❌ 其余 3 家无公开 API、需 JS 渲染 | 高 | 不做 |
| 直播源 | 3 | 零 | ❌ 无播放器内核 | 很高 | 不做 |
| 影视 APP / 空壳 / 开源项目 | 4+7+16 | — | — | — | 不进（生态参考） |
| 每日检测 + 评分模型 | — | `health.rs` 有半成品 | ✅ 能 | 低 | **真缺口 → §2.3** |

---

## 2. 三处真缺口

### 2.1 字幕（唯一的产品维度空白）

现状三模式全是「找到 → 打开网页」，字幕是一块完全空着的功能区。

| 级 | 做法 | 成本 | 状态 |
|---|---|---|---|
| **L1** | 详情面板加「字幕」按钮组 → 用清洗过的片名拼出字幕站搜索 URL → `open_url` | 半天 | ✅ 已做 |
| L2 | `SubtitleProvider` trait + 拉列表 + 下载 + 解压 + 按 `片名.SxxExx` 匹配本地文件 | 3–5 天 | 未做 |

L2 的真实阻力（不是难写，是难维护）：assrt 要 token、zimuku 有 CSRF 反爬、SubHD 返回格式怪。
**结论：先只做 L1**，等 L1 有真实用量再评估 L2。

### 2.2 JSON MacCMS 适配器 + TVBox 单仓导入（技术上最值钱的一条）

> **状态（2026-10-27）**：库层 + UI 接线都已完成。
> `bt-providers/src/maccms_json.rs`（解析 + 详情页推导）、`bt-providers/src/tvbox.rs`
> （单仓 `sites[]` → 只留 `type == 1`）、`config_store::save_video_sources`
> （落 `%APPDATA%\bt-search\videosources.json`）、设置面板「影视源」一行（导入 / 清空）。
> 导入的源与 5 家静态站**并列**参与视频搜索，家数一起计入「共 N 家」。

仓库那 17 条接口地址**不要内置**：多仓一个文件里几百个站、UI 无处呈现、地址高失效率、混灰产源
（仓库里那条「伦理接口」就是，不在可收录范围）。

可做的是两件基础设施：

1. **JSON MacCMS 适配器**：`?ac=videolist&wd=` 返回结构化 JSON（`vod_name` / `vod_year` / `vod_type_name` / `vod_play_url`）。
   比现有 HTML 选择器链**抗改版一个量级** —— 站点换模板会让 `SELECTOR_CHAINS` 全失效，JSON 接口不会。
2. **「自定义源导入」**：用户贴 TVBox 单仓 URL → 解析 `sites[]` → **只取 `type=1`** → 进视频模式。
   `indexers.json`（自定义 Torznab）是现成先例，架构同构。

⚠️ 只做「用户自填」，**不预置、不内置、不分发**配置。

⚠️ 已知能力短板：JSON 源多为**纯 API 主机**（没有对应 HTML 站点），
`VideoItem.detail_url` 只能按 MacCMS 标准路径 `/index.php/vod/detail/id/<vod_id>.html` 猜。
⇒ 在**没有播放器**的前提下，JSON 源的终点体验弱于 HTML 源。
这条决定了 §2.2 的完成定义停在「结构化结果 + 尽力打开」，
「直接播放 `vod_play_url` 里的 m3u8」属于**下一层**（需要外部播放器协议或内置内核，与 25MB 门冲突）。

### 2.3 站点存活状态灯

仓库那套「每日检测 + 多快净稳评分 + 状态徽章」的方法可参考，而本项目**已经把前半段做完了**：
`bt-app/src/health.rs`（每次搜索记一行 JSONL，`STALE_AFTER=3` 判「可能已改版」）+ `scripts/provider-health.mjs` + `fetch-fixture.yml`。

缺口只有一环：**这个信号没进 UI**。站点死了在界面上和「这个关键词真没资源」长得一模一样。

> **状态（2026-10-27）**：磁力侧**早就有**了 —— `ui_model::chip_for` 在
> `count == 0 && stale_runs >= STALE_AFTER` 时给 chip 加「⚠连空 N 次」，
> 并有 3 条单测钉住（`a_stale_engine_gets_a_warning_marker` 等）。
> 本轮补的是**视频侧**：视频搜索现在也往 `health.jsonl` 记（id 前缀 `video:`，
> 与 BT 引擎共用一个文件但不撞 id），并在状态栏报「N 家疑似改版」。
>
> 仍未做：站点开关列表（`SiteRow`）没有状态色 —— 它只反映「开/关」，
> 不反映「这家最近一直空手」。要做需要先想清楚「关掉的站该不该显示健康色」。

---

## 3. 明确不做

| 项 | 不做的理由 |
|---|---|
| 直播源（M3U/IPTV） | 解析不难，但一播就得内置播放器内核（libmpv ≈ +40–60MB），直接撞 25MB 体积门，且偏离「搜索」定位 |
| 内置 TVBox 多仓地址 | 几百站点无法呈现 + 高失效率 + 合规风险 |
| 其余 3 家网盘搜索 | 无公开 API、需 JS 渲染/登录，抓取成本高于价值 |
| 影视 APP / TVBox 空壳 / 开源项目表 | 属于竞品与生态，不是功能 |

---

## 4. 顺手值得偷的一个设计（不立项，先记）

**Kazumi 式的规则外置订阅**：把站点选择器抽成外部 JSON，改版时用户热更新，不用发版。

对本项目特别对症 —— 本机不能编译、每次改 provider 是「写码 → CI 3 分钟 → 发版」，
而站点改版是常态。前提：**只允许纯 CSS 选择器，不解释脚本**，否则就是可执行注入面。
与 §2.2 的「自定义源」是同一套配置机制，可以合并设计。

---

## 5. 排期

| 序 | 项 | 依赖 | 门 |
|---|---|---|---|
| 1 | §2.1 字幕 L1 | 无 | `cargo test`（`subs.rs` 纯函数单测） |
| 2 | §2.2 JSON 适配器 + TVBox 解析（库层） | 无 | 离线单测（本地 fixture，不打网） |
| 3 | §2.2 UI 接线（设置里导入单仓） | 2 | 需人工点一遍 |
| 4 | §2.3 状态灯 | 无 | 见 `health.rs` 已有测试 |

---

## 6. 附：探活记录（本机，仅作参考，**不可作结论**）

本机有 MITM 透明代理 + 部分 DNS 污染，单点探活已知不可信（见 `AGENTS.md`「网络环境经验」）。

| 目标 | 结果 | 解读 |
|---|---|---|
| `https://subhd.tv/search/test` | 200 | ✅ URL 形态确认 |
| `https://zimuku.org/search?q=` | 404，且 `/` 也 404 | 存疑：疑似代理劫持（重定向串畸形） |
| `https://assrt.net/` | 连不上（000） | 本机 DNS 问题，**不代表站挂** |
| `https://www.opensubtitles.org/...` | 401 + Anubis 质询页 | 反爬，真实浏览器可过 |
| `appmovie.art` HTML 搜索页 | 200 | 与现有 provider 一致 |
| `appmovie.art/api.php/provide/vod/` | 403 | 可能是 WAF，需带 Referer/UA 重试 |

⇒ 字幕站 URL 与 JSON 端点的**最终确认必须走浏览器或 CI**，别用本机 curl 下结论。

---

## 7. 附：同类「资源清单仓」调研（2026-10-27，两轮独立调研的合并结论）

> 口径：与参照物同类的**清单 / 导航 / 收录仓**（不是播放器 App）。star 与提交日期为当日实测。
> ⚠️ 参照物已改名：`laoma2053` → **`laoma528/awesome-zhuiju-free`**（13.7k★）。

### 7.1 参照物的真实结构（比 README 复杂）

| 项 | 事实 |
|---|---|
| 主数据 | `resources/resources.json`(158KB) + `resources/schema.json`(6.2KB, JSON Schema draft 2020-12) |
| 分类 | schema 允许 **11** 类，实际用 **9** 类：online_video 54 / tvbox_config 17 / magnet_search 16 / open_source 16 / player 7 / subscription 5 / video_app 4 / cloud_search 4 / subtitles 3 |
| 评分 | **5 项**（`more/speed/clean/stable/ease`，各 1–5、步进 0.1）—— README 只讲前 4 项 |
| 风险 | 4 项（`copyright/safety/privacy/payment`，枚举 `low/medium/high/unknown`） |
| 验证态 | 6 态；**实际分布 `caution 101 / recommended 17 / removed 8`** —— 八成条目是「谨慎收录」，这是合规姿态 |
| workflow | 5 条：`check-availability`(`0 1 * * *`) / `validate-data` / `issue-to-resource-pr` / `weekly-release` / `update-open-source-stats` |

### 7.2 值得跟的仓库

| 仓库 | ★ | 垄断了什么 | 自动检测 |
|---|---|---|---|
| `mylazily/ziyuanzhan` | 143 | **自动化最完善**（每 6h，一份 `sources.json` 派生 online/offline/fastest/top-rated/stats 五视图，含 `uptime`/`responseTime`/`totalResources`） | ✅ 但**机器聚合、无人工精选、零风险标注** |
| `hafrey1/LunaTV-config` | 4.5k | **唯一做「搜索级」验证**：真关键词查一次再校验 `data.list` 非空且含关键词；69 条就是标准 MacCMS JSON 源 | ✅ `0 17 * * *` |
| `youhunwl/TVAPP` | 26.5k | APK 实体托管 + `状态/备注` 两列运营范式 | ❌ 手工 🟢/🔴 |
| `imDazui/Tvlist-awesome-m3u-m3u8` | 30.3k | 按省份的 IPTV 源（30+ 省） | ❌ 且 2025-11 已停更 |
| `iptv-org/iptv` | 142k | **CI 范式**：只对 `git diff` 出的文件跑 lint + 每日重建 | ✅ |
| `qist/tvbox` | 12.4k | TVBox 配置库（257 份 JSON，每日解密拆分） | ⚠️ 只拆分不检测 |
| `Guovin/iptv-api` | 25.8k | 阈值化质量过滤（`min_resolution`/`min_speed`/广告过滤） | ⚠️ 主流程已删 cron |

### 7.3 三条结论

1. **没有和参照物一样宽的同类** —— 它是「TVBox 源清单 + IPTV + APK + 网盘搜索」的合并体，同类各自只占一块。
2. **「人工精选 + 结构化数据 + 自动检测 + 风险标注」四件齐活的，只有参照物自己**（`ziyuanzhan` 有自动化无精选；`imDazui` 有精选无自动化；`tvapk` 只是二进制托管）。
3. **字幕站清单没有专门仓库** —— 参照物在 `subtitles` 这一格基本独占。

### 7.4 可直接搬的两件东西

| 实现 | 出处 | 对本项目 |
|---|---|---|
| 逐条 marker 替换（`<!-- availability:{id} -->`），改一条只重写那一行，避免整篇 diff 抖动 | `check-availability.mjs:116-124` | 若将来做「站点状态徽章」的生成脚本可抄 |
| 搜索级验证：真关键词查一次，读 `data.list` 非空且含关键词；区分「无结果」与「不匹配」 | `LunaTV-config/check_api.js:65-78` | **正是 §2.3 缺口 3 的下一步** —— 现在只判「连得上 + 返回几条」，不判「搜到的对不对」 |
| 探活省流量：`range: bytes=0-1023` 取 1KB 即 cancel、UA 带爬虫礼貌标识、仅 5xx 重试 | `check-availability.mjs:58-68` | `scripts/provider-health.mjs` 可对齐 |
| TVBox 单仓来源 | `LunaTV-config.json` 的 69 条 | 直接喂 §2.2 做好的「影视源导入」框 |

### 7.5 两条补充机制（第二轮复核原始文件后的结论）

**9. 变更级 lint 是「PR 门禁」，不是「每日检测器」** —— `iptv-org/iptv/.github/workflows/check.yml`
只有 `on: pull_request` + `workflow_dispatch`，**没有 `schedule`**。省 CI 靠三层：
`git diff --diff-filter=ACMRT --name-only master -- streams/`（排除删除）→ 每步挂
`if: steps.files.outputs.any_changed == 'true'`（无变更连 `npm install` 都不跑）→
`concurrency: cancel-in-progress: true`（连推只跑最后一次）。
⚠️ **本项目现在别套用**：`validate-data` 在 126 条规模是毫秒级，diff 过滤的收益要上千条才显现 —— 过度工程。

**10. ⚠️ 定时检测会「静默失效」** —— GitHub 在仓库 **60 天无活动后自动停用 `schedule`**。
一旦每日检测被停用，README 上的 🟢 会一直挂着旧状态、读者无从察觉。
参照物的缓解办法就是它自己的**逐条日期 marker**（`check-availability.mjs:155-160` 的
`"availability-date"`）—— **把「检测时间」暴露在页面上是这类仓库的必需品，不是装饰**。
`Guovin/iptv-api` 更进一步：`main.yml` 顶部明写不要在 fork 里加 `on: schedule`，
且产物只走 Pages Artifact / 每次运行独立的 Release，**不得提交回 Git**（防滥用 + 防污染历史）。

> 本项目自查（2026-10-27）：`.github/workflows/` 里 `cron:` / `schedule:` **0 处** ——
> 全部是 push / PR / `workflow_dispatch` 触发，天然不受「60 天停用」影响。这条只是记账。

### 7.6 参照物自己的实现缺陷（对着抄之前先知道）

**状态灯只覆盖 100/126 —— 这是过滤条件，不是漏挂。**（本节初稿曾按「10 条漏挂」记，已更正）
`check-availability.mjs:132-133`：

```js
const featuredResources = resourcesData.resources.filter(
  (resource) => resource.featured && resource.category !== "open_source"
);
```

`featured && category !== "open_source"` 精确等于 **100**，与 README 里 marker 的 id 集合做过
**集合相等校验**：完全一致。
- `126 − 116(featured) = 10` 条非 featured
- `116 − 16(open_source) = 100`

⇒ `open_source` 全缺灯是**代码硬编码排除**；`removed` 的 8 条因 `featured:false` 被隐藏，属正确行为。

**真正的缺陷：`featured` 一个布尔背了三种语义。**

| 语义 | 出处 |
|---|---|
| 进 README 分类列表 + 首页精选 | `sync-readme.mjs:514,546`（`filter(r => r.featured)`） |
| 参与存活检测 | `check-availability.mjs:133` |
| 公开可见 | 上两条的副作用 |

于是三种本该独立的产品意图**无法表达**：「保留数据暂不上架」「上架但不检测」「不上首页但留在分类」。
实证：`subscription/iptv-org`、`subscription/iptv-cqshushu-com-31` 两条 `status=caution`、
`last_checked=2026-10-10`，**数据里有、README 里 0 次出现**，连心跳都没有。
（另 8 条 `featured=false` 的 online_video **全部 `status=removed`**，那是对的。）

**更严重的是：分类徽章的计数也是 featured 口径**（`sync-readme.mjs:524`
`count = featuredResources.filter(...)`），所以**公开数字比实际少**：

| 徽章 | README 显示 | `resources.json` 实际 |
|---|---|---|
| 在线影视 | **46** | 54 |
| 直播源 | **3** | 5 |

⇒ 读者看到的「在线影视 46」不是收录数，是 featured 数；14 万星的 `iptv-org`
在「直播源」分类里连计数都不占。这不是「藏起来」，是**公开数字错**。

⇒ 三条修正后的建议：
1. `featured:false` 只允许与 `verification.status === "removed"` 共存（8 条合规、2 条 `caution` 违规，可机检）；
2. 或拆字段 `listed` / `featured` / `monitored`，让 `monitored` 独立于 `listed`；
3. **分类徽章计数改用 `category === id && status !== "removed"`**，别用 featured 口径。

⇒ **缺口 3 别照抄 `featured`**：拆成 `listed`（上架）/ `featured`（精选）/ `monitored`（检测）三个独立布尔。

**「静默漏挂」这个坑不存在。** `check-availability.mjs:126`：
`if (!pattern.test(readme)) throw new Error("README marker missing: ...")` ——
检测集合内缺 marker 直接抛错、CI 变红。所以「在数据校验脚本里交叉校验 marker 存在性」是重复劳动，**不必做**。
该加的不变式换成这条：**`featured:false` 只允许与 `verification.status === "removed"` 共存**。

### 7.7 两套指标别混用（会自相矛盾）

| 数据 | 字段 | 语义 |
|---|---|---|
| `reports/availability.json` | `resource_id/url/status/http_status/response_ms/checked_at/attempts/error` | **本次判定** |
| `verification.method/status/last_checked` | `manual|automated|mixed` × 6 态 | **人工/流程状态** |
| LunaTV `report.md` | `成功次数/失败次数/成功率/最近7天趋势`（30 天窗口） | **历史统计** |

混用会出现「成功率 96.7% 但 `status=pending`」这种行。
建议口径：评分里的「稳」取**历史统计**，页面状态灯只取**本次判定**。

**落到本项目**：我们的 `health.jsonl` 是**每用户本地**数据（不进仓库），
与 `Guovin/iptv-api`「产物不得提交回 Git」的立场一致，也自然绕开了 §7.5 的 60 天停用问题。
**别为了做状态灯把检测结果写进仓库** —— 那是把别人踩过的两个坑一起踩。



