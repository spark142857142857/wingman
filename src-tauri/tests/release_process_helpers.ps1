function Start-WingmanGuiProcess {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Executable,
    [Parameter(Mandatory = $true)]
    [string]$WorkingDirectory,
    [string[]]$Arguments = @(),
    [int]$TimeoutSeconds = 15
  )

  $resolvedExecutable = (Resolve-Path -LiteralPath $Executable).Path
  $startedAt = Get-Date
  $startArguments = @{
    FilePath = $resolvedExecutable
    WorkingDirectory = $WorkingDirectory
    PassThru = $true
  }
  if ($Arguments.Count -ne 0) {
    $startArguments.ArgumentList = $Arguments
  }
  $launcher = Start-Process @startArguments
  $null = $launcher.Handle
  if (-not $launcher.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $launcher.Id -Force -ErrorAction SilentlyContinue
    throw "Wingman launcher did not finish within $TimeoutSeconds seconds."
  }
  $launcher.WaitForExit()
  $launcher.Refresh()
  if ($launcher.ExitCode -ne 0) {
    throw "Wingman launcher exited with code $($launcher.ExitCode)."
  }

  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    $candidate = Get-CimInstance Win32_Process -Filter "Name = 'wingman.exe'" |
      Where-Object {
        if ($_.ExecutablePath -ne $resolvedExecutable -or $_.ProcessId -eq $launcher.Id) {
          return $false
        }
        $process = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
        return $process -and $process.StartTime -ge $startedAt
      } |
      Select-Object -First 1
    if ($candidate) {
      return Get-Process -Id $candidate.ProcessId -ErrorAction Stop
    }
    Start-Sleep -Milliseconds 25
  } while ((Get-Date) -lt $deadline)

  throw 'Wingman launcher returned success without a live GUI child.'
}
