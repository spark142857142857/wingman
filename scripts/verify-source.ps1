[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-VerificationStep {
    param(
        [Parameter(Mandatory)]
        [string] $Name,

        [Parameter(Mandatory)]
        [string] $Executable,

        [Parameter(Mandatory)]
        [string[]] $Arguments
    )

    Write-Host "==> $Name"
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

$RepositoryRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepositoryRoot
try {
    Invoke-VerificationStep -Name 'TypeScript typecheck' -Executable 'npm.cmd' -Arguments @('run', 'typecheck')
    Invoke-VerificationStep -Name 'Automated contract tests' -Executable 'npm.cmd' -Arguments @('test')
    Invoke-VerificationStep -Name 'Production frontend build' -Executable 'npm.cmd' -Arguments @('run', 'build')
    Invoke-VerificationStep -Name 'Rust formatting' -Executable 'cargo.exe' -Arguments @('fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--all', '--', '--check')
    Invoke-VerificationStep -Name 'Rust Clippy' -Executable 'cargo.exe' -Arguments @('clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--all-features', '--', '-D', 'warnings')
}
finally {
    Pop-Location
}

Write-Host 'Deterministic source gate passed.'
