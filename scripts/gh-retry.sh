#!/usr/bin/env bash
# gh 的自动重试包装。
#
# ## 为什么需要它
#
# 本机有一层**透明拦截代理**（`Via: Caddy`），对 `api.github.com` 的请求会
# **间歇性直接返回 403**。实测 2026-10-09：
#
#   走本机代理(:1267)  9/10 → 8/12 → 4/12   （会波动）
#   走本机代理(:80)                    5/12
#   直连(绕代理)        7/10
#
# 也就是说 **哪条路都不稳**，而且 403 是**成簇**出现的（见过连续 5 次）。
# 失败特征很好认：**0.08 秒秒拒**（成功的要 0.4~1.0 秒）、响应体为空、
# `Content-Length: 0`、带 `Via: Caddy`。
#
# 所以这**不是 GitHub 拒绝、也不是 token 问题** —— token 完全正常
# （`gho_` 格式，scope 含 `repo` / `workflow` / `admin:org` 等，`gh auth status` 是 ✓）。
# 表现就是 `gh run list` / `gh api ...` / `gh workflow run` 随机报
# `HTTP 403: 403 Forbidden`，**重试即可**（已实测：`gh workflow run live-smoke.yml`
# 靠这个包装一次成功）。
#
# ⚠️ 旧笔记里「`gh` 必须绕开代理（加 `env -u HTTP_PROXY ...`）」的说法**已不成立** ——
# 那是 2026-10-07 在另一个代理实例上观察到的；现在绕不绕都会间歇 403。
#
# ## 用法
#
# 把命令里的 `gh` 换成这个脚本，其余参数原样传：
#
#   scripts/gh-retry.sh run list --branch feat/rust
#   scripts/gh-retry.sh run view --job=<JOB_ID> --log
#   scripts/gh-retry.sh workflow run live-smoke.yml --ref feat/rust
#
# 可用环境变量：`GH_RETRY_MAX`（默认 10）、`GH_RETRY_DELAY`（默认 2 秒）。
# 按实测约 50% 失败率 + 成簇特性，默认值下最坏约 18 秒、基本必成。
#
# ## 设计取舍
#
# - **只在输出里出现 `403` 时重试**；其它错误立刻透传，免得把真错误重试成噪音
# - **stdout 不做缓冲**，所以 `gh run watch` / `run view --log` 这类流式命令照常可用
# - **`gh auth ...` 直接透传**（交互式命令，重试会吞掉提示）
# - `gh run view --log` 的日志走 stdout，不受这里重定向 stderr 的影响

set -uo pipefail

# 交互式命令不包装，直接透传
if [ "${1:-}" = "auth" ]; then
  exec gh "$@"
fi

MAX=${GH_RETRY_MAX:-10}
DELAY=${GH_RETRY_DELAY:-2}

errf=$(mktemp)
trap 'rm -f "$errf"' EXIT

n=0
while :; do
  n=$((n + 1))
  if command gh "$@" 2>"$errf"; then
    exit 0
  fi
  rc=$?

  if [ "$n" -ge "$MAX" ] || ! grep -q '403' "$errf"; then
    cat "$errf" >&2
    exit "$rc"
  fi

  echo "[gh-retry] 第 $n 次撞上 403（本机代理的间歇性拒绝），${DELAY}s 后重试…" >&2
  sleep "$DELAY"
done
