# Slint spike —— M1 风险验证

> 这个目录回答一个问题：**`docs/ROADMAP.md` D8「直接上 Slint」这一步，会不会因为
> Slint #13548 而直接作废？**
>
> 结论要写回 `AGENTS.md` 的「Slint 探针结论（留档）」；操作手册见
> `docs/AGENT_HANDBOOK.md` §1。

## 为什么要有 `--selftest`

2026-10-07 那版 spike（分支与 workflow 已于 2026-10-09 删除，代码在本次重建中找回）
**只被 CI 编译过，从来没有被运行过** —— 于是「编译通过」被当成了「渲染没问题」，
而 #13548 恰好只在**真的跑文本布局**时才 panic。这是 M1 一直没闭合的原因。

现在 workflow 多了两条硬步骤：

1. `cargo build --release --locked`
2. `slint-spike.exe --selftest` —— **真的把窗口开起来、真的滚、真的渲染**

自检跑不到底 = 进程死 = CI 红。

### ⚠️ 为什么要「强制同步重绘」

第一版自检只靠定时器 + 事件循环重绘，结果 440 步只花了 **0.38s**（≈0.86ms/步）——
全窗口 1180×800 的软件重绘不可能这么快。也就是说：`ListView` 的**实例化**确实发生了
（行高固定 80px，建实例不用量文本），但**重绘没发生**，而 parley 的 #13548
panic 发生在**绘制期**的文本布局里。**那一轮的「绿」其实什么都没证明。**

现在每步都调一次 `Window::take_snapshot()`：它会**重新渲染整个场景**，
于是每步都真的走一次文本布局。日志里每行末尾的 `强制重绘 N` 就是它成功的次数。

退出码语义（CI 里会照着报错）：

| 码 | 含义 |
|---|---|
| 0 | 自检跑到底 = 六条里 1/2/3/4/6 都过 |
| 101 | 进程 panic —— 若正文含 `is not a char boundary`，就是 **#13548 复现** |
| 2 | **仪器失效**：一次强制重绘都没成功，本轮没测到绘制期布局，**结论无效** |

## 六条及格线怎么覆盖

| # | 及格线 | 覆盖方式 | 是「实测」还是「构造覆盖」 |
|---|---|---|---|
| 1 | 200 条中文标题 + 高亮 + 自动换行全部渲染、不 panic | 逐条滚过去，按卡片 `idx` **去重统计实际被实例化的条目** | 实测（覆盖率 + 不 panic） |
| 2 | 关键词落在第 2 行也 OK | 中文前缀长度 50..=109 逐字扫描，关键词落点扫过整行并越过软换行边界 | **构造覆盖** |
| 3 | 关键词正好压在换行边界也 OK | 同上，步长 1 个汉字 | **构造覆盖** |
| 4 | 西里尔也过一遍 | 50 条真实俄语标题，提取自 `test/fixtures/megapeer-1080p.html`（windows-1251） | 实测（覆盖率 + 不 panic） |
| 5 | 报出实测自包含 exe 体积 | `slint-spike.yml` 的「报告体积」步骤 | 实测 |
| 6 | `ListView` 虚拟化生效 | 2000 条只读不滚，读实例化数 | 实测 |

⚠️ **2/3 不许写成「实测」**：Rust 侧拿不到字体度量，无法反查关键词落在第几行。
这两条是**构造**保证覆盖（扫描必然经过边界），真正被实测的是「这些情形会不会 panic」。

## 怎么读 CI 结果

- 绿 = 进程跑到底 = 六条里的 1/2/3/4/6 都过了（体积在日志里）
- 红 = 看日志里的 `[m1][PANIC]`：
  - 正文含 `is not a char boundary` → **#13548 复现**，走 `AGENT_HANDBOOK.md` §1.4 的 Plan B
  - 出事位置 = 最后一行 `[m1][...]` 步进日志的 `step` / `覆盖` —— 那就是滚到哪炸的
- 产物：`m1-selftest-log`（原始日志）、`slint-spike-windows-x64`（可双击人工复验）

## 本机怎么跑

**跑不了，也别试。** 本机没有 `cargo`（`CARGO_HOME` 已清），且用户明确要求
「不要在本地新增构建环境」。一切编译与运行都在 CI 里。

本机能做的只有格式化：

```bash
"/d/Vibe-Coding/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin/rustfmt.exe" \
  --check --edition 2021 spike/slint-ui/src/main.rs spike/slint-ui/src/m1.rs
```

## 文件

| 文件 | 说明 |
|---|---|
| `ui/app.slint` | 界面。⚠️ 富文本属性必须是 `styled-text`（不是 `string`）；高亮色烘进 markup，所以切主题要整体重建模型 |
| `src/main.rs` | 交互式 GUI（默认）+ 共享的 `highlight()` / `md()` / `hl_hex()` |
| `src/m1.rs` | `--selftest` 自检驱动 |
| `Cargo.lock` | 568 个包，`--locked` 构建；**不要**手改，要加依赖时按 workflow 里的注释走 |

## 已知坑（别重踩）

- ❌ `@markdown("\{expr}")` —— 官方文档原文 "Any text passed as an argument to the macro
  will be escaped"，插值进去的内容**永远不参与解析**。运行时富文本只能走
  `slint::StyledText::from_markdown()`。
- ❌ 别在 `.slint` 的 `init` 里写全局属性 → 属性依赖环，旧版实测 200 条触发 **2628 次**实例化。
  计数一律走 `callback` 进 Rust 的 `Cell`。
- ❌ `StyledText` 没有 `wrap` 属性。
- CI 的 Windows runner 没有 GPU → 必须 `SLINT_BACKEND=winit-software`（真窗口 + 软件渲染器）。
