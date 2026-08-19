param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [ValidateRange(1, 1000)]
    [int]$LaunchCount = 100,
    [ValidateRange(1, 120)]
    [int]$TimeoutSeconds = 30,
    [ValidateRange(1, 1024)]
    [int]$ReleaseCeilingMiB = 100,
    [switch]$KeepProfile
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'release_process_helpers.ps1')

function Get-ProcessTreeIds {
    param([uint32]$RootProcessId)

    $processes = @(Get-CimInstance Win32_Process)
    $ids = [Collections.Generic.HashSet[uint32]]::new()
    [void]$ids.Add($RootProcessId)
    do {
        $countBefore = $ids.Count
        foreach ($process in $processes) {
            if ($ids.Contains([uint32]$process.ParentProcessId)) {
                [void]$ids.Add([uint32]$process.ProcessId)
            }
        }
    } while ($ids.Count -gt $countBefore)
    return @($ids)
}

function Stop-WingmanLaunch {
    param(
        [Diagnostics.Process]$Process,
        [uint32[]]$OwnedProcessIds
    )

    $liveProcess = Get-Process -Id $Process.Id -ErrorAction SilentlyContinue
    if ($liveProcess) {
        [void]$liveProcess.CloseMainWindow()
        if (-not $liveProcess.WaitForExit(5000)) {
            Stop-Process -Id $liveProcess.Id -Force
        }
    }
    foreach ($ownedProcessId in $OwnedProcessIds) {
        if ($ownedProcessId -ne [uint32]$PID) {
            Stop-Process -Id $ownedProcessId -Force -ErrorAction SilentlyContinue
        }
    }
}

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$targetRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\target'))
$profileRoot = Join-Path $targetRoot (
    'local-app-data-' + $PID + '-' + [Guid]::NewGuid().ToString('N')
)
$profileRoot = [IO.Path]::GetFullPath($profileRoot)
if (-not $profileRoot.StartsWith($targetRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw "Profile root escaped the release target directory: $profileRoot"
}
if (Test-Path -LiteralPath $profileRoot) {
    throw "Fresh profile root already exists: $profileRoot"
}

$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw 'Close the existing release Wingman process before running this test.'
}

$profileVariable = 'WEBVIEW2_USER_DATA_FOLDER'
$previousProfile = [Environment]::GetEnvironmentVariable($profileVariable, 'Process')
$activeApp = $null
$activeTreeIds = @()
$profileBytes = [long]0
try {
    [Environment]::SetEnvironmentVariable($profileVariable, $profileRoot, 'Process')
    for ($launch = 1; $launch -le $LaunchCount; $launch++) {
        $activeApp = Start-WingmanGuiProcess `
            -Executable $resolvedExecutable `
            -WorkingDirectory (Get-Location).Path `
            -Arguments @('--shell', 'powershell') `
            -TimeoutSeconds $TimeoutSeconds

        $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
        do {
            Start-Sleep -Milliseconds 25
            $activeApp.Refresh()
            if ($activeApp.HasExited) {
                throw "Wingman exited before launch $launch became interactive."
            }
        } while ($activeApp.MainWindowTitle -ne 'Wingman - Ready' -and (Get-Date) -lt $deadline)
        if ($activeApp.MainWindowTitle -ne 'Wingman - Ready') {
            throw "Wingman launch $launch did not become interactive within $TimeoutSeconds seconds."
        }

        $activeTreeIds = @(Get-ProcessTreeIds -RootProcessId $activeApp.Id)
        Stop-WingmanLaunch -Process $activeApp -OwnedProcessIds $activeTreeIds
        $activeApp = $null
        $activeTreeIds = @()

        if ($launch -eq 1 -or $launch -eq $LaunchCount -or $launch % 10 -eq 0) {
            Write-Output "Completed clean launch $launch/$LaunchCount."
        }
    }

    $profileFiles = @(Get-ChildItem -LiteralPath $profileRoot -Recurse -File)
    if ($profileFiles.Count -ne 0) {
        $profileBytes = [long](($profileFiles | Measure-Object -Property Length -Sum).Sum)
    }
    $releaseCeilingBytes = [long]$ReleaseCeilingMiB * 1MB
    $result = [ordered]@{
        launchCount = $LaunchCount
        profileBytes = $profileBytes
        profileMiB = [Math]::Round($profileBytes / 1MB, 3)
        releaseCeilingBytes = $releaseCeilingBytes
    }
    Write-Output ('WINGMAN_LOCAL_APP_DATA_V1=' + ($result | ConvertTo-Json -Compress))
    if ($profileBytes -gt $releaseCeilingBytes) {
        throw "Local app data uses $profileBytes bytes after $LaunchCount launches, exceeding the $releaseCeilingBytes-byte release ceiling."
    }
}
finally {
    [Environment]::SetEnvironmentVariable($profileVariable, $previousProfile, 'Process')
    if ($activeApp) {
        Stop-WingmanLaunch -Process $activeApp -OwnedProcessIds $activeTreeIds
    }
    if (-not $KeepProfile -and (Test-Path -LiteralPath $profileRoot)) {
        Remove-Item -LiteralPath $profileRoot -Recurse -Force
    }
}
