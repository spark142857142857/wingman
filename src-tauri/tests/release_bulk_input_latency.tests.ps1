param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$TimeoutSeconds = 120,
    [double]$P95CeilingMilliseconds = 200
)

$ErrorActionPreference = "Stop"
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$probeVariable = "WINGMAN_PERF_BULK_LATENCY_PROBE"
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
        $title -notlike "Wingman - Bulk Latency *" -and
        -not $app.HasExited -and
        (Get-Date) -lt $deadline
    )

    if ($app.HasExited) {
        throw "Release GUI exited before the bulk input-latency distribution completed."
    }
    if ($title -notmatch '^Wingman - Bulk Latency ([0-9]+\.[0-9]) ([0-9]+\.[0-9]) ([0-9]+\.[0-9])\|(.+)$') {
        throw "Release GUI did not report 100 bulk input-latency samples within $TimeoutSeconds seconds; title was '$title'."
    }
    if (-not $shell) {
        throw "Release GUI reported bulk input latency without an active PowerShell PTY session."
    }

    $culture = [Globalization.CultureInfo]::InvariantCulture
    $reportedMedian = [double]::Parse($Matches[1], $culture)
    $reportedP95 = [double]::Parse($Matches[2], $culture)
    $reportedMaximum = [double]::Parse($Matches[3], $culture)
    $samples = @($Matches[4].Split(',') | ForEach-Object {
        [double]::Parse($_, $culture)
    })
    if ($samples.Count -ne 100) {
        throw "Expected exactly 100 latency samples, received $($samples.Count)."
    }
    if (@($samples | Where-Object { [double]::IsNaN($_) -or [double]::IsInfinity($_) -or $_ -lt 0 }).Count -ne 0) {
        throw "Latency samples must be finite, non-negative milliseconds."
    }

    $sorted = @($samples | Sort-Object)
    $median = ($sorted[49] + $sorted[50]) / 2
    $p95 = $sorted[94]
    $maximum = $sorted[99]
    if ([Math]::Abs($median - $reportedMedian) -gt 0.11 -or
        [Math]::Abs($p95 - $reportedP95) -gt 0.11 -or
        [Math]::Abs($maximum - $reportedMaximum) -gt 0.11) {
        throw "Reported latency summary does not match the raw distribution."
    }
    if ($p95 -gt $P95CeilingMilliseconds) {
        throw "Bulk output input latency p95 was $p95 ms, above the $P95CeilingMilliseconds ms release ceiling."
    }

    Write-Output ("Bulk input latency: median {0:N1} ms, p95 {1:N1} ms, maximum {2:N1} ms." -f $median, $p95, $maximum)
    Write-Output ("Raw samples (ms): " + (($samples | ForEach-Object { $_.ToString("F1", $culture) }) -join ", "))
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
