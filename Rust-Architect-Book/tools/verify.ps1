<#
.SYNOPSIS
  Compiles (and runs) book listings on the Rust Playground and checks every
  "// verify:" header line. No local Rust toolchain is needed.

.DESCRIPTION
  Header format (one or more lines anywhere in the file):

      // verify: <mode> <outcome> [needle]

  mode     debug | release, optionally with an edition override: debug@2021
           and, for miri/miri-ok, "+tree" to use Tree Borrows instead of Stacked Borrows: debug+tree
           and "+nightly" to compile that one check on the nightly channel: debug+nightly
  outcome  ok              must compile and exit 0 (stdout is printed)
           build           must compile (built as a lib, never run: for code that hangs by design)
           test            must compile and pass `cargo test` (the #[cfg(test)] tests in the file)
           panic <needle>  must compile, then panic with <needle> in stderr
           crash <needle>  must compile, then fail at run time (abort/signal) with <needle> in stderr
           error:E0502     must FAIL to compile with error[E0502]
           error:<word>    must FAIL to compile, stderr containing <word> (lint names, "reserved", ...)
           miri <needle>   run under Miri (nightly); must report Undefined Behavior with <needle> in stderr
           miri-ok         run under Miri; must finish with no UB reported

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\verify.ps1 listings\part-01
  powershell -ExecutionPolicy Bypass -File tools\verify.ps1 listings\part-01\ch03-02-drop-order.rs -ShowStderr
#>
param(
    [Parameter(Mandatory = $true)] [string] $Path,
    [string] $Channel = 'stable',
    [string] $Edition = '2024',
    [switch] $ShowStderr
)

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$files = if (Test-Path $Path -PathType Container) {
    Get-ChildItem $Path -Filter *.rs | Sort-Object Name
} else {
    Get-Item $Path
}

$versions = Invoke-RestMethod -Uri 'https://play.rust-lang.org/meta/versions'
Write-Host ("Rust Playground {0}: rustc {1}, edition {2}" -f $Channel, $versions.$Channel.rustc.version, $Edition)
Write-Host ''

$failed = 0
foreach ($file in $files) {
    $code = [IO.File]::ReadAllText($file.FullName)
    $checks = [regex]::Matches($code, '(?m)^// verify: (\S+) (\S+)(?: (.*))?\r?$')
    if ($checks.Count -eq 0) { Write-Host "SKIP  $($file.Name) (no verify header)"; continue }

    foreach ($c in $checks) {
        # NB: PowerShell variable names are case-insensitive, so never reuse a parameter's name here.
        # "+tree" in the mode (e.g. debug+tree) runs miri/miri-ok under Tree Borrows instead of Stacked Borrows.
        # "+nightly" (e.g. debug+nightly) compiles that check on the nightly channel (for #![feature] / rustc_attrs dumps).
        $modeRaw = $c.Groups[1].Value
        $aliasing = 'stacked'
        if ($modeRaw.Contains('+tree')) { $aliasing = 'tree'; $modeRaw = $modeRaw.Replace('+tree', '') }
        $checkChannel = $Channel
        if ($modeRaw.Contains('+nightly')) { $checkChannel = 'nightly'; $modeRaw = $modeRaw.Replace('+nightly', '') }
        $modeSpec = $modeRaw.Split('@')
        $checkMode = $modeSpec[0]
        $checkEdition = if ($modeSpec.Count -gt 1) { $modeSpec[1] } else { $Edition }
        $outcome = $c.Groups[2].Value
        $needle = $c.Groups[3].Value.Trim()
        $crateType = if ($outcome -eq 'build') { 'lib' } else { 'bin' }

        $body = @{
            channel = $checkChannel; mode = $checkMode; edition = $checkEdition; crateType = $crateType
            tests = ($outcome -eq 'test'); backtrace = $false; code = $code
        } | ConvertTo-Json -Compress
        $uri = 'https://play.rust-lang.org/execute'
        if ($outcome -eq 'miri' -or $outcome -eq 'miri-ok') {
            $uri = 'https://play.rust-lang.org/miri'
            $body = @{ code = $code; edition = $checkEdition; tests = $false; aliasingModel = $aliasing } | ConvertTo-Json -Compress
        }
        # Decode the response as UTF-8 explicitly (Windows PowerShell 5.1 guesses Latin-1 otherwise).
        $raw = Invoke-WebRequest -UseBasicParsing -Uri $uri -Method Post `
            -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body))
        $resp = [Text.Encoding]::UTF8.GetString($raw.RawContentStream.ToArray()) | ConvertFrom-Json
        $stderr = [string]$resp.stderr

        if ($outcome -eq 'ok' -or $outcome -eq 'build' -or $outcome -eq 'test' -or $outcome -eq 'miri-ok') {
            $pass = [bool]$resp.success
        } elseif ($outcome -eq 'miri') {
            $pass = (-not $resp.success) -and $stderr.Contains('Undefined Behavior') -and $stderr.Contains($needle)
        } elseif ($outcome -eq 'panic') {
            $pass = (-not $resp.success) -and $stderr.Contains('panicked') -and $stderr.Contains($needle)
        } elseif ($outcome -eq 'crash') {
            $pass = (-not $resp.success) -and $stderr.Contains($needle)
        } elseif ($outcome -match '^error:(E\d{4})$') {
            $pass = (-not $resp.success) -and $stderr.Contains("error[$($Matches[1])]")
        } elseif ($outcome -match '^error:(\w+)$') {
            $pass = (-not $resp.success) -and $stderr.Contains($Matches[1])
        } else {
            $pass = $false
            $stderr = "unknown outcome '$outcome'`n" + $stderr
        }

        $label = if ($pass) { 'PASS' } else { 'FAIL' }
        if (-not $pass) { $failed++ }
        Write-Host ("{0}  {1}  [{2} {3}{4}]" -f $label, $file.Name, $c.Groups[1].Value, $outcome, $(if ($needle) { " '$needle'" } else { '' }))

        # Compiler diagnostics without cargo's progress lines.
        $diag = ($stderr -split "`n" | Where-Object { $_ -notmatch '^\s*(Compiling|Finished|Running|Blocking|Downloaded|Updating)\b' }) -join "`n"
        if ($resp.stdout) { Write-Host ('      stdout | ' + (([string]$resp.stdout).TrimEnd() -replace "`n", "`n      stdout | ")) }
        if ($ShowStderr -or -not $pass -or $outcome -ne 'ok') {
            if ($diag.Trim()) { Write-Host ('      stderr | ' + ($diag.TrimEnd() -replace "`n", "`n      stderr | ")) }
        } elseif ($diag -match 'warning:') {
            Write-Host '      (compiled with warnings; rerun with -ShowStderr)'
        }
    }
}

Write-Host ''
if ($failed -gt 0) { Write-Host "$failed check(s) FAILED"; exit 1 } else { Write-Host 'All checks passed.' }
