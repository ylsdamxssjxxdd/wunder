#!/usr/bin/env python3
"""给 32 位 compositor 补上地址空间上限。

Remotion 的 memory.rs 用 sysinfo 读「系统可用内存」来定帧缓存上限和 OOM 阈值。
在 64 位主机上跑 32 位进程时，读到的仍是宿主总量（本机 64GB → 上限 20GB），
而进程实际只有 4GB 用户地址空间，缓存会把地址空间吃穿。

实测：4K（3840×2160）单帧 RGBA 约 33MB，compositor 私有内存在第 103 帧升到
3.66GB，随即进程死亡，上层报 EPIPE。1080p 因为单帧只有 8MB，缓存撑不到上限，
所以看不出问题。

修补方式：32 位 Windows 目标下把「可用内存」钳到 1.5GB，
于是缓存上限 = 768MB（4K 约 23 帧，足够流式编码），OOM 判断也重新有意义。
"""
import os
import sys

crate_root = sys.argv[1] if len(sys.argv) > 1 else "/b/crate"
path = os.path.join(crate_root, "rust", "memory.rs")
if not os.path.isfile(path):
    raise SystemExit(f"memory.rs not found: {path}")

MARKER = "32-bit address space cap"
src = open(path, encoding="utf-8").read()
if MARKER in src:
    print("memory.rs already patched")
    sys.exit(0)

anchor = """pub fn get_available_memory() -> u64 {
    let mut sys = System::new();
"""
inject = """pub fn get_available_memory() -> u64 {
    // 32-bit address space cap: sysinfo reports host memory, which is meaningless
    // for a 32-bit process limited to 4GB of user address space.
    #[cfg(all(windows, target_pointer_width = "32"))]
    {
        const CAP: u64 = 1536 * 1024 * 1024;
        let mut sys = System::new();
        sys.refresh_memory();
        let total = sys.total_memory();
        let used = sys.used_memory();
        let available = if used >= total { 0 } else { total - used };
        return available.min(CAP);
    }

    #[allow(unreachable_code)]
    let mut sys = System::new();
"""
if anchor not in src:
    raise SystemExit("memory.rs structure changed; refusing to patch blindly")

src = src.replace(anchor, inject, 1)
open(path, "w", encoding="utf-8", newline="\n").write(src)
print("memory.rs patched with 32-bit address space cap")
