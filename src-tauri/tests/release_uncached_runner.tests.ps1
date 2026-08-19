param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Prepare", "Sample", "Validate")]
    [string]$Mode,
    [Parameter(Mandatory = $true)]
    [string]$FixtureRoot,
    [string]$Operation,
    [string]$RecordPath,
    [int]$RequiredSamplesPerOperation = 5,
    [int]$MaximumBootAgeMinutes = 15
)

$ErrorActionPreference = "Stop"
$validOperations = @("grep", "find", "cat", "sort")
$manifest = Join-Path (Split-Path $PSScriptRoot -Parent) "Cargo.toml"

if (-not [IO.Path]::IsPathRooted($FixtureRoot) -or [IO.Path]::GetPathRoot($FixtureRoot) -eq "\") {
    throw "FixtureRoot must be an absolute path."
}
$fixturePath = [IO.Path]::GetFullPath($FixtureRoot)
if ([string]::IsNullOrWhiteSpace($RecordPath)) {
    $RecordPath = Join-Path $fixturePath "uncached-samples.jsonl"
}
$recordFile = [IO.Path]::GetFullPath($RecordPath)

function Invoke-PerformanceTest {
    param(
        [string]$TestName,
        [string]$SelectedOperation
    )

    $previousRoot = [Environment]::GetEnvironmentVariable(
        "WINGMAN_UNCACHED_FIXTURE_ROOT",
        "Process"
    )
    $previousOperation = [Environment]::GetEnvironmentVariable(
        "WINGMAN_UNCACHED_OPERATION",
        "Process"
    )
    try {
        [Environment]::SetEnvironmentVariable(
            "WINGMAN_UNCACHED_FIXTURE_ROOT",
            $fixturePath,
            "Process"
        )
        [Environment]::SetEnvironmentVariable(
            "WINGMAN_UNCACHED_OPERATION",
            $SelectedOperation,
            "Process"
        )
        $previousPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $lines = @(& cargo test --release --manifest-path $manifest `
                --test runner_performance_contract $TestName -- --ignored --exact --nocapture 2>&1)
            $cargoExitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previousPreference
        }
        if ($cargoExitCode -ne 0) {
            throw "Release runner performance test failed:`n$($lines -join [Environment]::NewLine)"
        }
        return $lines
    }
    finally {
        [Environment]::SetEnvironmentVariable(
            "WINGMAN_UNCACHED_FIXTURE_ROOT",
            $previousRoot,
            "Process"
        )
        [Environment]::SetEnvironmentVariable(
            "WINGMAN_UNCACHED_OPERATION",
            $previousOperation,
            "Process"
        )
    }
}

function Read-Samples {
    if (-not (Test-Path -LiteralPath $recordFile -PathType Leaf)) {
        return @()
    }
    return @(
        Get-Content -LiteralPath $recordFile -Encoding utf8 |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
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

if ($Mode -eq "Prepare") {
    $lines = Invoke-PerformanceTest `
        -TestName "prepare_uncached_runner_fixture" `
        -SelectedOperation ""
    $marker = @($lines | Where-Object { $_ -like "*WINGMAN_RUNNER_UNCACHED_FIXTURE_V1=*" })
    if ($marker.Count -ne 1) {
        throw "Fixture preparation did not emit exactly one fixture marker."
    }
    Write-Output $marker[0]
    exit 0
}

if ($Mode -eq "Sample") {
    if ($validOperations -notcontains $Operation) {
        throw "Sample mode requires Operation to name grep, find, cat, or sort."
    }
    $os = Get-CimInstance Win32_OperatingSystem
    $bootUtc = $os.LastBootUpTime.ToUniversalTime()
    $bootAge = (Get-Date).ToUniversalTime() - $bootUtc
    if ($bootAge.TotalMinutes -gt $MaximumBootAgeMinutes) {
        throw ("The current boot is {0:N1} minutes old; run within {1} minutes of a controlled restart." -f `
            $bootAge.TotalMinutes, $MaximumBootAgeMinutes)
    }
    $samples = Read-Samples
    if (@($samples | Where-Object { $_.BootUtc -eq $bootUtc.ToString("O") }).Count -ne 0) {
        throw "This Windows boot already contributed an uncached sample; restart before the next operation."
    }

    $lines = Invoke-PerformanceTest `
        -TestName "uncached_runner_timing_sample" `
        -SelectedOperation $Operation
    $markerLine = @($lines | Where-Object { $_ -like "*WINGMAN_RUNNER_UNCACHED_V1=*" })
    if ($markerLine.Count -ne 1) {
        throw "Uncached sample did not emit exactly one result marker."
    }
    $json = ($markerLine[0] -split "WINGMAN_RUNNER_UNCACHED_V1=", 2)[1]
    $sample = $json | ConvertFrom-Json
    if ($sample.operation -ne $Operation -or $sample.cache_state -ne "fixture-first-read") {
        throw "Uncached sample marker did not match the requested operation and cache state."
    }
    $commit = (& git -C (Split-Path $PSScriptRoot -Parent | Split-Path -Parent) rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $commit -notmatch '\A[0-9a-f]{40}\z') {
        throw "Could not record the exact source commit."
    }
    $record = [ordered]@{
        Schema = "wingman.runner-uncached.v1"
        RecordedUtc = (Get-Date).ToUniversalTime().ToString("O")
        BootUtc = $bootUtc.ToString("O")
        Commit = $commit
        OsCaption = [string]$os.Caption
        OsVersion = [string]$os.Version
        OsBuild = [string]$os.BuildNumber
        Architecture = [string]$os.OSArchitecture
        Operation = [string]$sample.operation
        ElapsedMilliseconds = [double]$sample.elapsed_ms
        CorpusBytes = [uint64]$sample.corpus_bytes
        CorpusRecords = [uint64]$sample.corpus_records
        FindEntries = [uint64]$sample.find_entries
        SortRecords = [uint64]$sample.sort_records
    }
    $parent = Split-Path $recordFile -Parent
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        [void](New-Item -ItemType Directory -Path $parent)
    }
    Add-Content -LiteralPath $recordFile -Encoding utf8 -Value ($record | ConvertTo-Json -Compress)
    [pscustomobject]$record | ConvertTo-Json -Depth 3
    exit 0
}

if ($RequiredSamplesPerOperation -lt 5) {
    throw "RequiredSamplesPerOperation cannot be below the five-sample contract minimum."
}
$samples = Read-Samples
$summaries = [System.Collections.Generic.List[object]]::new()
foreach ($name in $validOperations) {
    $operationSamples = @($samples | Where-Object {
        $_.Schema -eq "wingman.runner-uncached.v1" -and $_.Operation -eq $name
    })
    $uniqueBoots = @($operationSamples.BootUtc | Sort-Object -Unique)
    if ($uniqueBoots.Count -lt $RequiredSamplesPerOperation) {
        throw "$name has $($uniqueBoots.Count) unique-boot samples; $RequiredSamplesPerOperation are required."
    }
    if ($uniqueBoots.Count -ne $operationSamples.Count) {
        throw "$name contains duplicate samples from the same Windows boot."
    }
    $values = [double[]]@($operationSamples.ElapsedMilliseconds)
    $summaries.Add([pscustomobject]@{
        Operation = $name
        SampleCount = $values.Count
        MedianMilliseconds = Get-Median -Values $values
        P95Milliseconds = Get-Percentile -Values $values -Percentile 0.95
        MaximumMilliseconds = [double](($values | Measure-Object -Maximum).Maximum)
        RawMilliseconds = $values
    })
}

[pscustomobject]@{
    Schema = "wingman.runner-uncached-distribution.v1"
    RequiredSamplesPerOperation = $RequiredSamplesPerOperation
    Results = $summaries.ToArray()
} | ConvertTo-Json -Depth 5
