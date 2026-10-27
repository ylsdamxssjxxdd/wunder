"""Validate release architecture and the Ubuntu 18.04 glibc ceiling."""

from pathlib import Path
import re
import struct
import subprocess
import sys


def main():
    directory = Path(sys.argv[1])
    architecture = sys.argv[2]
    machine = {"amd64": 62, "arm64": 183}[architecture]
    appimages = list(directory.glob("*-linux-" + architecture + ".AppImage"))
    cli = list(directory.glob("wunder-cli-*-linux-" + architecture))
    if len(appimages) != 1 or len(cli) != 1:
        raise RuntimeError("expected exactly one desktop AppImage and one CLI executable")
    for artifact in appimages + cli:
        with artifact.open("rb") as stream:
            header = stream.read(20)
        if header[:4] != b"\x7fELF" or struct.unpack("<H", header[18:20])[0] != machine:
            raise RuntimeError("artifact ELF architecture mismatch: " + artifact.name)
    versions = subprocess.check_output(["readelf", "--version-info", str(cli[0])]).decode()
    required = [tuple(map(int, version.split(".")))
                for version in re.findall(r"GLIBC_([0-9.]+)", versions)]
    if not required or max(required) > (2, 27):
        raise RuntimeError("CLI requires a newer glibc than Ubuntu 18.04")
    with appimages[0].open("rb") as stream:
        runtime = stream.read(2 * 1024 * 1024)
    if not any(runtime[index + 28:index + 32] == b"\x04\x00\x00\x00"
               for index in range(len(runtime) - 96) if runtime[index:index + 4] == b"hsqs"):
        raise RuntimeError("desktop AppImage has no valid SquashFS payload")
    print("PASS: " + architecture + " ELF headers, AppImage payload and CLI glibc compatibility")


if __name__ == "__main__":
    main()
