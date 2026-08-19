param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [ValidateSet('powershell', 'cmd')]
    [string]$ShellKind = 'powershell',
    [int]$TimeoutSeconds = 30,
    [switch]$PassThru
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'release_process_helpers.ps1')
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$probeVariable = "WINGMAN_PERF_INPUT_ECHO_PROBE"
$previousProbe = [Environment]::GetEnvironmentVariable($probeVariable, "Process")
$startedAt = Get-Date
try {
    [Environment]::SetEnvironmentVariable($probeVariable, "1", "Process")
    $app = Start-WingmanGuiProcess `
        -Executable $resolvedExecutable `
        -WorkingDirectory (Get-Location).Path `
        -Arguments @('--shell', $ShellKind)
}
finally {
    [Environment]::SetEnvironmentVariable($probeVariable, $previousProbe, "Process")
}

$shell = $null
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 25
        $app.Refresh()
        $shellName = if ($ShellKind -eq 'powershell') { 'powershell.exe' } else { 'cmd.exe' }
        $shell = Get-CimInstance Win32_Process -Filter "Name = '$shellName'" | Where-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            if (-not $process -or $process.StartTime -lt $startedAt) {
                return $false
            }
            if ($ShellKind -eq 'powershell') {
                return $_.CommandLine -like '*-NoLogo*-NoExit*-Command*WingmanReadinessPipe*'
            }
            return $_.CommandLine -like '*cmd.exe*/K*chcp 65001*'
        } | Select-Object -First 1
    } while (
        $app.MainWindowTitle -ne "Wingman - Echoed" -and
        -not $app.HasExited -and
        (Get-Date) -lt $deadline
    )

    if ($app.HasExited) {
        throw "Release GUI exited before the normal-input echo probe completed."
    }
    if ($app.MainWindowTitle -ne "Wingman - Echoed") {
        throw "Release GUI did not accept and render the normal-input echo probe within $TimeoutSeconds seconds; title was '$($app.MainWindowTitle)'."
    }
    if (-not $shell) {
        throw "Release GUI reported the normal-input echo without an active $ShellKind PTY session."
    }

    $elapsed = ((Get-Date) - $startedAt).TotalMilliseconds
    if ($PassThru) {
        Write-Output ([pscustomobject]@{
            Shell = $ShellKind
            ElapsedMilliseconds = [double]$elapsed
        })
    }
    else {
        Write-Output ("Release GUI accepted and rendered normal $ShellKind input in {0:N1} ms." -f $elapsed)
    }
}
finally {
    $liveApp = Get-Process -Id $app.Id -ErrorAction SilentlyContinue
    if ($liveApp) {
        [void]$liveApp.CloseMainWindow()
        if (-not $liveApp.WaitForExit(5000)) {
            Stop-Process -Id $app.Id -Force
        }
    }
    if ($shell) {
        Stop-Process -Id $shell.ProcessId -Force -ErrorAction SilentlyContinue
    }
}
