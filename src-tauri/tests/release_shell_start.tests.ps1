param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [int]$TimeoutSeconds = 8
)

$ErrorActionPreference = "Stop"
$resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
$existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedExecutable
})
if ($existing.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$startedAt = Get-Date
$app = Start-Process -FilePath $resolvedExecutable -WorkingDirectory (Get-Location).Path -PassThru
$shell = $null
try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 100
        $shell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
            $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $process -and
                $process.StartTime -ge $startedAt -and
                $_.CommandLine -like '*-NoLogo*-NoExit*-ExecutionPolicy*Bypass*-Command*WINGMAN_INTEGRATION_SCRIPT*'
        } | Select-Object -First 1
    } while (-not $shell -and (Get-Date) -lt $deadline)

    if (-not $shell) {
        throw "Release GUI did not start its PowerShell PTY session within $TimeoutSeconds seconds."
    }

    Write-Output "Release GUI started PowerShell PTY process $($shell.ProcessId)."
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
        $liveShell = Get-Process -Id $shell.ProcessId -ErrorAction SilentlyContinue
        if ($liveShell) {
            Stop-Process -Id $shell.ProcessId -Force
        }
    }
}
