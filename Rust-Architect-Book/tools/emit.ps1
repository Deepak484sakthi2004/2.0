<#
.SYNOPSIS
  Shows what rustc generates for a listing (assembly, LLVM IR, MIR, HIR, or macro expansion),
  using the Rust Playground. Used for the book's "what does the compiler emit?" sections.

.NOTES
  - Target 'hir' and 'expand' need the nightly channel (the flags behind them are unstable).
  - When inspecting a lib crate in release mode, mark small functions #[inline(never)]:
    rustc treats small leaf functions as cross-crate-inlinable and won't emit them standalone.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File tools\emit.ps1 listings\part-02\ch03-02-let-x.rs -Target asm -Mode release
  powershell -ExecutionPolicy Bypass -File tools\emit.ps1 listings\part-02\ch03-02-let-x.rs -Target mir -Mode debug
  powershell -ExecutionPolicy Bypass -File tools\emit.ps1 listings\part-02\ch06-03-derive.rs -Target expand
#>
param(
    [Parameter(Mandatory = $true)] [string] $Path,
    [ValidateSet('asm', 'llvm-ir', 'mir', 'hir', 'expand')] [string] $Target = 'asm',
    [ValidateSet('debug', 'release')] [string] $Mode = 'release',
    [string] $Channel = 'stable',
    [string] $Edition = '2024',
    [ValidateSet('lib', 'bin')] [string] $CrateType = 'lib',
    [switch] $Raw
)

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$code = [IO.File]::ReadAllText((Resolve-Path $Path))
if ($Target -in @('hir', 'expand')) { $Channel = 'nightly' }

if ($Target -eq 'expand') {
    $uri = 'https://play.rust-lang.org/macro-expansion'
    $body = @{ code = $code; edition = $Edition } | ConvertTo-Json -Compress
} else {
    $uri = 'https://play.rust-lang.org/compile'
    $body = @{
        target = $Target; assemblyFlavor = 'intel'; demangleAssembly = 'demangle'
        processAssembly = $(if ($Raw) { 'raw' } else { 'filter' })
        channel = $Channel; mode = $Mode; edition = $Edition; crateType = $CrateType
        tests = $false; backtrace = $false; code = $code
    } | ConvertTo-Json -Compress
}

$resp = Invoke-RestMethod -Uri $uri -Method Post -ContentType 'application/json; charset=utf-8' `
    -Body ([Text.Encoding]::UTF8.GetBytes($body))

Write-Host ("# {0} | target={1} mode={2} channel={3} edition={4} success={5}" -f `
    (Split-Path $Path -Leaf), $Target, $Mode, $Channel, $Edition, $resp.success)
if ($resp.code) { Write-Output $resp.code } elseif ($resp.stdout) { Write-Output $resp.stdout }
if (-not $resp.success -and $resp.stderr) { Write-Host '--- stderr ---'; Write-Host $resp.stderr }
