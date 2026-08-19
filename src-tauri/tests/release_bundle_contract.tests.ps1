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
$transportReferences = @($scriptLines | Where-Object {
  $_ -match 'powershell_runner_transport\.ps1'
})

if ($runnerInstall.Count -ne 1 -or $runnerDelete.Count -ne 1) {
  throw 'NSIS must install and uninstall exactly one wingman-runner.exe'
}
if ($transportReferences.Count -ne 0) {
  throw 'NSIS must not expose the compiled PowerShell transport as a writable installed script'
}

$releaseRunner = Join-Path $releaseRoot 'wingman-runner.exe'
if (-not (Test-Path -LiteralPath $releaseRunner -PathType Leaf) -or
    (Get-Item -LiteralPath $releaseRunner).Length -eq 0) {
  throw "Cargo-built release runner is missing or empty: $releaseRunner"
}

$releaseApplication = Join-Path $releaseRoot 'wingman.exe'
$applicationBytes = [System.IO.File]::ReadAllBytes($releaseApplication)
$peOffset = [BitConverter]::ToInt32($applicationBytes, 0x3c)
$optionalHeaderOffset = $peOffset + 24
$optionalHeaderMagic = [BitConverter]::ToUInt16($applicationBytes, $optionalHeaderOffset)
if ($optionalHeaderMagic -ne 0x10b -and $optionalHeaderMagic -ne 0x20b) {
  throw 'Release application has an invalid PE optional header'
}
$subsystem = [BitConverter]::ToUInt16($applicationBytes, $optionalHeaderOffset + 0x44)
if ($subsystem -ne 3) {
  throw 'wingman.exe must use the console subsystem so shells wait for launcher status'
}
$applicationText = [Text.Encoding]::ASCII.GetString($applicationBytes)
if ($applicationText -notmatch '<consoleAllocationPolicy[^>]*>detached</consoleAllocationPolicy>') {
  throw 'Release application does not embed the detached console-allocation policy'
}

$tauriConfig = Get-Content -Raw -LiteralPath (Join-Path $tauriRoot 'tauri.conf.json') |
  ConvertFrom-Json
if ($tauriConfig.bundle.externalBin -contains 'binaries/wingman-runner') {
  throw 'Runner is a Cargo bin and must not also be declared as externalBin'
}
if ($tauriConfig.bundle.resources) {
  throw 'PowerShell transport must remain compiled into wingman.exe'
}

Write-Output "Release bundle contract passed: $($installer.FullName)"
