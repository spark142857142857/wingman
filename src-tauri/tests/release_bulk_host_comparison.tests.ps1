param(
    [Parameter(Mandatory = $true)]
    [string]$WingmanExecutable,
    [ValidateSet('powershell', 'cmd')]
    [string]$ShellKind = 'powershell',
    [int]$RunCount = 3,
    [int]$TimeoutSeconds = 90,
    [double]$TargetRatio = 2,
    [double]$ReleaseCeilingRatio = 3
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot 'release_process_helpers.ps1')
if ($RunCount -lt 3) {
    throw "RunCount must be at least 3."
}

$resolvedWingman = (Resolve-Path -LiteralPath $WingmanExecutable).Path
$existingWingman = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" | Where-Object {
    $_.ExecutablePath -eq $resolvedWingman
})
if ($existingWingman.Count -ne 0) {
    throw "Close the existing release Wingman process before running this test."
}

$terminalPackage = Get-AppxPackage -Name Microsoft.WindowsTerminal
if (-not $terminalPackage) {
    throw "Microsoft Windows Terminal is not installed."
}
$wtCommand = Get-Command wt.exe -ErrorAction Stop

if (-not ("WingmanBenchmarkWindow" -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class WingmanBenchmarkWindow {
    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetWindowPos(
        IntPtr hWnd,
        IntPtr hWndInsertAfter,
        int x,
        int y,
        int width,
        int height,
        uint flags
    );

    [DllImport("dwmapi.dll")]
    public static extern int DwmFlush();

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);

    public static string WindowTitle(IntPtr hWnd) {
        var text = new StringBuilder(512);
        GetWindowText(hWnd, text, text.Capacity);
        return text.ToString();
    }

    public static IntPtr FindVisibleWindow(string exactTitle) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr hWnd, IntPtr lParam) {
            if (IsWindowVisible(hWnd) && WindowTitle(hWnd) == exactTitle) {
                found = hWnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static uint WindowProcessId(IntPtr hWnd) {
        uint processId;
        GetWindowThreadProcessId(hWnd, out processId);
        return processId;
    }

    public static bool CloseWindow(IntPtr hWnd) {
        return PostMessage(hWnd, 0x0010, IntPtr.Zero, IntPtr.Zero);
    }
}
"@
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
    return @($ids)
}

function Stop-OwnedProcessTree {
    param([int]$RootProcessId)

    $ownedIds = @(Get-ProcessTreeIds -RootProcessId $RootProcessId)
    $root = Get-Process -Id $RootProcessId -ErrorAction SilentlyContinue
    if ($root -and $root.MainWindowHandle -ne 0) {
        [void]$root.CloseMainWindow()
        [void]$root.WaitForExit(5000)
    }
    foreach ($ownedId in $ownedIds) {
        Stop-Process -Id $ownedId -Force -ErrorAction SilentlyContinue
    }
}

function Get-WindowBounds {
    param([IntPtr]$WindowHandle)

    if ($WindowHandle -eq [IntPtr]::Zero) {
        throw "The benchmark window handle is empty."
    }
    $rect = New-Object WingmanBenchmarkWindow+RECT
    if (-not [WingmanBenchmarkWindow]::GetWindowRect($WindowHandle, [ref]$rect)) {
        throw "GetWindowRect failed for benchmark window $WindowHandle."
    }
    return [pscustomobject]@{
        Width = $rect.Right - $rect.Left
        Height = $rect.Bottom - $rect.Top
    }
}

function Set-WindowBounds {
    param(
        [IntPtr]$WindowHandle,
        [int]$Width,
        [int]$Height
    )

    $noMoveNoOrderNoActivate = [uint32]0x0016
    if (-not [WingmanBenchmarkWindow]::SetWindowPos(
        $WindowHandle,
        [IntPtr]::Zero,
        0,
        0,
        $Width,
        $Height,
        $noMoveNoOrderNoActivate
    )) {
        throw "SetWindowPos failed for Windows Terminal window $WindowHandle."
    }
    Start-Sleep -Milliseconds 100
    $actual = Get-WindowBounds -WindowHandle $WindowHandle
    if ($actual.Width -ne $Width -or $actual.Height -ne $Height) {
        throw "Windows Terminal window was $($actual.Width)x$($actual.Height), expected ${Width}x${Height}."
    }
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

function Wait-ForTwoCompositorFrames {
    for ($frame = 0; $frame -lt 2; $frame++) {
        $result = [WingmanBenchmarkWindow]::DwmFlush()
        if ($result -ne 0) {
            throw "DwmFlush failed with HRESULT $result."
        }
    }
}

function Measure-WingmanBulkRender {
    $probeVariable = "WINGMAN_PERF_BULK_OUTPUT_PROBE"
    $previousProbe = [Environment]::GetEnvironmentVariable($probeVariable, "Process")
    $startedAt = Get-Date
    try {
        [Environment]::SetEnvironmentVariable($probeVariable, "1", "Process")
        $app = Start-WingmanGuiProcess `
            -Executable $resolvedWingman `
            -WorkingDirectory (Get-Location).Path `
            -Arguments @('--shell', $ShellKind)
    }
    finally {
        [Environment]::SetEnvironmentVariable($probeVariable, $previousProbe, "Process")
    }

    try {
        $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
        do {
            Start-Sleep -Milliseconds 25
            $app.Refresh()
            if ($app.HasExited) {
                throw "Wingman exited before the deterministic bulk output rendered."
            }
        } while ($app.MainWindowTitle -ne "Wingman - Bulk Rendered" -and (Get-Date) -lt $deadline)

        if ($app.MainWindowTitle -ne "Wingman - Bulk Rendered") {
            throw "Wingman did not validate and render 100,000 deterministic lines within $TimeoutSeconds seconds."
        }
        $treeIds = @(Get-ProcessTreeIds -RootProcessId $app.Id)
        $shell = Find-WingmanShellProcess -ShellKind $ShellKind -StartedAt $startedAt
        if (-not $shell -or $treeIds -notcontains [uint32]$shell.ProcessId) {
            throw "Wingman reported bulk rendering without an active integrated $ShellKind session."
        }

        $bounds = Get-WindowBounds -WindowHandle $app.MainWindowHandle
        return [pscustomobject]@{
            ElapsedMilliseconds = ((Get-Date) - $startedAt).TotalMilliseconds
            Width = $bounds.Width
            Height = $bounds.Height
        }
    }
    finally {
        if ($app) {
            Stop-OwnedProcessTree -RootProcessId $app.Id
        }
    }
}

function Measure-WindowsTerminalBulkRender {
    param(
        [int]$Width,
        [int]$Height
    )

    $token = [Guid]::NewGuid().ToString("N")
    $readyTitle = "Wingman-WT-Ready-$token"
    $completeTitle = "Wingman-WT-Bulk-$token"
    $readyPath = Join-Path ([IO.Path]::GetTempPath()) "wingman-wt-$token.ready"
    $escapedReadyPath = $readyPath.Replace("'", "''")
    $script =
        "[Console]::Title='$readyTitle';" +
        "`$deadline=[DateTime]::UtcNow.AddSeconds(30);" +
        "while(-not(Test-Path -LiteralPath '$escapedReadyPath')){" +
        "if([DateTime]::UtcNow -ge `$deadline){exit 3};Start-Sleep -Milliseconds 10};" +
        '$p=([char]0xe9).ToString()*55;' +
        '$s=''__WINGMAN_BULK_''+''START__'';$e=''__WINGMAN_BULK_''+''END__'';' +
        '[Console]::Out.Write($s+"`r`n");' +
        'for($i=0;$i -lt 100000;$i++){[Console]::Out.Write(("{0:D6}:{1}`r`n" -f $i,$p))};' +
        '[Console]::Out.Write($e+"`r`n");[Console]::Out.Flush();' +
        "[Console]::Title='$completeTitle';Start-Sleep -Seconds 300"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
    $windowName = "wingman-benchmark-$token"
    $windowHandle = [IntPtr]::Zero
    $benchmarkShell = $null
    $startedAt = Get-Date

    try {
        [void](Start-Process -FilePath $wtCommand.Source -ArgumentList @(
            "-w", $windowName, "--size", "120,30", "nt",
            "powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive",
            "-EncodedCommand", $encoded
        ) -PassThru)

        $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
        do {
            Start-Sleep -Milliseconds 25
            $windowHandle = [WingmanBenchmarkWindow]::FindVisibleWindow($readyTitle)
        } while (
            $windowHandle -eq [IntPtr]::Zero -and
            (Get-Date) -lt $deadline
        )
        if ($windowHandle -eq [IntPtr]::Zero) {
            throw "The matched Windows Terminal benchmark window did not appear."
        }

        Set-WindowBounds -WindowHandle $windowHandle -Width $Width -Height $Height
        [IO.File]::WriteAllBytes($readyPath, [byte[]]@())

        do {
            Start-Sleep -Milliseconds 25
            $currentTitle = [WingmanBenchmarkWindow]::WindowTitle($windowHandle)
        } while ($currentTitle -ne $completeTitle -and (Get-Date) -lt $deadline)
        if ($currentTitle -ne $completeTitle) {
            throw "Windows Terminal did not finish the 100,000-line workload within $TimeoutSeconds seconds."
        }
        Wait-ForTwoCompositorFrames

        $benchmarkShell = Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" | Where-Object {
            $_.CommandLine -and $_.CommandLine.Contains($encoded)
        } | Select-Object -First 1
        if (-not $benchmarkShell) {
            throw "Windows Terminal reported completion without the benchmark PowerShell session."
        }

        return [pscustomobject]@{
            ElapsedMilliseconds = ((Get-Date) - $startedAt).TotalMilliseconds
            Width = $Width
            Height = $Height
        }
    }
    finally {
        if ($windowHandle -ne [IntPtr]::Zero) {
            [void][WingmanBenchmarkWindow]::CloseWindow($windowHandle)
        }
        else {
            foreach ($title in $readyTitle, $completeTitle) {
                $orphanedHandle = [WingmanBenchmarkWindow]::FindVisibleWindow($title)
                if ($orphanedHandle -ne [IntPtr]::Zero) {
                    [void][WingmanBenchmarkWindow]::CloseWindow($orphanedHandle)
                }
            }
        }
        if ($benchmarkShell) {
            Stop-Process -Id $benchmarkShell.ProcessId -Force -ErrorAction SilentlyContinue
        }
        else {
            Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" -ErrorAction SilentlyContinue |
                Where-Object { $_.CommandLine -and $_.CommandLine.Contains($encoded) } |
                ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
        }
        Remove-Item -LiteralPath $readyPath -Force -ErrorAction SilentlyContinue
    }
}

$wingmanSamples = [System.Collections.Generic.List[double]]::new()
$windowsTerminalSamples = [System.Collections.Generic.List[double]]::new()
$matchedWidth = 0
$matchedHeight = 0

for ($run = 1; $run -le $RunCount; $run++) {
    $wingman = Measure-WingmanBulkRender
    if ($matchedWidth -eq 0) {
        $matchedWidth = $wingman.Width
        $matchedHeight = $wingman.Height
    }
    elseif ($wingman.Width -ne $matchedWidth -or $wingman.Height -ne $matchedHeight) {
        throw "Wingman window size changed between runs."
    }
    $wingmanSamples.Add($wingman.ElapsedMilliseconds)

    $windowsTerminal = Measure-WindowsTerminalBulkRender -Width $matchedWidth -Height $matchedHeight
    $windowsTerminalSamples.Add($windowsTerminal.ElapsedMilliseconds)
    Write-Output ("Run {0}: Wingman {1:N1} ms; Windows Terminal {2:N1} ms." -f
        $run, $wingman.ElapsedMilliseconds, $windowsTerminal.ElapsedMilliseconds)
}

$wingmanMedian = Get-Median -Values $wingmanSamples.ToArray()
$windowsTerminalMedian = Get-Median -Values $windowsTerminalSamples.ToArray()
if ($windowsTerminalMedian -le 0) {
    throw "Windows Terminal median must be positive."
}
$ratio = $wingmanMedian / $windowsTerminalMedian
$result = [ordered]@{
    WindowsTerminalVersion = $terminalPackage.Version.ToString()
    WindowsVersion = [Environment]::OSVersion.Version.ToString()
    PowerShellVersion = $PSVersionTable.PSVersion.ToString()
    WingmanShell = $ShellKind
    WorkingDirectory = (Get-Location).Path
    WindowWidth = $matchedWidth
    WindowHeight = $matchedHeight
    WingmanMilliseconds = $wingmanSamples.ToArray()
    WingmanMedianMilliseconds = $wingmanMedian
    WindowsTerminalMilliseconds = $windowsTerminalSamples.ToArray()
    WindowsTerminalMedianMilliseconds = $windowsTerminalMedian
    WingmanToWindowsTerminalRatio = $ratio
    TargetRatio = $TargetRatio
    ReleaseCeilingRatio = $ReleaseCeilingRatio
    TargetMet = $ratio -le $TargetRatio
    ReleaseCeilingMet = $ratio -le $ReleaseCeilingRatio
}
[pscustomobject]$result | ConvertTo-Json -Depth 3

if ($ratio -gt $ReleaseCeilingRatio) {
    throw "Wingman median was $ratio times Windows Terminal, above the ${ReleaseCeilingRatio}x release ceiling."
}
