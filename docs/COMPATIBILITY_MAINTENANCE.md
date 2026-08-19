# Compatibility Maintenance Contract

Status: current operations contract; exact supported releases are selected per
product release.

## Principle

External Windows and shell updates trigger compatibility verification. They do
not silently redefine Wingman's owned P0 command meanings.

## Recommended initial support profile

| Area | Initial support |
| --- | --- |
| Windows | supported Windows 11 general-availability releases at 24H2 or later |
| Architecture | x64 |
| Shells | `cmd.exe`, Windows PowerShell 5.1 (`powershell.exe`) |
| PowerShell 7 | no support promise until it has its own launch and test matrix |
| Windows Server | no support promise initially |
| Windows 10 | excluded from new support scope |

Every Wingman release records its contract version, runner protocol version,
tested Windows releases, tested shells, architecture, and test date. The
default test matrix contains the current and immediately preceding supported
Windows release.

## Update triggers

| Change | Required verification |
| --- | --- |
| monthly Windows update | transport smoke test: launch, prompt marker, PTY, UTF-8, cwd, Ctrl+C |
| Windows feature update | full P0 contract and terminal-session/shell-integration matrix |
| Windows PowerShell 5.1 update | full PowerShell transport and editing-fallback suite |
| future PowerShell 7 support | its own versioned matrix before support is declared |
| Rust, Tauri, or terminal dependency update | reproducible build, PTY/input/output regression, and performance-budget canary |
| Wingman release | full current support matrix |

## Test layers

1. **Contract tests**: lexer, parser, catalog validation, diagnostics, and exit
   behavior without a shell.
2. **Runner filesystem tests**: temporary Windows directories exercise paths,
   redirection, read-only and access failures, and reparse-point safety.
3. **Text stream tests**: split UTF-8/BOM decoding, records, final newline,
   bounded pipeline flow, redirection, short-circuit, outcome priority, and
   partial output follow the [text record and stream
   contract](TEXT_STREAM_MODEL.md).
4. **Shell transport tests**: each supported shell verifies inherited cwd,
   `PATH`, UTF-8, streaming output, and cancellation.
5. **Terminal session tests**: prompt evidence, Unicode editing, completion
   fallback, paste, recall, foreground input, and shell transitions follow the
   [terminal submission and session contract](TERMINAL_SESSION_CONTRACT.md).
6. **Update canary**: representative P0 flows run on each supported Windows
   and shell combination.

Canaries include Korean and space-containing paths as well as hidden and
read-only files. They validate Windows path and UTF-8 transport, not merely
English ASCII happy paths.

## Triage

| Classification | Response |
| --- | --- |
| Wingman contract defect | fix runner and add a regression test |
| shell transport defect | fix the transport shim and add an integration test |
| Windows environment difference | evaluate support scope and strengthen safety rules if needed |
| native pass-through behavior | not a Wingman P0 defect |

## Change policy

- A bug fix restores the documented contract and may be a patch release.
- A new command or option expands the contract and requires a documented
  feature release.
- A changed output, exit-code, or safety meaning requires a contract-version
  change, updated tests, release notes, and user review.
- An urgent safety restriction may be made sooner, but must document why the
  previous behavior was unsafe.

Unsupported Windows or shell versions are not blocked from running Wingman,
but receive best-effort treatment and are not release-blocking test targets.

## Dependency flow

Dependency upgrades are separate from P0 semantic changes. Each candidate is
locked, rebuilt reproducibly, and validated by the terminal regression suite
and P0 canary before adoption.
