$ErrorActionPreference = 'Stop'

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$manifestPath = Join-Path $projectRoot 'src-tauri\Cargo.toml'

& cargo build --manifest-path $manifestPath --bins
if ($LASTEXITCODE -ne 0) {
  throw "Cargo binary build failed with exit code $LASTEXITCODE"
}

$metadata = (& cargo metadata --manifest-path $manifestPath --no-deps --format-version 1) |
  Out-String |
  ConvertFrom-Json
$package = $metadata.packages | Where-Object { $_.name -eq 'wingman' }
$binaryTargets = @($package.targets | Where-Object { $_.kind -contains 'bin' } | ForEach-Object name)
if ($binaryTargets -notcontains 'wingman' -or $binaryTargets -notcontains 'wingman-runner') {
  throw 'Cargo package does not expose both application and runner binaries'
}

$sidecarPath = Join-Path $metadata.target_directory 'debug\wingman-runner.exe'
if (-not (Test-Path -LiteralPath $sidecarPath -PathType Leaf)) {
  throw "Cargo-built sidecar is missing: $sidecarPath"
}

Remove-Item Env:\WINGMAN_BROKER_PIPE -ErrorAction SilentlyContinue
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = $sidecarPath
$startInfo.Arguments = '0123456789abcdef0123456789abcdef'
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.CreateNoWindow = $true
$startInfo.EnvironmentVariables.Remove('WINGMAN_BROKER_PIPE')
$runnerProcess = [System.Diagnostics.Process]::Start($startInfo)
$runnerStdout = $runnerProcess.StandardOutput.ReadToEnd()
$runnerStderr = $runnerProcess.StandardError.ReadToEnd()
$runnerProcess.WaitForExit()
if ($runnerProcess.ExitCode -ne 2) {
  throw "prepared sidecar returned $($runnerProcess.ExitCode) instead of 2"
}
if (-not [string]::IsNullOrEmpty($runnerStdout)) {
  throw "prepared sidecar wrote unexpected stdout: $runnerStdout"
}
if ($runnerStderr.Trim() -ne 'wingman-runner: broker endpoint is unavailable') {
  throw "unexpected prepared sidecar diagnostic: $($runnerStderr.Trim())"
}

$tauriConfig = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'src-tauri\tauri.conf.json') |
  ConvertFrom-Json
if ($tauriConfig.bundle.externalBin -contains 'binaries/wingman-runner') {
  throw 'Tauri must not duplicate the Cargo-built runner as an external binary'
}
if ($tauriConfig.bundle.resources.'src/powershell_runner_transport.ps1' -ne
    'powershell_runner_transport.ps1') {
  throw 'Tauri bundle does not map the PowerShell integration script to the runtime resource path'
}

$cargoManifest = Get-Content -Raw -LiteralPath $manifestPath
if ($cargoManifest -notmatch '(?m)^default-run\s*=\s*"wingman"\s*$') {
  throw 'Cargo does not identify wingman as the default application binary'
}

Write-Output 'Sidecar packaging tests passed.'
