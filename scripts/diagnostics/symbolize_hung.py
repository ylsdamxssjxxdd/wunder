"""Resolve raw stack addresses from dump_hung_threads.py against modules + PDB.

Usage: python scripts/diagnostics/symbolize_hung.py <pid> <stacks-raw.txt>
"""
import ctypes
import re
import subprocess
import sys
from ctypes import wintypes
from pathlib import Path

k32 = ctypes.windll.kernel32
psapi = ctypes.windll.psapi
k32.OpenProcess.restype = ctypes.c_void_p
k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
psapi.EnumProcessModules.argtypes = [ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
psapi.GetModuleFileNameExW.argtypes = [ctypes.c_void_p, ctypes.c_void_p, wintypes.LPWSTR, wintypes.DWORD]
psapi.GetModuleInformation.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, wintypes.DWORD]

SYMBOLIZER = r"C:\Program Files\AMD\ROCm\7.1\bin\llvm-symbolizer.exe"


class MODULEINFO(ctypes.Structure):
    _fields_ = [("lpBaseOfDll", ctypes.c_void_p), ("SizeOfImage", wintypes.DWORD), ("EntryPoint", ctypes.c_void_p)]


def modules(pid):
    proc = k32.OpenProcess(0x0410, False, pid)  # QUERY_INFORMATION|VM_READ
    if not proc:
        raise ctypes.WinError()
    try:
        needed = wintypes.DWORD()
        psapi.EnumProcessModules(proc, None, 0, ctypes.byref(needed))
        count = needed.value // ctypes.sizeof(ctypes.c_void_p)
        handles = (ctypes.c_void_p * count)()
        psapi.EnumProcessModules(proc, handles, needed.value, ctypes.byref(needed))
        out = []
        for h in handles:
            if not h:
                continue
            buf = ctypes.create_unicode_buffer(512)
            psapi.GetModuleFileNameExW(proc, h, buf, 512)
            info = MODULEINFO()
            psapi.GetModuleInformation(proc, h, ctypes.byref(info), ctypes.sizeof(info))
            out.append((info.lpBaseOfDll, info.SizeOfImage, buf.value))
        return out
    finally:
        k32.CloseHandle(proc)


def main():
    pid = int(sys.argv[1])
    lines = Path(sys.argv[2]).read_text().splitlines()
    mods = modules(pid)
    addrs = sorted({int(m.group(1), 16) for line in lines for m in [re.match(r"\s+0x([0-9a-f]+)$", line)] if m})

    # Batch-symbolize addresses belonging to the app exe via llvm-symbolizer.
    exe_entries = [(b, s, p) for b, s, p in mods if "wunder-frontend-slint" in p]
    cache = {}
    for base, size, path in exe_entries:
        targets = [a for a in addrs if base <= a < base + size]
        if not targets:
            continue
        args = [SYMBOLIZER, "--demangle", "--functions=linkage", "--exe", path]
        args += [hex(a - base + 0x140000000) for a in targets]
        result = subprocess.run(args, capture_output=True, text=True, timeout=120)
        chunks = result.stdout.strip().split("\n\n")
        if len(chunks) != len(targets):
            print(f"[symbolizer mismatch: {len(chunks)} chunks vs {len(targets)} addrs; stderr={result.stderr[:300]}]", file=sys.stderr)
        for addr, chunk in zip(targets, chunks):
            cache[addr] = chunk.replace("\n", "  ")

    for line in lines:
        m = re.match(r"(\s+)0x([0-9a-f]+)$", line)
        if not m:
            print(line)
            continue
        addr = int(m.group(2), 16)
        if addr in cache:
            print(f"{m.group(1)}{cache[addr]}")
            continue
        for base, size, path in mods:
            if base <= addr < base + size:
                print(f"{m.group(1)}{Path(path).name}+0x{addr - base:x}")
                break
        else:
            print(line)


if __name__ == "__main__":
    main()
