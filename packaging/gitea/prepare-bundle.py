"""Prepare a relocatable offline Gitea deployment with an offline Rust builder."""

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


GITEA = "docker.gitea.com/gitea:28.2.0"
RUNNER = "docker.gitea.com/runner:5.2.3"

# 每个剖面是一套可移植套件：任务镜像、SDK 必需目录、卷名前缀都不同，
# 但工作流与 CI 脚本完全共用（标签里的架构由 runner 注册时决定）。
PROFILES = {
    "arm64": {
        "platform": "linux/arm64",
        "architecture": "arm64",
        "host_uname": "aarch64",
        "sdk_profile": "kylin-arm",
        "stack_prefix": "offline-gitea",
        "builder_dockerfile": "Dockerfile.builder-arm64",
        "builder_tag": "wunder-offline-builder:20261027-arm64-mounted",
        "sdk_required": ["rust/toolchains", "cargo-vendor-slint", "linux-arm64-ubuntu18/root",
                         "linux-amd64-ubuntu18/root", "appimage-runtime",
                         "mingw-i686-windows-gnu", "web-wasm"],
        "squashfs_processors": "2",
        "runner_capacity": "1",
        "build_jobs": "4",
    },
    "amd64": {
        "platform": "linux/amd64",
        "architecture": "amd64",
        "host_uname": "x86_64",
        "sdk_profile": "kylin-x86",
        "stack_prefix": "offline-gitea-x86",
        # 基镜像是 builders/build-kylin-x86-sdk-docker.sh 产出的 x86_64 SDK 镜像，
        # 已带 gcc/cmake/ninja/nasm/clang 与交叉工具链，套件只补 git 和 CI 脚本。
        "builder_dockerfile": "Dockerfile.builder-amd64",
        "builder_tag": "wunder-offline-builder:20261102-amd64-mounted",
        # host-tools-amd64 带 .complete 标记与已解压的 aarch64 交叉编译器，
        # rcho 的 prepare_x86_cross_toolchain.sh 会直接短路，不必再带 1.4G 的 archives 归档。
        "sdk_required": ["rust/toolchains", "cargo-vendor-slint", "linux-amd64-ubuntu18/root",
                         "linux-arm64-ubuntu18/root", "host-tools-amd64", "appimage-runtime",
                         "mingw-i686-windows-gnu"],
        "squashfs_processors": "3",
        # x86_64 构建机按 24 核 / 64G 量级配置：三个任务并行，每个 6 线程。
        "runner_capacity": "3",
        "build_jobs": "6",
    },
}

# 预置补充包：离线端不复建（要在目标 libc 上编译 git、铺 Python 运行时），随套件导出直接复用。
# Linux 两包是 .run 安装包的必需输入；win7-ia32 那包是 Windows 安装器（Inno Setup）的输入，
# Linux 侧组不了安装包，但仍然带进套件，交付到离线机后可由 Windows 端直接取用。
SUPPLEMENT_PACKS = {
    "amd64": "wunder-supplement-linux-amd64.tar.gz",
    "arm64": "wunder-supplement-linux-arm64.tar.gz",
    "win7-ia32": "wunder-supplement-win7-ia32-common.zip",
}
SUPPLEMENT_REQUIRED = ("amd64", "arm64")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def export_stream(command, archive):
    temporary = archive.with_suffix(archive.suffix + ".tmp")
    process = subprocess.Popen(command, stdout=subprocess.PIPE)
    try:
        with temporary.open("wb") as stream:
            with gzip.GzipFile(filename="", mode="wb", compresslevel=1, fileobj=stream, mtime=0) as compressed:
                shutil.copyfileobj(process.stdout, compressed, length=4 * 1024 * 1024)
        if process.wait():
            raise RuntimeError("archive export failed: " + archive.name)
        temporary.replace(archive)
    finally:
        process.stdout.close()
        if process.poll() is None:
            process.terminate()
            process.wait()
        if temporary.exists():
            temporary.unlink()


def run(*arguments):
    subprocess.run(arguments, check=True)


def has_image(reference):
    return subprocess.run(["docker", "image", "inspect", reference],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0


def render_runner(source, profile, labels):
    text = (source / "runner.yaml").read_text(encoding="utf-8")
    replacements = {
        '"@RUNNER_CAPACITY@"': profile["runner_capacity"],
        "@WUNDER_BUILD_JOBS@": profile["build_jobs"],
        "@WUNDER_SQUASHFS_PROCESSORS@": profile["squashfs_processors"],
        "@BUILDER_LABEL@": labels["builder"],
        "@RCHO_LABEL@": labels["rcho"],
        "@STACK_PREFIX@": profile["stack_prefix"],
    }
    for token, value in replacements.items():
        text = text.replace(token, value)
    if any(token in text for token in ("@RUNNER_CAPACITY@", "@STACK_PREFIX@", "@BUILDER_LABEL@", "@RCHO_LABEL@")):
        raise RuntimeError("runner.yaml has an unrendered token")
    return text


def write_env(source, profile, labels):
    lines = []
    for line in (source / ".env.example").read_text(encoding="utf-8").splitlines():
        key = line.split("=", 1)[0].strip() if "=" in line and not line.startswith("#") else ""
        if key in {"CI_PLATFORM", "STACK_PREFIX", "SDK_PROFILE", "GITEA_IMAGE", "RUNNER_IMAGE",
                   "BUILDER_IMAGE", "RUNNER_LABELS", "RUNNER_NAME", "WUNDER_BUILD_JOBS"}:
            continue
        lines.append(line)
    values = {
        "CI_PLATFORM": profile["platform"],
        "STACK_PREFIX": profile["stack_prefix"],
        "SDK_PROFILE": profile["sdk_profile"],
        "GITEA_IMAGE": GITEA,
        "RUNNER_IMAGE": RUNNER,
        "BUILDER_IMAGE": profile["builder_tag"],
        "RUNNER_LABELS": ",".join((labels["builder"], labels["rcho"])),
        "RUNNER_NAME": "offline-linux-" + profile["architecture"],
        "WUNDER_BUILD_JOBS": profile["build_jobs"],
    }
    lines.append("")
    lines.extend(key + "=" + value for key, value in values.items())
    return "\n".join(lines) + "\n"


def stage_supplement_prebuilt(source_dir, sdk_directory):
    """Collect the prebuilt supplement packs into the SDK-relative layout.

    离线端不复建补充包（要在目标 libc 上编译 git、铺 Python 运行时），所以补充包在联网准备机上
    产好后随套件导出；导出前先复核 SHA256，避免把坏包固化进套件。
    """
    staging = sdk_directory / ".supplement-stage"
    if staging.exists():
        shutil.rmtree(staging)
    target = staging / "supplement-prebuilt"
    members = []
    for architecture, pack_name in SUPPLEMENT_PACKS.items():
        pack = source_dir / pack_name
        checksum = source_dir / (pack_name + ".sha256")
        if not pack.is_file():
            if architecture in SUPPLEMENT_REQUIRED:
                raise RuntimeError("prebuilt supplement pack is missing: " + pack_name)
            print("跳过补充包预置（文件不存在）: " + pack_name, flush=True)
            continue
        if not checksum.is_file():
            raise RuntimeError("prebuilt supplement pack has no .sha256 beside it: " + pack_name)
        if sha256(pack) != checksum.read_text(encoding="utf-8").split()[0]:
            raise RuntimeError("prebuilt supplement pack checksum mismatch: " + pack_name)
        arch_target = target / architecture
        arch_target.mkdir(parents=True)
        shutil.copy2(pack, arch_target / pack_name)
        shutil.copy2(checksum, arch_target / (pack_name + ".sha256"))
        members.append(architecture)
    archive = sdk_directory / "supplement-prebuilt.tar.gz"
    export_stream(["tar", "-C", str(staging), "-cf", "-", "supplement-prebuilt"], archive)
    shutil.rmtree(staging)
    return members, archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=sorted(PROFILES), required=True)
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--supplement-prebuilt", type=Path, required=True,
                        help="directory with the prebuilt supplement packs and their .sha256 sidecars")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--reuse-sdk-archive", action="store_true")
    args = parser.parse_args()

    source = Path(__file__).resolve().parent
    profile = PROFILES[args.arch]
    platform = profile["platform"]
    output = args.output.resolve()
    images = output / "images"
    images.mkdir(parents=True, exist_ok=True)

    labels = {
        "builder": "wunder-offline:docker://" + profile["builder_tag"],
        # 舵机与蛹共用同一套 Rust 任务镜像，只是路由标签不同；rcho 的离线流水线用这个标签。
        "rcho": "rcho-offline:docker://" + profile["builder_tag"],
    }

    for name in (GITEA, RUNNER):
        # 准备机可能已经从上一版套件 docker load 过这些镜像，有本地副本就不必再依赖网络。
        if not has_image(name):
            run("docker", "pull", "--platform", platform, name)
    if not args.skip_build:
        run("docker", "build", "--platform", platform,
            "-f", str(source / profile["builder_dockerfile"]), "-t", profile["builder_tag"], str(source))
    for name in (profile["builder_tag"],):
        if not has_image(name):
            raise RuntimeError("任务镜像缺失，去掉 --skip-build 或先准备基镜像: " + name)

    sdk_directory = output / "rust-builder"
    sdk_directory.mkdir(exist_ok=True)
    sdk_archive = sdk_directory / "offline-sdk.tar.gz"
    if not args.reuse_sdk_archive:
        sdk = args.sdk.resolve()
        for relative in profile["sdk_required"]:
            if not (sdk / relative).exists():
                raise RuntimeError("required SDK directory is missing: " + relative)
        print("Exporting separate Rust-builder archive (SDK is not part of any image)", flush=True)
        export_stream(["docker", "run", "--rm", "--platform", platform, "--network", "none",
                       "-v", str(sdk) + ":/payload/offline:ro", "--entrypoint", "tar", profile["builder_tag"],
                       "-C", "/payload", "-cf", "-", *("offline/" + relative for relative in profile["sdk_required"])],
                      sdk_archive)
    elif not sdk_archive.is_file():
        raise RuntimeError("no SDK archive to reuse")
    else:
        expected = (sdk_directory / "SHA256SUMS").read_text(encoding="utf-8").split()[0]
        if sha256(sdk_archive) != expected:
            raise RuntimeError("existing SDK archive checksum mismatch")
    (sdk_directory / "SHA256SUMS").write_bytes((sha256(sdk_archive) + "  offline-sdk.tar.gz\n").encode())

    prebuilt_arches, prebuilt_archive = stage_supplement_prebuilt(args.supplement_prebuilt.resolve(), sdk_directory)
    (sdk_directory / "SHA256SUMS").write_bytes(
        (sha256(sdk_archive) + "  offline-sdk.tar.gz\n"
         + sha256(prebuilt_archive) + "  supplement-prebuilt.tar.gz\n").encode())

    (output / "runner.yaml").write_text(render_runner(source, profile, labels), encoding="utf-8")
    (output / ".env").write_text(write_env(source, profile, labels), encoding="utf-8")
    for name in ("compose.yaml", "bootstrap.sh", "load-images.sh", "prepare-sdk.sh", ".env.example", "README.md"):
        shutil.copy2(source / name, output / name)
    for name in ("VALIDATION.md",):
        if (source / name).is_file():
            shutil.copy2(source / name, output / name)
    shutil.copytree(source / "ci", output / "ci", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__"))
    workflow = source.parents[1] / ".gitea/workflows/offline-linux.yml"
    destination = output / "examples/.gitea/workflows/offline-linux.yml"
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(workflow, destination)
    shutil.copytree(source / "ci", output / "examples/packaging/gitea/ci", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__"))

    manifest = []
    checksums = []
    for image, filename in ((GITEA, "gitea-" + profile["architecture"] + ".tar.gz"),
                            (RUNNER, "runner-" + profile["architecture"] + ".tar.gz"),
                            (profile["builder_tag"], "builder-" + profile["architecture"] + ".tar.gz")):
        archive = images / filename
        inspection = json.loads(subprocess.check_output(
            ["docker", "image", "inspect", "--platform", platform, image]))[0]
        if inspection["Architecture"] != profile["architecture"]:
            raise RuntimeError("image architecture is not " + profile["architecture"] + ": " + image)
        print("Exporting " + image, flush=True)
        export_stream(["docker", "save", "--platform", platform, image], archive)
        digest = sha256(archive)
        checksums.append(digest + "  images/" + filename)
        manifest.append({"image": image, "id": inspection["Id"], "architecture": profile["architecture"],
                         "file": filename, "bytes": archive.stat().st_size, "sha256": digest})
    (images / "SHA256SUMS").write_bytes(("\n".join(checksums) + "\n").encode("utf-8"))
    (images / "manifest.json").write_bytes((json.dumps(manifest, indent=2) + "\n").encode("utf-8"))
    print("Offline deployment prepared: " + str(output))
    print("Supplement prebuilt arches: " + ", ".join(prebuilt_arches))


if __name__ == "__main__":
    main()
