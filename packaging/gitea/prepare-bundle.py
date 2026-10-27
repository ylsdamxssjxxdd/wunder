"""Prepare a relocatable ARM64 Gitea deployment with an offline Rust builder."""

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


GITEA = "docker.gitea.com/gitea:28.2.0"
RUNNER = "docker.gitea.com/runner:5.2.3"
BUILDER = "wunder-offline-builder:20261027-arm64-mounted"


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--reuse-sdk-archive", action="store_true")
    args = parser.parse_args()
    source = Path(__file__).resolve().parent
    output = args.output.resolve()
    images = output / "images"
    images.mkdir(parents=True, exist_ok=True)
    for name in (GITEA, RUNNER):
        run("docker", "pull", "--platform", "linux/arm64", name)
    if not args.skip_build:
        run("docker", "build", "--platform", "linux/arm64",
            "-f", str(source / "Dockerfile.builder-arm64"), "-t", BUILDER, str(source))
    sdk_directory = output / "rust-builder"
    sdk_directory.mkdir(exist_ok=True)
    sdk_archive = sdk_directory / "offline-sdk.tar.gz"
    if not args.reuse_sdk_archive:
        sdk = args.sdk.resolve()
        required = ["rust/toolchains", "cargo-vendor-slint", "linux-arm64-ubuntu18/root",
                    "linux-amd64-ubuntu18/root", "appimage-runtime",
                    "mingw-i686-windows-gnu", "web-wasm"]
        for relative in required:
            if not (sdk / relative).is_dir():
                raise RuntimeError("required SDK directory is missing: " + relative)
        print("Exporting separate Rust-builder archive (SDK is not part of any image)", flush=True)
        export_stream(["docker", "run", "--rm", "--platform", "linux/arm64", "--network", "none",
                       "-v", str(sdk) + ":/payload/offline:ro", "--entrypoint", "tar", BUILDER,
                       "-C", "/payload", "-cf", "-", *("offline/" + relative for relative in required)], sdk_archive)
    elif not sdk_archive.is_file():
        raise RuntimeError("no SDK archive to reuse")
    else:
        expected = (sdk_directory / "SHA256SUMS").read_text(encoding="utf-8").split()[0]
        if sha256(sdk_archive) != expected:
            raise RuntimeError("existing SDK archive checksum mismatch")
    (sdk_directory / "SHA256SUMS").write_bytes((sha256(sdk_archive) + "  offline-sdk.tar.gz\n").encode())
    for name in ("compose.yaml", "runner.yaml", "bootstrap.sh", "load-images.sh", "prepare-sdk.sh", ".env.example", "README.md"):
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
    for image, filename in ((GITEA, "gitea-arm64.tar.gz"), (RUNNER, "runner-arm64.tar.gz"),
                            (BUILDER, "builder-arm64.tar.gz")):
        archive = images / filename
        inspection = json.loads(subprocess.check_output(["docker", "image", "inspect", "--platform", "linux/arm64", image]))[0]
        if inspection["Architecture"] != "arm64":
            raise RuntimeError("image architecture is not ARM64: " + image)
        print("Exporting " + image, flush=True)
        export_stream(["docker", "save", "--platform", "linux/arm64", image], archive)
        digest = sha256(archive)
        checksums.append(digest + "  images/" + filename)
        manifest.append({"image": image, "id": inspection["Id"], "architecture": "arm64",
                         "file": filename, "bytes": archive.stat().st_size, "sha256": digest})
    (images / "SHA256SUMS").write_bytes(("\n".join(checksums) + "\n").encode("utf-8"))
    (images / "manifest.json").write_bytes((json.dumps(manifest, indent=2) + "\n").encode("utf-8"))
    print("Offline deployment prepared: " + str(output))


if __name__ == "__main__":
    main()
