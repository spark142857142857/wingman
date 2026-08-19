param(
  [string] $SetupPath = '',
  [string] $InstallDirectory = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if ([string]::IsNullOrWhiteSpace($SetupPath)) {
  $SetupPath = Join-Path $repoRoot 'src-tauri\target\release\bundle\nsis\Wingman_0.1.0_x64-setup.exe'
}
if ([string]::IsNullOrWhiteSpace($InstallDirectory)) {
  $unicodeSuffix = ([char] 0xD55C).ToString() + [char] 0xAE00
  $InstallDirectory = Join-Path $repoRoot ('src-tauri\target\installer-smoke ' + $unicodeSuffix)
}

$SetupPath = [IO.Path]::GetFullPath($SetupPath)
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
$allowedInstallRoot = [IO.Path]::GetFullPath((Join-Path $repoRoot 'src-tauri\target'))
if (-not $InstallDirectory.StartsWith($allowedInstallRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
  throw "Installer smoke directory must remain under $allowedInstallRoot"
}
if (-not (Test-Path -LiteralPath $SetupPath -PathType Leaf)) {
  throw "NSIS setup is missing: $SetupPath"
}

$appKey = 'HKCU:\Software\wingman\Wingman'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wingman'
$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalProcessPath = $env:Path
$installationActive = $false
$installedFileCeilingBytes = 60L * 1024L * 1024L

function Get-UserPath {
  $value = [Environment]::GetEnvironmentVariable('Path', 'User')
  if ($null -eq $value) { '' } else { $value }
}

function Get-ExactTokenCount([string] $PathValue) {
  @(
    $PathValue.Split([char] ';') |
      Where-Object { [StringComparer]::OrdinalIgnoreCase.Equals($_, $InstallDirectory) }
  ).Count
}

function Get-PathMarker {
  $value = Get-ItemProperty `
    -LiteralPath $appKey `
    -Name WingmanPathAdded `
    -ErrorAction SilentlyContinue
  if ($null -eq $value) { $null } else { $value.WingmanPathAdded }
}

function Assert-CleanInstallerState {
  if (Test-Path -LiteralPath $InstallDirectory) {
    throw "Installer smoke directory already exists: $InstallDirectory"
  }
  if (Test-Path -LiteralPath $uninstallKey) {
    throw 'Wingman already has a current-user uninstall registration.'
  }
  if ($null -ne (Get-PathMarker)) {
    throw 'A stale Wingman PATH ownership marker exists.'
  }
}

function Invoke-Installer {
  $process = Start-Process `
    -FilePath $SetupPath `
    -ArgumentList @('/S', ('/D=' + $InstallDirectory)) `
    -Wait `
    -PassThru
  if ($process.ExitCode -ne 0) {
    throw "Installer exited with $($process.ExitCode)."
  }
  $script:installationActive = $true
}

function Invoke-InPlaceReinstall {
  $process = Start-Process `
    -FilePath $SetupPath `
    -ArgumentList @('/S', '/UPDATE', ('/D=' + $InstallDirectory)) `
    -Wait `
    -PassThru
  if ($process.ExitCode -ne 0) {
    throw "In-place installer exited with $($process.ExitCode)."
  }
}

function Invoke-Uninstaller {
  $uninstaller = Join-Path $InstallDirectory 'uninstall.exe'
  if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
    throw "Installed uninstaller is missing: $uninstaller"
  }
  $process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    throw "Uninstaller exited with $($process.ExitCode)."
  }
  $script:installationActive = $false
}

function Assert-InstalledPayload {
  foreach ($name in 'wingman.exe', 'wingman-runner.exe', 'uninstall.exe') {
    if (-not (Test-Path -LiteralPath (Join-Path $InstallDirectory $name) -PathType Leaf)) {
      throw "Installed payload is missing $name."
    }
  }
  if (Get-ChildItem -LiteralPath $InstallDirectory -Filter '*.ps1' -Recurse -File) {
    throw 'Installer left an executable PowerShell source file in the install tree.'
  }
}

function Assert-InstalledFootprint {
  $files = @(Get-ChildItem -LiteralPath $InstallDirectory -Recurse -File)
  $installedBytes = [long](($files | Measure-Object -Property Length -Sum).Sum)
  if ($installedBytes -gt $installedFileCeilingBytes) {
    throw "Installed payload uses $installedBytes bytes, exceeding the $installedFileCeilingBytes-byte release ceiling."
  }
  Write-Output "WINGMAN_INSTALLED_FOOTPRINT_V1=$installedBytes"
}

function Assert-PublicCommandLaunch {
  $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
  $env:Path = $machinePath + ';' + (Get-UserPath)
  try {
    $cmdOutput = @(& cmd.exe /d /c 'where wingman && wingman --version' 2>&1)
    if ($LASTEXITCODE -ne 0) {
      throw "cmd failed to launch wingman: $($cmdOutput -join [Environment]::NewLine)"
    }
    $cmdPaths = @($cmdOutput | Where-Object { $_ -match '\\wingman\.exe$' })
    if (
      $cmdPaths.Count -lt 1 -or
      -not [StringComparer]::OrdinalIgnoreCase.Equals($cmdPaths[0], (Join-Path $InstallDirectory 'wingman.exe')) -or
      $cmdOutput -notcontains 'wingman 0.1.0'
    ) {
      throw "cmd resolved an unexpected Wingman command: $($cmdOutput -join [Environment]::NewLine)"
    }

    $psOutput = @(
      & powershell.exe `
        -NoLogo `
        -NoProfile `
        -NonInteractive `
        -Command '(Get-Command wingman).Source; wingman --version' 2>&1
    )
    if ($LASTEXITCODE -ne 0) {
      throw "PowerShell failed to launch wingman: $($psOutput -join [Environment]::NewLine)"
    }
    if (
      -not [StringComparer]::OrdinalIgnoreCase.Equals($psOutput[0], (Join-Path $InstallDirectory 'wingman.exe')) -or
      $psOutput -notcontains 'wingman 0.1.0'
    ) {
      throw "PowerShell resolved an unexpected Wingman command: $($psOutput -join [Environment]::NewLine)"
    }
  } finally {
    $env:Path = $originalProcessPath
  }
}

try {
  Assert-CleanInstallerState

  Invoke-Installer
  Assert-InstalledPayload
  Assert-InstalledFootprint
  if ((Get-ExactTokenCount (Get-UserPath)) -ne 1 -or (Get-PathMarker) -ne 1) {
    throw 'A fresh install did not create one owned PATH token.'
  }
  Assert-PublicCommandLaunch

  Invoke-InPlaceReinstall
  if ((Get-ExactTokenCount (Get-UserPath)) -ne 1 -or (Get-PathMarker) -ne 1) {
    throw 'An in-place reinstall lost PATH ownership or duplicated the token.'
  }

  Invoke-Uninstaller
  if (-not ((Get-UserPath) -ceq $originalUserPath)) {
    throw 'Uninstall did not restore the exact original user PATH.'
  }
  if (
    (Test-Path -LiteralPath $InstallDirectory) -or
    (Test-Path -LiteralPath $uninstallKey) -or
    $null -ne (Get-PathMarker)
  ) {
    throw 'Uninstall left an install tree, uninstall registration, or PATH ownership marker.'
  }

  $preexistingPath = if ([string]::IsNullOrEmpty($originalUserPath)) {
    $InstallDirectory
  } elseif ($originalUserPath.EndsWith(';')) {
    $originalUserPath + $InstallDirectory
  } else {
    $originalUserPath + ';' + $InstallDirectory
  }
  [Environment]::SetEnvironmentVariable('Path', $preexistingPath, 'User')

  Invoke-Installer
  Assert-InstalledPayload
  Assert-InstalledFootprint
  if ((Get-ExactTokenCount (Get-UserPath)) -ne 1 -or (Get-PathMarker) -ne 0) {
    throw 'Installer claimed ownership of a pre-existing PATH token.'
  }

  Invoke-Uninstaller
  if (-not ((Get-UserPath) -ceq $preexistingPath)) {
    throw 'Uninstall changed a PATH token that Wingman did not create.'
  }
  if ($null -ne (Get-PathMarker)) {
    throw 'Uninstall retained the non-owned PATH marker.'
  }

  'Installer smoke tests passed.'
} finally {
  $env:Path = $originalProcessPath
  if ($installationActive -and (Test-Path -LiteralPath (Join-Path $InstallDirectory 'uninstall.exe'))) {
    $cleanup = Start-Process `
      -FilePath (Join-Path $InstallDirectory 'uninstall.exe') `
      -ArgumentList '/S' `
      -Wait `
      -PassThru
    if ($cleanup.ExitCode -ne 0) {
      Write-Warning "Cleanup uninstaller exited with $($cleanup.ExitCode)."
    }
  }
  if (-not ((Get-UserPath) -ceq $originalUserPath)) {
    [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
  }
}
