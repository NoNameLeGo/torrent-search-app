# CI 失败史与防线（2026-10-16）

> 这份文档的**唯一目的**是让同样的失败别再出现。
> 数据来自 `scripts/ci-failure-audit.sh`（它从 GitHub Actions 拉真实日志），**不是回忆**。

## 一、实测数据（最近 80 个 run 里的 9 次失败）

| # | run | sha | 失败步骤 | 错误签名 | 归类 |
|---|---|---|---|---|---|
| 1 | 38850568449 | `e22aa4d` | M6 全量比较 | `exit code 1`（差分 3 处） | 🟢 **正确的红** |
| 2 | 38855469827 | `07597cc` | cargo test | `error[E0433]` cannot find type `SearchOutcome` | 🔴 编译 |
| 3 | 38856398669 | `4abed4b` | cargo test | `slint build failed: Unknown property color in Button` | 🔴 Slint |
| 4 | 38858548083 | `004b90d` | cargo test | `assertion left == right` failed | 🔴 断言 |
| 5 | 38863602290 | `5fea93c` | cargo test | `panicked at ui_model::tests::a_slightly_stale_engine_is_not_marked` | 🔴 断言 |
| 6 | 38864914651 | `1957245` | cargo test | `error[E0106]` missing lifetime specifier | 🔴 编译 |
| 7 | 38865280107 | `abbd846` | cargo test | `error[E0425]` cannot find function `local_name` | 🔴 编译 |
| 8 | 38865613968 | `64f1fa3` | cargo test | `assertion left == right` failed: 只能走 guid 兜底 | 🔴 断言 |
| 9 | 38866143314 | `cce4e9c` | cargo test | `panicked at tests::entity_decoding_and_the_fallback_levels` | 🔴 断言 |

**归类计数**

| 类别 | 次数 | 占比 |
|---|---|---|
| 测试断言靠心算 | **4** | 44% |
| 本机没有编译器 → API/语法猜错 | 3 | 33% |
| 用了没验证过的 Slint 属性 | 1 | 11% |
| 正确的红（工具真的抓到了缺陷） | 1 | 11% |

**两条最重要的结论**

1. **8/9 是流程问题，不是设计问题。** 这一路的架构、类型、数据流几乎没错过 ——
   错的都是**机械性的、可以靠"先跑一遍"消除的**。
2. **9 次里有 8 次都倒在同一个步骤（`cargo test`）上** —— 说明这道门是有效的：
   它就是设计来把这些问题挡在 `main` 之前的（代价是每轮 ~3 分钟 CI + 上下文重建）。

## 二、四个类别，逐个说清怎么避免

### A. 断言靠心算（4 次，最大的一类）

| # | 我以为 | 实际 |
|---|---|---|
| 4 | `relevance("ubuntu server iso", "ubuntu server") == 700` | **900** —— 它 `startsWith` ⇒ 基础分是 500 不是 300 |
| 4 | 分类 chip 按 `chips[1]` 是「电影」 | 同票按**名字**排，而中文是**码位序**（剧 U+5267 < 电 U+7535） |
| 5 | 插 `STALE_AFTER - 1` 就"不到阈值" | `apply_report` **先 +1 再用** ⇒ 正好顶到阈值 |
| 8 | 稀疏 item 的 `detailUrl` 走 guid | 它**同时有 link**，而兜底序是 comments → link → guid |
| 9 | `magnet` 里含 `&dn=Rock` | magnet 走 **enclosure**，`&dn` 在 **link** 上 |

**防线（成本最低、收益最大的一条）**：

> **只要断言依赖"某个函数内部怎么算"，就先在本机把那个算术跑一遍再写。**

现成的两条路子（都已用过，且**用过的都没返工**）：
- `node -e` 跑 JS 语义的算术（`relevance` 分档、`parseInt` 宽松语义）
- `python -c` 跑 XML/文件解析的事实（fixture 的兜底走向）

⚠️ **第 9 条有个额外教训**：我**上一刻刚用 Python 打印过**那两个值，
结论就在屏幕上，却只用在了下面一条断言上 —— **验过 ≠ 用上**。
核对要覆盖**所有受影响的断言**，不是"验一个点、改一个点"。

### B. 本机没有编译器 → API/语法猜错（3 次）

本机 `cargo` 不可用是**既定约束**（见 CLAUDE.md），所以只能靠纪律：

| # | 错在哪 | 防线 |
|---|---|---|
| 2 | 用了 `SearchOutcome` 却忘了进 `use crate::{...}` | **新增一个类型前，先看该文件的 `use` 清单**；改完 grep 一次"用到的符号 vs 已引的符号" |
| 6 | `fn local_name(n: Node<'_, '_>) -> &str` —— 匿名 `'_` 无法与返回关联 | **别抽只省几个字符的 helper**；参数是匿名生命周期时返回值就没法关联 |
| 7 | 删了 helper，**漏了第三处调用** | **删/改一个符号后 grep 全部引用**；且 🔴 见下面 |

🔴 **第 7 条真正的问题不是"漏了"**：自查输出**明明报了那一行**
（`⚠️ L86: ... local_name(*c) == "attr"`），我**看都没看就提交推送了**。
**报了不处理，比不查更糟 —— 它给了虚假的安全感。**

⇒ 所以现在的做法是**把守卫写成机械的**：提交前用脚本判断"该符号的调用是否归零"，
不为 0 就 `exit 1`，不依赖我的眼睛。

### C. 用了没验证过的 Slint 属性（1 次）

`Button` 没有 `color` 属性（std-widgets 只认 `text/enabled/icon/primary`）。

**防线**：`.slint` 里**只用 M1 实测过的元素与属性**（这条规矩写在 `app.slint` 文件头，
是我越界了）。要引入新控件/新属性，**先写进探针 workflow 实测**再进主 UI。

### D. 静默失败（不在 CI 日志里，但代价更大 —— 已记 7 种）

这一类**不会让 CI 变红**，会让**我以为它绿了**：

| 现象 | 真因 |
|---|---|
| `git push` 报 `Everything up-to-date` | `feat/tauri` **没有 upstream 跟踪**，裸 `git push` 静默失败 |
| `git commit` 报 `did not match any file(s) known to git` | 提交消息里的**反引号**被 PowerShell 当转义符（2 次）／**双引号**截断了字符串 |
| 我读到"CI 全绿" | 上面两个失败之后 `git push` 没做事，**我读的是上一个提交的绿 run** |
| `Cannot run a document in the middle of a pipeline` | 在 **PowerShell 里跑 bash 脚本**（`scripts/gh-retry.sh`）—— 重试循环空转 25 次，**workflow 触发根本没发生** |
| `WinError 193: %1 不是有效的 Win32 应用程序` | **Python 在 Windows 上执行 `.sh`** —— 同样是"调了跑不起来的脚本" |
| `gh run view --json artifacts` 返回**空** | **那个字段根本不存在**（`run view` 没有 `artifacts`）⇒ 我据此报告"产物列表是空的"，差点得出"没有 artifact"的结论（实际 `gh api` 一查就有） |
| 自查脚本报了 `⚠️ L86: ...` | 输出就在眼前，**我看都没看就提交推送了** ⇒ CI `E0425`。**报了不处理，比不查更糟** |

**防线（都已落地）**：
1. **提交消息一律走文件**：`git commit -F .cache/commitmsg.txt` —— 不走 shell，就没有引号问题
2. **推分支显式写 `git push origin <branch>`**（本仓库 `main`/`feat/tauri` 都可能没 upstream）
3. **别用 `Select-Object -Last 1` 过滤推送/命令输出** —— 出错时最后一行往往是空的
4. **每步核 `git log` / `git status` / 与远端是否一致**，不拿"CI 绿"当本次改动的证据
5. **工具要用对执行器**：bash 脚本交给 **bash 执行器**（`ops_bash` + `script_path`），
   不要从 PowerShell 调 —— 长脚本一律写成 `.sh` 文件再跑
6. **空结果 ≠ 不存在**：查询返回空时**换一个 API 再问一次**再下结论
   （`gh run view --json artifacts` 空 → `gh api .../artifacts` 立刻有）
7. **自查输出必须看，而且要看懂**：更好的做法是把守卫写成**机械的**
   （脚本判断"该符号的调用是否归零"，不为 0 就 `exit 1` 不提交），不依赖眼睛

> ⚠️ 这一类的共同形状：**失败不报错，只是"什么都没发生"**。
> 所以判据不能是"命令没报错"，而必须是"**我看到了它做成的证据**"
> （`git log` 里有那个 sha、artifact 列表里有那个名字）。

## 三、怎么重跑这份审计

```bash
scripts/ci-failure-audit.sh 80      # 参数是"最多看几个 run"
```

它做三件事：拉失败 run → 取**失败的步骤名** → 从日志里挑**错误签名**并归类。
⚠️ 实现上有两条踩过的坑写在脚本头注释里：**bash 负责调 `gh`、Python 只负责解析**
（Python 在 Windows 上执行不了 `.sh`，会 `WinError 193`），以及 `gh` 要包 `gh-retry.sh`。

## 四、一句话总结

> **设计几乎没错过；错的全是机械性的。**
> 断言先跑一遍、Slint 只用验过的属性、删符号就 grep 全部引用、
> **自查输出必须看**、提交消息走文件、别让输出过滤吃掉退出码。

> 还有一条贯穿全篇的：**这个环境里的失败不报错，只是"什么都没发生"**
> （push 报 up-to-date、查询返回空、脚本根本没跑起来）。
> 所以每步都要问一句：**我看到的"成功"，是它做成了的证据，还是它什么都没做？**
