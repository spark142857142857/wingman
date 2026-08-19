param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [ValidateSet('powershell', 'cmd')]
    [string]$ShellKind = 'powershell',
    [int]$TimeoutSeconds = 60
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

$probeVariable = "WINGMAN_PERF_BULK_OUTPUT_PROBE"
$previousProbe = [Environment]::GetEnvironmentVariable($probeVariable, "Process")
$startedAt = Get-Date
try {
    [Environment]::SetEnvironmentVariable($probeVariable, "1", "Process")
    $app = Start-WingmanGuiProcess `
        -Executable $resolvedExecutable `
        -WorkingDirectory (Get-Location).Path `
        -Arguments @('--shell', $ShellKind)
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
        $shell = Find-WingmanShellProcess -ShellKind $ShellKind -StartedAt $startedAt
    } while (
        $app.MainWindowTitle -ne "Wingman - Bulk Rendered" -and
        -not $app.HasExited -and
        (Get-Date) -lt $deadline
    )

    if ($app.HasExited) {
        throw "Release GUI exited before the deterministic bulk output rendered."
    }
    if ($app.MainWindowTitle -ne "Wingman - Bulk Rendered") {
        throw "Release GUI did not validate and render 100,000 deterministic lines within $TimeoutSeconds seconds; title was '$($app.MainWindowTitle)'."
    }
    if (-not $shell) {
        throw "Release GUI reported bulk rendering without an active $ShellKind PTY session."
    }

    $elapsed = ((Get-Date) - $startedAt).TotalMilliseconds
    Write-Output ("Release GUI validated and rendered 100,000 lines in $ShellKind (11,900,000 UTF-8 bytes) in {0:N1} ms." -f $elapsed)
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
