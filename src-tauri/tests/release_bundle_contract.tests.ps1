param(
  [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$tauriRoot = Join-Path $projectRoot 'src-tauri'

if (-not $SkipBuild) {
  Push-Location $projectRoot
  try {
    & npx.cmd tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) {
      throw "Tauri NSIS build failed with exit code $LASTEXITCODE"
    }
  } finally {
    Pop-Location
  }
}

$hostTriple = (& rustc --print host-tuple).Trim()
$bundleArchitecture = switch -Regex ($hostTriple) {
  '^x86_64-' { 'x64'; break }
  '^aarch64-' { 'arm64'; break }
  '^i686-' { 'x86'; break }
  default { throw "Unsupported Windows bundle architecture: $hostTriple" }
}

$releaseRoot = Join-Path $tauriRoot 'target\release'
$nsisScript = Join-Path $releaseRoot "nsis\$bundleArchitecture\installer.nsi"
$installer = Get-ChildItem -LiteralPath (Join-Path $releaseRoot 'bundle\nsis') `
  -Filter 'Wingman_*-setup.exe' -File |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1

if (-not (Test-Path -LiteralPath $nsisScript -PathType Leaf)) {
  throw "Generated NSIS script is missing: $nsisScript"
}
if ($null -eq $installer -or $installer.Length -eq 0) {
  throw 'Generated NSIS installer is missing or empty'
}

$scriptLines = Get-Content -LiteralPath $nsisScript
$runnerInstall = @($scriptLines | Where-Object {
  $_ -match '^\s*File /a "/oname=wingman-runner\.exe" '
})
$runnerDelete = @($scriptLines | Where-Object {
  $_ -eq '    Delete "$INSTDIR\wingman-runner.exe"'
})
$transportInstall = @($scriptLines | Where-Object {
  $_ -match '^\s*File /a "/oname=powershell_runner_transport\.ps1" '
})
$transportDelete = @($scriptLines | Where-Object {
  $_ -eq '    Delete "$INSTDIR\powershell_runner_transport.ps1"'
})

if ($runnerInstall.Count -ne 1 -or $runnerDelete.Count -ne 1) {
  throw 'NSIS must install and uninstall exactly one wingman-runner.exe'
}
if ($transportInstall.Count -ne 1 -or $transportDelete.Count -ne 1) {
  throw 'NSIS must install and uninstall the PowerShell transport at the runtime resource root'
}

$releaseRunner = Join-Path $releaseRoot 'wingman-runner.exe'
if (-not (Test-Path -LiteralPath $releaseRunner -PathType Leaf) -or
    (Get-Item -LiteralPath $releaseRunner).Length -eq 0) {
  throw "Cargo-built release runner is missing or empty: $releaseRunner"
}

$tauriConfig = Get-Content -Raw -LiteralPath (Join-Path $tauriRoot 'tauri.conf.json') |
  ConvertFrom-Json
if ($tauriConfig.bundle.externalBin -contains 'binaries/wingman-runner') {
  throw 'Runner is a Cargo bin and must not also be declared as externalBin'
}
if ($tauriConfig.bundle.resources.'src/powershell_runner_transport.ps1' -ne
    'powershell_runner_transport.ps1') {
  throw 'PowerShell transport resource mapping does not match the runtime lookup path'
}

Write-Output "Release bundle contract passed: $($installer.FullName)"
