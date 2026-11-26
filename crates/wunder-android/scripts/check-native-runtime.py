#!/usr/bin/env python3
"""检查 libwunder_android.so 与随包 libc++_shared.so 是否自洽。

背景：cdylib 里静态链接了 Skia 等 C++ 产物，它们调用 NDK 的 libc++；APK 又把
一份 libc++_shared.so 放在同目录，运行时由 WunderActivity 先加载。如果 C++ 产物
用的 NDK 与打包的 libc++ 不是同一个版本（最常见的原因：换过 NDK，但 skia-bindings
的产物缓存按 feature key 命中，没有重新编译），链接阶段不会有任何报错，装到手机上
一打开就死在 dlopen：

    dlopen failed: cannot locate symbol "_ZNSt6__ndk113__hash_memoryEPKvm"

（__hash_memory 是 libc++ 20 的内部符号，NDK 30 的产物引用它，NDK 28 的
libc++_shared.so 并不导出。）

这个脚本在打包前把「未定义的 libc++ 符号」与「随包运行时的导出符号」对一遍，
不闭合就让构建失败，而不是把一个必然闪退的 APK 发出去。

用法：
    check-native-runtime.py --so <libwunder_android.so> --libcxx <libc++_shared.so> \
        [--nm <llvm-nm 路径>]
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys

# libc++ 在 NDK 里的内联命名空间；模板的内联/弱符号可能由任意一个 C++ 产物定义，
# 但真正落到运行时库的未定义符号一定是这个前缀。
# 注意 __cxa_* 一类由 bionic 的 libc.so 提供（符号带 @LIBC 版本），不属于这里。
LIBCXX_PREFIXES = ("_ZNSt6__ndk1", "_ZN10__cxxabiv1")

UNDEFINED_RE = re.compile(r"^\s*U\s+(\S+)\s*$")
DEFINED_RE = re.compile(r"^[0-9a-fA-F]+\s+[A-Za-z]\s+(\S+)\s*$")


def find_nm(explicit: str | None) -> str:
    if explicit:
        return explicit
    for name in ("llvm-nm", "nm"):
        found = shutil.which(name)
        if found:
            return found
    raise SystemExit("check-native-runtime: 找不到 nm/llvm-nm，无法检查符号")


def symbols(nm: str, path: str, mode: str) -> set[str]:
    """mode: "undefined" | "defined"，只读动态符号表。"""
    flag = "--undefined-only" if mode == "undefined" else "--defined-only"
    try:
        output = subprocess.run(
            [nm, "-D", flag, path],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    except subprocess.CalledProcessError as error:
        raise SystemExit(
            f"check-native-runtime: 读取 {os.path.basename(path)} 的符号表失败：{error.stderr.strip()}"
        )
    pattern = UNDEFINED_RE if mode == "undefined" else DEFINED_RE
    return {
        match.group(1)
        for line in output.splitlines()
        # 带 @LIBC/@GLIBC 版本后缀的属于 bionic/libc，不走 libc++。
        if (match := pattern.match(line)) and "@" not in match.group(1)
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--so", required=True, help="libwunder_android.so")
    parser.add_argument("--libcxx", required=True, help="随包 libc++_shared.so")
    parser.add_argument("--nm", default=None, help="llvm-nm 路径（默认从 PATH 找）")
    args = parser.parse_args()

    for path in (args.so, args.libcxx):
        if not os.path.isfile(path):
            raise SystemExit(f"check-native-runtime: 缺少 {path}")

    nm = find_nm(args.nm)
    undefined = symbols(nm, args.so, "undefined")
    provided = symbols(nm, args.libcxx, "defined")

    wanted = {name for name in undefined if name.startswith(LIBCXX_PREFIXES)}
    missing = sorted(wanted - provided)
    if not missing:
        print(
            f"native runtime ok: {len(wanted)} 个 libc++ 符号全部由 "
            f"{os.path.basename(args.libcxx)} 提供"
        )
        return 0

    shown = "\n".join(f"  {name}" for name in missing[:20])
    more = "" if len(missing) <= 20 else f"\n  ……共 {len(missing)} 个"
    print(
        "native runtime 不自洽：libwunder_android.so 需要以下 libc++ 符号，"
        f"但随包的 {os.path.basename(args.libcxx)} 没有导出：\n{shown}{more}\n\n"
        "原因通常是 C++ 产物（Skia 等）与打包的 libc++ 来自不同版本的 NDK。\n"
        "处理：用当前 NDK 重新编译 C++ 产物，例如\n"
        "  cargo clean -p skia-bindings --target <target>   # 再重新构建\n"
        "或清空该 target 的构建目录后重建，并确保 ANDROID_NDK_ROOT 指向同一套 NDK。",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
