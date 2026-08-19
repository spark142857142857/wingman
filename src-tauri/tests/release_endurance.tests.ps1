param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$DurationMinutes = 30,
    [int]$SampleIntervalSeconds = 10,
    [int]$StartupTimeoutSeconds = 45,
    [switch]$Smoke
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'release_process_helpers.ps1')

if (-not $Smoke -and $DurationMinutes -ne 30) {
    throw "The release endurance gate has a fixed 30-minute duration."
}
if ($SampleIntervalSeconds -lt 1) {
    throw "SampleIntervalSeconds must be at least 1."
}

function Get-ProcessTreeIds {
    param([int]$RootProcessId)

    $processes = @(Get-CimInstance Win32_Process)
    $ids = [Collections.Generic.HashSet[uint32]]::new()
    [void]$ids.Add([uint32]$RootProcessId)
    do {
        $before = $ids.Count
        foreach ($process in $processes) {
            if ($ids.Contains([uint32]$process.ParentProcessId)) {
                [void]$ids.Add([uint32]$process.ProcessId)
            }
        }
    } while ($ids.Count -gt $before)
    return @($ids | Sort-Object)
}

function Get-TreeMemory {
    param([uint32[]]$ProcessIds)

    $wanted = [Collections.Generic.HashSet[uint32]]::new()
    foreach ($processId in $ProcessIds) {
        [void]$wanted.Add([uint32]$processId)
    }
    $privateWorkingSet = [uint64]0
    $workingSet = [uint64]0
    $privateBytes = [uint64]0
    foreach ($counter in Get-CimInstance Win32_PerfRawData_PerfProc_Process) {
        if ($wanted.Contains([uint32]$counter.IDProcess)) {
            $privateWorkingSet += [uint64]$counter.WorkingSetPrivate
            $workingSet += [uint64]$counter.WorkingSet
            $privateBytes += [uint64]$counter.PrivateBytes
        }
    }
    return [pscustomobject]@{
        PrivateWorkingSetMiB = [double]$privateWorkingSet / 1MB
        WorkingSetMiB = [double]$workingSet / 1MB
        PrivateBytesMiB = [double]$privateBytes / 1MB
    }
}

function Get-Median {
    param([double[]]$Values)

    if ($Values.Count -eq 0) {
        throw "Cannot calculate a median from an empty sample set."
    }
    $sorted = [double[]]@($Values | Sort-Object)
    $middle = [Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) {
        return $sorted[$middle]
    }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

function Get-SettledTreeMemory {
    param(
        [int]$RootProcessId,
        [int]$SampleCount,
        [int]$IntervalMilliseconds
    )

    $samples = [Collections.Generic.List[object]]::new()
    $lastProcessIds = [uint32[]]@()
    for ($index = 0; $index -lt $SampleCount; $index++) {
        if ($index -gt 0) {
            Start-Sleep -Milliseconds $IntervalMilliseconds
        }
        $lastProcessIds = [uint32[]]@(Get-ProcessTreeIds -RootProcessId $RootProcessId)
        $samples.Add((Get-TreeMemory -ProcessIds $lastProcessIds))
    }

    $privateWorkingSet = [double[]]@($samples | ForEach-Object { $_.PrivateWorkingSetMiB })
    $workingSet = [double[]]@($samples | ForEach-Object { $_.WorkingSetMiB })
    $privateBytes = [double[]]@($samples | ForEach-Object { $_.PrivateBytesMiB })
    return [pscustomobject]@{
        ProcessIds = $lastProcessIds
        PrivateWorkingSetMiB = Get-Median -Values $privateWorkingSet
        WorkingSetMiB = Get-Median -Values $workingSet
        PrivateBytesMiB = Get-Median -Values $privateBytes
        PrivateWorkingSetSamplesMiB = $privateWorkingSet
        WorkingSetSamplesMiB = $workingSet
        PrivateBytesSamplesMiB = $privateBytes
    }
}

function Wait-ForTitle {
    param(
        [Diagnostics.Process]$Process,
        [string]$Prefix,
        [int]$TimeoutSeconds
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 100
        $Process.Refresh()
        if ($Process.HasExited) {
            throw "Release GUI exited while waiting for title '$Prefix'."
        }
        if ($Process.MainWindowTitle.StartsWith($Prefix, [StringComparison]::Ordinal)) {
            return $Process.MainWindowTitle
        }
    } while ((Get-Date) -lt $deadline)
    throw "Timed out waiting for title '$Prefix'; last title was '$($Process.MainWindowTitle)'."
}

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$sandbox = Join-Path ([IO.Path]::GetTempPath()) ("wingman-endurance-{0}-{1}" -f $PID, [Guid]::NewGuid().ToString("N"))
[void](New-Item -ItemType Directory -Path $sandbox)
Set-Content `
    -LiteralPath (Join-Path $sandbox "wingman-endurance.txt") `
    -Encoding utf8 `
    -Value "__WINGMAN_ENDURANCE_FOLLOW_READY__"
$probeName = "WINGMAN_PERF_ENDURANCE_PROBE"
$previousProbe = [Environment]::GetEnvironmentVariable($probeName, "Process")
$app = $null
$knownTreeIds = @()
$settledSampleCount = 5
$settledSampleIntervalMilliseconds = 250

try {
    try {
        [Environment]::SetEnvironmentVariable($probeName, "1", "Process")
        $app = Start-WingmanGuiProcess -Executable $resolvedExecutable -WorkingDirectory $sandbox
    }
    finally {
        [Environment]::SetEnvironmentVariable($probeName, $previousProbe, "Process")
    }

    [void](Wait-ForTitle -Process $app -Prefix "Wingman - Endurance Baseline" -TimeoutSeconds $StartupTimeoutSeconds)
    $baseline = Get-SettledTreeMemory `
        -RootProcessId $app.Id `
        -SampleCount $settledSampleCount `
        -IntervalMilliseconds $settledSampleIntervalMilliseconds
    $baselineIds = [uint32[]]@($baseline.ProcessIds)
    $knownTreeIds = @($baselineIds)
    $baselineProcesses = @(Get-CimInstance Win32_Process | Where-Object {
        $baselineIds -contains [uint32]$_.ProcessId
    })
    $baselineNames = @($baselineProcesses.Name | Sort-Object -Unique)
    if ($baselineNames -notcontains "msedgewebview2.exe" -or $baselineNames -notcontains "powershell.exe") {
        throw "The endurance baseline did not contain WebView2 and PowerShell."
    }

    if ($Smoke) {
        $cycleTitle = Wait-ForTitle -Process $app -Prefix "Wingman - Endurance Cycle 1" -TimeoutSeconds 60
        $smokeIds = [uint32[]]@(Get-ProcessTreeIds -RootProcessId $app.Id)
        $knownTreeIds = @($smokeIds)
        $smokeNames = @(Get-CimInstance Win32_Process | Where-Object {
            $smokeIds -contains [uint32]$_.ProcessId
        } | Select-Object -ExpandProperty Name -Unique)
        if ($smokeNames -contains "wingman-runner.exe") {
            throw "The completed smoke cycle left a runner alive."
        }
        [pscustomobject]@{
            Schema = "wingman.endurance-smoke.v2"
            CycleTitle = $cycleTitle
            SettledSampleCount = $settledSampleCount
            SettledSampleIntervalMilliseconds = $settledSampleIntervalMilliseconds
            BaselinePrivateWorkingSetMiB = $baseline.PrivateWorkingSetMiB
            BaselinePrivateWorkingSetSamplesMiB = $baseline.PrivateWorkingSetSamplesMiB
            ProcessNames = @($smokeNames | Sort-Object)
        } | ConvertTo-Json -Depth 3
        exit 0
    }

    $privateSamples = [Collections.Generic.List[double]]::new()
    $workingSetSamples = [Collections.Generic.List[double]]::new()
    $privateBytesSamples = [Collections.Generic.List[double]]::new()
    $processCountSamples = [Collections.Generic.List[int]]::new()
    $deadline = (Get-Date).AddMinutes($DurationMinutes).AddSeconds(90)
    $completionTitle = $null
    do {
        Start-Sleep -Seconds $SampleIntervalSeconds
        $app.Refresh()
        if ($app.HasExited) {
            throw "Release GUI exited before the endurance workload completed."
        }
        if ($app.MainWindowTitle.StartsWith("Wingman - Endurance Failed", [StringComparison]::Ordinal)) {
            throw "The in-app endurance workload failed at '$($app.MainWindowTitle)'."
        }
        if ($app.MainWindowTitle.StartsWith("Wingman - Endurance Complete", [StringComparison]::Ordinal)) {
            $completionTitle = $app.MainWindowTitle
        }

        $treeIds = [uint32[]]@(Get-ProcessTreeIds -RootProcessId $app.Id)
        $knownTreeIds = @($treeIds)
        $memory = Get-TreeMemory -ProcessIds $treeIds
        $privateSamples.Add($memory.PrivateWorkingSetMiB)
        $workingSetSamples.Add($memory.WorkingSetMiB)
        $privateBytesSamples.Add($memory.PrivateBytesMiB)
        $processCountSamples.Add($treeIds.Count)
    } while (-not $completionTitle -and (Get-Date) -lt $deadline)

    if (-not $completionTitle) {
        throw "The 30-minute endurance workload did not complete before the bounded deadline."
    }
    if ($completionTitle -notmatch '\AWingman - Endurance Complete ([1-9][0-9]*)\z') {
        throw "Malformed endurance completion title '$completionTitle'."
    }
    $cycleCount = [int]$Matches[1]
    $final = Get-SettledTreeMemory `
        -RootProcessId $app.Id `
        -SampleCount $settledSampleCount `
        -IntervalMilliseconds $settledSampleIntervalMilliseconds
    $finalIds = [uint32[]]@($final.ProcessIds)
    $knownTreeIds = @($finalIds)
    $finalProcesses = @(Get-CimInstance Win32_Process | Where-Object {
        $finalIds -contains [uint32]$_.ProcessId
    })
    if (@($finalProcesses | Where-Object { $_.Name -eq "wingman-runner.exe" }).Count -ne 0) {
        throw "The completed endurance workload left a runner alive."
    }
    $growthMiB = $final.PrivateWorkingSetMiB - $baseline.PrivateWorkingSetMiB
    $growthPercent = if ($baseline.PrivateWorkingSetMiB -gt 0) {
        100 * $growthMiB / $baseline.PrivateWorkingSetMiB
    }
    else {
        [double]::PositiveInfinity
    }

    $result = [ordered]@{
        Schema = "wingman.endurance.v2"
        DurationMinutes = $DurationMinutes
        CycleCount = $cycleCount
        SampleIntervalSeconds = $SampleIntervalSeconds
        SettledSampleCount = $settledSampleCount
        SettledSampleIntervalMilliseconds = $settledSampleIntervalMilliseconds
        BaselinePrivateWorkingSetMiB = $baseline.PrivateWorkingSetMiB
        BaselinePrivateWorkingSetSamplesMiB = $baseline.PrivateWorkingSetSamplesMiB
        BaselineWorkingSetSamplesMiB = $baseline.WorkingSetSamplesMiB
        BaselinePrivateBytesSamplesMiB = $baseline.PrivateBytesSamplesMiB
        FinalPrivateWorkingSetMiB = $final.PrivateWorkingSetMiB
        FinalPrivateWorkingSetSamplesMiB = $final.PrivateWorkingSetSamplesMiB
        FinalWorkingSetSamplesMiB = $final.WorkingSetSamplesMiB
        FinalPrivateBytesSamplesMiB = $final.PrivateBytesSamplesMiB
        GrowthMiB = $growthMiB
        GrowthPercent = $growthPercent
        PrivateWorkingSetMiB = [double[]]$privateSamples.ToArray()
        WorkingSetMiB = [double[]]$workingSetSamples.ToArray()
        PrivateBytesMiB = [double[]]$privateBytesSamples.ToArray()
        ProcessCounts = [int[]]$processCountSamples.ToArray()
        FinalProcessNames = @($finalProcesses.Name | Sort-Object -Unique)
    }
    [pscustomobject]$result | ConvertTo-Json -Depth 5

    if ($growthMiB -gt 50) {
        throw "Endurance private working-set growth $growthMiB MiB exceeded the 50 MiB release ceiling."
    }
    if ($growthPercent -gt 20) {
        throw "Endurance private working-set growth $growthPercent% exceeded the 20% release ceiling."
    }
    if ($final.PrivateWorkingSetMiB -gt 350) {
        throw "Final settled private working set exceeded the 350 MiB release ceiling."
    }
}
finally {
    if ($app) {
        $liveApp = Get-Process -Id $app.Id -ErrorAction SilentlyContinue
        if ($liveApp) {
            [void]$liveApp.CloseMainWindow()
            if (-not $liveApp.WaitForExit(5000)) {
                Stop-Process -Id $app.Id -Force
            }
        }
    }
    foreach ($processId in $knownTreeIds) {
        if ($processId -ne [uint32]$PID) {
            $leftover = Get-Process -Id $processId -ErrorAction SilentlyContinue
            if ($leftover) {
                Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
            }
        }
    }
    if (Test-Path -LiteralPath $sandbox -PathType Container) {
        Remove-Item -LiteralPath $sandbox -Recurse -Force
    }
}
