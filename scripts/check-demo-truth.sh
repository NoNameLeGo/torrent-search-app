#!/usr/bin/env bash
# 机械守卫：demo.rs 的断言值必须与 node 跑出来的真值一致，全对才提交。
#
# ⚠️ 输出**只用 ASCII**：Python + emoji + GBK 控制台会 UnicodeEncodeError
#    ⇒ 检查全过、退出码却是 1。这个坑我在 docs/CI_FAILURES.md 里记过一次，
#    刚才用内联 python 又踩了一次（内联脚本拿不到那个 reconfigure 前导）。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

python - <<'PY'
import pathlib, sys
src = pathlib.Path("crates/bt-providers/src/demo.rs").read_text(encoding="utf-8")

# 左边 = node 跑 JS demo.js 得到的真值（scripts/demo-truth.cjs），右边 = 用途
checks = [
    ("ubuntu [720p] - S01E01 [TV]", "p0.name"),
    ("50_263_070_484", "p0.size"),
    ("Some(1983)", "p0.seeders"),
    ("Some(1246)", "p0.leechers"),
    ("ffb92c9167cb2c4a3bd6896fe88b0afcbbc298c9", "p0.info_hash"),
    ("27_669_761_040", "p1.size"),
    ("12_170_077_176", "p19.size"),
    ("13_882_762_656", "p2[0].size"),
    ("1_740_729_600_000", "p0.date (NOW - 686d)"),
    ("83ddb3201b19e650e514e72868850728bbd39f9d", "cjk p0.info_hash"),
    ("ubuntu%20%5B720p%5D%20-%20S01E01%20%5BTV%5D", "magnet dn encoding"),
]
bad = 0
for v, label in checks:
    ok = v in src
    bad += 0 if ok else 1
    print(f"  {'OK  ' if ok else 'FAIL'} {label} = {v}")

print()
print("demo assertions match the node truth" if bad == 0 else f"{bad} MISMATCHES")
sys.exit(0 if bad == 0 else 1)
PY
