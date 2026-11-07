"""Validate the release asset set against the GitHub release shape.

每个架构交付三件：桌面 AppDir 归档、`.portable`、`.run`；Win7 交桌面 exe。裸 CLI 与补充包
不进 Release（GitHub 的产物通配也收不到它们），但 CLI 仍作为架构与 glibc 门禁的样本，
所以由 build.sh 转存到缓存卷后按第三个参数传进来。
"""

from pathlib import Path
import fnmatch
import re
import struct
import subprocess
import sys
import tarfile
import tempfile
import zipfile


LINUX_MACHINES = {"x64": 62, "arm64": 183}
RUN_MARKER = b"__WUNDER_PAYLOAD_BELOW__\n"
PORTABLE_MARKER = b"__WUNDER_PORTABLE_PAYLOAD__\n"
GZIP_MAGIC = b"\x1f\x8b\x08"


def find_single(directory, pattern, description):
    matches = sorted(path for path in directory.glob(pattern) if path.is_file())
    if len(matches) != 1:
        raise RuntimeError("expected exactly one %s (%s), found %d" % (description, pattern, len(matches)))
    return matches[0]


def check_elf(path, machine):
    with path.open("rb") as stream:
        header = stream.read(20)
    if header[:4] != b"\x7fELF" or struct.unpack("<H", header[18:20])[0] != machine:
        raise RuntimeError("ELF architecture mismatch: " + path.name)


def check_pe_i386(path):
    with path.open("rb") as stream:
        image = stream.read(4096)
    if image[:2] != b"MZ":
        raise RuntimeError("not a PE image: " + path.name)
    header_offset = struct.unpack("<I", image[60:64])[0]
    if image[header_offset:header_offset + 4] != b"PE\x00\x00":
        raise RuntimeError("missing PE signature: " + path.name)
    if struct.unpack("<H", image[header_offset + 4:header_offset + 6])[0] != 0x14C:
        raise RuntimeError("PE architecture is not i386: " + path.name)


def check_glibc(executable):
    versions = subprocess.check_output(["readelf", "--version-info", str(executable)]).decode()
    required = [tuple(int(part) for part in version.split("."))
                for version in re.findall(r"GLIBC_([0-9.]+)", versions)]
    if not required or max(required) > (2, 27):
        raise RuntimeError("executable requires a newer glibc than Ubuntu 18.04")


def desktop_member(archive, name):
    """Stream one member out of the AppDir tar.gz so the shipped ELF gets gated."""
    with tarfile.open(str(archive), "r:gz") as stream:
        for member in stream:
            if member.name in ("./" + name, name) and member.isfile():
                temporary = tempfile.NamedTemporaryFile(prefix="wunder-ci-", suffix=".elf", delete=False)
                with temporary:
                    with stream.extractfile(member) as source:
                        temporary.write(source.read())
                return Path(temporary.name)
    raise RuntimeError("desktop archive is missing " + name + ": " + archive.name)


def check_desktop_archive(archive, architecture):
    with tarfile.open(str(archive), "r:gz") as stream:
        names = set(stream.getnames())
    if "./AppRun" not in names or "./usr/bin/wunder-desktop" not in names:
        raise RuntimeError("desktop archive is not an AppDir layout: " + archive.name)
    executable = desktop_member(archive, "usr/bin/wunder-desktop")
    try:
        check_elf(executable, LINUX_MACHINES[architecture])
        check_glibc(executable)
    finally:
        executable.unlink()


def check_self_extractor(path, architecture):
    """Both Linux delivery forms are a shell header plus an appended gzip payload."""
    marker = RUN_MARKER if path.name.endswith(".run") else PORTABLE_MARKER
    with path.open("rb") as stream:
        head = stream.read(65536)
    if not head.startswith(b"#!"):
        raise RuntimeError("self-extracting header is missing: " + path.name)
    boundary = head.find(marker)
    if boundary < 0:
        raise RuntimeError("payload boundary " + marker.decode().strip() + " not found in " + path.name)
    offset = boundary + len(marker)
    with path.open("rb") as stream:
        stream.seek(offset)
        if stream.read(3) != GZIP_MAGIC:
            raise RuntimeError("payload of " + path.name + " is not a gzip member")
    if ("-" + architecture) not in path.name:
        raise RuntimeError("artifact name does not carry the " + architecture + " label: " + path.name)


def reject_unexpected(directory, patterns):
    """Release 附件必须是确定的那几件，多出来的文件（裸二进制、组包中间件）一律判失败。"""
    allowed = list(patterns) + ["SHA256SUMS-*.txt"]
    unexpected = sorted(path.name for path in directory.iterdir()
                        if path.is_file() and not any(fnmatch.fnmatch(path.name, rule) for rule in allowed))
    if unexpected:
        raise RuntimeError("unexpected files in the release asset set: " + ", ".join(unexpected))


def check_inno_payload(path):
    """ISCC appends its setup-data block near the end of the stub."""
    size = path.stat().st_size
    with path.open("rb") as stream:
        stream.seek(max(0, size - 8 * 1024 * 1024))
        tail = stream.read()
    if b"Inno Setup Setup Data" not in tail:
        raise RuntimeError("not an Inno Setup installer (setup-data block missing): " + path.name)


def main():
    directory = Path(sys.argv[1])
    architecture = sys.argv[2]
    extra = Path(sys.argv[3]) if len(sys.argv) > 3 else None
    if architecture == "win7-installer":
        installer = find_single(directory, "wunder-desktop-*-win7-x86-setup.exe", "Win7 installer")
        check_pe_i386(installer)
        check_inno_payload(installer)
        reject_unexpected(directory, ["wunder-desktop-*-win7-x86-setup.exe"])
        print("PASS: win7-x86 setup installer is an i386 PE with an Inno Setup payload")
        return
    if architecture == "win7-x86":
        desktop = find_single(directory, "wunder-desktop-*-win7-x86.exe", "Win32 desktop executable")
        check_pe_i386(desktop)
        if extra is not None and extra.is_file():
            check_pe_i386(extra)
        reject_unexpected(directory, ["wunder-desktop-*-win7-x86.exe"])
        print("PASS: win7-x86 PE headers (i386)")
        return
    if architecture == "android-arm64":
        apk = find_single(directory, "wunder-android-*-arm64.apk", "Android APK")
        with zipfile.ZipFile(str(apk)) as archive:
            names = set(archive.namelist())
            required = {
                "lib/arm64-v8a/libwunder_android.so",
                "lib/arm64-v8a/liboperit_proot.so",
                "assets/ubuntu/rootfs.tar.xz",
                "assets/ubuntu/packs/common.tar.xz",
            }
            missing = sorted(required - names)
            if missing:
                raise RuntimeError("APK is missing required entries: " + ", ".join(missing))
            header = archive.open("lib/arm64-v8a/libwunder_android.so").read(20)
        if header[:4] != b"\x7fELF" or struct.unpack("<H", header[18:20])[0] != 183:
            raise RuntimeError("libwunder_android.so is not an AArch64 ELF")
        reject_unexpected(directory, ["wunder-android-*-arm64.apk"])
        print("PASS: android-arm64 APK (runtime assets + AArch64 ELF)")
        return
    desktop = find_single(directory, "wunder-desktop-*-linux-" + architecture + ".tar.gz", "desktop archive")
    portable = find_single(directory, "wunder-desktop-*-linux-*.portable", "portable package")
    run = find_single(directory, "wunder-desktop-*-linux-*.run", "run installer")
    check_desktop_archive(desktop, architecture)
    check_self_extractor(portable, architecture)
    check_self_extractor(run, architecture)
    if extra is not None and extra.is_file():
        check_elf(extra, LINUX_MACHINES[architecture])
        check_glibc(extra)
    reject_unexpected(directory, ["wunder-desktop-*-linux-" + architecture + ".tar.gz",
                                  "wunder-desktop-*-linux-*.portable", "wunder-desktop-*-linux-*.run"])
    print("PASS: " + architecture + " desktop archive, portable and installer payloads, ELF and glibc gates")


if __name__ == "__main__":
    main()
