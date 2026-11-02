"""Walk every thread stack of a (possibly hung) local x64 process via dbghelp.

Usage: python scripts/diagnostics/dump_hung_threads.py <pid> [pdb-dir]
Frames resolve against the PDBs found next to the executable (debug builds).
"""
import ctypes
import sys
from ctypes import wintypes
from pathlib import Path

k32 = ctypes.windll.kernel32
dbg = ctypes.windll.dbghelp
psapi = ctypes.windll.psapi

# Handle-returning APIs must not truncate to 32-bit int.
for _fn in (k32.OpenProcess, k32.OpenThread, k32.CreateToolhelp32Snapshot):
    _fn.restype = ctypes.c_void_p
k32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
k32.OpenThread.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
dbg.SymInitialize.argtypes = [ctypes.c_void_p, ctypes.c_char_p, wintypes.BOOL]
dbg.SymInitialize.restype = wintypes.BOOL
dbg.StackWalk64.argtypes = [
    wintypes.DWORD, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
    ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
    ctypes.c_void_p,
]
dbg.StackWalk64.restype = wintypes.BOOL

TH32CS_SNAPTHREAD = 0x00000004
THREAD_SUSPEND_RESUME = 0x0002
THREAD_GET_CONTEXT = 0x0008
THREAD_QUERY_INFORMATION = 0x0040
PROCESS_ALL_ACCESS = 0x1F0FFF

CONTEXT_AMD64 = 0x00100000
CONTEXT_FULL = CONTEXT_AMD64 | 0x1 | 0x2 | 0x4 | 0x8  # CONTROL|INTEGER|SEGMENTS|FLOATING

OFF_CONTEXT_FLAGS = 0x30
OFF_RSP = 0x98
OFF_RBP = 0xA0
OFF_RIP = 0xF8
CONTEXT_SIZE = 1232

IMAGE_FILE_MACHINE_AMD64 = 0x8664
AddrModeFlat = 3


class THREADENTRY32(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("cntUsage", wintypes.DWORD),
        ("th32ThreadID", wintypes.DWORD),
        ("th32OwnerProcessID", wintypes.DWORD),
        ("tpBasePri", wintypes.LONG),
        ("tpDeltaPri", wintypes.LONG),
        ("dwFlags", wintypes.DWORD),
    ]


def threads_of(pid):
    snapshot = k32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    if snapshot == ctypes.c_void_p(-1).value:
        raise OSError("CreateToolhelp32Snapshot failed")
    try:
        entry = THREADENTRY32()
        entry.dwSize = ctypes.sizeof(THREADENTRY32)
        ok = k32.Thread32First(snapshot, ctypes.byref(entry))
        while ok:
            if entry.th32OwnerProcessID == pid:
                yield entry.th32ThreadID
            ok = k32.Thread32Next(snapshot, ctypes.byref(entry))
    finally:
        k32.CloseHandle(snapshot)


def make_context():
    buf = (ctypes.c_uint8 * (CONTEXT_SIZE + 16))()
    base = ctypes.addressof(buf)
    aligned = (base + 15) & ~15
    ctx = ctypes.c_void_p(aligned)
    ctypes.memset(aligned, 0, CONTEXT_SIZE)
    ctypes.c_uint32.from_address(aligned + OFF_CONTEXT_FLAGS).value = CONTEXT_FULL
    return buf, ctx


def reg(ctx, off):
    return ctypes.c_uint64.from_address(ctx.value + off).value


class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [
        ("SizeOfStruct", wintypes.ULONG),
        ("TypeIndex", wintypes.ULONG),
        ("Reserved", ctypes.c_uint64 * 2),
        ("Index", wintypes.ULONG),
        ("Size", wintypes.ULONG),
        ("ModBase", ctypes.c_uint64),
        ("Flags", wintypes.ULONG),
        ("Value", ctypes.c_uint64),
        ("Address", ctypes.c_uint64),
        ("Register", wintypes.ULONG),
        ("Scope", wintypes.ULONG),
        ("Tag", wintypes.ULONG),
        ("NameLen", wintypes.ULONG),
        ("MaxNameLen", wintypes.ULONG),
        ("Name", ctypes.c_char * 1024),
    ]


dbg.SymFromAddr.argtypes = [ctypes.c_void_p, ctypes.c_uint64,
                            ctypes.POINTER(ctypes.c_uint64), ctypes.POINTER(SYMBOL_INFO)]
dbg.SymFromAddr.restype = wintypes.BOOL


def sym_name(process, addr):
    info = SYMBOL_INFO()
    info.SizeOfStruct = ctypes.sizeof(SYMBOL_INFO)
    info.MaxNameLen = 1023
    disp = ctypes.c_uint64(0)
    if dbg.SymFromAddr(process, ctypes.c_uint64(addr), ctypes.byref(disp), ctypes.byref(info)):
        name = info.Name.decode("utf-8", "replace")
        return f"{name}+0x{disp.value:x}" if disp.value else name
    return f"0x{addr:016x}"


def walk(process, thread_handle):
    buf, ctx = make_context()
    if not k32.GetThreadContext(thread_handle, ctx):
        raise ctypes.WinError()
    # STACKFRAME64: ADDRESS64 x4 then the rest; 256 bytes is generous.
    frame = (ctypes.c_uint8 * 256)()
    faddr = ctypes.addressof(frame)

    def set_addr(member_off, value):
        ctypes.c_uint64.from_address(faddr + member_off).value = value
        ctypes.c_uint16.from_address(faddr + member_off + 12).value = AddrModeFlat

    set_addr(0, reg(ctx, OFF_RIP))    # AddrPC
    set_addr(16, reg(ctx, OFF_RBP))   # AddrFrame
    set_addr(32, reg(ctx, OFF_RSP))   # AddrStack

    dbg.SymFunctionTableAccess64.restype = ctypes.c_void_p
    dbg.SymGetModuleBase64.restype = ctypes.c_uint64
    frames = []
    for _ in range(128):
        ok = dbg.StackWalk64(
            IMAGE_FILE_MACHINE_AMD64, process, thread_handle,
            faddr, ctx, None,
            ctypes.cast(dbg.SymFunctionTableAccess64, ctypes.c_void_p),
            ctypes.cast(dbg.SymGetModuleBase64, ctypes.c_void_p),
            None,
        )
        if not ok:
            break
        pc = ctypes.c_uint64.from_address(faddr + 0).value
        if pc == 0:
            break
        frames.append(pc)
    return frames


def main():
    pid = int(sys.argv[1])
    pdb_dir = sys.argv[2] if len(sys.argv) > 2 else None
    process = k32.OpenProcess(PROCESS_ALL_ACCESS, False, pid)
    if not process:
        raise ctypes.WinError()
    dbg.SymSetOptions(0x00000004 | 0x00000100 | 0x00000010)  # UNDNAME | DEFERRED_LOADS | LOAD_LINES
    search = pdb_dir or ""
    if not dbg.SymInitialize(process, search.encode("utf-8"), True):
        raise ctypes.WinError()
    out = []
    for tid in list(threads_of(pid)):
        th = k32.OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                            False, tid)
        if not th:
            continue
        out.append(f"--- thread {tid} ---")
        k32.SuspendThread(th)
        try:
            for pc in walk(process, th):
                out.append(f"  {sym_name(process, pc)}")
        except OSError as err:
            out.append(f"  <walk failed: {err}>")
        finally:
            k32.ResumeThread(th)
            k32.CloseHandle(th)
    dbg.SymCleanup(process)
    k32.CloseHandle(process)
    print("\n".join(out))


if __name__ == "__main__":
    main()
