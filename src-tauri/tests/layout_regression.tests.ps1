$ErrorActionPreference = 'Stop'

$edge = Join-Path ${env:ProgramFiles(x86)} 'Microsoft\Edge\Application\msedge.exe'
if (-not (Test-Path -LiteralPath $edge)) {
  throw "Microsoft Edge was not found at $edge"
}

$page = [System.Uri]::new((Resolve-Path (Join-Path $PSScriptRoot 'layout_regression.html'))).AbsoluteUri
$sandbox = Join-Path $env:TEMP "wingman-layout-test-$PID"

try {
  New-Item -ItemType Directory -Path $sandbox -Force | Out-Null
  $match = $null
  $edgeErrors = @()

  for ($launch = 0; $launch -lt 3 -and $null -eq $match; $launch++) {
    $stdout = Join-Path $sandbox "stdout-$launch.txt"
    $stderr = Join-Path $sandbox "stderr-$launch.txt"
    $arguments = @(
      '--headless',
      '--disable-gpu',
      '--no-first-run',
      '--dump-dom',
      '--virtual-time-budget=1000',
      '--window-size=400,700',
      "--user-data-dir=$sandbox\edge-profile-$launch",
      $page
    )
    Start-Process -FilePath $edge -ArgumentList $arguments -Wait -WindowStyle Hidden `
      -RedirectStandardOutput $stdout -RedirectStandardError $stderr

    for ($poll = 0; $poll -lt 20; $poll++) {
      $html = Get-Content -LiteralPath $stdout -Raw -ErrorAction SilentlyContinue
      if ($html) {
        $candidate = [regex]::Match($html, '<output id="metrics">(?<json>\{.*?\})</output>')
        if ($candidate.Success) {
          $match = $candidate
          break
        }
      }
      Start-Sleep -Milliseconds 100
    }

    $edgeError = Get-Content -LiteralPath $stderr -Raw -ErrorAction SilentlyContinue
    if ($edgeError) {
      $edgeErrors += $edgeError.Trim()
    }
  }

  if ($null -eq $match) {
    $edgeError = if ($edgeErrors.Count -gt 0) {
      $edgeErrors -join ' | '
    } else {
      'No Edge stderr was captured.'
    }
    throw "Layout metrics were not rendered by the headless browser. $edgeError"
  }

  $metrics = $match.Groups['json'].Value | ConvertFrom-Json
  foreach ($name in @('stage', 'terminal', 'statusbar')) {
    if ($metrics.$name -gt $metrics.viewport) {
      throw "$name width $($metrics.$name) exceeded viewport width $($metrics.viewport)"
    }
  }
} finally {
  Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output 'Responsive layout regression tests passed.'
