#!/usr/bin/env bash
# 导出改动文件（补齐 scripts/export_patch_files.ps1 的 Linux 版本）
#
# 语义与 PowerShell 版一致：导出 <base>^..HEAD 之间新增/修改/重命名的文件快照，
# 即 <base> 这次提交本身的改动也包含在内；删除项只记录到 __deleted_files.txt，
# 不在目标目录里执行删除，方便对端按清单清理。
#
# 用法：
#   scripts/export_patch_files.sh <base-commit> <输出目录> [--keep-existing]
#
# 输出目录必须位于仓库之外，默认会被清空（--keep-existing 可保留已有内容）。
set -euo pipefail

usage() {
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
}

base="${1:-}"
out_dir="${2:-}"
keep_existing=0

[[ -n "$base" && -n "$out_dir" ]] || usage
if [[ "${3:-}" == "--keep-existing" ]]; then
    keep_existing=1
elif [[ -n "${3:-}" ]]; then
    usage
fi

repo_root="$(git rev-parse --show-toplevel)"
repo_root="$(realpath -e "$repo_root")"

if [[ "$base" =~ ^[0-9a-fA-F]+$ && ${#base} -ne 40 ]]; then
    echo "错误：看起来是裸 SHA-1，但长度为 ${#base}，应为 40：$base" >&2
    exit 1
fi

resolved_base="$(git rev-parse --quiet --verify "${base}^{commit}" || true)"
if [[ -z "$resolved_base" ]]; then
    echo "错误：找不到提交对象：$base" >&2
    exit 1
fi

out_abs="$(realpath -m "$out_dir")"
if [[ "$out_abs" == "$repo_root" ]]; then
    echo "错误：输出目录不能是仓库根目录" >&2
    exit 1
fi
if [[ "$out_abs" == "$repo_root"/* ]]; then
    echo "错误：输出目录不能位于仓库内部" >&2
    exit 1
fi

if [[ -e "$out_abs" && ! -d "$out_abs" ]]; then
    echo "错误：输出路径已存在且不是目录：$out_abs" >&2
    exit 1
fi
mkdir -p "$out_abs"
if [[ $keep_existing -eq 0 ]]; then
    find "$out_abs" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
fi

# -z：以 NUL 分隔，安全处理含空格、中文、引号的路径
copy_paths=()
delete_paths=()
while IFS= read -r -d '' status; do
    case "$status" in
        R*|C*)
            IFS= read -r -d '' old_path
            IFS= read -r -d '' new_path
            [[ "$status" == R* ]] && delete_paths+=("$old_path")
            copy_paths+=("$new_path")
            ;;
        D*)
            IFS= read -r -d '' path
            delete_paths+=("$path")
            ;;
        A*|M*|T*|U*)
            IFS= read -r -d '' path
            copy_paths+=("$path")
            ;;
        *)
            echo "错误：未处理的 git status：$status" >&2
            exit 1
            ;;
    esac
done < <(git -c core.quotepath=false diff --name-status -z --find-renames "${resolved_base}^" HEAD)

copied_count=0
declare -A copied_seen=()
for rel in "${copy_paths[@]}"; do
    [[ -n "${copied_seen[$rel]:-}" ]] && continue
    copied_seen[$rel]=1
    src="$repo_root/$rel"
    if [[ ! -f "$src" ]]; then
        echo "错误：源文件缺失：$src" >&2
        exit 1
    fi
    dst="$out_abs/$rel"
    mkdir -p "$(dirname "$dst")"
    cp -p -- "$src" "$dst"
    copied_count=$((copied_count + 1))
done

manifest="$out_abs/__patch_manifest.txt"
delete_list="$out_abs/__deleted_files.txt"

# 清单统一 CRLF + UTF-8(无 BOM)，与 PowerShell 版一致，便于 Windows 端直接读取
manifest_tmp="$(mktemp)"
delete_tmp="$(mktemp)"
trap 'rm -f "$manifest_tmp" "$delete_tmp"' EXIT

{
    printf 'base_commit_inclusive=%s\r\n' "$resolved_base"
    printf 'comparison=%s^..HEAD\r\n' "$resolved_base"
    printf 'head_commit=%s\r\n' "$(git rev-parse HEAD)"
    printf 'export_source=%s\r\n' "$repo_root"
    printf 'export_target=%s\r\n' "$out_abs"
    printf 'copied_files=%s\r\n' "$copied_count"
    printf 'deleted_files=%s\r\n' "${#delete_paths[@]}"
    printf 'generated_at=%s\r\n' "$(date '+%Y-%m-%d %H:%M:%S')"
} > "$manifest_tmp"

if [[ ${#delete_paths[@]} -gt 0 ]]; then
    printf '%s\r\n' "${delete_paths[@]}" > "$delete_tmp"
else
    : > "$delete_tmp"
fi

mv -- "$delete_tmp" "$delete_list"
mv -- "$manifest_tmp" "$manifest"
trap - EXIT

echo "导出完成：$out_abs"
echo "复制文件：$copied_count"
echo "删除文件：${#delete_paths[@]}"
echo "清单：$manifest"
echo "删除清单：$delete_list"
