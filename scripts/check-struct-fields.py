#!/usr/bin/env python3
"""守卫：测试里访问的字段，结构体里真的存在吗？

## 为什么需要它

2026-10-16 红了一轮：CI 报 `E0609: no field 'browseable' on type '&ProviderInfo'` ——
我在测试里写了 `demo.browseable`，而 `ProviderInfo` 只有
`{ id, name, enabled, demo, paginated }`（与 JS 的 `list()` 一致）。

"断言值 vs 真值"那类守卫抓不到这种错：它是**编译错**，不是值错。

## ⚠️ 这个守卫自己错过两次（都记下来）

1. 第一版直接正则扫 `demo.<名字>` —— 把**注释里**的 `demo.browseable`
   和文件名 `demo.js` 都算成越界字段 ⇒ 误报。
   ⇒ 现在**先剥注释再扫**。
2. 第二版把变量名放宽到 `p` / `info` —— 结果 `p` 在 HTML/CSS 一类的字符串里
   到处都是（扫出了个 `p.js`）⇒ 又误报。
   ⇒ 现在只扫**明确列出的变量名**（下面 TARGETS 里的 `vars`），
      宁可窄：它只要覆盖"我实际会写错的那几个"就够了。

守卫自己写错的代价和白写一遍一样（一轮 CI + 上下文重建）。

用法：python scripts/check-struct-fields.py
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# (结构体文件, 结构体名, 扫描文件, 变量名清单)
TARGETS = [
    (
        ROOT / "crates/bt-providers/src/registry.rs",
        "ProviderInfo",
        ROOT / "crates/bt-providers/tests/aggregate.rs",
        ["demo", "demo_info"],
    ),
]


def strip_comments(text: str) -> str:
    """剥掉行注释与文档注释（⚠️ 粗糙但够用：这些文件里字符串字面量不含 `//`）。"""
    out = []
    for line in text.splitlines():
        if line.strip().startswith("//"):
            continue
        idx = line.find("//")
        out.append(line[:idx] if idx >= 0 else line)
    return "\n".join(out)


def fields_of(path: pathlib.Path, struct: str) -> set:
    src = strip_comments(path.read_text(encoding="utf-8"))
    m = re.search(rf"pub struct {struct} \{{(.*?)\}}", src, re.S)
    if not m:
        print(f"  FAIL 找不到 pub struct {struct} in {path.name}")
        sys.exit(1)
    return set(re.findall(r"pub (\w+):", m.group(1)))


# ⚠️ 启发式的必要补丁：`demo.js` 这种**文件名**会匹配成"字段 js"。
# 这是第三次修正了 —— 守卫写宽的代价就是不断误报，所以这里选择
# "显式列出已知的非字段后缀"，而不是继续加正则的花样。
FILE_EXTENSIONS = {
    "js", "cjs", "mjs", "rs", "py", "sh", "json", "toml", "md", "xml", "slint", "yml", "yaml",
}


bad = 0
for struct_file, struct, use_file, var_names in TARGETS:
    fields = fields_of(struct_file, struct)
    src = strip_comments(use_file.read_text(encoding="utf-8"))

    accessed = set()
    for v in var_names:
        for m in re.finditer(rf"\b{re.escape(v)}\.(\w+)", src):
            # 方法调用带括号，字段访问不带
            if re.match(r"\s*\(", src[m.end():]):
                continue
            # 文件名（demo.js）不是字段访问
            if m.group(1) in FILE_EXTENSIONS:
                continue
            accessed.add(m.group(1))

    unknown = sorted(a for a in accessed if a not in fields)
    print(f"  {struct} 字段: {sorted(fields)}")
    print(f"  {use_file.name} 里 {var_names} 访问: {sorted(accessed)}")
    if unknown:
        bad += len(unknown)
        print(f"  FAIL 越界字段: {unknown}")
    else:
        print("  OK 没有越界字段")

print()
print("no unknown fields" if bad == 0 else f"{bad} UNKNOWN FIELDS")
sys.exit(0 if bad == 0 else 1)
