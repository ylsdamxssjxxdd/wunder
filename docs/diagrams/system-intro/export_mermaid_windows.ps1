<#
使用说明（Windows）：
1) 将 docs/系统介绍.md 中的 Mermaid 代码块导出为 .mmd 源文件
2) 使用 mermaid-cli 渲染为 svg 或 png
3) 默认输出到当前脚本所在目录
#>

[CmdletBinding()]
param(
  [ValidateSet('svg', 'png')]
  [string]$Format = 'svg'
)

# 解析脚本目录与仓库根目录，避免依赖当前工作目录
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Resolve-Path (Join-Path $scriptDir '..\..\..')
$inputPath = Join-Path $repoRoot 'docs\系统介绍.md'
$outDir = $scriptDir

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
      throw "Mermaid 代码块数量超过预期，检查 docs/系统介绍.md"
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
  throw ("Mermaid 代码块数量不一致，期望 {0} 实际 {1}" -f $names.Count, $index)
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
