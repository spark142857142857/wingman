param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$TimeoutSeconds = 30,
    [switch]$PassThru
)

$ErrorActionPreference = "Stop"
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
    $app = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Get-Location).Path -PassThru
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
        $shell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $process -and
                $process.StartTime -ge $startedAt -and
                $_.CommandLine -like '*-NoLogo*-NoExit*-Command*WingmanReadinessPipe*'
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
        throw "Release GUI reported the normal-input echo without an active PowerShell PTY session."
    }

    $elapsed = ((Get-Date) - $startedAt).TotalMilliseconds
    if ($PassThru) {
        Write-Output ([pscustomobject]@{
            ElapsedMilliseconds = [double]$elapsed
        })
    }
    else {
        Write-Output ("Release GUI accepted and rendered normal PowerShell input in {0:N1} ms." -f $elapsed)
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
