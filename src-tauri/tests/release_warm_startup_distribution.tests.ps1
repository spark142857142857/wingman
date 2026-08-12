param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$WarmupCount = 3,
    [int]$SampleCount = 20,
    [int]$TimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"

if ($WarmupCount -lt 0) {
    throw "WarmupCount cannot be negative."
}
if ($SampleCount -lt 2) {
    throw "SampleCount must be at least 2."
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

$singleRunHarness = Join-Path $PSScriptRoot "release_shell_echo.tests.ps1"
for ($warmup = 1; $warmup -le $WarmupCount; $warmup++) {
    $result = & $singleRunHarness `
        -Executable $Executable `
        -TimeoutSeconds $TimeoutSeconds `
        -PassThru
    Write-Output ("Warmup {0}/{1}: {2:N1} ms" -f $warmup, $WarmupCount, $result.ElapsedMilliseconds)
}

$samples = [System.Collections.Generic.List[double]]::new()
for ($sample = 1; $sample -le $SampleCount; $sample++) {
    $result = & $singleRunHarness `
        -Executable $Executable `
        -TimeoutSeconds $TimeoutSeconds `
        -PassThru
    $samples.Add([double]$result.ElapsedMilliseconds)
    Write-Output ("Sample {0}/{1}: {2:N1} ms" -f $sample, $SampleCount, $result.ElapsedMilliseconds)
}

$median = Get-Median -Values $samples.ToArray()
$p95 = Get-Percentile -Values $samples.ToArray() -Percentile 0.95
$maximum = [double](($samples | Measure-Object -Maximum).Maximum)

Write-Output ("Warm startup median: {0:N1} ms; p95: {1:N1} ms; maximum: {2:N1} ms." -f $median, $p95, $maximum)
Write-Output ("Raw samples (ms): " + (($samples | ForEach-Object { $_.ToString("F1", [Globalization.CultureInfo]::InvariantCulture) }) -join ", "))
