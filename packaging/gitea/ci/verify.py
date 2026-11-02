"""Validate the release asset set against the GitHub release shape.

每个架构交付三件：桌面 AppDir 归档、`.portable`、`.run`；Win7 交桌面 exe。裸 CLI 与补充包
不进 Release（GitHub 的产物通配也收不到它们），但 CLI 仍作为架构与 glibc 门禁的样本，
所以由 build.sh 转存到缓存卷后按第三个参数传进来。
"""

from pathlib import Path
import re
import struct
import subprocess
import sys
import tarfile
import tempfile


LINUX_MACHINES = {"amd64": 62, "arm64": 183}
ARCH_LABELS = {"amd64": "x64", "arm64": "arm64"}
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
    if "./AppRun" not in names or "./usr/bin/wunder-frontend-slint" not in names:
        raise RuntimeError("desktop archive is not an AppDir layout: " + archive.name)
    executable = desktop_member(archive, "usr/bin/wunder-frontend-slint")
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
    label = ARCH_LABELS[architecture]
    if ("-" + label) not in path.name:
        raise RuntimeError("artifact name does not carry the " + label + " label: " + path.name)


def main():
    directory = Path(sys.argv[1])
    architecture = sys.argv[2]
    extra = Path(sys.argv[3]) if len(sys.argv) > 3 else None
    if architecture == "win7-x86":
        desktop = find_single(directory, "wunder-frontend-slint-*-win7-x86.exe", "Win32 desktop executable")
        check_pe_i386(desktop)
        if extra is not None and extra.is_file():
            check_pe_i386(extra)
        if list(directory.glob("wunder-supplement-*")) or list(directory.glob("wunder-*.run")):
            raise RuntimeError("supplement or Linux installer leaked into the win7 asset set")
        print("PASS: win7-x86 PE headers (i386)")
        return
    desktop = find_single(directory, "wunder-slint-*-linux-" + architecture + ".tar.gz", "desktop archive")
    portable = find_single(directory, "wunder-*-*.portable", "portable package")
    run = find_single(directory, "wunder-*-*.run", "run installer")
    check_desktop_archive(desktop, architecture)
    check_self_extractor(portable, architecture)
    check_self_extractor(run, architecture)
    if extra is not None and extra.is_file():
        check_elf(extra, LINUX_MACHINES[architecture])
        check_glibc(extra)
    if list(directory.glob("wunder-cli-*")) or list(directory.glob("wunder-supplement-*")):
        raise RuntimeError("raw CLI or supplement leaked into the release asset set")
    print("PASS: " + architecture + " desktop archive, portable and installer payloads, ELF and glibc gates")


if __name__ == "__main__":
    main()
