param(
  [switch] $RequireSignature
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$config = Get-Content -Raw -LiteralPath (Join-Path $tauriRoot 'tauri.conf.json') |
  ConvertFrom-Json

$expectedCsp = @(
  "default-src 'self'"
  "script-src 'self'"
  "style-src 'self' 'unsafe-inline'"
  'connect-src ipc: http://ipc.localhost'
  "img-src 'self' data:"
  "font-src 'self'"
  "object-src 'none'"
  "frame-src 'none'"
  "worker-src 'none'"
  "media-src 'none'"
  "form-action 'none'"
  "base-uri 'none'"
) -join '; '
if ($config.app.security.csp -ne $expectedCsp) {
  throw 'Tauri CSP differs from the reviewed production-local allowlist.'
}

$capabilityPath = Join-Path $tauriRoot 'capabilities\default.json'
$capability = Get-Content -Raw -LiteralPath $capabilityPath | ConvertFrom-Json
$capabilityWindows = @($capability.windows)
$capabilityPermissions = @($capability.permissions)
if ($capabilityWindows.Count -ne 1 -or $capabilityWindows[0] -ne 'main') {
  throw 'The default capability must apply only to the main window.'
}
$expectedPermissions = @('core:event:allow-listen', 'core:event:allow-unlisten')
if (
  $capabilityPermissions.Count -ne $expectedPermissions.Count -or
  @(Compare-Object $capabilityPermissions $expectedPermissions).Count -ne 0
) {
  throw 'The Tauri capability inventory contains an unreviewed permission.'
}

$package = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'package.json') | ConvertFrom-Json
$expectedRuntimePackages = @(
  '@tauri-apps/api'
  '@xterm/addon-fit'
  '@xterm/xterm'
)
$runtimePackages = @($package.dependencies.PSObject.Properties.Name)
if (
  $runtimePackages.Count -ne $expectedRuntimePackages.Count -or
  @(Compare-Object $runtimePackages $expectedRuntimePackages).Count -ne 0
) {
  throw 'The frontend runtime dependency inventory contains an unreviewed package.'
}

$cargoManifest = Get-Content -Raw -LiteralPath (Join-Path $tauriRoot 'Cargo.toml')
foreach ($forbiddenPlugin in 'shell', 'opener', 'http', 'clipboard', 'updater', 'log', 'notification') {
  if ($cargoManifest -match "tauri-plugin-$forbiddenPlugin") {
    throw "Cargo enables the unreviewed Tauri $forbiddenPlugin plugin."
  }
}

$terminalSecurity = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src\terminal-security.ts')
$frontend = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src\main.ts')
if (
  $terminalSecurity -notmatch 'activate:\s*\(\)\s*=>\s*undefined' -or
  $frontend -notmatch 'linkHandler:\s*blockedTerminalLinkHandler'
) {
  throw 'Untrusted terminal hyperlinks are not pinned to the inert handler.'
}
if ($frontend -match 'window\.open|location\s*=|location\.(assign|replace)|innerHTML\s*=') {
  throw 'The terminal frontend contains an unreviewed navigation or HTML injection sink.'
}

$manifest = Get-Content -Raw -LiteralPath (Join-Path $tauriRoot 'windows-app-manifest.xml')
if ($manifest -notmatch '<requestedExecutionLevel level="asInvoker" uiAccess="false"\s*/>') {
  throw 'The application manifest does not explicitly prohibit self-elevation.'
}

$releaseRoot = Join-Path $tauriRoot 'target\release'
$installer = Get-ChildItem -LiteralPath (Join-Path $releaseRoot 'bundle\nsis') `
  -Filter 'Wingman_*-setup.exe' `
  -File |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if ($null -eq $installer) {
  throw 'Release NSIS installer is missing.'
}
$artifacts = @(
  (Join-Path $releaseRoot 'wingman.exe')
  (Join-Path $releaseRoot 'wingman-runner.exe')
  $installer.FullName
)
$signatureReport = foreach ($artifact in $artifacts) {
  if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
    throw "Release security artifact is missing: $artifact"
  }
  $signature = Get-AuthenticodeSignature -LiteralPath $artifact
  [pscustomobject]@{
    Artifact = [IO.Path]::GetFileName($artifact)
    Status = $signature.Status.ToString()
    Signer = if ($null -eq $signature.SignerCertificate) {
      ''
    } else {
      $signature.SignerCertificate.Subject
    }
  }
}
if ($RequireSignature) {
  $invalid = @($signatureReport | Where-Object { $_.Status -ne 'Valid' })
  if ($invalid.Count -ne 0) {
    throw "Release signing gate failed: $($invalid.Artifact -join ', ')"
  }
}

Write-Output 'Local release security contract passed.'
$signatureReport | Format-Table -AutoSize
if (-not $RequireSignature) {
  Write-Output 'Authenticode is reported separately; publishing must rerun with -RequireSignature.'
}
