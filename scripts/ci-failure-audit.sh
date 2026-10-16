#!/usr/bin/env bash
# 审计 CI 失败：失败的 run / 失败的步骤 / 错误签名。
#
# ⚠️ 三条都是踩过的坑：
#   1. 写成脚本文件（内联会被 PowerShell 吃掉 $、反引号、引号 —— 栽过三次）
#   2. **bash 负责调 gh，Python 只负责解析** —— Python 在 Windows 上执行不了 .sh
#      （WinError 193），所以别让 Python 去 subprocess 调 gh-retry.sh
#   3. gh 要包 gh-retry.sh（本机间歇 403）
set -uo pipefail
cd "$(dirname "$0")/.."

GH=scripts/gh-retry.sh
LIMIT="${1:-80}"
mkdir -p .cache/ci

echo "=== 最近 $LIMIT 个 run ==="
$GH run list --limit "$LIMIT" --json databaseId,workflowName,conclusion,headSha,createdAt \
  > .cache/ci/runs.json 2>/dev/null || { echo "gh run list 失败"; exit 1; }

IDS=$(python -c "
import json,pathlib
rs=json.loads(pathlib.Path('.cache/ci/runs.json').read_text(encoding='utf-8'))
print(' '.join(str(r['databaseId']) for r in rs if r.get('conclusion')=='failure'))
")
COUNT=$(echo "$IDS" | wc -w)
echo "  失败 run: $COUNT"
[ "$COUNT" = "0" ] && exit 0

echo "=== 拉每个失败 run 的 jobs 与日志 ==="
for id in $IDS; do
  [ -s ".cache/ci/jobs-$id.json" ] || $GH run view "$id" --json jobs > ".cache/ci/jobs-$id.json" 2>/dev/null
  [ -s ".cache/ci/log-$id.txt" ]   || $GH run view "$id" --log     > ".cache/ci/log-$id.txt"   2>/dev/null
  echo "  $id ok"
done

python - <<'PY'
import json, pathlib, re

ci = pathlib.Path(".cache/ci")
runs = {str(r["databaseId"]): r for r in json.loads((ci / "runs.json").read_text(encoding="utf-8"))}

PATTERNS = [
    (r"error\[E\d+\][^\n]*",        "rustc"),
    (r"slint build failed[^\n]*",   "slint"),
    (r"Unknown property[^\n]*",     "slint"),
    (r"cannot find function[^\n]*", "rustc"),
    (r"assertion[^\n]*failed[^\n]*","assert"),
    (r"panicked at[^\n]*",          "panic"),
    (r"##\[error\][^\n]*",          "step"),
    (r"FAILED\.[^\n]*",             "test"),
    (r"^error: [^\n]*",             "rustc"),
]

print("\n=== 失败的 run / 步骤 / 签名 ===")
summary = []
for id, r in sorted(runs.items(), key=lambda kv: int(kv[0])):
    if r.get("conclusion") != "failure":
        continue
    sha, wf = r["headSha"][:7], r["workflowName"]

    step = "(取不到)"
    jf = ci / f"jobs-{id}.json"
    if jf.exists() and jf.stat().st_size:
        try:
            jobs = json.loads(jf.read_text(encoding="utf-8"))["jobs"]
            bad = [s for j in jobs for s in j.get("steps", []) if s.get("conclusion") == "failure"]
            if bad:
                step = bad[0]["name"]
        except Exception as e:
            step = f"(解析失败 {e})"

    sig = None
    lf = ci / f"log-{id}.txt"
    if lf.exists() and lf.stat().st_size:
        text = re.sub(r"\x1b\[[0-9;]*m", "", lf.read_text(encoding="utf-8", errors="replace"))
        lines = [l.split("\t", 1)[-1].strip() for l in text.splitlines()]
        for pat, kind in PATTERNS:
            for l in lines:
                if re.search(pat, l):
                    sig = f"[{kind}] {l[:140]}"
                    break
            if sig:
                break

    summary.append((id, sha, wf, step, sig))
    print(f"\n  run {id}  {sha}  {wf}")
    print(f"    步骤: {step}")
    print(f"    签名: {sig or '(没匹配到)'}")

print("\n=== 按签名归类 ===")
buckets = {}
for _, _, _, _, sig in summary:
    key = "未匹配"
    if sig:
        key = re.sub(r"\b\d+\b", "N", sig[:60])
    buckets.setdefault(key, []).append(1)
for k, v in sorted(buckets.items(), key=lambda kv: -len(kv[1])):
    print(f"  ×{len(v)}  {k}")
PY
