#!/usr/bin/env bash
# Assemble the single-file portable build: a POSIX shell launcher followed by
# the AppDir archive produced by the desktop build. It runs with no FUSE, no
# libfuse2, no AppImage runtime and no installer, and unpacks itself into
# <home>/app/desktop on first launch.
#
# This entry runs on any Linux host with sh, tar and awk; it never rebuilds
# Rust binaries.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "$0")/.." && pwd -P)"
header="$repo_root/packaging/linux/portable/portable-header.sh"
arch=""
desktop_archive=""
output_dir=""
version=""
marker='__WUNDER_PORTABLE_PAYLOAD__'

usage() {
  cat <<'EOF'
Usage:
  build.sh portable -a linux-amd64|linux-arm64 [options]

Options:
  -a, --arch             Target architecture. Defaults to the host architecture.
      --desktop-archive  AppDir tar.gz. Defaults to
                         target/slint/dist/linux-<arch>/wunder-slint-<ver>-linux-<arch>.tar.gz
  -o, --output-dir DIR   Output directory. Defaults to the archive's directory.
      --version VER      Override the version stamped into the launcher.
  -h, --help             Show this help.

The portable file writes <home>/app/desktop and <home>/.wunder-home, the same
layout the .run installer produces, so user data stays interchangeable.
EOF
}

fail() {
  echo "[wunder-portable] $*" >&2
  exit 2
}

while (($#)); do
  case "$1" in
    -a|--arch)
      shift
      (($#)) || fail "missing value after $1"
      arch="$1"
      ;;
    --desktop-archive)
      shift
      (($#)) || fail "missing value after $1"
      desktop_archive="$1"
      ;;
    -o|--output-dir)
      shift
      (($#)) || fail "missing value after $1"
      output_dir="$1"
      ;;
    --version)
      shift
      (($#)) || fail "missing value after $1"
      version="$1"
      ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unsupported option: $1" ;;
  esac
  shift
done

if [[ -z "$arch" ]]; then
  case "$(uname -m)" in
    x86_64|amd64) arch="linux-amd64" ;;
    aarch64|arm64) arch="linux-arm64" ;;
    *) fail "could not infer the host architecture; pass -a linux-amd64|linux-arm64" ;;
  esac
fi

case "$arch" in
  linux-amd64)
    short_arch=amd64
    arch_label=x64
    ;;
  linux-arm64)
    short_arch=arm64
    arch_label=arm64
    ;;
  *) fail "unsupported architecture: $arch" ;;
esac

if [[ -z "$version" ]]; then
  manifest="$repo_root/frontend-slint/Cargo.toml"
  [[ -f "$manifest" ]] || fail "missing version source: $manifest"
  version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
fi
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "invalid version: ${version:-empty}"

dist_dir="$repo_root/target/slint/dist/$arch"
if [[ -z "$desktop_archive" ]]; then
  desktop_archive="$dist_dir/wunder-slint-$version-linux-$short_arch.tar.gz"
fi
[[ -f "$desktop_archive" ]] || fail "desktop archive is missing: $desktop_archive (build the desktop target first)"
[[ -f "$header" ]] || fail "portable header is missing: $header"

# The header must already end with a newline: the payload starts on the line
# after the marker, so an extra blank line would break the archive.
last_byte="$(tail -c 1 "$header" | od -An -tu1 | tr -d ' ')"
[[ "$last_byte" == "10" ]] || fail "portable header must end with a newline"

if [[ -z "$output_dir" ]]; then
  output_dir="$(cd -- "$(dirname -- "$desktop_archive")" && pwd)"
fi

output="$output_dir/wunder-$version-$arch_label.portable"
temporary="$output.tmp"
rm -f -- "$temporary"
mkdir -p "$output_dir"

# Strip CR in case the header was checked out on Windows, then append the
# archive byte-for-byte.
if ! sed "s|@VERSION@|$version|g; s/\r$//" "$header" > "$temporary"; then
  fail "could not render the portable header"
fi
cat -- "$desktop_archive" >> "$temporary"

# The bytes right after the marker line must be a gzip member.
payload_line="$(LC_ALL=C awk -v m="$marker" '$0 == m { print NR + 1; exit 0 }' "$temporary")"
[[ -n "$payload_line" ]] || fail "rendered header lost the payload marker"
magic="$(tail -n +"$payload_line" "$temporary" | head -c 2 | od -An -tx1 | tr -d ' \n')"
[[ "$magic" == "1f8b" ]] || fail "payload is not gzip data (magic: $magic)"

chmod 0755 "$temporary"
mv -f -- "$temporary" "$output"

echo "[wunder-portable] portable: $output"
echo "[wunder-portable] size: $(stat -c '%s' "$output") bytes, version: $version, arch: $arch_label"
echo "[wunder-portable] the file cannot carry a Unix execute bit on Windows; run chmod +x on Linux"
