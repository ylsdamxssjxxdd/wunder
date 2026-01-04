﻿<#
使用说明（Windows）：
1) 将指定 Markdown 中的 Mermaid 代码块导出为 .mmd 源文件
2) 使用 mermaid-cli 渲染为 svg 或 png
3) 默认输入 docs/系统介绍.md，输出到当前脚本所在目录
#>

[CmdletBinding()]
param(
  [ValidateSet('svg', 'png')]
  [string]$Format = 'svg',
  # 可选：指定输入文件路径（相对仓库根目录或绝对路径）
  [string]$InputPath = '',
  # 可选：指定输出目录（相对仓库根目录或绝对路径）
  [string]$OutputDir = '',
  # 可选：允许输入文档的 Mermaid 块数量少于默认清单
  [switch]$AllowPartial
)

# 解析脚本目录与仓库根目录，避免依赖当前工作目录
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Resolve-Path (Join-Path $scriptDir '..\..\..')
# 计算输入路径：默认使用 docs/系统介绍.md
if ([string]::IsNullOrWhiteSpace($InputPath)) {
  $inputPath = Join-Path $repoRoot 'docs\系统介绍.md'
} else {
  $inputPath = $InputPath
  if (-not (Split-Path $inputPath -IsAbsolute)) {
    $inputPath = Join-Path $repoRoot $inputPath
  }
}
# 计算输出路径：默认输出到脚本目录
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
  $outDir = $scriptDir
} else {
  $outDir = $OutputDir
  if (-not (Split-Path $outDir -IsAbsolute)) {
    $outDir = Join-Path $repoRoot $outDir
  }
  New-Item -ItemType Directory -Force -Path $outDir | Out-Null
}

# Mermaid 块顺序与命名（与文档内出现顺序一致）
$names = @(
  '01-system-components',
  '02-request-flow',
  '03-tool-management-flow',
  '04-session-state',
  '05-context-compaction-flow'
)

if (-not (Test-Path $inputPath)) {
  throw "未找到输入文件：$inputPath"
}

# 提取 Mermaid 代码块，写入 .mmd 源文件
$lines = Get-Content -Path $inputPath -Encoding UTF8
$inside = $false
$current = @()
$index = 0
foreach ($line in $lines) {
  if ($line.Trim() -eq '```mermaid') {
    $inside = $true
    $current = @()
    continue
  }
  if ($inside -and $line.Trim() -eq '```') {
    if ($index -ge $names.Count) {
      throw "Mermaid 代码块数量超过预期，检查输入文件：$inputPath"
    }
    $path = Join-Path $outDir ($names[$index] + '.mmd')
    Set-Content -Path $path -Value $current -Encoding UTF8
    $index++
    $inside = $false
    continue
  }
  if ($inside) {
    $current += $line
  }
}
if ($inside) {
  throw 'Mermaid 代码块未闭合，请检查输入文档'
}
if ($index -ne $names.Count) {
  if ($AllowPartial -and $index -lt $names.Count) {
    Write-Warning ("Mermaid 代码块数量少于默认清单：期望 {0} 实际 {1}" -f $names.Count, $index)
  } else {
    throw ("Mermaid 代码块数量不一致，期望 {0} 实际 {1}" -f $names.Count, $index)
  }
}

# 设置本机浏览器路径（优先 Edge/Chrome），避免 puppeteer 自动下载
if (-not $env:PUPPETEER_EXECUTABLE_PATH) {
  $candidates = @(
    'C:\Program Files\Microsoft\Edge\Application\msedge.exe',
    'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
    'C:\Program Files\Google\Chrome\Application\chrome.exe',
    'C:\Program Files (x86)\Google\Chrome\Application\chrome.exe'
  )
  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      $env:PUPPETEER_EXECUTABLE_PATH = $candidate
      break
    }
  }
}

if (-not (Get-Command npx -ErrorAction SilentlyContinue)) {
  throw '未找到 npx，请先安装 Node.js'
}

# 渲染 .mmd 为目标格式（默认 svg）
Get-ChildItem -Path $outDir -Filter *.mmd | ForEach-Object {
  $outPath = [System.IO.Path]::ChangeExtension($_.FullName, ".$Format")
  npx -y @mermaid-js/mermaid-cli -i $_.FullName -o $outPath
}
