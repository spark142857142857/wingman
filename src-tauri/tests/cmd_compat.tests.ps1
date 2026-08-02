$ErrorActionPreference = 'Stop'
$sandbox = Join-Path $env:TEMP "wingman-cmd-compat-$PID"

function Assert-True($Actual, [string]$Label) {
  if (-not $Actual) { throw "$Label expected a true value" }
}

try {
  New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
  Push-Location $sandbox

  & cmd.exe /d /c 'mkdir demo\nested'
  Assert-True (Test-Path -LiteralPath (Join-Path $sandbox 'demo\nested') -PathType Container) 'mkdir -p mapping'

  & cmd.exe /d /c 'if exist sample.txt (copy /b sample.txt +,, >nul) else (type nul > sample.txt)'
  Assert-True (Test-Path -LiteralPath (Join-Path $sandbox 'sample.txt') -PathType Leaf) 'touch mapping'

  Set-Content -LiteralPath (Join-Path $sandbox 'app.txt') -Value @('TODO', 'done')
  $grepOutput = (& cmd.exe /d /c 'findstr /i /n /v /c:missing app.txt') | Out-String
  Assert-True ($grepOutput -match '1:TODO') 'grep option mapping'

  $pipelineOutput = (& cmd.exe /d /c 'type app.txt | findstr /c:TODO | powershell.exe -NoLogo -NoProfile -Command "$input | Select-Object -First 1"') | Out-String
  Assert-True ($pipelineOutput.Trim() -eq 'TODO') 'cat grep head pipeline mapping'

  $lineCount = (& cmd.exe /d /c 'type app.txt | find /c /v ""') | Out-String
  Assert-True ($lineCount.Trim() -eq '2') 'wc line count mapping'

  $tailOutput = (& cmd.exe /d /c 'type app.txt | powershell.exe -NoLogo -NoProfile -Command "$input | Select-Object -Last 1"') | Out-String
  Assert-True ($tailOutput.Trim() -eq 'done') 'tail pipeline mapping'

  $numericSort = (& cmd.exe /d /c '(echo 10&echo 2)|powershell.exe -NoLogo -NoProfile -Command "$input | Sort-Object { [double]$_ }"') | Out-String
  $numericLines = @($numericSort.Trim() -split '\r?\n' | ForEach-Object { $_.Trim() })
  Assert-True (($numericLines -join ',') -eq '2,10') 'numeric sort pipeline mapping'

  & cmd.exe /d /c 'type app.txt | findstr /c:TODO > result.txt'
  Assert-True ((Get-Content -LiteralPath (Join-Path $sandbox 'result.txt')).Trim() -eq 'TODO') 'pipeline redirection mapping'

  & cmd.exe /d /c 'if exist demo\NUL (rmdir /s /q demo) else (del /f /q demo)'
  Assert-True (-not (Test-Path -LiteralPath (Join-Path $sandbox 'demo'))) 'rm -rf mapping'

  Write-Output 'cmd compatibility tests passed.'
} finally {
  Pop-Location -ErrorAction SilentlyContinue
  $resolvedTemp = [System.IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
  $resolvedSandbox = [System.IO.Path]::GetFullPath($sandbox)
  if ($resolvedSandbox.StartsWith($resolvedTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
    Remove-Item -LiteralPath $resolvedSandbox -Recurse -Force -ErrorAction SilentlyContinue
  }
}
