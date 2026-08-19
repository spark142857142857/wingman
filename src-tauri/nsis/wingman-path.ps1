[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [ValidateSet('Install', 'Uninstall')]
  [string] $Mode
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$appKey = 'HKCU:\Software\wingman\Wingman'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Wingman'

function Get-WingmanInstallDirectory {
  $value = (Get-ItemProperty -LiteralPath $uninstallKey -Name InstallLocation).InstallLocation
  $directory = $value.Trim([char] 34)
  if (
    [string]::IsNullOrWhiteSpace($directory) -or
    -not [IO.Path]::IsPathRooted($directory) -or
    $directory.Contains(';')
  ) {
    throw 'Wingman has an invalid registered install directory.'
  }

  $directory
}

function Test-ExactPathToken([string[]] $Tokens, [string] $Expected) {
  foreach ($token in $Tokens) {
    if ([StringComparer]::OrdinalIgnoreCase.Equals($token, $Expected)) {
      return $true
    }
  }

  $false
}

$installDirectory = Get-WingmanInstallDirectory
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($null -eq $userPath) {
  $userPath = ''
}
$tokens = @($userPath.Split([char] ';'))

if ($Mode -eq 'Install') {
  $alreadyPresent = Test-ExactPathToken $tokens $installDirectory
  $marker = Get-ItemProperty `
    -LiteralPath $appKey `
    -Name WingmanPathAdded `
    -ErrorAction SilentlyContinue
  $previousMarker = if ($null -eq $marker) { 0 } else { $marker.WingmanPathAdded }
  $owned = $previousMarker -eq 1

  if (-not $alreadyPresent) {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) {
      $installDirectory
    } elseif ($userPath.EndsWith(';')) {
      $userPath + $installDirectory
    } else {
      $userPath + ';' + $installDirectory
    }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    $owned = $true
  }

  Set-ItemProperty -LiteralPath $appKey -Name WingmanPathAdded -Value ([int] $owned)
  return
}

$marker = Get-ItemProperty `
  -LiteralPath $appKey `
  -Name WingmanPathAdded `
  -ErrorAction SilentlyContinue
$addedByWingman = if ($null -eq $marker) { 0 } else { $marker.WingmanPathAdded }
if ($addedByWingman -eq 1) {
  $index = -1
  for ($candidate = $tokens.Count - 1; $candidate -ge 0; $candidate--) {
    if ([StringComparer]::OrdinalIgnoreCase.Equals($tokens[$candidate], $installDirectory)) {
      $index = $candidate
      break
    }
  }

  if ($index -ge 0) {
    $remaining = [Collections.Generic.List[string]]::new()
    foreach ($token in $tokens) {
      $remaining.Add($token)
    }
    $remaining.RemoveAt($index)
    [Environment]::SetEnvironmentVariable('Path', ($remaining -join ';'), 'User')
  }
}

# This key is created and owned by Wingman's per-user installer. Removing only
# the PATH marker leaves Tauri's default install-directory value behind.
Remove-Item -LiteralPath $appKey -Force -ErrorAction SilentlyContinue
