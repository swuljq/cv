$ErrorActionPreference = 'Stop'
$files = @('mobile/index.html', 'mobile/src/index.html')
foreach ($file in $files) {
  $content = Get-Content $file -Raw
  foreach ($marker in @('shouldStayConnected', 'scheduleReconnect', 'visibilitychange', 'pageshow')) {
    if ($content -notmatch [regex]::Escape($marker)) { throw "$file missing lifecycle marker: $marker" }
  }
}
Write-Output 'Mobile lifecycle checks passed.'
