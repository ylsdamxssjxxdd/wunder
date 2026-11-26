#!/usr/bin/env python3
"""检查 libwunder_android.so 与随包 libc++_shared.so 是否自洽。

背景：cdylib 里静态链接了 Skia 等 C++ 产物，它们调用 NDK 的 libc++；APK 又把
一份 libc++_shared.so 放在同目录，运行时由 WunderActivity 先加载。如果 C++ 产物
用的 NDK 与打包的 libc++ 不是同一个版本（最常见的原因：换过 NDK，但 skia-bindings
的产物缓存按 feature key 命中，没有重新编译；或者 CI runner 预设了旧 NDK），
链接阶段不会有任何报错，装到手机上一打开就死在 dlopen：

    dlopen failed: cannot locate symbol "_ZNSt6__ndk113__hash_memoryEPKvm"

（__hash_memory 是 libc++ 20 才有的内部符号，旧 NDK 的 libc++_shared.so 不导出。）

这个脚本在打包前把「未定义的 libc++ 符号」与「随包运行时的导出符号」对一遍，
不闭合就让构建失败，而不是把一个必然闪退的 APK 发出去。

注意：离线 CI 镜像里的 python3 只有 3.6/3.7，这里不能用海象运算符、
subprocess 的 text=/capture_output= 等 3.7+ 写法。

用法：
    check-native-runtime.py --so <libwunder_android.so> --libcxx <libc++_shared.so> \
        [--nm <llvm-nm 路径>]
"""

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


def emit(stream, text):
    """写输出；容器里 locale 可能是 POSIX/C，中文会编码失败，退化成 ASCII。"""
    try:
        stream.write(text)
    except UnicodeEncodeError:
        stream.write(text.encode("ascii", "replace").decode("ascii"))


def find_nm(explicit):
    if explicit:
        return explicit
    for name in ("llvm-nm", "nm"):
        found = shutil.which(name)
        if found:
            return found
    raise SystemExit("check-native-runtime: nm/llvm-nm not found")


def symbols(nm, path, mode):
    """mode: "undefined" | "defined"，只读动态符号表。"""
    flag = "--undefined-only" if mode == "undefined" else "--defined-only"
    try:
        process = subprocess.run(
            [nm, "-D", flag, path],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            universal_newlines=True,
        )
    except OSError as error:
        raise SystemExit("check-native-runtime: cannot run %s: %s" % (nm, error))
    if process.returncode != 0:
        raise SystemExit(
            "check-native-runtime: cannot read symbols of %s: %s"
            % (os.path.basename(path), (process.stderr or "").strip())
        )
    pattern = UNDEFINED_RE if mode == "undefined" else DEFINED_RE
    result = set()
    for line in (process.stdout or "").splitlines():
        match = pattern.match(line)
        if not match:
            continue
        name = match.group(1)
        # 带 @LIBC/@GLIBC 版本后缀的属于 bionic/libc，不走 libc++。
        if "@" in name:
            continue
        result.add(name)
    return result


def main():
    parser = argparse.ArgumentParser(description="检查安卓原生库与随包 libc++ 是否匹配")
    parser.add_argument("--so", required=True, help="libwunder_android.so")
    parser.add_argument("--libcxx", required=True, help="随包 libc++_shared.so")
    parser.add_argument("--nm", default=None, help="llvm-nm 路径（默认从 PATH 找）")
    args = parser.parse_args()

    for path in (args.so, args.libcxx):
        if not os.path.isfile(path):
            raise SystemExit("check-native-runtime: missing %s" % path)

    nm = find_nm(args.nm)
    undefined = symbols(nm, args.so, "undefined")
    provided = symbols(nm, args.libcxx, "defined")

    wanted = set(name for name in undefined if name.startswith(LIBCXX_PREFIXES))
    missing = sorted(wanted - provided)
    if not missing:
        emit(
            sys.stdout,
            "native runtime ok: %d libc++ symbols provided by %s\n"
            % (len(wanted), os.path.basename(args.libcxx)),
        )
        return 0

    shown = "\n".join("  " + name for name in missing[:20])
    more = "" if len(missing) <= 20 else "\n  ... %d more" % (len(missing) - 20)
    emit(
        sys.stderr,
        "native runtime mismatch: libwunder_android.so needs these libc++ symbols, "
        "missing from %s:\n%s%s\n\n"
        "The C++ objects (Skia) and the packaged libc++ come from different NDKs.\n"
        "Use a matching libc++ (e.g. NDK 30) or rebuild the C++ objects:\n"
        "  cargo clean -p skia-bindings --target <target>\n"
        % (os.path.basename(args.libcxx), shown, more)
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
