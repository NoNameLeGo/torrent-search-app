#!/usr/bin/env python3
"""体检 `.github/workflows/*.yml` —— 专抓「workflow 文件本身是坏的」这一类。

为什么需要它：**workflow 文件坏掉的时候，那条 workflow 是不会跑的**。
GitHub 会建一个 run，但里面**一个 job 都没有**，`gh run view --log-failed` 输出为空，
只有一句 "This run likely failed because of a workflow file issue"。
事后完全无从回溯。

历史事故（2026-08-14）：那次的「同步 Tauri Build workflow 到 main」把
`main` 上的 `.github/workflows/tauri-build.yml` 写成了 **0 字节**，
之后 6 天里 `main` 上每一次 push 都产生一个 0 job 的失败 run（共 15 次），
直到 2026-08-20 才被 `fix(ci): 恢复 main 分支空的 tauri-build.yml` 修掉。
—— 那段时间「远程构建总是出问题」的真凶就是这一个空文件。

用法：
    python3 scripts/check-workflows.py      # 退出码非 0 = 有问题

Git Bash / CI 上直接能看中文；Windows 的 GBK 控制台会显示乱码（已强制 UTF-8 输出，
不会崩），只看开头的 `OK` / `FAIL` 就够。

有 pyyaml 就做结构检查（触发条件 / jobs / runs-on），没装就退化成字符串级检查
（体积 + 关键字），反正**空文件和明显缺块**这两类都能抓到。
"""

import glob
import os
import sys

# Windows 控制台默认 GBK，直接 print 中文/对钩会 UnicodeEncodeError。
# 强制 UTF-8 输出（终端本身还是 GBK 的话会显示乱码，但不会崩）。
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

try:
    import yaml
except ImportError:  # 没装就退化成字符串级检查，不因为它失败
    yaml = None

# 一个真 workflow 不可能小于这个数（历史那个空文件是 0 字节）
MIN_BYTES = 200


def check(path):
    """返回错误信息列表，空列表 = 通过。"""
    problems = []

    size = os.path.getsize(path)
    if size < MIN_BYTES:
        # 这条是重点：0 字节的文件 GitHub 认为"workflow 文件有问题"，run 里没有 job
        problems.append(
            f"只有 {size} 字节 —— 空壳 workflow 会让每次 push 都变成 0 job 的失败"
        )
        return problems

    with open(path, encoding="utf-8") as fh:
        text = fh.read().replace("\r\n", "\n")

    if yaml is None:
        for needle in ("\non:", "\njobs:", "runs-on:"):
            if needle not in text:
                problems.append(f"找不到 `{needle.strip()}`（没装 pyyaml，只能字符串级检查）")
        return problems

    try:
        doc = yaml.safe_load(text)
    except Exception as e:  # noqa: BLE001 — 任何解析错误都要报出来
        return [f"YAML 解析失败：{str(e).splitlines()[0]}"]

    if not isinstance(doc, dict):
        return [f"顶层不是映射（解析出来是 {type(doc).__name__}）"]

    # YAML 1.1 会把裸写的 `on:` 解析成布尔 True，两种键都要认
    if "on" not in doc and True not in doc:
        problems.append("缺少 `on:` 触发条件")

    jobs = doc.get("jobs")
    if not isinstance(jobs, dict) or not jobs:
        problems.append("没有 `jobs:`（或它是空的）")
        return problems

    for name, job in jobs.items():
        if not isinstance(job, dict):
            problems.append(f"job `{name}` 不是映射")
        elif "runs-on" not in job and "uses" not in job:
            problems.append(f"job `{name}` 既没有 `runs-on` 也没有 `uses`")

    return problems


def main():
    files = sorted(glob.glob(".github/workflows/*.yml"))
    files += sorted(glob.glob(".github/workflows/*.yaml"))

    if not files:
        print("FAIL 一个 workflow 文件都没找到（应该在仓库根目录跑）")
        return 1

    failed = 0
    for path in files:
        problems = check(path)
        if problems:
            failed += 1
            for p in problems:
                print(f"FAIL {path}: {p}")
        else:
            jobs = 0
            if yaml is not None:
                with open(path, encoding="utf-8") as fh:
                    jobs = len(yaml.safe_load(fh)["jobs"])
            print(f"OK   {path} ({os.path.getsize(path)} 字节, {jobs} 个 job)")

    if failed:
        print(
            f"\n{failed}/{len(files)} 个 workflow 文件有问题。"
            "坏掉的 workflow 不会自己报错 —— 它只是让每次 push 变成 0 job 的失败。"
        )
        return 1

    print(f"\n{len(files)} 个 workflow 文件都正常。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
