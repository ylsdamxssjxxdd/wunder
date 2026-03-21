#!/usr/bin/env bash
set -euo pipefail

# 使用说明（Linux）：
# 1) 将 docs/系统介绍.md 中的 Mermaid 代码块导出为 .mmd 源文件
# 2) 使用 mermaid-cli 渲染为 svg 或 png
# 3) 默认输出到当前脚本所在目录

FORMAT="${1:-svg}"
if [[ "$FORMAT" != "svg" && "$FORMAT" != "png" ]]; then
  echo "仅支持 svg 或 png 格式" >&2
  exit 2
fi

# 解析脚本目录与仓库根目录，避免依赖当前工作目录
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd)"
INPUT_PATH="$REPO_ROOT/docs/系统介绍.md"
OUT_DIR="$SCRIPT_DIR"

# Mermaid 块顺序与命名（与文档内出现顺序一致）
NAMES=(
  "01-system-components"
  "02-request-flow"
  "03-tool-management-flow"
  "04-session-state"
  "05-context-compaction-flow"
)

if [[ ! -f "$INPUT_PATH" ]]; then
  echo "未找到输入文件：$INPUT_PATH" >&2
  exit 3
fi

# 提取 Mermaid 代码块，写入 .mmd 源文件
inside=0
current=""
index=0
while IFS= read -r line; do
  if [[ "$line" == '```mermaid' ]]; then
    inside=1
    current=""
    continue
  fi
  if [[ $inside -eq 1 && "$line" == '```' ]]; then
    if [[ $index -ge ${#NAMES[@]} ]]; then
      echo "Mermaid 代码块数量超过预期，检查 docs/系统介绍.md" >&2
      exit 4
    fi
    printf "%s" "$current" > "$OUT_DIR/${NAMES[$index]}.mmd"
    index=$((index + 1))
    inside=0
    continue
  fi
  if [[ $inside -eq 1 ]]; then
    current+="${line}"$'\n'
  fi
done < "$INPUT_PATH"

if [[ $inside -eq 1 ]]; then
  echo "Mermaid 代码块未闭合，请检查输入文档" >&2
  exit 5
fi
if [[ $index -ne ${#NAMES[@]} ]]; then
  echo "Mermaid 代码块数量不一致，期望 ${#NAMES[@]} 实际 $index" >&2
  exit 6
fi

# 设置本机浏览器路径（优先 Chrome/Chromium/Edge），避免 puppeteer 自动下载
if [[ -z "${PUPPETEER_EXECUTABLE_PATH:-}" ]]; then
  for candidate in google-chrome chromium chromium-browser microsoft-edge msedge; do
    if command -v "$candidate" >/dev/null 2>&1; then
      export PUPPETEER_EXECUTABLE_PATH
      PUPPETEER_EXECUTABLE_PATH="$(command -v "$candidate")"
      break
    fi
  done
fi

if ! command -v npx >/dev/null 2>&1; then
  echo "未找到 npx，请先安装 Node.js" >&2
  exit 7
fi

# 渲染 .mmd 为目标格式（默认 svg）
for file in "$OUT_DIR"/*.mmd; do
  out="${file%.mmd}.$FORMAT"
  npx -y @mermaid-js/mermaid-cli -i "$file" -o "$out"
done
