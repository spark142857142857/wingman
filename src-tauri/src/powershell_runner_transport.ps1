$script:WingmanReadinessNonce = $env:WINGMAN_SESSION_NONCE
$script:WingmanReadinessPipe = $env:WINGMAN_READINESS_PIPE
$script:WingmanIsolatedPerformanceHistory =
  $env:WINGMAN_PERF_ISOLATED_HISTORY -ceq '1'
Remove-Item Env:WINGMAN_SESSION_NONCE -ErrorAction SilentlyContinue
Remove-Item Env:WINGMAN_READINESS_PIPE -ErrorAction SilentlyContinue
Remove-Item Env:WINGMAN_PERF_ISOLATED_HISTORY -ErrorAction SilentlyContinue

if (
  $script:WingmanReadinessNonce -cmatch '\A[0-9A-Fa-f]{32}\z' -and
  $script:WingmanReadinessPipe -cmatch '\A[A-Za-z0-9._-]{1,128}\z'
) {
  Import-Module PSReadLine -ErrorAction Stop
  if ($script:WingmanIsolatedPerformanceHistory) {
    Set-PSReadLineOption -HistorySaveStyle SaveNothing
  }
  Set-PSReadLineKeyHandler `
    -Chord 'Ctrl+x,Ctrl+w' `
    -BriefDescription 'WingmanReplaceLineV1' `
    -Description 'Clear the verified editor buffer before a Wingman prepared request.' `
    -ScriptBlock {
      [Microsoft.PowerShell.PSConsoleReadLine]::RevertLine()
    }

  $script:WingmanReadinessClient = $null
  $script:WingmanReadinessSequence = [uint64] 0
  $script:WingmanReadinessLatch = $false
  $script:WingmanOriginalPrompt = $function:prompt

  function global:prompt {
    $replacementHandler = Get-PSReadLineKeyHandler |
      Where-Object {
        $_.Key -eq 'Ctrl+x,Ctrl+w' -and
        $_.Function -eq 'WingmanReplaceLineV1'
      } |
      Select-Object -First 1
    $script:WingmanReadinessLatch = $null -ne $replacementHandler

    if ($null -ne $script:WingmanOriginalPrompt) {
      return & $script:WingmanOriginalPrompt
    }
    return "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) "
  }

  if ($null -ne $function:PSConsoleHostReadLine) {
    function global:PSConsoleHostReadLine {
      Microsoft.PowerShell.Core\Set-StrictMode -Off

      $script:WingmanReadinessSequence++
      $signalReadiness = $script:WingmanReadinessLatch
      $script:WingmanReadinessLatch = $false
      if ($signalReadiness -and $null -eq $script:WingmanReadinessClient) {
        try {
          $script:WingmanReadinessClient = New-Object System.IO.Pipes.NamedPipeClientStream(
            '.',
            $script:WingmanReadinessPipe,
            [System.IO.Pipes.PipeDirection]::Out,
            [System.IO.Pipes.PipeOptions]::Asynchronous
          )
          $script:WingmanReadinessClient.Connect(250)
        } catch {
          if ($null -ne $script:WingmanReadinessClient) {
            $script:WingmanReadinessClient.Dispose()
          }
          $script:WingmanReadinessClient = $null
        }
      }
      if ($signalReadiness -and $null -ne $script:WingmanReadinessClient) {
        $locationKind = if (
          $executionContext.SessionState.Path.CurrentLocation.Provider.Name -eq 'FileSystem'
        ) {
          'filesystem'
        } else {
          'non-filesystem'
        }
        $shellDepth = [Math]::Max(0, [int] $nestedPromptLevel)
        $frame = "1;$script:WingmanReadinessNonce;$script:WingmanReadinessSequence;powershell;$shellDepth;$locationKind;psreadline-replace-v1`n"
        $bytes = [System.Text.Encoding]::ASCII.GetBytes($frame)
        try {
          $write = $script:WingmanReadinessClient.BeginWrite(
            $bytes,
            0,
            $bytes.Length,
            $null,
            $null
          )
          if ($write.AsyncWaitHandle.WaitOne(50)) {
            $script:WingmanReadinessClient.EndWrite($write)
            $write.AsyncWaitHandle.Close()
          } else {
            $write.AsyncWaitHandle.Close()
            $script:WingmanReadinessClient.Dispose()
            $script:WingmanReadinessClient = $null
          }
        } catch {
          if ($null -ne $script:WingmanReadinessClient) {
            $script:WingmanReadinessClient.Dispose()
          }
          $script:WingmanReadinessClient = $null
        }
      }

      return [Microsoft.PowerShell.PSConsoleReadLine]::ReadLine(
        $host.Runspace,
        $ExecutionContext
      )
    }
  }
}

function global:Invoke-WingmanPrepared {
  [CmdletBinding()]
  param(
    [Parameter(Mandatory = $true)]
    [string] $RequestId
  )

  if ((Get-Location).Provider.Name -ne 'FileSystem') {
    [Console]::Error.WriteLine('wingman: Familiar commands require a FileSystem location')
    $global:LASTEXITCODE = 1
    return
  }

  if ($RequestId -cnotmatch '\A[0-9A-Fa-f]{32}\z') {
    [Console]::Error.WriteLine('wingman: invalid prepared request ID')
    $global:LASTEXITCODE = 2
    return
  }

  if ([string]::IsNullOrWhiteSpace($env:WINGMAN_RUNNER_PATH)) {
    [Console]::Error.WriteLine('wingman: runner path is unavailable')
    $global:LASTEXITCODE = 2
    return
  }

  & $env:WINGMAN_RUNNER_PATH $RequestId
}
