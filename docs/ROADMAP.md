# 路线图（2026-10-11 重订）

> 这份**取代**了 `AGENTS.md` 里的「阶段一 / 阶段二」说法，以及 `docs/BT_APP_DESIGN.md`（已删除）。
> 重订的原因不是进度变化，而是**目标被问清了** —— 旧路线建立在一条被推翻的隐含假设上。
> 决策编号用 **D**（`AGENTS.md` 表里的 Q1~Q6 是 2026-10-08 的旧编号，其中 **Q2 / Q6 已作废**，见 §5）。

---

## 0. 为什么重订

旧路线是这么推出来的：

```
阶段一 = Rust 核心 + 复用现有 WebView 前端（public/ 一行不改）
阶段二 = 才决定要不要上 Slint
```

它的隐含假设是：**「能搜到东西」本身是目标**，所以先把核心搬齐、前端凑合复用就行。

2026-10-11 的一轮追问把这条假设问掉了：

| 你的回答 | 推论 |
|---|---|
| 这个工具**基本不用**（重写本身才是目的） | 搜索不是你的刚需 |
| 「**体积小 / 内存低**」是**硬需求** | 成品属性是硬指标 |
| 现在的 Web UI **能用但想换** | 终局形态是原生应用 |
| 自用 + 小圈子，GitHub Release 分发 | 不需要公开维护线，但需要「下载即用」 |

合起来：**真正的目标是「又小、又快、原生的桌面成品」，搜索只是它的载体。**

于是旧路线有两个问题：

1. **「复用 WebView」永远给不出那个成品** —— 它只是把 89MB 的 sidecar 换掉，前端仍是网页跑在 WebView 里，且依赖 WebView2 运行时。
2. **它会多写一层终局用不上的东西** —— Slint 是**进程内直调 Rust**，不需要 HTTP 服务，也不需要 SSE。旧路线却把「axum + `/api/*` + SSE 事件契约」当成阶段一的主体。

→ 结论：**跳过 WebView 中间态，直接上 Slint**；核心做成**库**，Slint 是它唯一的消费者。

---

## 1. 目标与成功标准

**目标**：一个 Windows 桌面应用 —— **单文件、零运行时依赖、启动快、内存低**；能聚合搜索种子站，能把磁力推送到下载器。

**成功标准**（把「小快原生」变成可判定的东西）

| # | 标准 | 怎么判定 | 备注 |
|---|---|---|---|
| S1 | 打包成**单文件自包含 exe** | `dist/` 产物 + 看体积 | ✅ **2026-10-16 M7 实测：18,610,176 字节（18.0MB）**，已加 `lto=true`/`codegen-units=1`/`strip=true`。⚠️ M1 的 14.5MB 是 **spike/slint-ui 的 exe**（不是 bt-app）且**无静态 CRT** ⇒ **不可直接比**；有意义的对照是 **Electron portable ≈151MB** |
| S2 | **零运行时依赖** —— 拷到干净 Windows 直接双击能跑 | 实测（最好虚拟机/另一台机） | 🟡 **旁证已过**：`+crt-static` 下 exe 只依赖 Windows 系统 DLL（kernel32/user32/gdi32/opengl32/dwrite…），**无 vcruntime140/msvcp140**；`rust-size.yml` 把这条写成了**机器断言**。⏳ 正式判据仍需人工在干净 Windows 上双击一次 |
| S3 | 冷启动**明显快于** Electron 版 | 秒表 / 日志打点 | 建议目标 < 1s（**待你确认**） |
| S4 | 空闲内存**明显低于** Electron 版 | 任务管理器 | 建议目标 < 100MB（**待你确认**） |
| S5 | 中文标题 + 关键词高亮 + 自动换行**不崩** | M1 验证（Slint #13548） | ✅ **已过（2026-10-16）** |
| S6 | 搜索 → 结果列表 → 磁力 → 推送下载器 **全链路通过** | 手工走一遍 | MVP 的定义 |

⚠️ S3/S4 的数值是我给的建议，你可以改。关键不是数字，而是**它们必须可判定** —— 否则「小快原生」会退化成感觉。

---

## 2. 架构

### 2.1 crate 布局（5 个 + 1 个二进制）

```
crates/bt-core/         领域类型 / normalize / http / dom / og-meta        ✅ 已有
crates/bt-providers/    各站点引擎 + 注册表 + **聚合调度**                  ✅ 已有 16 个
crates/bt-downloaders/  qB / Transmission / aria2·Motrix / Gopeed          ✅ 已有（M3，4 家）
crates/bt-app/          **Slint 应用（二进制）** —— 唯一的 UI               ✅ 已有（M4）
crates/bt-torznab/      Torznab 索引器（Jackett / Prowlarr / *arr）        🟡 **解析层已完成**；请求层/Provider/接入见 §2.4
```

**三处要说明的取舍：**

- **聚合调度放 `bt-providers`，不放 `bt-app`。** 它是「并发跑一批 provider 并把结果合起来」，只依赖 provider 的元数据（enabled / paginated / browseable），跟 UI 无关。放这里就能**不依赖 Slint 单独测试** —— 这点很重要，因为本机跑不了 cargo，测试是 CI 上唯一的信号。
- **`bt-torznab` 于 2026-10-16 开建**（D17 原本是「本期不做，只留口子」）—— 现在的状态是**解析层已落地**：
  `parse_items(xml) -> Vec<RawItem>`（`roxmltree`），字段映射逐条抄自 `src/providers/torznab.js`，
  fixture 在 `test/fixtures/torznab-search.xml` + 校验脚本 `scripts/check-torznab-fixture.py`。
  ⚠️ **不能用 `bt_core::dom`**：JS 用的是 cheerio 的 `xmlMode: true`，
  而 html5ever 没有 XML 模式，自闭合的 `<torznab:attr ... />` 会被"打开"成后续兄弟的父节点。
- **`bt-app` 的动态 provider 追加**（`add_dynamic_factory`）是 Torznab 接入的前提 ——
  现在 `provider_set()` 只支持"注入**替换**"，没有"追加"。


### 2.2 数据流（全进程内，**没有 HTTP**）

```
Slint UI ──callback──▶ bt-app (Rust)
                          │
                          ├─ bt_providers::aggregate::search_all(query, opts)   8 路并发 ──▶ 各站点
                          ├─ bt_providers::<site>::resolve_magnet(detail_url)   惰性取磁力
                          ├─ bt_core::og::fetch_details(url)                    海报 / 描述
                          └─ bt_downloaders::push(kind, cfg, magnet)            推送到下载器
                          │
        ▲ 进度回流 ─────────┘  slint::invoke_from_event_loop（每站点一回来就更新 UI）
```

⚠️ **线程模型是这个架构最容易写错的地方**（也是原来 SSE 承担的工作）：

- 网络 IO 必须在 **tokio 运行时**上跑；Slint 的 UI 是**单线程**的。
- 后台任务**不能直接改 UI 属性** → 必须 `slint::invoke_from_event_loop(..)` 或 `weak.upgrade_in_event_loop(..)` 把更新**跨回 UI 线程**。
- 「哪个站先回来就先显示谁的」这个体验，现在靠**每站点一个 callback**实现（原来靠 SSE 的 `event: provider`）。
- 别用「等全部跑完再一次性填」—— 那会让 8 路并发 + 慢站的等待完全暴露给用户。

### 2.3 存储

- **配置/收藏/历史**：本地文件（JSON）即可，别引数据库（YAGNI）。
- ⚠️ 沿用现有教训：**写文件用 写临时文件 + rename**（`torznabStore` 曾因裸 `writeFileSync` 在 EACCES/ENOSPC 时崩请求）。
- ⚠️ **别把任何密钥明文塞进仓库**：`.gitignore` 里 `data/` 的约定要继承。

### 2.4 `Provider` trait —— **现在该建了**（理由和上次不同）

`AGENTS.md` 里写着「暂时不引入 `Provider` trait，等 HTML 组落地后看清共性再定」，那是**对的时候**：当时的 10 份副本差异只是「站点名」，一个 `run_mirrors` helper 就够。

但**现在时机到了**，因为出现了三个真实的消费者：

1. **聚合层要并发遍历异构的 provider 列表**（16 个 + 未来动态插入的） → 需要统一签名
2. **D17 要给 Torznab 留口子** → 需要能插入一个非编译期固定的 provider（trait object / 工厂）
3. **`resolve_magnet` 只有 6/16 家有** → 这正是 trait **默认方法**要表达的东西（可选能力），之前用不着是因为没人需要「统一地调用一个可能不存在的方法」

→ 建议形状（**待你在评审时确认**）：

```rust
// ⚠️ 这是 2026-10-11 的**草案**。2026-10-16 开工前勘明后有两处修订，见下：
//   1) `async fn` → **boxed future**：AFIT 让 trait **不能做 trait object**，
//      而 D17 的 Torznab 口子要插入**带状态**的 provider（URL + API key）
//      ⇒ 必须是 `Box<dyn Provider>`。详见 `AGENTS.md`「M2 开工前勘明」⑧。
//   2) `"no_resolver"` → **`"no resolver"`**：核对 `server.js`，JS 的 404 文案是
//      带空格的 `no resolver`，且它是**端点级**行为（不是 provider 返回值）。
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn paginated(&self) -> bool { false }      // 单个 provider 覆盖
    fn browseable(&self) -> bool { false }
    async fn search(&self, q: &str, page: u32) -> SearchOutcome;
    /// 默认：不支持惰性解析磁力（照抄 JS —— 没有 resolver 就是没有）
    async fn resolve_magnet(&self, _url: &str) -> MagnetOutcome {
        MagnetOutcome::err("no resolver")
    }
}
```

⚠️ **另有一处必须改的既有类型**：`SearchOutcome.has_more` 现在是 `bool`，
但 JS 里 `hasMore` 可能是 `undefined`（provider 没表态），而 `undefined` 与显式 `false`
在 `aggregateHasMore` 里走**不同**分支 ⇒ 得改成 **`Option<bool>`**。
爆炸半径实测 = `lib.rs` 3 行、测试 0 处、66 个 `SearchOutcome::ok/err` 调用点不受影响。

⚠️ **注意**：引入 trait 会让 16 个 provider 各加一个 impl，**代码量是净增**。这不是为了省行数，是为了让聚合层和 Torznab 接口成立。如果评审时你认为不值，替代方案是「注册表存函数指针 struct」—— 但那样 `resolve_magnet` 的可选性会难看。

---

## 3. 里程碑

| 阶段 | 内容 | 验收 | 状态 |
|---|---|---|---|
| **M0** | `run_mirrors` 提取（消 10 份副本，净 −73 行） | CI 绿 | ✅ **已完成** |
| **M1** | **Slint 风险验证**（#13548 + 体积 + 虚拟化） | 见 `AGENT_HANDBOOK.md` §1 的 6 条及格线 | ✅ **已完成（2026-10-16，#13548 未复现）** |
| **M2** | 聚合层（`aggregate` 模块）+ `Provider` trait | 与 JS 的 3 条语义逐条一致 + CI 测试 | ✅ **已完成（2026-10-16，347 tests / 9 条新验收）** |
| **M3** | `bt-downloaders`（4 个客户端） | 4 家的 push / test / detect 都有离线测试 | ✅ **已完成（2026-10-16，34 条离线测试）** |
| **M4** | `bt-app` Slint MVP | S6 全链路手工走通 | ✅ **已完成（2026-10-16：UI + 接线 + 映射层 + 全链路 `--selftest` 12/12）** |
| **M5** | 引擎体检 → 砍掉死的 | 产出一张「谁还活着」的表 + 砍完的注册表 | ✅ **已完成（2026-10-16：表有了，结论是「一个都不砍」——单边 403 不足以判死，见 AGENTS.md「provider 探活」）** |
| **M6** | **差分验证**（与 Node 逐字段比对） | 差异全部命中白名单 | ✅ **已完成（2026-10-16）**：16 家全量、**0 处未豁免差异**、246 处命中白名单（9 条有意 divergence）；过程中抓到 1 个真移植 bug。门已放开到全量 |
| **M7** | 打包 + GitHub Release | S1/S2 达标，小圈子能下载到 | ✅ **已完成（2026-10-16）**：S1 达标（18.0MB，`rust-size.yml` 有 25MB 回归门）；S2 机器断言过（无 MSVC 运行时依赖）；release 已发布（`v0.3.6-beta` → `v0.3.8-beta` → **`v0.3.10-beta`**；`v0.3.9-beta` 因 `--locked` 撞上未同步的 `Cargo.lock` 而**没发出去**，其 tag 已删）。⚠️ **S2 仍需人工在干净 Windows 上双击一次**（见 §1 的 S2 行） |

**顺序上有三条硬约束，别调换：**

1. **M1 必须在 M2 之前。** Slint 挂了，M2 的整个架构（库 + 进程内直调）就得重新考虑。它是唯一的一票否决项，所以放最前。→ ✅ **已兑现（2026-10-16）**：M1 通过，架构不必重做。
2. **M6 必须在「删 Node 版」之前。** 删了 Node，D18 说的「逐字段一致」这条验收线**永远无法再验证**。
3. **M5 必须在 M2/M6 的"补引擎"之前。** 现在没有理由继续搬（D13）—— 先知道谁还活着，再决定投不投。

**M4 之后就可以自己用了**；M5~M7 是把「能用」变成「能给人用」。

---

## 4. 风险登记

| # | 风险 | 影响 | 应对 |
|---|---|---|---|
| **R1** | **Slint #13548** —— 内联样式跨软/硬换行 + 换行后是多字节字符 → parley panic。「中文标题 + 关键词高亮 + 自动换行」**正中靶心** | **一票否决 Slint** | ✅ **已实测解除（2026-10-16）**：200 条中文 + 50 条西里尔、580 次强制同步重绘，**未复现**。口径与踩坑见 `AGENTS.md`「Slint 探针结论」；Plan B 仍留在 `AGENT_HANDBOOK.md` §1.4 备用 |
| **R2** | **「不测 Tauri + Rust 核心的体积」是知情接受的风险** —— 已知 89MB 是「Tauri + node sidecar」，而 Tauri 壳本身很小；若它也是 ~8MB，押 Slint 可能是白冒 R1 | 可能做了无谓的赌注 | ⚠️ **数字变了（2026-10-16）**：M1 实测自包含 exe = **14.5MB**，不是留档的 6.7MB（2.2×，原因未查明；profile 只设了 `opt-level = 2`，无 lto / 无 strip）。**这条风险的账要按 14.5MB 重算**，S1 的判定口径也跟着改 |
| **R3** | 「小快原生」与「引擎越多越好」本质冲突：引擎多 = 启动慢、包大、维护重、失效多 | 成品属性被稀释 | M5 摸存活后砍；D13 |
| **R4** | **参考物窗口**：删掉 Node 版后，「与 Node 逐字段一致」再无真值 | 验收线永久失效 | M6 必须早于删除；`test/fixtures/` 永久保留（D18） |
| **R5** | Slint 生态比 Web 窄，未知坑多 | 踩坑成本 | 已知坑全部写进 `AGENT_HANDBOOK.md` §1（`styled-text` / 别在 `init` 写全局属性 / 切主题要重建模型 / 无 wrap 属性） |
| **R6** | 自建 UI + 自建核心 = 两件事都从零 | 周期长、可能半途而废 | 核心已有 16 个 provider 的家底；**UI 只做 MVP**（S6），不做花活 |

---

## 5. 决策登记（canonical）

**用 D 编号。** `AGENTS.md` 表里的 Q1~Q6 是 2026-10-08 的旧编号 —— 保留以免翻历史时对不上，但**作废项以本文为准**。

| # | 决定 | 状态 |
|---|---|---|
| **D1** | 平台：Windows-only（代码里别写死平台相关） | 沿用 |
| **D2** | 产品形态：桌面原生；不做浏览器访问、不做对外 HTTP 服务 | 沿用（旧 Q1） |
| **D3** | ~~「先不动 UI，复用 `public/` 前端把核心跑通」~~ | ❌ **作废**（旧 Q2，见 D8） |
| **D4** | ~~WebView ↔ Rust 通信方案（同源 axum vs Tauri IPC）~~ | ❌ **作废**（旧 Q6 —— 没有 WebView 就没有这个问题） |
| **D5** | **切 `main` 的门 = 「聚合层跑通」**（口径 C）。不要求 40 个引擎齐，也不把差分当硬门 | ✅ 已拍板 |
| **D6** | 站点改版怎么发现：CI 只跑离线 fixture（稳定的门）+ 手动触发的联网冒烟（允许失败） | 沿用（旧 Q5） |
| **D7** | **主目标 = 「小、快、原生的桌面成品」**；搜索是载体，不是目的 | ✅ 新 |
| **D8** | **终局 UI = 直接上 Slint** —— 跳过 Tauri 实测、跳过 WebView 中间态 | ✅ 新 |
| **D9** | **架构：核心是库；不写 HTTP、不写 SSE**；Slint 进程内直调 | ✅ 新 |
| **D10** | 目标用户：**自用 + 小圈子**（不公开发布/不追求陌生人可用） | ✅ 新 |
| **D11** | 当前使用强度：**基本不用** —— 重写本身就是目的 | ✅ 新（支撑 D13/D14） |
| **D12** | 「体积小 / 内存低」= **硬需求** | ✅ 新（支撑 D8） |
| **D13** | **引擎策略：先摸存活 → 砍掉死的；不再盲搬**（"多"只是感觉，不是需求） | ✅ 新 |
| **D14** | **MVP = 搜索 + 推送下载器**（Torznab 不在内） | ✅ 新 |
| **D15** | 分发：**GitHub Release**（朋友自助下载） | ✅ 新 |
| **D16** | 推送客户端：**qB / Transmission / aria2·Motrix / Gopeed 四个都要** | ✅ 新 |
| **D17** | **Torznab 本期不做，但引擎抽象要留口子**（它是"引擎腐坏"的战略解药：用 Jackett/Prowlarr 的维护换自己的维护） | ✅ 新 |
| **D18** | **Node 版留到差分验证（M6）做完再删**；`test/fixtures/` **永久保留** | ✅ 新 |
| **D19** | Slint #13548 风险**立刻验**（= M1） | ✅ 新 |

**小项的默认值**（不想要就说，否则按此执行）：

| # | 默认 | 依据 |
|---|---|---|
| d1 | 镜像回退保持**顺序** `await`（不改成并行） | 已在 `run_mirrors` 文档注释里写明这是**有意 divergence**（JS 是并行） |
| d2 | 把「抓页面第一条 `magnet:`」提成独立函数 → 顺手修好 **nyaa 取不到磁力** | 原实现挂在已死的 1337x 上，属功能性失效 |
| d3 | **不做**端到端差分（G2） | D9 之后没有两个后端可对 —— 这个需求随 HTTP 一起消失了 |
| d4 | 「前端没在用的端点」**不存在了** | 同上，没有 HTTP 就没有端点 |

---

## 6. 不做什么（YAGNI）

- ❌ 不做浏览器 / HTTP 服务 / SSE（D2、D9）
- ❌ 不做 Torznab（D17；只留抽象口子）
- ❌ 不建 `bt-torznab` crate（没有消费者）
- ❌ 不建单独的聚合 crate（约 150 行，放 `bt-providers` 就够）
- ❌ 不搬剩余 24 个 provider（D13：先摸存活）
- ❌ 不引并发库（Tokio `Semaphore` 就是现成的）
- ❌ 不做结果持久化数据库（JSON 文件够）
- ❌ 不做阶段二的「Slint 再评估」—— Slint **就是**终局（D8）

---

## 7. 文档地图

| 文件 | 角色 | 谁看 |
|---|---|---|
| `AGENTS.md` | **agent 契约**：禁改清单、进度表、上游 bug 账、CI 失败账 | agent（自动加载） |
| **`docs/ROADMAP.md`** | **本文** —— 目标 / 架构 / 里程碑 / 风险 / 决策（why & where） | 你 + agent |
| **`docs/AGENT_HANDBOOK.md`** | **每步怎么做**：命令、验收标准、Slint 及格线、Plan B | agent（动手前必读） |
| `CLAUDE.md` | 快速上手指南 | agent |
| `README.md` | 用户视角 | 你 + 小圈子 |

⚠️ **已删除**：`docs/BT_APP_DESIGN.md` —— 它建立在「阶段一 = Rust 核心 + WebView」之上，一半内容（axum、`/api/*`、SSE 事件契约、端点清单、D1 决策）随 D8/D9 一起作废；仍有效的部分（聚合要复刻的 JS 语义、差分方案、trait 理由）已并入本文与 `AGENT_HANDBOOK.md`。
