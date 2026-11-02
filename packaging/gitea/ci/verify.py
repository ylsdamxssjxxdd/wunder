"""Validate release architecture and the Ubuntu 18.04 glibc ceiling."""

from pathlib import Path
import re
import struct
import subprocess
import sys
import tarfile


LINUX_MACHINES = {"amd64": 62, "arm64": 183}


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
    machine = struct.unpack("<H", image[header_offset + 4:header_offset + 6])[0]
    if machine != 0x14C:
        raise RuntimeError("PE architecture is not i386: " + path.name)


def check_desktop_archive(archive, binary_name):
    with tarfile.open(archive, "r:gz") as stream:
        names = set(stream.getnames())
    if "./AppRun" not in names or "./usr/bin/" + binary_name not in names:
        raise RuntimeError("desktop archive is not an AppDir layout: " + archive.name)


def check_glibc(executable):
    versions = subprocess.check_output(["readelf", "--version-info", str(executable)]).decode()
    required = [tuple(int(part) for part in version.split("."))
                for version in re.findall(r"GLIBC_([0-9.]+)", versions)]
    if not required or max(required) > (2, 27):
        raise RuntimeError("CLI requires a newer glibc than Ubuntu 18.04")


def main():
    directory = Path(sys.argv[1])
    architecture = sys.argv[2]
    if architecture == "win7-x86":
        desktop = list(directory.glob("wunder-frontend-slint-*-win7-x86.exe"))
        cli = list(directory.glob("wunder-cli-*-win7-x86.exe"))
        if len(desktop) != 1 or len(cli) != 1:
            raise RuntimeError("expected exactly one desktop and one CLI Win32 executable")
        for artifact in desktop + cli:
            check_pe_i386(artifact)
        print("PASS: win7-x86 PE headers (i386)")
        return
    machine = LINUX_MACHINES[architecture]
    desktop = list(directory.glob("wunder-slint-*-linux-" + architecture + ".tar.gz"))
    cli = list(directory.glob("wunder-cli-*-linux-" + architecture))
    if len(desktop) != 1 or len(cli) != 1:
        raise RuntimeError("expected exactly one desktop archive and one CLI executable")
    check_elf(cli[0], machine)
    check_glibc(cli[0])
    check_desktop_archive(desktop[0], "wunder-frontend-slint")
    print("PASS: " + architecture + " ELF headers, AppDir layout and CLI glibc compatibility")


if __name__ == "__main__":
    main()
