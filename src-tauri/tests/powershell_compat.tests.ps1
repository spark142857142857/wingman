$ErrorActionPreference = 'Stop'
$profilePath = Join-Path $PSScriptRoot '..\src\powershell_compat.ps1'
$flagPath = Join-Path $env:TEMP "wingman-compat-test-$PID.flag"
$sandbox = Join-Path $env:TEMP "wingman-compat-test-$PID"
$env:WINGMAN_COMPAT_FLAG = $flagPath

function Assert-Equal($Expected, $Actual, [string]$Label) {
  if ($Expected -ne $Actual) {
    throw "$Label expected '$Expected', got '$Actual'"
  }
}

try {
  New-Item -ItemType Directory -Path (Join-Path $sandbox 'nested') -Force | Out-Null
  $textPath = Join-Path $sandbox 'sample text.txt'
  $typescriptPath = Join-Path $sandbox 'nested\sample.ts'
  $logPath = Join-Path $sandbox 'nested\ignored.log'
  Set-Content -LiteralPath $textPath -Value @('Alpha', 'Beta', 'ALPHA', 'TODO', 'TODOs')
  Set-Content -LiteralPath $typescriptPath -Value @('const value = "TODO";', 'done')
  Set-Content -LiteralPath $logPath -Value 'TODO'

  Set-Content -LiteralPath $flagPath -NoNewline -Value '1'
  . $profilePath

  $firstMatch = 'Alpha', 'beta', 'ALPHA' | grep -i alpha | head -n 1
  Assert-Equal 'Alpha' $firstMatch 'grep | head'
  Assert-Equal 'alpha' ('Alpha', 'alpha' | grep alpha) 'grep is case-sensitive by default'
  Assert-Equal 'a.b' ('axb', 'a.b' | grep -F 'a.b') 'grep fixed string'
  Assert-Equal 'keep' ('drop', 'keep' | grep -v drop) 'grep invert match'
  Assert-Equal 2 ('Alpha', 'ALPHA', 'beta' | grep -ic alpha) 'grep count'
  Assert-Equal '2:Beta' (grep -n Beta $textPath) 'grep line number'
  Assert-Equal 'TODO' ('TODO', 'TODOs' | grep -w TODO) 'grep whole word'

  $matchingFiles = @(grep -rl TODO $sandbox --include '*.ts')
  Assert-Equal 1 $matchingFiles.Count 'grep recursive include count'
  Assert-Equal $typescriptPath $matchingFiles[0] 'grep recursive include path'
  $excludedFiles = @(grep -rl TODO $sandbox --exclude '*.txt' --exclude '*.ts')
  Assert-Equal 1 $excludedFiles.Count 'grep recursive exclude count'
  Assert-Equal $logPath $excludedFiles[0] 'grep recursive exclude path'

  $quietOutput = @(grep -q missing $textPath)
  Assert-Equal 0 $quietOutput.Count 'grep quiet output'
  Assert-Equal 1 $global:LASTEXITCODE 'grep no-match exit code'

  $uniqueCounts = @('3', '1', '1', '2' | sort -n | uniq -c)
  Assert-Equal 3 $uniqueCounts.Count 'sort | uniq count'
  Assert-Equal '      2 1' $uniqueCounts[0] 'uniq duplicate count'

  $wordCount = 'one two', 'three' | wc -l -w
  Assert-Equal '2 3' $wordCount 'wc pipeline'

  Assert-Equal 'a,c' ('a,b,c' | cut -d ',' -f '1,3') 'cut fields'
  Assert-Equal 'bcd' ('abcdef' | cut -c '2-4') 'cut character range'
  Assert-Equal 'ABC123' ('abc123' | tr 'a-z' 'A-Z') 'tr range translation'
  Assert-Equal 'ABC' ('abc' | tr '[:lower:]' '[:upper:]') 'tr character classes'
  Assert-Equal 'abc' ('a1b2c3' | tr -d '0-9') 'tr delete'
  Assert-Equal 'ab' ('aaab' | tr -s 'a') 'tr squeeze'

  Assert-Equal 'bar foo' ('foo foo' | sed 's/foo/bar/') 'sed first substitution'
  Assert-Equal 'bar bar' ('foo foo' | sed 's/foo/bar/g') 'sed global substitution'
  Assert-Equal 'bar' ('FOO' | sed 's/foo/bar/i') 'sed ignore case'
  Assert-Equal '123-abc' ('abc123' | sed 's/([a-z]+)([0-9]+)/\2-\1/') 'sed capture groups'
  Assert-Equal 'keep' @('drop', 'keep' | sed '/drop/d') 'sed delete matching line'
  Assert-Equal 'bar' @('foo', 'other' | sed -n 's/foo/bar/p') 'sed quiet print'

  function Join-TestArguments { $args -join ':' }
  Assert-Equal 'one two three' @('one two', 'three' | xargs) 'xargs default echo'
  $xargsBatches = @('one two', 'three' | xargs -n 2 Join-TestArguments)
  Assert-Equal 'one:two' $xargsBatches[0] 'xargs first batch'
  Assert-Equal 'three' $xargsBatches[1] 'xargs second batch'
  Assert-Equal 'prefix=value:suffix' @('value' | xargs -I '{}' Join-TestArguments 'prefix={}' 'suffix') 'xargs placeholder'
  Assert-Equal ';Write-Output HACK' @(';Write-Output HACK' | xargs -I '{}' Join-TestArguments '{}') 'xargs does not evaluate input'

  $typescriptFiles = @(find $sandbox -name '*.ts' -type f)
  Assert-Equal 1 $typescriptFiles.Count 'find name and type'
  Assert-Equal $typescriptPath $typescriptFiles[0] 'find TypeScript path'
  Assert-Equal 1 @(find $sandbox -iname '*.TS' -type f).Count 'find case-insensitive name'
  Assert-Equal 1 @(find $sandbox -maxdepth 1 -type f).Count 'find maxdepth'
  Assert-Equal 2 @(find $sandbox -mindepth 2 -type f).Count 'find mindepth'
  Assert-Equal 2 @(find $sandbox -type f -size +10c).Count 'find size'
  Assert-Equal 3 @(find $sandbox -type f -mtime 0).Count 'find mtime'

  $tailResult = @('one', 'two', 'three' | tail -n 2)
  Assert-Equal 'two' $tailResult[0] 'tail first line'
  Assert-Equal 'three' $tailResult[1] 'tail last line'

  Assert-Equal "     1`tAlpha" (cat -n $textPath | head -n 1) 'cat line numbers'

  $createdDirectory = Join-Path $sandbox 'created\deep'
  mkdir -p $createdDirectory
  Assert-Equal $true (Test-Path -LiteralPath $createdDirectory -PathType Container) 'mkdir -p'
  $createdFile = Join-Path $createdDirectory 'touch.txt'
  touch $createdFile
  Assert-Equal $true (Test-Path -LiteralPath $createdFile -PathType Leaf) 'touch'
  rm -rf (Join-Path $sandbox 'created')
  Assert-Equal $false (Test-Path -LiteralPath (Join-Path $sandbox 'created')) 'rm -rf'

  Set-Content -LiteralPath $flagPath -NoNewline -Value '0'
  Assert-Equal $false (Test-WingmanCompat) 'compat off'

  Write-Output 'PowerShell compatibility tests passed.'
} finally {
  Remove-Item -LiteralPath $flagPath -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
}
