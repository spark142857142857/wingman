function Test-WingmanCompat {
  if (-not $env:WINGMAN_COMPAT_FLAG) { return $false }
  try {
    return (Get-Content -LiteralPath $env:WINGMAN_COMPAT_FLAG -Raw -ErrorAction Stop).Trim() -eq '1'
  } catch {
    return $false
  }
}

function Invoke-WingmanExternal {
  param(
    [string]$Name,
    [object[]]$Arguments,
    [object[]]$PipelineItems
  )

  $external = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($external) {
    if ($PipelineItems.Count -gt 0) {
      $PipelineItems | & $external.Source @Arguments
    } else {
      & $external.Source @Arguments
    }
    return
  }

  Write-Error "$Name`: command not found"
}

function global:grep {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    Invoke-WingmanExternal 'grep' $args $pipelineItems
    return
  }

  $recursive = $false
  $ignoreCase = $false
  $fixed = $false
  $invert = $false
  $countOnly = $false
  $lineNumbers = $false
  $wordMatch = $false
  $quiet = $false
  $filesOnly = $false
  $includePatterns = @()
  $excludePatterns = @()
  $pattern = $null
  $paths = @()
  $parseOptions = $true

  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if ($parseOptions -and $argument -eq '--') {
      $parseOptions = $false
      continue
    }
    if ($parseOptions -and ($argument -eq '--include' -or $argument -eq '--exclude')) {
      if ($index + 1 -ge $args.Count) { Write-Error "grep: option $argument requires a pattern"; return }
      $index++
      if ($argument -eq '--include') { $includePatterns += [string]$args[$index] } else { $excludePatterns += [string]$args[$index] }
      continue
    }
    if ($parseOptions -and $argument -match '^--(include|exclude)=(.+)$') {
      if ($Matches[1] -eq 'include') { $includePatterns += $Matches[2] } else { $excludePatterns += $Matches[2] }
      continue
    }
    if ($parseOptions -and $argument.StartsWith('--')) {
      switch ($argument) {
        '--recursive' { $recursive = $true }
        '--ignore-case' { $ignoreCase = $true }
        '--fixed-strings' { $fixed = $true }
        '--invert-match' { $invert = $true }
        '--count' { $countOnly = $true }
        '--line-number' { $lineNumbers = $true }
        '--word-regexp' { $wordMatch = $true }
        '--quiet' { $quiet = $true }
        '--files-with-matches' { $filesOnly = $true }
        default { Write-Error "grep: unsupported option $argument"; return }
      }
      continue
    }
    if ($parseOptions -and $argument.StartsWith('-') -and $argument.Length -gt 1) {
      $flags = $argument.Substring(1)
      foreach ($flag in $flags.ToCharArray()) {
        switch ([string]$flag) {
          { $_ -in @('r', 'R') } { $recursive = $true; break }
          'i' { $ignoreCase = $true; break }
          'F' { $fixed = $true; break }
          'v' { $invert = $true; break }
          'c' { $countOnly = $true; break }
          'n' { $lineNumbers = $true; break }
          'w' { $wordMatch = $true; break }
          'q' { $quiet = $true; break }
          'l' { $filesOnly = $true; break }
          default { Write-Error "grep: unsupported option -$flag"; return }
        }
      }
      continue
    }
    if ($null -eq $pattern) { $pattern = $argument } else { $paths += $argument }
  }

  if ($null -eq $pattern) {
    Write-Error 'grep: missing search pattern'
    return
  }

  $selectPattern = $pattern
  if ($wordMatch) {
    $wordPattern = if ($fixed) { [regex]::Escape($pattern) } else { $pattern }
    $selectPattern = "\b(?:$wordPattern)\b"
    $fixed = $false
  }
  $selectArguments = @{ Pattern = $selectPattern }
  if (-not $ignoreCase) { $selectArguments.CaseSensitive = $true }
  if ($fixed) { $selectArguments.SimpleMatch = $true }
  if ($invert) { $selectArguments.NotMatch = $true }

  $multipleFiles = $false
  if ($recursive) {
    if ($paths.Count -eq 0) { $paths = @('.') }
    $searchFiles = @()
    foreach ($path in $paths) {
      if (Test-Path -LiteralPath $path -PathType Leaf) {
        $searchFiles += (Get-Item -LiteralPath $path).FullName
      } else {
        $searchFiles += Get-ChildItem -Path $path -File -Recurse -Force -ErrorAction SilentlyContinue |
          Select-Object -ExpandProperty FullName
      }
    }
    if ($includePatterns.Count -gt 0) {
      $searchFiles = @($searchFiles | Where-Object {
        $name = Split-Path $_ -Leaf
        @($includePatterns | Where-Object { $name -like $_ }).Count -gt 0
      })
    }
    if ($excludePatterns.Count -gt 0) {
      $searchFiles = @($searchFiles | Where-Object {
        $name = Split-Path $_ -Leaf
        @($excludePatterns | Where-Object { $name -like $_ }).Count -eq 0
      })
    }
    $multipleFiles = $true
    $matches = if ($searchFiles.Count -gt 0) { @(Select-String @selectArguments -Path $searchFiles) } else { @() }
  } elseif ($paths.Count -gt 0) {
    $matches = @(Select-String @selectArguments -Path $paths)
    $multipleFiles = @($matches | Select-Object -ExpandProperty Path -Unique).Count -gt 1
  } elseif ($pipelineItems.Count -gt 0) {
    $matches = @($pipelineItems | Select-String @selectArguments)
  } else {
    Write-Error 'grep: missing file or pipeline input'
    return
  }

  $global:LASTEXITCODE = if ($matches.Count -gt 0) { 0 } else { 1 }
  if ($quiet) { return }
  if ($filesOnly) {
    $matches | Where-Object { $_.Path -and $_.Path -ne 'InputStream' } | Select-Object -ExpandProperty Path -Unique
    return
  }
  if ($countOnly) { $matches.Count; return }

  foreach ($match in $matches) {
    $prefix = ''
    if ($multipleFiles -and $match.Path -and $match.Path -ne 'InputStream') { $prefix += "$($match.Path):" }
    if ($lineNumbers) { $prefix += "$($match.LineNumber):" }
    "$prefix$($match.Line)"
  }
}

function global:head {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    Invoke-WingmanExternal 'head' $args $pipelineItems
    return
  }

  $lineCount = 10
  $paths = @()
  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if ($argument -eq '-n' -and $index + 1 -lt $args.Count) {
      $index++
      $lineCount = [Math]::Max(0, [int]$args[$index])
    } elseif ($argument -match '^-(\d+)$') {
      $lineCount = [int]$Matches[1]
    } else {
      $paths += $argument
    }
  }

  if ($paths.Count -gt 0) {
    Get-Content -Path $paths | Select-Object -First $lineCount
  } else {
    $pipelineItems | Select-Object -First $lineCount
  }
}

function global:tail {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    Invoke-WingmanExternal 'tail' $args $pipelineItems
    return
  }

  $lineCount = 10
  $follow = $false
  $paths = @()
  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if ($argument -eq '-n' -and $index + 1 -lt $args.Count) {
      $index++
      $lineCount = [Math]::Max(0, [int]$args[$index])
    } elseif ($argument -match '^-(\d+)$') {
      $lineCount = [int]$Matches[1]
    } elseif ($argument -eq '-f') {
      $follow = $true
    } else {
      $paths += $argument
    }
  }

  if ($paths.Count -gt 0) {
    if ($follow) { Get-Content -Path $paths -Tail $lineCount -Wait } else { Get-Content -Path $paths -Tail $lineCount }
  } else {
    $pipelineItems | Select-Object -Last $lineCount
  }
}

function global:find {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    Invoke-WingmanExternal 'find' $args $pipelineItems
    return
  }

  $roots = @()
  $namePattern = $null
  $ignoreNameCase = $false
  $itemType = $null
  $minDepth = 1
  $maxDepth = [int]::MaxValue
  $sizeRule = $null
  $mtimeRule = $null
  $parsingPredicates = $false
  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if (-not $parsingPredicates -and -not $argument.StartsWith('-')) {
      $roots += $argument
      continue
    }
    $parsingPredicates = $true
    if ($argument -in @('-name', '-iname', '-type', '-mindepth', '-maxdepth', '-size', '-mtime')) {
      if ($index + 1 -ge $args.Count) { Write-Error "find: predicate $argument requires a value"; return }
      $index++
      $value = [string]$args[$index]
      switch ($argument) {
        '-name' { $namePattern = $value; $ignoreNameCase = $false }
        '-iname' { $namePattern = $value; $ignoreNameCase = $true }
        '-type' {
          if ($value -notin @('f', 'd')) { Write-Error "find: unsupported type $value"; return }
          $itemType = $value
        }
        '-mindepth' { $minDepth = [Math]::Max(0, [int]$value) }
        '-maxdepth' { $maxDepth = [Math]::Max(0, [int]$value) }
        '-size' { $sizeRule = $value }
        '-mtime' { $mtimeRule = $value }
      }
    } elseif ($argument -eq '-print') {
      continue
    } else {
      Write-Error "find: unsupported predicate $argument"
      return
    }
  }
  if ($roots.Count -eq 0) { $roots = @('.') }

  foreach ($root in $roots) {
    $rootItem = Get-Item -LiteralPath $root -ErrorAction SilentlyContinue
    if (-not $rootItem) { continue }
    $rootPath = $rootItem.FullName.TrimEnd('\', '/')
    Get-ChildItem -LiteralPath $rootPath -Recurse -Force -ErrorAction SilentlyContinue |
      Where-Object {
        $relative = $_.FullName.Substring($rootPath.Length).TrimStart('\', '/')
        $depth = @($relative -split '[\\/]' | Where-Object { $_ }).Count
        $nameMatches = $null -eq $namePattern -or
          ($ignoreNameCase -and $_.Name -like $namePattern) -or
          (-not $ignoreNameCase -and $_.Name -clike $namePattern)
        $typeMatches = $null -eq $itemType -or
          ($itemType -eq 'f' -and -not $_.PSIsContainer) -or
          ($itemType -eq 'd' -and $_.PSIsContainer)

        $sizeMatches = $true
        if ($null -ne $sizeRule) {
          if ($sizeRule -notmatch '^([+-]?)(\d+)([cCkKmMgG]?)$') { Write-Error "find: invalid size $sizeRule"; return }
          $comparison = $Matches[1]
          $sizeValue = [double]$Matches[2]
          $unit = $Matches[3].ToLowerInvariant()
          $multiplier = switch ($unit) { 'c' { 1 } 'k' { 1KB } 'm' { 1MB } 'g' { 1GB } default { 512 } }
          $expectedBytes = $sizeValue * $multiplier
          $actualBytes = if ($_.PSIsContainer) { 0 } else { [double]$_.Length }
          $sizeMatches = if ($comparison -eq '+') { $actualBytes -gt $expectedBytes } elseif ($comparison -eq '-') { $actualBytes -lt $expectedBytes } else { $actualBytes -ge $expectedBytes -and $actualBytes -lt ($expectedBytes + $multiplier) }
        }

        $mtimeMatches = $true
        if ($null -ne $mtimeRule) {
          if ($mtimeRule -notmatch '^([+-]?)(\d+)$') { Write-Error "find: invalid mtime $mtimeRule"; return }
          $comparison = $Matches[1]
          $days = [int]$Matches[2]
          $ageDays = [Math]::Floor(((Get-Date) - $_.LastWriteTime).TotalDays)
          $mtimeMatches = if ($comparison -eq '+') { $ageDays -gt $days } elseif ($comparison -eq '-') { $ageDays -lt $days } else { $ageDays -eq $days }
        }

        $depth -ge $minDepth -and $depth -le $maxDepth -and $nameMatches -and $typeMatches -and $sizeMatches -and $mtimeMatches
      } |
      ForEach-Object { $_.FullName }
  }
}

function global:Invoke-WingmanSort {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    $pipelineItems | Sort-Object @args
    return
  }

  $descending = $false
  $numeric = $false
  $unique = $false
  $paths = @()
  foreach ($argumentValue in $args) {
    $argument = [string]$argumentValue
    if ($argument.StartsWith('-')) {
      if ($argument -match 'r') { $descending = $true }
      if ($argument -match 'n') { $numeric = $true }
      if ($argument -match 'u') { $unique = $true }
    } else {
      $paths += $argument
    }
  }
  $items = if ($paths.Count -gt 0) { Get-Content -Path $paths } else { $pipelineItems }
  if ($numeric) {
    $items | Sort-Object { [double]$_ } -Descending:$descending -Unique:$unique
  } else {
    $items | Sort-Object -Descending:$descending -Unique:$unique
  }
}

function global:uniq {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    Invoke-WingmanExternal 'uniq' $args $pipelineItems
    return
  }

  $showCount = $args -contains '-c'
  $paths = @($args | Where-Object { -not ([string]$_).StartsWith('-') })
  $items = if ($paths.Count -gt 0) { @(Get-Content -Path $paths) } else { $pipelineItems }
  $previous = $null
  $count = 0
  foreach ($itemValue in $items) {
    $item = [string]$itemValue
    if ($count -eq 0 -or $item -eq $previous) {
      $previous = $item
      $count++
      continue
    }
    if ($showCount) { '{0,7} {1}' -f $count, $previous } else { $previous }
    $previous = $item
    $count = 1
  }
  if ($count -gt 0) {
    if ($showCount) { '{0,7} {1}' -f $count, $previous } else { $previous }
  }
}

function global:wc {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    Invoke-WingmanExternal 'wc' $args $pipelineItems
    return
  }

  $lineOnly = $args -contains '-l'
  $wordOnly = $args -contains '-w'
  $charOnly = $args -contains '-c'
  $paths = @($args | Where-Object { -not ([string]$_).StartsWith('-') })
  $items = if ($paths.Count -gt 0) { @(Get-Content -Path $paths) } else { $pipelineItems }
  $text = $items -join [Environment]::NewLine
  $lines = $items.Count
  $words = if ([string]::IsNullOrWhiteSpace($text)) { 0 } else { @($text -split '\s+' | Where-Object { $_ }).Count }
  $characters = $text.Length
  if ($lineOnly -or $wordOnly -or $charOnly) {
    $values = @()
    if ($lineOnly) { $values += $lines }
    if ($wordOnly) { $values += $words }
    if ($charOnly) { $values += $characters }
    $values -join ' '
  } else {
    "$lines $words $characters"
  }
}

function Get-WingmanSelectionIndices {
  param([string]$Spec, [int]$Length)
  $indices = @()
  foreach ($part in $Spec.Split(',')) {
    $start = 0
    $end = 0
    if ($part -match '^(\d+)$') {
      $start = [int]$Matches[1]
      $end = $start
    } elseif ($part -match '^(\d+)-(\d+)$') {
      $start = [int]$Matches[1]
      $end = [int]$Matches[2]
    } elseif ($part -match '^(\d+)-$') {
      $start = [int]$Matches[1]
      $end = $Length
    } elseif ($part -match '^-(\d+)$') {
      $start = 1
      $end = [int]$Matches[1]
    } else {
      Write-Error "invalid range: $part"
      return
    }
    $start = [Math]::Max(1, $start)
    $end = [Math]::Min($Length, $end)
    for ($position = $start; $position -le $end; $position++) { $indices += ($position - 1) }
  }
  $indices | Select-Object -Unique
}

function global:cut {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'cut' $args $pipelineItems; return }

  $delimiter = "`t"
  $fieldSpec = $null
  $characterSpec = $null
  $suppressMissingDelimiter = $false
  $paths = @()
  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if ($argument -in @('-d', '--delimiter', '-f', '--fields', '-c', '--characters', '-b', '--bytes')) {
      if ($index + 1 -ge $args.Count) { Write-Error "cut: option $argument requires a value"; return }
      $index++
      $value = [string]$args[$index]
      switch ($argument) {
        { $_ -in @('-d', '--delimiter') } { $delimiter = $value; break }
        { $_ -in @('-f', '--fields') } { $fieldSpec = $value; break }
        default { $characterSpec = $value }
      }
    } elseif ($argument -match '^--(delimiter|fields|characters|bytes)=(.+)$') {
      switch ($Matches[1]) {
        'delimiter' { $delimiter = $Matches[2] }
        'fields' { $fieldSpec = $Matches[2] }
        default { $characterSpec = $Matches[2] }
      }
    } elseif ($argument -match '^-([dfcb])(.+)$') {
      switch ($Matches[1]) {
        'd' { $delimiter = $Matches[2] }
        'f' { $fieldSpec = $Matches[2] }
        default { $characterSpec = $Matches[2] }
      }
    } elseif ($argument -eq '-s' -or $argument -eq '--only-delimited') {
      $suppressMissingDelimiter = $true
    } elseif ($argument.StartsWith('-')) {
      Write-Error "cut: unsupported option $argument"
      return
    } else {
      $paths += $argument
    }
  }

  if (($null -eq $fieldSpec) -eq ($null -eq $characterSpec)) {
    Write-Error 'cut: specify exactly one of fields (-f) or characters (-c)'
    return
  }
  if ($null -ne $fieldSpec -and $delimiter.Length -ne 1) {
    Write-Error 'cut: delimiter must be a single character'
    return
  }

  $items = if ($paths.Count -gt 0) { @(Get-Content -Path $paths) } else { $pipelineItems }
  foreach ($itemValue in $items) {
    $line = [string]$itemValue
    if ($null -ne $fieldSpec) {
      if (-not $line.Contains($delimiter)) {
        if (-not $suppressMissingDelimiter) { $line }
        continue
      }
      $fields = [regex]::Split($line, [regex]::Escape($delimiter))
      $indices = @(Get-WingmanSelectionIndices $fieldSpec $fields.Count)
      @($indices | ForEach-Object { $fields[$_] }) -join $delimiter
    } else {
      $characters = $line.ToCharArray()
      $indices = @(Get-WingmanSelectionIndices $characterSpec $characters.Count)
      -join @($indices | ForEach-Object { $characters[$_] })
    }
  }
}

function Expand-WingmanCharacterSet {
  param([string]$Set)
  switch ($Set) {
    '[:lower:]' { return [char[]]([string]::Join('', ([char[]](97..122)))) }
    '[:upper:]' { return [char[]]([string]::Join('', ([char[]](65..90)))) }
    '[:digit:]' { return [char[]]([string]::Join('', ([char[]](48..57)))) }
    '[:space:]' { return [char[]]" `t`r`n" }
  }

  $characters = @()
  for ($index = 0; $index -lt $Set.Length; $index++) {
    $character = $Set[$index]
    if ($character -eq '\' -and $index + 1 -lt $Set.Length) {
      $index++
      $escaped = switch ($Set[$index]) { 'n' { "`n" } 'r' { "`r" } 't' { "`t" } default { $Set[$index] } }
      $characters += [char]$escaped
    } elseif ($index + 2 -lt $Set.Length -and $Set[$index + 1] -eq '-') {
      $rangeEnd = [int][char]$Set[$index + 2]
      for ($code = [int][char]$character; $code -le $rangeEnd; $code++) { $characters += [char]$code }
      $index += 2
    } else {
      $characters += [char]$character
    }
  }
  $characters
}

function global:tr {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'tr' $args $pipelineItems; return }

  $delete = $false
  $squeeze = $false
  $sets = @()
  foreach ($argumentValue in $args) {
    $argument = [string]$argumentValue
    if ($argument.StartsWith('-') -and $argument.Length -gt 1) {
      foreach ($flag in $argument.Substring(1).ToCharArray()) {
        if ($flag -eq 'd') { $delete = $true } elseif ($flag -eq 's') { $squeeze = $true } else { Write-Error "tr: unsupported option -$flag"; return }
      }
    } else {
      $sets += $argument
    }
  }
  if ($sets.Count -lt 1 -or (-not $delete -and -not $squeeze -and $sets.Count -lt 2)) { Write-Error 'tr: missing character set'; return }

  $setOne = @(Expand-WingmanCharacterSet $sets[0])
  $setTwo = if ($sets.Count -gt 1) { @(Expand-WingmanCharacterSet $sets[1]) } else { @() }
  $translation = @{}
  if (-not $delete -and $setTwo.Count -gt 0) {
    for ($index = 0; $index -lt $setOne.Count; $index++) {
      $targetIndex = [Math]::Min($index, $setTwo.Count - 1)
      $translation[[char]$setOne[$index]] = [char]$setTwo[$targetIndex]
    }
  }
  $deleteSet = [System.Collections.Generic.HashSet[char]]::new()
  if ($delete) { foreach ($character in $setOne) { $deleteSet.Add([char]$character) | Out-Null } }
  $squeezeCharacters = if ($setTwo.Count -gt 0) { $setTwo } else { $setOne }
  $squeezeSet = [System.Collections.Generic.HashSet[char]]::new()
  foreach ($character in $squeezeCharacters) { $squeezeSet.Add([char]$character) | Out-Null }

  foreach ($itemValue in $pipelineItems) {
    $builder = New-Object System.Text.StringBuilder
    $previous = $null
    foreach ($sourceCharacter in ([string]$itemValue).ToCharArray()) {
      if ($deleteSet.Contains($sourceCharacter)) { continue }
      $outputCharacter = if ($translation.ContainsKey($sourceCharacter)) { $translation[$sourceCharacter] } else { $sourceCharacter }
      if ($squeeze -and $null -ne $previous -and $outputCharacter -eq $previous -and $squeezeSet.Contains($outputCharacter)) { continue }
      [void]$builder.Append($outputCharacter)
      $previous = $outputCharacter
    }
    $builder.ToString()
  }
}

function Expand-WingmanSedReplacement {
  param([string]$Replacement, [System.Text.RegularExpressions.Match]$Match)
  $builder = New-Object System.Text.StringBuilder
  for ($index = 0; $index -lt $Replacement.Length; $index++) {
    $character = $Replacement[$index]
    if ($character -eq '\' -and $index + 1 -lt $Replacement.Length) {
      $index++
      $next = $Replacement[$index]
      if ($next -match '\d') { [void]$builder.Append($Match.Groups[[int][string]$next].Value) } else { [void]$builder.Append($next) }
    } elseif ($character -eq '&') {
      [void]$builder.Append($Match.Value)
    } else {
      [void]$builder.Append($character)
    }
  }
  $builder.ToString()
}

function global:sed {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'sed' $args $pipelineItems; return }

  $quiet = $false
  $expression = $null
  $paths = @()
  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if ($argument -eq '-n' -or $argument -eq '--quiet' -or $argument -eq '--silent') {
      $quiet = $true
    } elseif ($argument -eq '-e' -or $argument -eq '--expression') {
      if ($index + 1 -ge $args.Count) { Write-Error 'sed: option -e requires an expression'; return }
      $index++
      if ($null -ne $expression) { Write-Error 'sed: multiple expressions are not supported yet'; return }
      $expression = [string]$args[$index]
    } elseif ($null -eq $expression) {
      $expression = $argument
    } else {
      $paths += $argument
    }
  }
  if ($null -eq $expression) { Write-Error 'sed: missing expression'; return }
  $items = if ($paths.Count -gt 0) { @(Get-Content -Path $paths) } else { $pipelineItems }

  if ($expression -match '^/(.*)/([dp])$') {
    $regex = New-Object System.Text.RegularExpressions.Regex($Matches[1])
    $operation = $Matches[2]
    foreach ($itemValue in $items) {
      $line = [string]$itemValue
      $matched = $regex.IsMatch($line)
      if ($operation -eq 'd') {
        if (-not $matched -and -not $quiet) { $line }
      } else {
        if (-not $quiet) { $line }
        if ($matched) { $line }
      }
    }
    return
  }

  if (-not $expression.StartsWith('s') -or $expression.Length -lt 4) { Write-Error "sed: unsupported expression $expression"; return }
  $delimiter = $expression[1]
  $parts = @()
  $current = ''
  $escaped = $false
  for ($index = 2; $index -lt $expression.Length; $index++) {
    $character = $expression[$index]
    if ($escaped) {
      if ($character -eq $delimiter) { $current += $character } else { $current += '\' + $character }
      $escaped = $false
    } elseif ($character -eq '\') {
      $escaped = $true
    } elseif ($character -eq $delimiter -and $parts.Count -lt 2) {
      $parts += $current
      $current = ''
    } else {
      $current += $character
    }
  }
  $parts += $current
  if ($parts.Count -ne 3) { Write-Error "sed: invalid substitution $expression"; return }
  $pattern = $parts[0]
  $replacement = $parts[1]
  $flags = $parts[2]
  foreach ($flag in $flags.ToCharArray()) {
    if ($flag -notin @('g', 'i', 'p')) { Write-Error "sed: unsupported substitution flag $flag"; return }
  }
  $options = if ($flags.Contains('i')) { [System.Text.RegularExpressions.RegexOptions]::IgnoreCase } else { [System.Text.RegularExpressions.RegexOptions]::None }
  $regex = New-Object System.Text.RegularExpressions.Regex($pattern, $options)
  $replaceAll = $flags.Contains('g')
  $printMatch = $flags.Contains('p')
  foreach ($itemValue in $items) {
    $line = [string]$itemValue
    $matched = $regex.IsMatch($line)
    $evaluator = [System.Text.RegularExpressions.MatchEvaluator]{ param($match) Expand-WingmanSedReplacement $replacement $match }
    $rendered = if ($replaceAll) { $regex.Replace($line, $evaluator) } else { $regex.Replace($line, $evaluator, 1) }
    if (-not $quiet) { $rendered }
    if ($printMatch -and $matched) { $rendered }
  }
}

function Split-WingmanWords {
  param([string]$Text)
  foreach ($match in [regex]::Matches($Text, '(?:"[^"]*"|''[^'']*''|\S+)')) {
    $value = $match.Value
    if (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'"))) {
      $value = $value.Substring(1, $value.Length - 2)
    }
    $value
  }
}

function global:xargs {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'xargs' $args $pipelineItems; return }

  $maxArguments = [int]::MaxValue
  $placeholder = $null
  $nullDelimited = $false
  $noRunIfEmpty = $false
  $command = $null
  $commandArguments = @()
  for ($index = 0; $index -lt $args.Count; $index++) {
    $argument = [string]$args[$index]
    if ($null -ne $command) { $commandArguments += $argument; continue }
    if ($argument -in @('-n', '--max-args', '-I', '--replace')) {
      if ($index + 1 -ge $args.Count) { Write-Error "xargs: option $argument requires a value"; return }
      $index++
      if ($argument -in @('-n', '--max-args')) { $maxArguments = [Math]::Max(1, [int]$args[$index]) } else { $placeholder = [string]$args[$index] }
    } elseif ($argument -match '^-n(\d+)$') {
      $maxArguments = [Math]::Max(1, [int]$Matches[1])
    } elseif ($argument -match '^-I(.+)$') {
      $placeholder = $Matches[1]
    } elseif ($argument -eq '-0' -or $argument -eq '--null') {
      $nullDelimited = $true
    } elseif ($argument -eq '-r' -or $argument -eq '--no-run-if-empty') {
      $noRunIfEmpty = $true
    } elseif ($argument.StartsWith('-')) {
      Write-Error "xargs: unsupported option $argument"
      return
    } else {
      $command = $argument
    }
  }

  $records = if ($nullDelimited) {
    @(($pipelineItems -join "`n").Split([char]0) | Where-Object { $_ -ne '' })
  } elseif ($null -ne $placeholder) {
    @($pipelineItems | ForEach-Object { [string]$_ })
  } else {
    @(Split-WingmanWords ($pipelineItems -join ' '))
  }
  if ($records.Count -eq 0) {
    if (-not $noRunIfEmpty -and $null -eq $command) { '' }
    return
  }

  if ($null -ne $placeholder) {
    foreach ($record in $records) {
      $invocationArguments = @($commandArguments | ForEach-Object { ([string]$_).Replace($placeholder, $record) })
      if ($null -eq $command) { $invocationArguments -join ' ' } else { & $command @invocationArguments }
    }
    return
  }

  for ($offset = 0; $offset -lt $records.Count; $offset += $maxArguments) {
    $last = [Math]::Min($records.Count - 1, $offset + $maxArguments - 1)
    $batch = @($records[$offset..$last])
    if ($null -eq $command) { $batch -join ' ' } else { & $command @commandArguments @batch }
  }
}

function global:cat {
  $pipelineItems = @($input)
  if (-not (Test-WingmanCompat)) {
    if ($args.Count -gt 0) { Get-Content @args } else { $pipelineItems }
    return
  }

  $numberLines = $args -contains '-n'
  $paths = @($args | Where-Object { $_ -ne '-n' })
  $items = if ($paths.Count -gt 0) { @(Get-Content -Path $paths) } else { $pipelineItems }
  if ($numberLines) {
    for ($index = 0; $index -lt $items.Count; $index++) { "{0,6}`t{1}" -f ($index + 1), $items[$index] }
  } else {
    $items
  }
}

function global:ls {
  if (-not (Test-WingmanCompat)) { Get-ChildItem @args; return }
  $force = $false
  $paths = @()
  foreach ($argumentValue in $args) {
    $argument = [string]$argumentValue
    if ($argument.StartsWith('-')) {
      if ($argument -match 'a') { $force = $true }
    } else {
      $paths += $argument
    }
  }
  if ($paths.Count -eq 0) { $paths = @('.') }
  Get-ChildItem -Path $paths -Force:$force
}

function global:ll {
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'll' $args @(); return }
  ls -la @args
}

function global:touch {
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'touch' $args @(); return }
  if ($args.Count -eq 0) { Write-Error 'touch: missing file'; return }
  foreach ($path in $args) {
    if (Test-Path -LiteralPath $path) {
      (Get-Item -LiteralPath $path).LastWriteTime = Get-Date
    } else {
      New-Item -ItemType File -Path $path | Out-Null
    }
  }
}

function global:which {
  if (-not (Test-WingmanCompat)) { Invoke-WingmanExternal 'which' $args @(); return }
  if ($args.Count -eq 0) { Write-Error 'which: missing command'; return }
  Get-Command @args | Select-Object -ExpandProperty Source
}

function global:mkdir {
  if (-not (Test-WingmanCompat)) { New-Item -ItemType Directory @args; return }
  $paths = @($args | Where-Object { $_ -ne '-p' })
  if ($paths.Count -eq 0) { Write-Error 'mkdir: missing directory'; return }
  foreach ($path in $paths) { New-Item -ItemType Directory -Path $path -Force | Out-Null }
}

function global:rm {
  if (-not (Test-WingmanCompat)) { Remove-Item @args; return }
  $recurse = $false
  $force = $false
  $paths = @()
  foreach ($argumentValue in $args) {
    $argument = [string]$argumentValue
    if ($argument.StartsWith('-')) {
      if ($argument -match 'r|R') { $recurse = $true }
      if ($argument -match 'f') { $force = $true }
    } else {
      $paths += $argument
    }
  }
  if ($paths.Count -eq 0) { Write-Error 'rm: missing target'; return }
  Remove-Item -Path $paths -Recurse:$recurse -Force:$force
}

Remove-Item Alias:cat -Force -ErrorAction SilentlyContinue
Remove-Item Alias:ls -Force -ErrorAction SilentlyContinue
Remove-Item Alias:rm -Force -ErrorAction SilentlyContinue
Set-Alias -Name sort -Value Invoke-WingmanSort -Option AllScope -Force
