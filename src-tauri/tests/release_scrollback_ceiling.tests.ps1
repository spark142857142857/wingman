param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$TimeoutSeconds = 60,
    [int]$ExpectedScrollbackRows = 4000
)

$ErrorActionPreference = "Stop"
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$probeVariable = "WINGMAN_PERF_SCROLLBACK_PROBE"
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
    $title = ""
    do {
        Start-Sleep -Milliseconds 25
        $app.Refresh()
        $title = $app.MainWindowTitle
        $shell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $process -and
                $process.StartTime -ge $startedAt -and
                $_.CommandLine -like '*-NoLogo*-NoExit*-Command*WingmanReadinessPipe*'
        } | Select-Object -First 1
    } while (
        $title -notlike "Wingman - Scrollback *" -and
        -not $app.HasExited -and
        (Get-Date) -lt $deadline
    )

    if ($app.HasExited) {
        throw "Release GUI exited before the scrollback ceiling was measured."
    }
    if ($title -notmatch '^Wingman - Scrollback ([0-9]+) ([0-9]+) ([0-9]+)$') {
        throw "Release GUI did not report the scrollback measurement within $TimeoutSeconds seconds; title was '$title'."
    }
    if (-not $shell) {
        throw "Release GUI reported the scrollback measurement without an active PowerShell PTY session."
    }

    $configuredScrollbackRows = [int]$Matches[1]
    $viewportRows = [int]$Matches[2]
    $bufferRows = [int]$Matches[3]
    if ($configuredScrollbackRows -ne $ExpectedScrollbackRows) {
        throw "Configured scrollback was $configuredScrollbackRows rows, expected $ExpectedScrollbackRows."
    }
    if ($viewportRows -lt 1) {
        throw "Viewport rows must be positive; received $viewportRows."
    }
    if ($bufferRows -le $viewportRows) {
        throw "The 100,000-line workload did not populate scrollback; buffer had $bufferRows rows and viewport had $viewportRows."
    }

    $retainedScrollbackRows = $bufferRows - $viewportRows
    if ($retainedScrollbackRows -ne $ExpectedScrollbackRows) {
        throw "Retained scrollback was $retainedScrollbackRows rows after the 100,000-line workload, expected the configured $ExpectedScrollbackRows-row ceiling to be full."
    }

    Write-Output "Release GUI retained $retainedScrollbackRows scrollback rows after validating and rendering 100,000 lines; ceiling was $ExpectedScrollbackRows."
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
