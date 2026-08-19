param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$ShellTimeoutSeconds = 30,
    [int]$SettleSeconds = 10,
    [int]$SampleCount = 10,
    [int]$SampleIntervalSeconds = 1
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

function Get-Percentile {
    param(
        [double[]]$Values,
        [double]$Percentile
    )

    $ordered = @($Values | Sort-Object)
    $index = [Math]::Ceiling($Percentile * $ordered.Count) - 1
    return [double]$ordered[[Math]::Max(0, $index)]
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

function Get-CpuTimes {
    param([uint32[]]$ProcessIds)

    $times = @{}
    foreach ($processId in $ProcessIds) {
        $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
        if ($process) {
            $times[[uint32]$processId] = [double]$process.CPU
        }
    }
    return $times
}

function Get-TreeMemory {
    param([uint32[]]$ProcessIds)

    $wanted = [System.Collections.Generic.HashSet[uint32]]::new()
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

$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$startedAt = Get-Date
$app = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Get-Location).Path -PassThru
$knownTreeIds = @([uint32]$app.Id)
try {
    $shell = $null
    $deadline = (Get-Date).AddSeconds($ShellTimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 100
        $shell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $process -and
                $process.StartTime -ge $startedAt -and
                $_.CommandLine -like '*-NoLogo*-NoExit*-Command*WingmanReadinessPipe*'
        } | Select-Object -First 1
    } while (-not $shell -and (Get-Date) -lt $deadline)

    if (-not $shell) {
        throw "Release GUI did not start its PowerShell PTY session within $ShellTimeoutSeconds seconds."
    }

    $initialTreeIds = @(Get-ProcessTreeIds -RootProcessId $app.Id)
    if ($initialTreeIds -notcontains [uint32]$shell.ProcessId) {
        throw "The active PowerShell PTY session is not owned by the Wingman process tree."
    }

    Start-Sleep -Seconds $SettleSeconds

    $logicalProcessors = [int](Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
    $cpuSamples = [System.Collections.Generic.List[double]]::new()
    $privateWorkingSetSamples = [System.Collections.Generic.List[double]]::new()
    $workingSetSamples = [System.Collections.Generic.List[double]]::new()
    $privateBytesSamples = [System.Collections.Generic.List[double]]::new()
    $processCountSamples = [System.Collections.Generic.List[int]]::new()

    for ($sample = 0; $sample -lt $SampleCount; $sample++) {
        $beforeIds = @(Get-ProcessTreeIds -RootProcessId $app.Id)
        $beforeCpu = Get-CpuTimes -ProcessIds $beforeIds
        Start-Sleep -Seconds $SampleIntervalSeconds
        $afterIds = @(Get-ProcessTreeIds -RootProcessId $app.Id)
        $afterCpu = Get-CpuTimes -ProcessIds $afterIds

        $cpuDeltaSeconds = [double]0
        foreach ($processId in $afterIds) {
            if ($beforeCpu.ContainsKey([uint32]$processId) -and $afterCpu.ContainsKey([uint32]$processId)) {
                $delta = $afterCpu[[uint32]$processId] - $beforeCpu[[uint32]$processId]
                if ($delta -gt 0) {
                    $cpuDeltaSeconds += $delta
                }
            }
        }
        $cpuPercent = 100 * $cpuDeltaSeconds / ($SampleIntervalSeconds * $logicalProcessors)
        $memory = Get-TreeMemory -ProcessIds $afterIds

        $cpuSamples.Add($cpuPercent)
        $privateWorkingSetSamples.Add($memory.PrivateWorkingSetMiB)
        $workingSetSamples.Add($memory.WorkingSetMiB)
        $privateBytesSamples.Add($memory.PrivateBytesMiB)
        $processCountSamples.Add($afterIds.Count)
        $knownTreeIds = @($afterIds)
    }

    $finalTree = @(Get-CimInstance Win32_Process | Where-Object {
        $knownTreeIds -contains [uint32]$_.ProcessId
    })
    $names = @($finalTree.Name | Sort-Object -Unique)
    if ($names -notcontains "msedgewebview2.exe" -or $names -notcontains "powershell.exe") {
        throw "The measured process tree did not contain both WebView2 and the active PowerShell session."
    }

    $cpuArray = [double[]]$cpuSamples.ToArray()
    $privateWorkingSetArray = [double[]]$privateWorkingSetSamples.ToArray()
    $result = [ordered]@{
        Executable = $resolvedExecutable
        RootProcessId = $app.Id
        ShellProcessId = [int]$shell.ProcessId
        LogicalProcessors = $logicalProcessors
        SettleSeconds = $SettleSeconds
        SampleIntervalSeconds = $SampleIntervalSeconds
        ProcessNames = $names
        ProcessCounts = [int[]]$processCountSamples.ToArray()
        CpuPercent = $cpuArray
        CpuMedianPercent = Get-Median -Values $cpuArray
        CpuP95Percent = Get-Percentile -Values $cpuArray -Percentile 0.95
        PrivateWorkingSetMiB = $privateWorkingSetArray
        PrivateWorkingSetMedianMiB = Get-Median -Values $privateWorkingSetArray
        PrivateWorkingSetMaxMiB = ($privateWorkingSetArray | Measure-Object -Maximum).Maximum
        WorkingSetMiB = [double[]]$workingSetSamples.ToArray()
        PrivateBytesMiB = [double[]]$privateBytesSamples.ToArray()
    }

    [pscustomobject]$result | ConvertTo-Json -Depth 4

    if ($result.CpuMedianPercent -gt 0.5) {
        throw "Whole-tree median idle CPU $($result.CpuMedianPercent)% exceeded the 0.5% release ceiling."
    }
    if ($result.CpuP95Percent -gt 2.0) {
        throw "Whole-tree p95 idle CPU $($result.CpuP95Percent)% exceeded the 2% release ceiling."
    }
    if ($result.PrivateWorkingSetMaxMiB -gt 350) {
        throw "Whole-tree private working set $($result.PrivateWorkingSetMaxMiB) MiB exceeded the 350 MiB release ceiling."
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
            $leftover = Get-Process -Id $processId -ErrorAction SilentlyContinue
            if ($leftover) {
                Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
            }
        }
    }
}
