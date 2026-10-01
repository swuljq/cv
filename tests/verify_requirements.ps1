$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\docs\requirements.md'
if (-not (Test-Path $path)) { throw "requirements.md not found" }
$content = Get-Content -Raw $path
$required = @(
  '# ClipBridge 剪贴板共享软件需求文档（PRD）',
  '## 1. 背景与目标',
  '## 5. 核心功能需求',
  '## 7. 服务端需求',
  '## 8. 安全与隐私',
  '## 11. MVP 验收标准',
  '## 13. 待确认事项'
)
foreach ($heading in $required) {
  if ($content -notmatch [regex]::Escape($heading)) { throw "Missing heading: $heading" }
}
if ($content -notmatch 'Android' -or $content -notmatch 'Linux' -or $content -notmatch 'Windows') { throw 'Missing target platforms' }
if ($content -notmatch 'text \| image') { throw 'Missing clipboard event types' }
Write-Output 'requirements document checks passed'
