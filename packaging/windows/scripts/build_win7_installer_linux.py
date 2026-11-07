"""Assemble the Win7 x86 installer on a Linux host with Wine + Inno Setup 6.

`build_win7_installer.ps1` 的 Linux 对应实现，输入与产物名完全相同：桌面 exe、CLI exe、
win7 补充包 zip → `wunder-desktop-<版本>-win7-x86-setup.exe`。ISCC.exe 在 Wine 下以控制台模式运行，
不需要 X 服务；所有绝对路径先经 `winepath -w` 换成 Z: 形式再交给 ISCC，避免 Windows 侧
解析 Linux 路径。任务与文件区块的语义（默认勾选、组件顺序、按 MB 取整的容量标注）与
PowerShell 版逐条对齐，两个入口共用同一份 `payload.generated.iss` 契约。
"""

import argparse
import hashlib
import json
import math
import os
import re
import shutil
import struct
import subprocess
import sys
import zipfile
from pathlib import Path


COMPONENT_ORDER = ("python", "git", "rg", "ffmpeg", "7zip", "powershell", "browser")
TASK_NAMES = ("desktop", "cli")
# Inno parses task names as expressions, so a name may not start with a digit.
COMPONENT_TASK_NAMES = {"7zip": "sevenzip"}


def task_name(component):
    return COMPONENT_TASK_NAMES.get(component, component)


def fail(message):
    raise RuntimeError(message)


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_app_version(path):
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        fail("could not read the app version from %s: %s" % (path, error))
    version = str(payload.get("version", "")).strip()
    if not version:
        fail("missing version in " + str(path))
    return version


def read_cargo_version(manifest):
    text = manifest.read_text(encoding="utf-8")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        fail("could not read a package version from " + str(manifest))
    return match.group(1)


def numeric_version(version):
    parts = [part for part in re.split(r"[^0-9]+", version) if part][:3]
    while len(parts) < 3:
        parts.append("0")
    return ".".join(str(int(part)) for part in parts) + ".0"


def pe_machine(path):
    with path.open("rb") as stream:
        if stream.read(2) != b"MZ":
            fail("not a PE image: " + str(path))
        stream.seek(0x3C)
        offset = struct.unpack("<I", stream.read(4))[0]
        stream.seek(offset)
        if stream.read(4) != b"PE\x00\x00":
            fail("missing PE signature: " + str(path))
        return struct.unpack("<H", stream.read(2))[0]


class Wine(object):
    """Minimal wrapper so every path handed to ISCC is a Windows path."""

    def __init__(self, command, prefix):
        self.command = command
        self.prefix = prefix
        self.cache = {}

    def available(self):
        return bool(self.command) and shutil.which(self.command) is not None

    def topath(self, path):
        key = str(path)
        if key not in self.cache:
            if not self.command:
                self.cache[key] = key
                return self.cache[key]
            environment = dict(os.environ, WINEPREFIX=str(self.prefix))
            converted = subprocess.check_output([self.command, "winepath", "-w", key], env=environment)
            self.cache[key] = converted.decode().strip()
        return self.cache[key]

    def run(self, arguments):
        environment = dict(os.environ, WINEPREFIX=str(self.prefix), WINEDEBUG="-all")
        return subprocess.call([self.command] + list(arguments), env=environment)


def supplement_info(archive):
    sizes = dict((name, 0) for name in COMPONENT_ORDER)
    files = dict((name, 0) for name in COMPONENT_ORDER)
    unknown = set()
    metadata = None
    with zipfile.ZipFile(str(archive)) as stream:
        for entry in stream.infolist():
            name = entry.filename.replace("\\", "/")
            if name.startswith("/") or re.search(r"(^|/)\.\.(/|$)", name):
                fail("supplement archive contains an unsafe path: " + name)
            parts = name.split("/")
            if len(parts) >= 3 and parts[0] == "opt":
                if parts[1] not in sizes:
                    unknown.add(parts[1])
                    continue
                if not name.endswith("/"):
                    files[parts[1]] += 1
                sizes[parts[1]] += entry.file_size
            if name == "wunder-win7-supplement.json":
                metadata = json.loads(stream.read(entry).decode("utf-8"))
    if unknown:
        fail("补充包含有安装器未声明的组件（会静默漏装）: " + ", ".join(sorted(unknown)))
    return sizes, files, metadata


def install_supplement_stage(archive, destination):
    """Extract the pack once per SHA and reuse the tree on later runs."""
    digest = sha256_file(archive)
    marker = destination / ".wunder-pack.json"
    if marker.is_file() and (destination / "opt").is_dir():
        try:
            record = json.loads(marker.read_text(encoding="utf-8"))
        except ValueError:
            record = {}
        if record.get("sha256") == digest:
            print("[wunder-pack] reusing supplement stage: " + str(destination))
            return
    temporary = Path(str(destination) + ".tmp")
    if temporary.exists():
        shutil.rmtree(str(temporary))
    temporary.mkdir(parents=True)
    with zipfile.ZipFile(str(archive)) as stream:
        stream.extractall(str(temporary))
    (temporary / ".wunder-pack.json").write_text(json.dumps({
        "sha256": digest,
        "source": archive.name,
    }) + "\r\n", encoding="utf-8")
    if destination.exists():
        shutil.rmtree(str(destination))
    temporary.rename(destination)


def component_version(metadata, name):
    if not metadata:
        return ""
    entry = metadata.get(name) or {}
    return str(entry.get("version", ""))


def format_label(labels, name, version, size):
    template = labels.get(name) or name
    label = template.replace("{version}", version).replace("  ", " ").strip()
    return label + labels.get("approxSize", "").replace("{size}", str(int(math.ceil(size / 1048576.0))))


def build_generated_iss(labels, core, supplement_root, version, available, sizes, metadata):
    lines = ["[Tasks]"]
    for name in TASK_NAMES:
        body = (core / ("wunder-desktop.exe" if name == "desktop" else "wunder-cli.exe")).stat().st_size
        lines.append('Name: "%s"; Description: "%s"' % (name, format_label(labels, name, version, body)))
    for name in available:
        task = task_name(name)
        if not re.match(r"^[A-Za-z_][A-Za-z0-9_]*$", task):
            fail("Inno 任务名必须以字母或下划线开头，请为组件 %s 配置 COMPONENT_TASK_NAMES" % name)
        label = format_label(labels, name, component_version(metadata, name), sizes[name])
        lines.append('Name: "%s"; Description: "%s"' % (task, label))
    lines.append("")
    lines.append("[Files]")
    lines.append('Source: "%s"; DestDir: "{app}\\bin"; DestName: "wunder-desktop.exe"; '
                 'Flags: ignoreversion; Tasks: desktop' % wine_path(core / "wunder-desktop.exe"))
    lines.append('Source: "%s"; DestDir: "{app}\\bin"; DestName: "wunder-cli.exe"; '
                 'Flags: ignoreversion; Tasks: cli' % wine_path(core / "wunder-cli.exe"))
    lines.append('Source: "%s"; DestDir: "{app}"; DestName: ".wunder-home"; Flags: ignoreversion'
                 % wine_path(core / ".wunder-home"))
    for name in available:
        lines.append('Source: "%s"; DestDir: "{app}\\opt\\%s"; '
                     'Flags: ignoreversion recursesubdirs createallsubdirs; Tasks: %s'
                     % (wine_path(supplement_root / "opt" / name) + "\\*", name, task_name(name)))
    return "\r\n".join(lines) + "\r\n"


WINE = None


def wine_path(path):
    return WINE.topath(path) if WINE else str(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--supplement", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--compression", default="lzma2/normal")
    parser.add_argument("--wine", default=os.environ.get("WUNDER_WINE", "wine"))
    parser.add_argument("--iscc", default=os.environ.get("WUNDER_ISCC", ""))
    parser.add_argument("--version", default="")
    args = parser.parse_args()

    global WINE
    repo_root = Path(__file__).resolve().parents[3]
    installer_root = repo_root / "packaging" / "windows" / "installer"
    stage_root = repo_root / "temp_dir" / "installer" / "win7-x86-linux"

    prefix = Path(os.environ.get("WINEPREFIX", str(Path.home() / ".wine")))
    if not shutil.which(args.wine):
        fail("wine 不可用：" + args.wine)
    WINE = Wine(args.wine, prefix)

    iscc = Path(args.iscc) if args.iscc else Path("C:\\InnoSetup\\ISCC.exe")
    stage_root.mkdir(parents=True, exist_ok=True)
    args.output_dir.mkdir(parents=True, exist_ok=True)

    version = args.version or read_app_version(repo_root / "config" / "app_version.json")
    if read_cargo_version(repo_root / "Cargo.toml") != version:
        fail("app_version.json 与 workspace 版本不一致，请运行 scripts/sync_app_version.py 后用同一修订重建产物")
    if read_cargo_version(repo_root / "frontend-slint" / "Cargo.toml") != version:
        fail("app_version.json 与 frontend-slint 版本不一致，请用同一修订重建产物")
    for path, label in ((args.desktop, "desktop"), (args.cli, "cli")):
        if not path.is_file():
            fail(label + " 输入缺失: " + str(path))
        if pe_machine(path) != 0x014C:
            fail(label + " 必须是 i386 PE: " + str(path))

    core = stage_root / "core"
    if core.exists():
        shutil.rmtree(str(core))
    core.mkdir(parents=True)
    shutil.copy2(str(args.desktop), str(core / "wunder-desktop.exe"))
    shutil.copy2(str(args.cli), str(core / "wunder-cli.exe"))
    (core / ".wunder-home").write_text("portable\r\n", encoding="utf-8")

    sizes, files, metadata = supplement_info(args.supplement)
    if ((metadata or {}).get("7zip") or {}).get("bundled") and files["7zip"] == 0:
        fail("补充包元数据声明 7-Zip 已打包，但归档里缺少 opt/7zip/7z.exe: " + str(args.supplement))
    available = [name for name in COMPONENT_ORDER if files[name] > 0]
    if not available:
        fail("补充包里没有 opt/<component> 文件: " + str(args.supplement))
    supplement_root = stage_root / "supplement"
    install_supplement_stage(args.supplement, supplement_root)

    labels = json.loads((installer_root / "component-labels.json").read_text(encoding="utf-8"))
    (stage_root / "payload.generated.iss").write_text(
        build_generated_iss(labels, core, supplement_root, version, available, sizes, metadata),
        encoding="utf-8")
    for name in ("wunder.iss", "ChineseSimplified.isl"):
        shutil.copy2(str(installer_root / name), str(stage_root / name))
    for name, target in (("eva01-head.ico", "eva01-head.ico"), ("eva01-head.png", "eva01-head.png"),
                         ("wizard-banner.png", "wizard-banner.png")):
        shutil.copy2(str(repo_root / "images" / name), str(stage_root / target))

    output_base = "wunder-desktop-%s-win7-x86-setup" % version
    arguments = [str(iscc), "/Qp",
                 "/O" + wine_path(args.output_dir),
                 "/F" + output_base,
                 "/DAppVersion=" + version,
                 "/DVersionInfoVersion=" + numeric_version(version),
                 "/DOutputBaseFilename=" + output_base,
                 "/DPackCompression=" + args.compression,
                 "/DComponentsShown=%d" % len(available),
                 wine_path(stage_root / "wunder.iss")]
    print("[wunder-pack] components: " + ", ".join(available))
    print("[wunder-pack] iscc: " + str(iscc))
    if WINE.run(arguments):
        fail("Inno Setup 编译失败（wine 退出码非 0）")
    installer = args.output_dir / (output_base + ".exe")
    if not installer.is_file():
        fail("安装包未产出: " + str(installer))
    print("[wunder-pack] installer: %s (%d bytes, sha256 %s)"
          % (installer.name, installer.stat().st_size, sha256_file(installer)))


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        print("[wunder-pack] " + str(error), file=sys.stderr)
        sys.exit(2)
