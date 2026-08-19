param(
  [string]$Executable = 'src-tauri/target/release/wingman.exe',
  [int]$TimeoutSeconds = 15
)

$ErrorActionPreference = 'Stop'
$projectRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$resolvedExecutable = (Resolve-Path -LiteralPath (Join-Path $projectRoot $Executable)).Path
$artifactRoot = Join-Path $projectRoot "src-tauri\target\cli-launch-$([guid]::NewGuid().ToString('N'))"
$unicodeDirectoryName = "$([char]0xD55C)$([char]0xAE00) project"
$startDirectory = Join-Path $artifactRoot $unicodeDirectoryName
New-Item -ItemType Directory -Path $startDirectory -Force | Out-Null

function Invoke-CapturedLauncher {
  param(
    [Parameter(Mandatory = $true)]
    [AllowEmptyString()]
    [string]$Arguments,
    [Parameter(Mandatory = $true)]
    [int]$ExpectedExitCode,
    [string]$FilePath = $resolvedExecutable
  )

  $stdoutPath = Join-Path $artifactRoot "$([guid]::NewGuid().ToString('N')).stdout"
  $stderrPath = Join-Path $artifactRoot "$([guid]::NewGuid().ToString('N')).stderr"
  $startParameters = @{
    FilePath = $FilePath
    WorkingDirectory = $projectRoot
    RedirectStandardOutput = $stdoutPath
    RedirectStandardError = $stderrPath
    PassThru = $true
  }
  if ($Arguments.Length -ne 0) {
    $startParameters.ArgumentList = $Arguments
  }
  $process = Start-Process @startParameters
  $null = $process.Handle
  if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    throw "Launcher timed out: $Arguments"
  }
  $process.WaitForExit()
  $process.Refresh()
  if ($process.ExitCode -ne $ExpectedExitCode) {
    throw "Launcher '$Arguments' exited $($process.ExitCode), expected $ExpectedExitCode"
  }
  [pscustomobject]@{
    Process = $process
    Stdout = if (Test-Path -LiteralPath $stdoutPath) {
      Get-Content -Raw -LiteralPath $stdoutPath
    } else { '' }
    Stderr = if (Test-Path -LiteralPath $stderrPath) {
      Get-Content -Raw -LiteralPath $stderrPath
    } else { '' }
  }
}

function Stop-LaunchedWindow {
  param($GuiProcess, $ShellProcess)

  if ($GuiProcess) {
    $liveGui = Get-Process -Id $GuiProcess.ProcessId -ErrorAction SilentlyContinue
    if ($liveGui) {
      [void]$liveGui.CloseMainWindow()
      if (-not $liveGui.WaitForExit(5000)) {
        Stop-Process -Id $liveGui.Id -Force -ErrorAction SilentlyContinue
      }
    }
  }
  if ($ShellProcess) {
    Stop-Process -Id $ShellProcess.ProcessId -Force -ErrorAction SilentlyContinue
  }
}

function Assert-GuiLaunch {
  param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('powershell', 'cmd')]
    [string]$Shell
  )

  $startedAt = Get-Date
  $quotedDirectory = '"' + $startDirectory + '"'
  $launch = Invoke-CapturedLauncher `
    -Arguments "--shell $Shell -- $quotedDirectory" `
    -ExpectedExitCode 0
  if ($launch.Stdout -or $launch.Stderr) {
    throw "Successful launcher wrote unexpected output: $($launch.Stdout)$($launch.Stderr)"
  }

  $gui = $null
  $shellProcess = $null
  try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
      Start-Sleep -Milliseconds 50
      $gui = Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" |
        Where-Object {
          $_.ExecutablePath -eq $resolvedExecutable -and
          $_.ProcessId -ne $launch.Process.Id -and
          (Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue).StartTime -ge $startedAt
        } |
        Select-Object -First 1
      $shellProcess = if ($Shell -eq 'powershell') {
        Get-CimInstance Win32_Process -Filter "Name = 'powershell.exe'" |
          Where-Object {
            $candidate = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $candidate -and $candidate.StartTime -ge $startedAt -and
              $_.CommandLine -like '*-NoLogo*-NoExit*-Command*WingmanReadinessPipe*'
          } |
          Select-Object -First 1
      } else {
        Get-CimInstance Win32_Process -Filter "Name = 'cmd.exe'" |
          Where-Object {
            $candidate = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            $candidate -and $candidate.StartTime -ge $startedAt -and
              $_.CommandLine -like '*chcp 65001*'
          } |
          Select-Object -First 1
      }
    } while ((-not $gui -or -not $shellProcess) -and (Get-Date) -lt $deadline)

    if (-not $gui -or -not $shellProcess) {
      throw "Launcher did not leave a live $Shell GUI/PTTY session"
    }
    if ($gui.CommandLine -notmatch '^"[^"]+wingman\.exe" --wingman-internal-gui [0-9]+$') {
      throw "Internal GUI command line is not fixed and bounded: $($gui.CommandLine)"
    }
    if ($gui.CommandLine.Contains($startDirectory) -or $gui.CommandLine.Contains('--shell')) {
      throw 'Internal GUI command line leaked public path or shell values'
    }
    $guiProcess = Get-Process -Id $gui.ProcessId
    if ($guiProcess.MainWindowHandle -eq 0) {
      throw 'Internal GUI process has no top-level window'
    }
  } finally {
    Stop-LaunchedWindow -GuiProcess $gui -ShellProcess $shellProcess
  }
}

try {
  $existing = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" |
    Where-Object { $_.ExecutablePath -eq $resolvedExecutable })
  if ($existing.Count -ne 0) {
    throw 'Close existing Wingman processes for this executable before the CLI launch test'
  }

  $help = Invoke-CapturedLauncher -Arguments '--help' -ExpectedExitCode 0
  if ($help.Stdout -notmatch 'wingman \[--shell powershell\|cmd\]') {
    throw 'Help output is missing the public grammar'
  }
  $version = Invoke-CapturedLauncher -Arguments '--version' -ExpectedExitCode 0
  if ($version.Stdout.Trim() -ne 'wingman 0.1.0') {
    throw "Unexpected version output: $($version.Stdout.Trim())"
  }
  $syntax = Invoke-CapturedLauncher -Arguments '--shell invalid' -ExpectedExitCode 2
  if ($syntax.Stderr -notmatch '^wingman: invalid command line:') {
    throw 'Syntax failure did not produce the bounded launcher diagnostic'
  }
  $missing = Invoke-CapturedLauncher -Arguments 'missing-wingman-directory' -ExpectedExitCode 1
  if ($missing.Stderr -notmatch '^wingman: invalid start directory:') {
    throw 'Missing path did not fail before GUI initialization'
  }
  [void](Invoke-CapturedLauncher -Arguments '--wingman-internal-gui 0' -ExpectedExitCode 2)

  $isolatedExecutable = Join-Path $artifactRoot 'wingman.exe'
  Copy-Item -LiteralPath $resolvedExecutable -Destination $isolatedExecutable
  $missingRunner = Invoke-CapturedLauncher `
    -Arguments '' `
    -ExpectedExitCode 1 `
    -FilePath $isolatedExecutable
  if ($missingRunner.Stderr -notmatch '^wingman: could not (inspect|open) Wingman runner:') {
    throw "Missing packaged runner did not propagate a bounded pre-readiness failure: $($missingRunner.Stderr)"
  }
  $orphan = @(Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" |
    Where-Object { $_.ExecutablePath -eq $isolatedExecutable })
  if ($orphan.Count -ne 0) {
    $orphan | ForEach-Object {
      Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
    }
    throw 'Missing packaged runner left an internal GUI orphan'
  }

  Assert-GuiLaunch -Shell powershell
  Assert-GuiLaunch -Shell cmd
  Write-Output 'CLI launcher handoff tests passed.'
} finally {
  if (Test-Path -LiteralPath $artifactRoot) {
    Remove-Item -LiteralPath $artifactRoot -Recurse -Force
  }
}
