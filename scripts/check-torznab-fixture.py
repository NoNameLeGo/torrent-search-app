#!/usr/bin/env python3
"""校验 test/fixtures/torznab-search.xml 与 AGENTS.md「⑤ Torznab 的规格」一致。

⚠️ 为什么是 Python 而不是 node：本机**没装 cheerio**（它只在 CI 里 npm ci 才装），
所以 node 版校验脚本一跑就 MODULE_NOT_FOUND —— 而那次我还差点当成"验过了"。
Python 的 `xml.etree` 是**标准库里的真 XML 解析器**，够用而且不依赖任何安装。

⚠️ 它顺带证明了一件**关键事**：`<torznab:attr ... />` 是**自闭合的空元素、
而且是 item 的兄弟节点** —— 不是被 HTML 规则"打开"后嵌套了后续兄弟。
那正是 `bt_core::dom`（html5ever，无 XML 模式）**不能**用来解析 Torznab 的原因。

用法：python scripts/check-torznab-fixture.py
"""
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

# ⚠️ Windows 控制台默认是 GBK，**emoji 和部分字符会让 print 直接抛 UnicodeEncodeError**
# —— 那会让"所有检查都通过"的脚本以退出码 1 结束，自动化就会误判成失败。
# 第一版就是这么挂的。统一改 UTF-8 输出，剩下的交给终端。
try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
except Exception:  # pragma: no cover - 老 Python 没有 reconfigure
    pass

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "test" / "fixtures" / "torznab-search.xml"

bad = 0


def check(label, got, want):
    global bad
    ok = got == want
    if not ok:
        bad += 1
    print(f"  {'OK  ' if ok else 'FAIL'} {label}: {got!r}" + ("" if ok else f"  (期望 {want!r})"))


def attr_of(item, name):
    """Torznab 把 seeders/peers/infohash/category 放在 <torznab:attr name=.. value=..> 上。"""
    for e in item:
        # 命名空间前缀会被解析成 URI，所以按 local name 匹配
        if e.tag.endswith("attr") and e.get("name") == name:
            return e.get("value")
    return None


def text_of(item, tag):
    e = item.find(tag)
    return None if e is None else (e.text or "").strip()


tree = ET.parse(FIXTURE)
root = tree.getroot()
items = root.findall("./channel/item")
print(f"item 数: {len(items)}")
check("item 数", len(items), 3)

# 结构证据：attr 是 item 的直接子节点（HTML 解析器会把自闭合标签打开、把它们变成后代）
first_children = [e.tag for e in items[0]]
print(f"item[0] 的直接子节点: {first_children}")
check(
    "torznab:attr 是 item 的**直接子节点**（自闭合被正确识别）",
    sum(1 for t in first_children if t.endswith("attr")),
    3,
)

# ---- item 0：标准形态 ----
it = items[0]
print("\n[0] 标准形态")
check("title", text_of(it, "title"), "Ubuntu 24.04 LTS Desktop amd64")
check("size 元素", text_of(it, "size"), "5200936960")
enc = it.find("enclosure")
check("enclosure@length", None if enc is None else enc.get("length"), "5200936960")
check("enclosure@url 不是 magnet", (enc.get("url") or "").startswith("magnet:"), False)
check("infoHash 走 torznab:attr", attr_of(it, "infohash"), "0123456789abcdef0123456789abcdef01234567")
check("seeders", attr_of(it, "seeders"), "128")
check("leechers 取的是 peers", attr_of(it, "peers"), "31")
check("detailUrl 走 comments", text_of(it, "comments"), "https://tracker.example/details/12345")
check("pubDate", text_of(it, "pubDate"), "Wed, 15 Oct 2026 09:12:00 +0000")

# ---- item 1：实体 + 兜底 + 0 值 ----
it = items[1]
print("\n[1] 实体 / 兜底 / 0 值")
check("标题按 XML 实体解码（& 与引号）", text_of(it, "title"), 'Rock & Roll "Remastered" 1975 FLAC')
link = text_of(it, "link") or ""
check("link 里的 &amp; 也解码了", link.endswith("&dn=Rock"), True)
check("link 是 magnet（第二级兜底）", link.startswith("magnet:"), True)
enc = it.find("enclosure")
check("enclosure@url 是 magnet（第一级兜底）", (enc.get("url") or "").startswith("magnet:"), True)
check("**故意不给** infohash 属性（要走 magnet 兜底）", attr_of(it, "infohash"), None)
check("seeders 是字符串 '0'（不是 None）", attr_of(it, "seeders"), "0")
check("leechers 是字符串 '0'", attr_of(it, "peers"), "0")
check("magnet 里的 btih:", "abcdef0123456789abcdef0123456789abcdef01" in link, True)

# ---- item 2：全缺字段 ----
it = items[2]
print("\n[2] 边界（全缺）")
check("title", text_of(it, "title"), "Edge Case No Size")
check("没有 size 元素", text_of(it, "size"), None)
check("没有 enclosure", it.find("enclosure"), None)
check("没有 seeders", attr_of(it, "seeders"), None)
check("没有 pubDate", text_of(it, "pubDate"), None)
check("detailUrl 只能走 guid 兜底", text_of(it, "guid"), "https://tracker.example/details/1")

# ⚠️ 汇总行**只用 ASCII**：这里再放 emoji，GBK 控制台又会把脚本炸掉（已踩过一次）。
# 上面刚 reconfigure 过 stdout，但保不齐调用方把 stdout 换成别的流 —— 不冒这个险。
print()
print("fixture matches the spec table" if bad == 0 else f"{bad} MISMATCHES")
sys.exit(0 if bad == 0 else 1)
