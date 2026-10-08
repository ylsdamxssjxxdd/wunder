#!/usr/bin/env bash
set -euo pipefail

# Build the Linux Wunder supplement pack (opt/python + opt/git + opt/rg) for
# amd64/arm64. The script must run inside an Ubuntu 18.04 container so every
# binary matches the desktop release baseline (glibc 2.27); the arm64 pack is
# produced by running the same script in the arm64 variant of the image.
#
# Inputs (environment, all optional):
#   ARCH                          x86_64|aarch64|amd64|arm64 (default: host)
#   WUNDER_REPO_ROOT              repo checkout mounted in the container
#   WUNDER_OUTPUT_DIR             dist directory for the final tar.gz
#   WUNDER_SUPPLEMENT_BUILD_ROOT  build root (downloads/stage/dist below it)
#   WUNDER_SUPPLEMENT_PYPI_INDEX  pip index (default: manifest default)
#   WUNDER_SUPPLEMENT_REQUIREMENTS  requirements file override
#
# Output: <output_dir>/<packageName>-linux-<arch>.tar.gz with opt/ at the
# archive root, plus README-linux-supplement.txt and the manifest JSON.

repo_root="${WUNDER_REPO_ROOT:-/app}"
manifest_path="$repo_root/packaging/linux/scripts/linux-supplement-manifest.json"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Deterministic text handling: the pack manifest, README and bundled font
# names carry non-ASCII strings and the python3 heredocs below print with
# ensure_ascii=False. Under a bare C locale (Python 3.6 on bionic) stdout is
# ASCII and those prints crash with UnicodeEncodeError, so pin the stdio
# encoding for every python this script runs, in containers and offline
# builders alike.
export PYTHONIOENCODING=utf-8

log() { echo "[linux-supplement] $*"; }
fail() { echo "[linux-supplement] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

require_file "$manifest_path"
require_file "$repo_root/config/wunder-example.yaml"
for tool in python3 curl tar gzip sha256sum make gcc strip file find; do
  require_command "$tool"
done

case "${ARCH:-$(uname -m)}" in
  x86_64|amd64)  arch="amd64"; elf_machine="x86-64" ;;
  aarch64|arm64) arch="arm64"; elf_machine="aarch64" ;;
  *) fail "unsupported architecture: ${ARCH:-$(uname -m)}" ;;
esac
log "arch: $arch (ELF machine: $elf_machine)"

manifest_json="$(python3 - "$manifest_path" <<'PYEOF'
import json, sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)

def emit(**kwargs):
    import json as j
    print(j.dumps(kwargs, ensure_ascii=False))

emit(
    package_name=data["packageName"],
    default_build_root=data["defaultBuildRoot"],
    layout=data["layout"],
    python=data["python"],
    git=data["git"],
    rg=data["rg"],
    fonts=data.get("fonts", []),
    install=data["install"],
)
PYEOF
)"

json_field() {
  python3 - "$manifest_json" "$1" <<'PYEOF'
import json, sys

payload = json.loads(sys.argv[1])
current = payload
for token in sys.argv[2].split("."):
    if isinstance(current, list):
        current = current[int(token)]
    else:
        current = current[token]
print(current if not isinstance(current, (dict, list)) else json.dumps(current, ensure_ascii=False))
PYEOF
}

package_name="$(json_field package_name)"
python_version="$(json_field python.version)"
python_flavor="$(json_field python.flavor)"
python_release_tag="$(json_field python.releaseTag)"
python_get_pip_url="$(json_field python.getPipUrl)"
python_requirements="$(json_field python.requirementsPath)"
python_index_default="$(json_field python.defaultPackageIndexUrl)"
git_version="$(json_field git.version)"
git_flavor="$(json_field git.flavor)"
git_tarball_url="$(json_field git.tarball.url)"
git_tarball_file="$(json_field git.tarball.fileName)"
git_tarball_sha256="$(json_field git.tarball.sha256)"
rg_version="$(json_field rg.version)"
rg_flavor="$(json_field rg.flavor)"
python_archive_url="$(json_field "python.archives.$arch.url")"
python_archive_file="$(json_field "python.archives.$arch.fileName")"
rg_archive_url="$(json_field "rg.archives.$arch.url")"
rg_archive_file="$(json_field "rg.archives.$arch.fileName")"

[[ -n "$python_archive_url" && "$python_archive_url" != "null" ]] || fail "no python archive for $arch"
[[ -n "$rg_archive_url" && "$rg_archive_url" != "null" ]] || fail "no ripgrep archive for $arch"

default_build_root="$repo_root/$(json_field default_build_root)"
build_root="${WUNDER_SUPPLEMENT_BUILD_ROOT:-$default_build_root}"
mkdir -p "$build_root"
build_root="$(cd -- "$build_root" && pwd)"
downloads_dir="$build_root/downloads"
stage_dir="$build_root/stage"
package_root="$stage_dir/package-root"
dist_dir="${WUNDER_OUTPUT_DIR:-$build_root/dist}"
mkdir -p "$downloads_dir" "$dist_dir"
dist_dir="$(cd -- "$dist_dir" && pwd)"
requirements_path="${WUNDER_SUPPLEMENT_REQUIREMENTS:-$repo_root/$python_requirements}"
require_file "$requirements_path"
index_url="${WUNDER_SUPPLEMENT_PYPI_INDEX:-$python_index_default}"

python_root="$package_root/opt/python"
git_root="$package_root/opt/git"
rg_root="$package_root/opt/rg"
rm -rf "$stage_dir"
mkdir -p "$python_root" "$git_root" "$rg_root"

obtain_file() {
  # obtain_file <url> <destination> <expected-sha256>
  local url="$1" destination="$2" expected="$3"
  if [[ -f "$destination" ]] && echo "$expected  $destination" | sha256sum -c --status >/dev/null 2>&1; then
    log "reusing cached download: $(basename "$destination")"
    return
  fi
  log "downloading: $url"
  local temporary="$destination.tmp"
  curl -fL --retry 5 --retry-delay 3 --connect-timeout 30 -o "$temporary" "$url"
  echo "$expected  $temporary" | sha256sum -c --status >/dev/null
  mv -f "$temporary" "$destination"
}

sidecar_sha256() {
  # Fetches the <url>.sha256 companion published by the upstream release.
  local url="$1" destination="$2"
  local sidecar="$destination.sha256"
  if [[ ! -f "$sidecar" ]]; then
    curl -fL --retry 5 --retry-delay 3 --connect-timeout 30 -o "$sidecar" "$url.sha256"
  fi
  local digest
  digest="$(awk '{print $1}' "$sidecar")"
  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail "invalid sha256 sidecar for $(basename "$destination")"
  echo "$digest"
}

log "downloading CPython $python_version ($python_flavor)"
python_sha256="$(sidecar_sha256 "$python_archive_url" "$downloads_dir/$python_archive_file")"
obtain_file "$python_archive_url" "$downloads_dir/$python_archive_file" "$python_sha256"

log "extracting Python runtime"
python_extract="$stage_dir/python-extract"
mkdir -p "$python_extract"
tar -xzf "$downloads_dir/$python_archive_file" -C "$python_extract"
[[ -d "$python_extract/python" ]] || fail "python-build-standalone archive layout unexpected: missing python/"
# mv "$python_extract/python/." "$python_root/" renames onto an existing
# directory, which Docker Desktop's shared filesystem rejects with EBUSY
# (offline builders on similar setups hit the same wall). Stage by copying
# the contents instead; the extract dir is disposable.
cp -a "$python_extract/python/." "$python_root/"
rm -rf "$python_extract"

python_bin="$python_root/bin/python3"
[[ -f "$python_bin" ]] || fail "python3 missing from staged runtime: $python_bin"

if ! "$python_bin" -m pip --version >/dev/null 2>&1; then
  log "bootstrapping pip into embedded Python"
  get_pip="$downloads_dir/get-pip.py"
  if [[ ! -f "$get_pip" ]]; then
    curl -fL --retry 5 --retry-delay 3 --connect-timeout 30 -o "$get_pip" "$python_get_pip_url"
  fi
  "$python_bin" "$get_pip"
fi
log "embedded pip: $("$python_bin" -m pip --version)"

log "installing Python packages from $(basename "$requirements_path")"
"$python_bin" -m pip install \
  --disable-pip-version-check \
  --no-warn-script-location \
  --only-binary=:all: \
  --no-cache-dir \
  --no-compile \
  --index-url "$index_url" \
  -r "$requirements_path"

site_packages="$("$python_bin" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
log "site-packages: $site_packages"

# Match the Win7 supplement and the desktop runtime contract: the custom rc
# lives at <python_root>/etc/matplotlibrc and the runtime activates it via
# MATPLOTLIBRC (crates/wunder-runtime/src/core/python_runtime.rs). It must
# NOT replace the shipped mpl-data/matplotlibrc: matplotlib builds its
# rcParamsDefault by parsing that file, so overwriting it with a trimmed rc
# breaks every newer default key (KeyError: 'backend_fallback' on import).
matplotlibrc_source="$repo_root/config/matplotlibrc"
if [[ -f "$matplotlibrc_source" ]]; then
  etc_dir="$python_root/etc"
  mkdir -p "$etc_dir"
  cp -f "$matplotlibrc_source" "$etc_dir/matplotlibrc"
  log "installed matplotlibrc into etc/"
fi

# Mirror the Win7 supplement so bundled Python keeps stable CJK/Latin
# rendering; the fonts directory may be absent, in which case skip silently.
# Bundled fonts are ADDED to the shipped mpl-data font dir (no default files
# are replaced), so matplotlib's font scanner picks them up.
fonts_source_dir="$repo_root/fonts"
mpl_data_dir="$site_packages/matplotlib/mpl-data"
if [[ -d "$fonts_source_dir" && -d "$mpl_data_dir/fonts/ttf" ]]; then
  font_list="$(python3 - "$manifest_json" <<'PYEOF'
import json, sys

payload = json.loads(sys.argv[1])
for font in payload.get("fonts", []):
    print(font)
PYEOF
)"
  while IFS= read -r font; do
    [[ -n "$font" ]] || continue
    [[ -f "$fonts_source_dir/$font" ]] || continue
    cp -f "$fonts_source_dir/$font" "$mpl_data_dir/fonts/ttf/$font"
    log "bundled matplotlib font: $font"
  done <<< "$font_list"
fi

log "building Git $git_version from source"
obtain_file "$git_tarball_url" "$downloads_dir/$git_tarball_file" "$git_tarball_sha256"
git_src="$stage_dir/git-$git_version"
tar -xzf "$downloads_dir/$git_tarball_file" -C "$stage_dir"
[[ -d "$git_src" ]] || fail "git source tree missing after extraction: $git_src"
git_make_vars=(prefix="$git_root" NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1 NO_INSTALL_HARDLINKS=1)
make -j"$(nproc)" -C "$git_src" "${git_make_vars[@]}" all
make -C "$git_src" "${git_make_vars[@]}" install
git_bin="$git_root/bin/git"
[[ -f "$git_bin" ]] || fail "git binary missing after install: $git_bin"
# Best-effort strip of ELF payloads to keep the embedded tree compact.
find "$git_root" -type f -print0 | while IFS= read -r -d '' candidate; do
  if file -b "$candidate" | grep -q "ELF"; then
    strip --strip-unneeded "$candidate" 2>/dev/null || true
  fi
done
rm -rf "$git_src"

log "installing ripgrep $rg_version"
rg_sha256="$(sidecar_sha256 "$rg_archive_url" "$downloads_dir/$rg_archive_file")"
obtain_file "$rg_archive_url" "$downloads_dir/$rg_archive_file" "$rg_sha256"
rg_extract="$stage_dir/rg-extract"
mkdir -p "$rg_extract"
tar -xzf "$downloads_dir/$rg_archive_file" -C "$rg_extract"
rg_source_file="$(find "$rg_extract" -type f -name rg -print -quit)"
[[ -n "$rg_source_file" ]] || fail "rg binary not found after extracting $rg_archive_file"
mkdir -p "$rg_root/bin"
cp -f "$rg_source_file" "$rg_root/bin/rg"
chmod 0755 "$rg_root/bin/rg"
rg_license="$(find "$rg_extract" -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' \) -print -quit)"
[[ -z "$rg_license" ]] || cp -f "$rg_license" "$rg_root/"
rm -rf "$rg_extract"

requirements_entries="$("$python_bin" - "$requirements_path" <<'PYEOF'
import sys

entries = []
with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        line = line.strip()
        if line and not line.startswith("#"):
            entries.append(line)
print("\n".join(entries))
PYEOF
)"

cat > "$package_root/README-linux-supplement.txt" <<EOF
Wunder Linux supplement package

Arch: $arch
Python: $python_version ($python_flavor)
Python packages: $(echo "$requirements_entries" | wc -l) pinned requirements (see wunder-linux-supplement.json)
Git: $git_flavor $git_version
Ripgrep: $rg_flavor $rg_version

Usage:
1. Close Wunder Desktop.
2. Extract this archive into the desktop install directory, or (AppImage)
   into the directory that holds the .AppImage file.
3. Ensure the target now contains opt/python, opt/git, and opt/rg.
4. Start Wunder Desktop again; the native runtime prepends opt/python,
   opt/git, and opt/rg to PATH automatically.
EOF

python3 - "$manifest_json" "$arch" "$build_root" "$python_archive_url" "$git_tarball_url" "$rg_archive_url" "$requirements_path" "$package_root/wunder-linux-supplement.json" <<'PYEOF'
import json, sys
from datetime import datetime, timezone

payload, arch, build_root, python_url, git_url, rg_url, requirements_path, destination = sys.argv[1:9]
manifest = json.loads(payload)
requirements = []
with open(requirements_path, encoding="utf-8") as handle:
    for line in handle:
        line = line.strip()
        if line and not line.startswith("#"):
            requirements.append(line)

output = {
    "generatedAt": datetime.now(timezone.utc).isoformat(),
    "arch": arch,
    "buildRoot": build_root,
    "packageName": manifest["package_name"],
    "python": {
        "version": manifest["python"]["version"],
        "flavor": manifest["python"]["flavor"],
        "source": manifest["python"]["source"],
        "releaseTag": manifest["python"]["releaseTag"],
        "url": python_url,
        "indexUrl": manifest["python"]["defaultPackageIndexUrl"],
        "requirementsPath": requirements_path,
        "packages": requirements,
    },
    "git": {
        "version": manifest["git"]["version"],
        "flavor": manifest["git"]["flavor"],
        "source": manifest["git"]["source"],
        "url": git_url,
    },
    "rg": {
        "version": manifest["rg"]["version"],
        "flavor": manifest["rg"]["flavor"],
        "source": manifest["rg"]["source"],
        "url": rg_url,
    },
    "install": manifest["install"],
}
with open(destination, "w", encoding="utf-8") as handle:
    json.dump(output, handle, ensure_ascii=False, indent=2)
    handle.write("\n")
PYEOF

log "validating staged runtime"
plot_probe="$(json_field python.plotProbe)"
[[ "$plot_probe" = "True" || "$plot_probe" = "true" ]] && plot_probe="true" || plot_probe="false"

probe_script="$stage_dir/validate-python-profile.py"
python3 - "$manifest_json" "$plot_probe" "$probe_script" <<'PYEOF'
import json, sys

payload, plot_probe, destination = sys.argv[1], sys.argv[2], sys.argv[3]
modules = json.loads(payload)["python"]["validateImports"]
lines = [
    "import importlib",
    "import json",
    "from pathlib import Path",
    "",
    "modules = json.loads(r'''%s''')" % json.dumps(modules, ensure_ascii=False),
    "for module_name in modules:",
    "    importlib.import_module(module_name)",
    "",
]
if plot_probe == "true":
    lines += [
        "import matplotlib",
        "matplotlib.use('Agg')",
        "import matplotlib.pyplot as plt",
        "",
        "plt.plot([1, 2, 3], [1, 4, 9])",
        "plt.title('wunder-linux-python-profile')",
        "plt.savefig(Path('matplotlib-probe.png'))",
    ]
with open(destination, "w", encoding="utf-8") as handle:
    handle.write("\n".join(lines) + "\n")
PYEOF

probe_plot_png="$stage_dir/matplotlib-probe.png"
rm -f "$probe_plot_png"
(
  cd "$stage_dir"
  # Exercise the same rc the desktop runtime will activate via MATPLOTLIBRC.
  export MATPLOTLIBRC="$python_root/etc/matplotlibrc"
  OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 NUMEXPR_NUM_THREADS=1 \
    "$python_bin" "$probe_script"
)
if [[ "$plot_probe" = "true" && ! -f "$probe_plot_png" ]]; then
  fail "matplotlib plot probe did not produce output"
fi
log "validated Python profile imports"

"$python_bin" --version
"$git_bin" --version
"$rg_root/bin/rg" --version | head -n 1

for staged_binary in "$python_bin" "$git_bin" "$rg_root/bin/rg"; do
  # -L resolves aliases (opt/python/bin/python3 is a symlink to python3.8);
  # without it file reports "symbolic link to …" and no ELF at all.
  staged_machine="$(file -bL "$staged_binary")"
  case "$staged_machine" in
    *ELF*) ;;
    *) fail "staged binary is not ELF: $staged_binary" ;;
  esac
  echo "$staged_machine" | grep -qi "$elf_machine" || fail "staged binary has wrong architecture (expected $elf_machine): $staged_machine"
done
log "validated ELF architectures ($elf_machine)"

output_name="${package_name}-linux-${arch}.tar.gz"
output_path="$dist_dir/$output_name"
rm -f "$output_path"
log "packing supplement tar.gz: $output_path"
tar -C "$package_root" \
  --sort=name \
  --owner=0 --group=0 --numeric-owner \
  --format=gnu \
  -cf - . | gzip -n > "$output_path"

[[ -s "$output_path" ]] || fail "produced supplement archive is empty"
output_sha256="$(sha256sum "$output_path" | awk '{print $1}')"
log "output: $output_path"
log "sha256: $output_sha256"
log "size:   $(du -h "$output_path" | awk '{print $1}')"
rm -rf "$stage_dir"
