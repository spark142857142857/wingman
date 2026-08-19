param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$TimeoutSeconds = 30
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

$startedAt = Get-Date
$app = Start-WingmanGuiProcess -Executable $resolvedExecutable -WorkingDirectory (Get-Location).Path
$shell = $null
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 25
        $app.Refresh()
        $shell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $process -and
                $process.StartTime -ge $startedAt -and
                $_.CommandLine -like '*-NoLogo*-NoExit*-Command*WingmanReadinessPipe*'
        } | Select-Object -First 1
    } while (
        $app.MainWindowTitle -ne "Wingman - Ready" -and
        -not $app.HasExited -and
        (Get-Date) -lt $deadline
    )

    if ($app.HasExited) {
        throw "Release GUI exited before its editor became ready."
    }
    if ($app.MainWindowTitle -ne "Wingman - Ready") {
        throw "Release GUI did not expose verified editor readiness within $TimeoutSeconds seconds; title was '$($app.MainWindowTitle)'."
    }
    if (-not $shell) {
        throw "Release GUI reported editor readiness without an active PowerShell PTY session."
    }

    $elapsed = ((Get-Date) - $startedAt).TotalMilliseconds
    Write-Output ("Release GUI exposed verified PowerShell editor readiness in {0:N1} ms." -f $elapsed)
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
