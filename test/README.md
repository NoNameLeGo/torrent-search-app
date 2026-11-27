# 测试数据目录（test/）说明

⚠️ **这里已经没有 Node 测试运行器了**（2026-11-27 退役 Node/Electron 时一并删除）。

## 现在还剩什么

| 路径 | 作用 |
|---|---|
| `test/fixtures/` | **永久保留**的离线快照。Rust 侧测试直接读它：`crates/bt-providers/tests/*.rs`（每家一份）、`crates/bt-core/tests/dom_probes.rs`（`html-probes.expected.json` 是 cheerio 给的真值）。 |
| `test/fixtures/html-probes.expected.json` | HTML 选择器真值。**生成器已随 Node 一起删除**（曾由 `.github/workflows/html-probes.yml` 跑 cheerio 生成）⇒ 现在只读不写，若要更新只能从 git 历史里取回旧的生成脚本。 |

## 怎么跑测试

测试全在 Rust 侧：

```bash
cargo test --workspace          # 全部（本机没有 cargo，走 CI 的 rust.yml）
cargo test -p bt-providers      # 只跑 provider 解析
```

## 怎么加一份新 fixture

用 `.github/workflows/fetch-fixture.yml` 手动触发（它用 `curl`，不依赖 Node）：

```bash
scripts/gh-retry.sh workflow run fetch-fixture.yml --ref main \
  -f url='https://nyaa.si/?f=0&c=0_0&q=ubuntu' -f path='test/fixtures/nyaa-ubuntu.html'
```

⚠️ 抓到的东西**先看一眼内容**再当 fixture 用 —— Cloudflare 挑战页 / 停放页都是 200 且几千字节。
