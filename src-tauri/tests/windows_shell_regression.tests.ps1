$ErrorActionPreference = 'Stop'
$profilePath = Join-Path $PSScriptRoot '..\src\powershell_compat.ps1'
$flagPath = Join-Path $env:TEMP "wingman-shell-regression-$PID.flag"
$sandbox = Join-Path $env:TEMP "wingman-shell-regression-$PID"
$env:WINGMAN_COMPAT_FLAG = $flagPath

function Assert-Equal($Expected, $Actual, [string]$Label) {
  if ($Expected -ne $Actual) {
    throw "$Label expected '$Expected', got '$Actual'"
  }
}

function Assert-True($Actual, [string]$Label) {
  if (-not $Actual) { throw "$Label expected a true value" }
}

try {
  New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
  $unicodeName = "$([char]0xD55C)$([char]0xAE00) sample.txt"
  $samplePath = Join-Path $sandbox $unicodeName
  Set-Content -LiteralPath $samplePath -Encoding UTF8 -Value @('Alpha', 'Beta', 'Gamma')

  # Familiar ON must not interfere with explicit PowerShell cmdlets or normal programs.
  Set-Content -LiteralPath $flagPath -NoNewline -Value '1'
  . $profilePath

  Assert-Equal $PID (Get-Process -Id $PID).Id 'Get-Process passthrough'
  Assert-Equal 3 @(Get-Content -LiteralPath $samplePath).Count 'Get-Content passthrough'
  Assert-Equal '1,2,3' (@(3, 1, 2 | Sort-Object) -join ',') 'Sort-Object pipeline'
  Assert-True (@(Get-ChildItem -LiteralPath $sandbox).Count -ge 1) 'Get-ChildItem passthrough'

  $redirectPath = Join-Path $sandbox 'redirect.txt'
  'redirect works' > $redirectPath
  Assert-Equal 'redirect works' (Get-Content -LiteralPath $redirectPath).Trim() 'PowerShell redirection'

  $nativePipeline = Get-Content -LiteralPath $samplePath | Where-Object { $_ -match 'a$' } | Select-Object -First 1
  Assert-Equal 'Alpha' $nativePipeline 'native PowerShell pipeline'

  # Familiar OFF keeps common PowerShell aliases usable.
  Set-Content -LiteralPath $flagPath -NoNewline -Value '0'
  Assert-Equal $false (Test-WingmanCompat) 'compat off state'
  Assert-True (@(ls $sandbox).Count -ge 1) 'ls delegates to Get-ChildItem'
  Assert-Equal 3 @(cat $samplePath).Count 'cat delegates to Get-Content'
  Assert-Equal '1,2,3' (@(3, 1, 2 | sort) -join ',') 'sort keeps PowerShell alias behavior'

  $removePath = Join-Path $sandbox 'remove-me.txt'
  Set-Content -LiteralPath $removePath -Value 'temporary'
  rm $removePath
  Assert-Equal $false (Test-Path -LiteralPath $removePath) 'rm delegates to Remove-Item'

  # Real cmd.exe commands, pipes, variables, and redirection remain available.
  Assert-Equal 'CMD_OK' ((& cmd.exe /d /c 'echo CMD_OK') | Out-String).Trim() 'cmd echo'
  Assert-Equal 'VALUE_OK' ((& cmd.exe /d /v:on /c 'set WINGMAN_TEST=VALUE_OK&& echo !WINGMAN_TEST!') | Out-String).Trim() 'cmd environment variable'
  Assert-Equal 'alpha' ((& cmd.exe /d /c 'echo alpha|findstr alpha') | Out-String).Trim() 'cmd native pipeline'
  Assert-True (-not [string]::IsNullOrWhiteSpace(((& cmd.exe /d /c 'where cmd.exe') | Select-Object -First 1))) 'cmd where'

  $cmdRedirectPath = Join-Path $sandbox 'cmd-redirect.txt'
  $cmdRedirectCommand = 'echo cmd redirect>"{0}"' -f $cmdRedirectPath
  & cmd.exe /d /c $cmdRedirectCommand
  Assert-Equal 'cmd redirect' (Get-Content -LiteralPath $cmdRedirectPath).Trim() 'cmd redirection'

  Write-Output 'Windows shell regression tests passed.'
} finally {
  Remove-Item -LiteralPath $flagPath -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}
