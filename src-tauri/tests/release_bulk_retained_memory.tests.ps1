param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$PhaseTimeoutSeconds = 40,
    [int]$BaselineSettleSeconds = 10,
    [int]$PostClearSettleSeconds = 10,
    [int]$SampleCount = 10,
    [int]$SampleIntervalSeconds = 1,
    [double]$RetainedCeilingMiB = 50
)

$ErrorActionPreference = "Stop"
if ($SampleCount -lt 2) {
    throw "SampleCount must be at least 2."
}
if ($SampleIntervalSeconds -lt 1) {
    throw "SampleIntervalSeconds must be at least 1."
}

function Get-ProcessTreeIds {
    param([int]$RootProcessId)

    $processes = @(Get-CimInstance Win32_Process)
    $ids = [System.Collections.Generic.HashSet[uint32]]::new()
    [void]$ids.Add([uint32]$RootProcessId)
    do {
        $countBefore = $ids.Count
        foreach ($process in $processes) {
            if ($ids.Contains([uint32]$process.ParentProcessId)) {
                [void]$ids.Add([uint32]$process.ProcessId)
            }
        }
    } while ($ids.Count -gt $countBefore)
    return @($ids | Sort-Object)
}

function Get-TreePrivateWorkingSetMiB {
    param([uint32[]]$ProcessIds)

    $wanted = [System.Collections.Generic.HashSet[uint32]]::new()
    foreach ($processId in $ProcessIds) {
        [void]$wanted.Add([uint32]$processId)
    }
    $privateWorkingSet = [uint64]0
    foreach ($counter in Get-CimInstance Win32_PerfRawData_PerfProc_Process) {
        if ($wanted.Contains([uint32]$counter.IDProcess)) {
            $privateWorkingSet += [uint64]$counter.WorkingSetPrivate
        }
    }
    return [double]$privateWorkingSet / 1MB
}

function Get-TreePrivateWorkingSetByProcess {
    param([uint32[]]$ProcessIds)

    $wanted = [System.Collections.Generic.HashSet[uint32]]::new()
    foreach ($processId in $ProcessIds) {
        [void]$wanted.Add([uint32]$processId)
    }
    return @(Get-CimInstance Win32_PerfRawData_PerfProc_Process | Where-Object {
        $wanted.Contains([uint32]$_.IDProcess)
    } | ForEach-Object {
        [pscustomobject]@{
            ProcessId = [uint32]$_.IDProcess
            Name = $_.Name
            PrivateWorkingSetMiB = [double]$_.WorkingSetPrivate / 1MB
        }
    } | Sort-Object -Property PrivateWorkingSetMiB -Descending)
}

function Get-Median {
    param([double[]]$Values)

    $ordered = @($Values | Sort-Object)
    $middle = [Math]::Floor($ordered.Count / 2)
    if ($ordered.Count % 2 -eq 0) {
        return ([double]$ordered[$middle - 1] + [double]$ordered[$middle]) / 2
    }
    return [double]$ordered[$middle]
}

function Wait-ForTitle {
    param(
        [System.Diagnostics.Process]$Process,
        [string]$Expected,
        [int]$TimeoutSeconds
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 25
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "Release GUI exited while waiting for '$Expected'."
        }
    } while ($Process.MainWindowTitle -ne $Expected -and (Get-Date) -lt $deadline)
    if ($Process.MainWindowTitle -ne $Expected) {
        throw "Release GUI did not reach '$Expected' within $TimeoutSeconds seconds; title was '$($Process.MainWindowTitle)'."
    }
}

function Measure-PrivateWorkingSet {
    param(
        [System.Diagnostics.Process]$Process,
        [int]$Count,
        [int]$IntervalSeconds,
        [string]$ExpectedTitle
    )

    $samples = [System.Collections.Generic.List[double]]::new()
    for ($sample = 0; $sample -lt $Count; $sample++) {
        Start-Sleep -Seconds $IntervalSeconds
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "Release GUI exited during the $ExpectedTitle memory distribution."
        }
        if ($Process.MainWindowTitle -ne $ExpectedTitle) {
            throw "Release GUI left '$ExpectedTitle' before its memory distribution completed."
        }
        $treeIds = @(Get-ProcessTreeIds -RootProcessId $Process.Id)
        $samples.Add((Get-TreePrivateWorkingSetMiB -ProcessIds $treeIds))
    }
    return [double[]]$samples.ToArray()
}

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$probeVariable = "WINGMAN_PERF_BULK_RETENTION_PROBE"
$previousProbe = [Environment]::GetEnvironmentVariable($probeVariable, "Process")
$startedAt = Get-Date
try {
    [Environment]::SetEnvironmentVariable($probeVariable, "1", "Process")
    $app = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Get-Location).Path -PassThru
}
finally {
    [Environment]::SetEnvironmentVariable($probeVariable, $previousProbe, "Process")
}

$knownTreeIds = @([uint32]$app.Id)
try {
    Wait-ForTitle -Process $app -Expected "Wingman - Retention Baseline" -TimeoutSeconds $PhaseTimeoutSeconds

    $shell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
        $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
        $process -and
            $process.StartTime -ge $startedAt -and
            $_.CommandLine -like '*-NoLogo*-NoExit*-ExecutionPolicy*Bypass*-Command*WINGMAN_INTEGRATION_SCRIPT*'
    } | Select-Object -First 1
    if (-not $shell) {
        throw "Retention baseline started without an active PowerShell PTY session."
    }

    Start-Sleep -Seconds $BaselineSettleSeconds
    $baseline = Measure-PrivateWorkingSet -Process $app -Count $SampleCount -IntervalSeconds $SampleIntervalSeconds -ExpectedTitle "Wingman - Retention Baseline"
    $baselineByProcess = Get-TreePrivateWorkingSetByProcess -ProcessIds @(Get-ProcessTreeIds -RootProcessId $app.Id)

    Wait-ForTitle -Process $app -Expected "Wingman - Retention Cleared" -TimeoutSeconds $PhaseTimeoutSeconds
    Start-Sleep -Seconds $PostClearSettleSeconds
    $retained = Measure-PrivateWorkingSet -Process $app -Count $SampleCount -IntervalSeconds $SampleIntervalSeconds -ExpectedTitle "Wingman - Retention Cleared"

    $knownTreeIds = @(Get-ProcessTreeIds -RootProcessId $app.Id)
    $retainedByProcess = Get-TreePrivateWorkingSetByProcess -ProcessIds $knownTreeIds
    if ($knownTreeIds -notcontains [uint32]$shell.ProcessId) {
        throw "The active PowerShell PTY session left the Wingman process tree."
    }

    $baselineMedian = Get-Median -Values $baseline
    $retainedMedian = Get-Median -Values $retained
    $retainedMaximum = ($retained | Measure-Object -Maximum).Maximum
    $maximumGrowth = $retainedMaximum - $baselineMedian
    $result = [ordered]@{
        BaselinePrivateWorkingSetMiB = $baseline
        BaselineMedianMiB = $baselineMedian
        BaselineByProcess = $baselineByProcess
        RetainedPrivateWorkingSetMiB = $retained
        RetainedMedianMiB = $retainedMedian
        RetainedMaximumMiB = $retainedMaximum
        RetainedMedianGrowthMiB = $retainedMedian - $baselineMedian
        RetainedMaximumGrowthMiB = $maximumGrowth
        RetainedByProcess = $retainedByProcess
    }
    [pscustomobject]$result | ConvertTo-Json -Depth 3

    if ($maximumGrowth -gt $RetainedCeilingMiB) {
        throw "Post-clear private working set grew by $maximumGrowth MiB, above the $RetainedCeilingMiB MiB release ceiling."
    }
    if ($retainedMaximum -gt 350) {
        throw "Post-clear private working set $retainedMaximum MiB exceeded the 350 MiB absolute release ceiling."
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
    foreach ($processId in $knownTreeIds) {
        if ($processId -ne [uint32]$PID) {
            Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
        }
    }
}
