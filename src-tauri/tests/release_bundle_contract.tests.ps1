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
if ($applicationText -notmatch '<requestedExecutionLevel level="asInvoker" uiAccess="false"\s*/>') {
  throw 'Release application must explicitly inherit its caller token without elevation'
}

$tauriConfig = Get-Content -Raw -LiteralPath (Join-Path $tauriRoot 'tauri.conf.json') |
  ConvertFrom-Json
$bundleTargets = @($tauriConfig.bundle.targets)
if ($bundleTargets.Count -ne 1 -or $bundleTargets[0] -ne 'nsis') {
  throw 'Wingman must produce only the verified NSIS installer target'
}
if (
  $tauriConfig.bundle.windows.nsis.installMode -ne 'currentUser' -or
  $tauriConfig.bundle.windows.nsis.installerHooks -ne 'nsis/wingman-path.nsh'
) {
  throw 'NSIS must use the reviewed current-user PATH registration hook'
}
if ($tauriConfig.bundle.externalBin -contains 'binaries/wingman-runner') {
  throw 'Runner is a Cargo bin and must not also be declared as externalBin'
}
if ($tauriConfig.bundle.resources) {
  throw 'PowerShell transport must remain compiled into wingman.exe'
}

$pathHook = Join-Path $tauriRoot 'nsis\wingman-path.nsh'
$pathHelper = Join-Path $tauriRoot 'nsis\wingman-path.ps1'
if (
  -not (Test-Path -LiteralPath $pathHook -PathType Leaf) -or
  -not (Test-Path -LiteralPath $pathHelper -PathType Leaf)
) {
  throw 'NSIS PATH registration sources are missing'
}
$hookText = Get-Content -Raw -LiteralPath $pathHook
if (
  $hookText -notmatch 'NSIS_HOOK_POSTINSTALL' -or
  $hookText -notmatch 'NSIS_HOOK_PREUNINSTALL' -or
  $hookText -notmatch 'File /oname=\$PLUGINSDIR\\wingman-path\.ps1' -or
  $hookText -match 'ExecutionPolicy\s+Bypass'
) {
  throw 'NSIS PATH hook does not use the fixed temporary helper boundary'
}
foreach ($loaderName in 'INSTALL', 'UNINSTALL') {
  $loaderMatch = [regex]::Match(
    $hookText,
    "!define WINGMAN_PATH_${loaderName}_LOADER `"([A-Za-z0-9+/=]+)`""
  )
  if (-not $loaderMatch.Success) {
    throw "NSIS PATH $loaderName loader is missing"
  }
  $loaderSource = [Text.Encoding]::Unicode.GetString(
    [Convert]::FromBase64String($loaderMatch.Groups[1].Value)
  )
  [void] [ScriptBlock]::Create($loaderSource)
  if ($loaderMatch.Groups[1].Value.Length -ge 768) {
    throw "NSIS PATH $loaderName loader risks exceeding the 1024-character NSIS limit"
  }
}
[void] [ScriptBlock]::Create((Get-Content -Raw -LiteralPath $pathHelper))

$escapedHookPath = [regex]::Escape($pathHook)
if (-not ($scriptLines | Where-Object { $_ -match "^!include `"$escapedHookPath`"$" })) {
  throw 'Generated NSIS source does not include the reviewed PATH registration hook'
}
if (-not ($scriptLines | Where-Object { $_.Trim() -eq 'RequestExecutionLevel user' })) {
  throw 'Current-user NSIS installer must not request administrator elevation'
}

Write-Output "Release bundle contract passed: $($installer.FullName)"
