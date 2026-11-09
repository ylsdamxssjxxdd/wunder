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
# 老 Docker（18.09 / API 1.39）上的 Runner：0.2.11 的内置客户端下限是 API 1.24。
LEGACY_RUNNER = "gitea/act_runner:0.2.11"

# 每个剖面是一套可移植套件：任务镜像、SDK 必需目录、卷名前缀都不同，
# 但工作流与 CI 脚本完全共用（标签里的架构由 runner 注册时决定）。
PROFILES = {
    "arm64": {
        "platform": "linux/arm64",
        "architecture": "arm64",
        "host_uname": "aarch64",
        "sdk_profile": "kylin-arm",
        "stack_prefix": "offline-gitea",
        # 目标机是麒麟 V10 一类只有老 Docker Engine 的 ARM 服务器，Runner 固定在 0.2.11，
        # 镜像清单、compose v1/v2 与 load-images.sh 的下限校验都以它为准。
        "runner_image": LEGACY_RUNNER,
        "runner_min_api": "1.24",
        # 麒麟 V10 的 Docker 18.09/19.03 默认 seccomp 对 clone3 返回 EPERM，套件里的静态
        # qemu-x86_64-static（glibc 2.35）连 --version 都会在建线程时 abort；任务容器放开
        # seccomp。升到 Docker ≥20.10.10（默认 profile 对 clone3 返回 ENOSYS，glibc 会回退
        # clone）后可去掉。amd64 剖面不用 qemu，保持默认沙箱。
        "container_seccomp_opt": "--security-opt seccomp=unconfined ",
        "builder_dockerfile": "Dockerfile.builder-arm64",
        "builder_tag": "wunder-offline-builder:20261103-arm64-mounted",
        # Win7 桌面与 CLI 在 ARM 上仍原生交叉（build-std + SDK 内的 aarch64 版 MinGW 便携包），
        # 只有 Inno Setup 安装包需要 Wine：套件带 amd64 的 Wine 安装镜像，任务在 qemu-x86_64
        # binfmt 模拟下运行（见 binfmt/register.sh）；arm64 剖面因此必须随包导出 amd64 镜像。
        "installer_dockerfile": None,
        "installer_tag": "wunder-offline-installer:20261103-amd64-wine",
        "installer_arch": "amd64",
        # cargo-home-* 要留在归档里：rcho 的只读 SDK 视图靠把这几个名字换成可写目录来绕开 :ro，
        # 一个都不带的话 prepare-view.sh 会直接报「SDK 布局里没有可转换的目录」。
        "sdk_required": ["rust/toolchains", "cargo-vendor-slint", "linux-arm64-ubuntu18/root",
                         "linux-amd64-ubuntu18/root", "appimage-runtime",
                         "mingw-i686-windows-gnu", "web-wasm",
                         "cargo-home-slint", "cargo-home-slint-amd64",
                         "cargo-home-slint-win7", "cargo-home-slint-win32"],
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
        "runner_image": RUNNER,
        "runner_min_api": "1.40",
        "container_seccomp_opt": "",
        # 基镜像是 builders/build-kylin-x86-sdk-docker.sh 产出的 x86_64 SDK 镜像，
        # 已带 gcc/cmake/ninja/nasm/clang 与交叉工具链，套件只补 git 和 CI 脚本。
        "builder_dockerfile": "Dockerfile.builder-amd64",
        "builder_tag": "wunder-offline-builder:20261102-amd64-mounted",
        # Win7 安装包要在 Wine 下跑 ISCC.exe 组，交给独立的第四个镜像（Ubuntu 22.04 基线）。
        "installer_dockerfile": "Dockerfile.installer-amd64",
        "installer_tag": "wunder-offline-installer:20261103-amd64-wine",
        "installer_arch": "amd64",
        # host-tools-amd64 带 .complete 标记与已解压的 aarch64 交叉编译器，
        # rcho 的 prepare_x86_cross_toolchain.sh 会直接短路，不必再带 1.4G 的 archives 归档。
        "sdk_required": ["rust/toolchains", "cargo-vendor-slint", "linux-amd64-ubuntu18/root",
                         "linux-arm64-ubuntu18/root", "host-tools-amd64", "appimage-runtime",
                         "mingw-i686-windows-gnu"],
        "squashfs_processors": "3",
        # x86_64 构建机按 24 核 / 64G 量级配置：五个任务并行，每个 4 线程（20 道编译 ≤ 24 核）。
        "runner_capacity": "5",
        "build_jobs": "4",
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


def token_replacements(profile, labels):
    return {
        '"@RUNNER_CAPACITY@"': profile["runner_capacity"],
        "@WUNDER_BUILD_JOBS@": profile["build_jobs"],
        "@WUNDER_SQUASHFS_PROCESSORS@": profile["squashfs_processors"],
        "@BUILDER_LABEL@": labels["builder"],
        "@RCHO_LABEL@": labels["rcho"],
        "@INSTALLER_LABEL@": labels["installer"],
        "@STACK_PREFIX@": profile["stack_prefix"],
        "@CONTAINER_SECCOMP_OPT@": profile["container_seccomp_opt"],
    }


def render_text(source_text, profile, labels):
    text = source_text
    for token, value in token_replacements(profile, labels).items():
        text = text.replace(token, value)
    leftover = [token for token in ("@RUNNER_CAPACITY@", "@STACK_PREFIX@", "@BUILDER_LABEL@",
                                    "@RCHO_LABEL@", "@INSTALLER_LABEL@", "@WUNDER_BUILD_JOBS@",
                                    "@WUNDER_SQUASHFS_PROCESSORS@", "@CONTAINER_SECCOMP_OPT@") if token in text]
    if leftover:
        raise RuntimeError("模板里还有未渲染的占位符: " + ", ".join(leftover))
    return text


def render_runner(source, profile, labels):
    return render_text((source / "runner.yaml").read_text(encoding="utf-8"), profile, labels)


def write_env(source, profile, labels, android_path=""):
    lines = []
    for line in (source / ".env.example").read_text(encoding="utf-8").splitlines():
        key = line.split("=", 1)[0].strip() if "=" in line and not line.startswith("#") else ""
        if key in {"CI_PLATFORM", "STACK_PREFIX", "SDK_PROFILE", "GITEA_IMAGE", "RUNNER_IMAGE",
                   "RUNNER_MIN_DOCKER_API", "BUILDER_IMAGE", "INSTALLER_IMAGE", "RUNNER_LABELS",
                   "RUNNER_NAME", "WUNDER_BUILD_JOBS", "WUNDER_ANDROID_PATH"}:
            continue
        lines.append(line)
    values = {
        "CI_PLATFORM": profile["platform"],
        "STACK_PREFIX": profile["stack_prefix"],
        "SDK_PROFILE": profile["sdk_profile"],
        "GITEA_IMAGE": GITEA,
        "RUNNER_IMAGE": profile["runner_image"],
        "RUNNER_MIN_DOCKER_API": profile["runner_min_api"],
        "WUNDER_ANDROID_PATH": android_path,
        "BUILDER_IMAGE": profile["builder_tag"],
        "INSTALLER_IMAGE": profile["installer_tag"] or profile["builder_tag"],
        "RUNNER_LABELS": ",".join((labels["builder"], labels["rcho"], labels["installer"])),
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
    parser.add_argument("--android-dir", type=Path,
                        help="准备好的安卓工具链与 APK 内嵌资产目录（SDK/NDK/JDK/Gradle/rootfs/补充包），"
                             "导出时整体复制进套件的 android/；不提供则留空目录，安卓任务会因缺输入而失败")
    parser.add_argument("--binfmt-dir", type=Path,
                        help="含 qemu-i386-static 与 qemu-x86_64-static 的 aarch64 解释器目录；"
                             "arm64 剖面用它在宿主模拟 amd64 的 Win7 安装包镜像，导出到套件的 binfmt/")
    args = parser.parse_args()

    source = Path(__file__).resolve().parent
    profile = PROFILES[args.arch]
    platform = profile["platform"]
    output = args.output.resolve()
    images = output / "images"
    images.mkdir(parents=True, exist_ok=True)

    labels = {
        "builder": "wunder-offline:docker://" + profile["builder_tag"],
        # 命令行与桌面版共用同一套 Rust 任务镜像，只是路由标签不同；rcho 的离线流水线用这个标签。
        "rcho": "rcho-offline:docker://" + profile["builder_tag"],
        # 安装包标签名固定，值随剖面走：x86_64 原生跑 Wine 镜像，arm64 也指向同一个 amd64
        # 镜像，由宿主的 qemu binfmt 模拟执行（见 binfmt/register.sh）。
        # 少了冒号前的名字，compose 注册出来的就是没有名字的畸形标签，工作流排不到任何 runner。
        "installer": "wunder-offline-installer:docker://" + (profile["installer_tag"] or profile["builder_tag"]),
    }

    for name in (GITEA, profile["runner_image"]):
        # 准备机可能已经从上一版套件 docker load 过这些镜像，有本地副本就不必再依赖网络。
        if not has_image(name):
            run("docker", "pull", "--platform", platform, name)
    task_images = [profile["builder_tag"]]
    if not args.skip_build:
        run("docker", "build", "--platform", platform,
            "-f", str(source / profile["builder_dockerfile"]), "-t", profile["builder_tag"], str(source))
        if profile["installer_dockerfile"]:
            run("docker", "build", "--platform", platform,
                "-f", str(source / profile["installer_dockerfile"]), "-t", profile["installer_tag"], str(source))
    for name in task_images:
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
        # 链接包装必须自带静态 winpthread 组：posix-threads 版 MinGW 一旦被裸 -lwinpthread 链接，
        # 产出的 exe 在 Win7 上缺 libwinpthread-1.dll，而 CI 的 PE 门禁要到构建时才发现。
        # 导出这里先断一次，旧 SDK 打不进套件。
        wrapper = sdk / "mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-rust-gcc"
        if "--start-group" not in (wrapper.read_text(encoding="utf-8", errors="replace") if wrapper.is_file() else ""):
            raise RuntimeError("SDK 的 MinGW 链接包装缺少静态 winpthread 组，先刷新 SDK: " + str(wrapper))
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
    installer_foreign = (profile["installer_tag"] is not None
                         and profile["installer_arch"] != profile["architecture"])
    if installer_foreign:
        if args.binfmt_dir is None:
            raise RuntimeError("arm64 剖面需要 --binfmt-dir（aarch64 的 qemu-i386-static / "
                               "qemu-x86_64-static），否则 Win7 安装包无法在 ARM 上模拟运行")
        binfmt_target = output / "binfmt"
        shutil.copytree(source / "binfmt", binfmt_target, dirs_exist_ok=True)
        for name in ("qemu-i386-static", "qemu-x86_64-static"):
            binary = args.binfmt_dir.resolve() / name
            if not binary.is_file():
                raise RuntimeError("缺少 binfmt 解释器: " + str(binary))
            header = binary.read_bytes()[:20]
            if header[:4] != b"\x7fELF" or header[18:20] != b"\xb7\x00":
                raise RuntimeError("binfmt 解释器不是 aarch64 ELF: " + str(binary))
            shutil.copy2(binary, binfmt_target / name)
            (binfmt_target / name).chmod(0o755)
        (binfmt_target / "register.sh").chmod(0o755)
        (binfmt_target / "install-service.sh").chmod(0o755)
    android_target = output / "android"
    if args.android_dir is not None:
        android_source = args.android_dir.resolve()
        if not android_source.is_dir():
            raise RuntimeError("android toolchain directory does not exist: " + str(android_source))
        print("Copying Android toolchain and APK assets", flush=True)
        shutil.copytree(android_source, android_target, symlinks=True, dirs_exist_ok=True)
    else:
        android_target.mkdir(exist_ok=True)
        print("WARN: 未提供 --android-dir；套件缺安卓工具链，android 任务会失败（仅影响安卓构建）", flush=True)
    (output / ".env").write_text(write_env(source, profile, labels, str(android_target)), encoding="utf-8")
    for name in ("compose.yaml", "bootstrap.sh", "load-images.sh",
                 "prepare-sdk.sh", "upgrade-docker.sh", ".env.example", "README.md"):
        shutil.copy2(source / name, output / name)
    # Compose v1 的卷名与网络名写在键上，键不能用 ${VAR} 插值，所以这里按剖面渲染真实前缀。
    (output / "compose.v1.yaml").write_text(
        render_text((source / "compose.v1.yaml").read_text(encoding="utf-8"), profile, labels),
        encoding="utf-8")
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
    archives = [(GITEA, "gitea-" + profile["architecture"] + ".tar.gz"),
                (profile["runner_image"], "runner-" + profile["architecture"] + ".tar.gz"),
                (profile["builder_tag"], "builder-" + profile["architecture"] + ".tar.gz")]
    if profile["installer_tag"] and profile["installer_tag"] != profile["builder_tag"]:
        if not has_image(profile["installer_tag"]):
            raise RuntimeError("安装包镜像缺失: " + profile["installer_tag"])
        archives.append((profile["installer_tag"],
                         "installer-" + profile["installer_arch"] + ".tar.gz"))
    for image, filename in archives:
        archive = images / filename
        # 这里不用 --platform：交付用的镜像都是本地已存在的单架构副本（老套件 docker load 回来的
        # 也是单架构），加了平台断言反而会被 containerd 存储判成「不提供该平台」；
        # 架构直接读镜像配置，与剖面对不上就停，避免把跨架构镜像写进套件。
        # 安装包镜像在 arm64 剖面里是 amd64（宿主 binfmt 模拟），单独按 installer_arch 校验。
        inspection = json.loads(subprocess.check_output(["docker", "image", "inspect", image]))[0]
        expected_arch = (profile["installer_arch"] if image == profile["installer_tag"]
                         else profile["architecture"])
        if inspection["Architecture"] != expected_arch:
            raise RuntimeError("image architecture is not " + expected_arch + ": " + image)
        print("Exporting " + image, flush=True)
        export_stream(["docker", "save", image], archive)
        digest = sha256(archive)
        checksums.append(digest + "  images/" + filename)
        entry = {"image": image, "id": inspection["Id"], "architecture": inspection["Architecture"],
                 "file": filename, "bytes": archive.stat().st_size, "sha256": digest}
        if inspection["Architecture"] != profile["architecture"]:
            entry["emulated"] = True
        manifest.append(entry)
    (images / "SHA256SUMS").write_bytes(("\n".join(checksums) + "\n").encode("utf-8"))
    (images / "manifest.json").write_bytes((json.dumps(manifest, indent=2) + "\n").encode("utf-8"))
    print("Offline deployment prepared: " + str(output))
    print("Supplement prebuilt arches: " + ", ".join(prebuilt_arches))


if __name__ == "__main__":
    main()
