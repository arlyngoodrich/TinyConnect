[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $Arguments
)

$ErrorActionPreference = 'Stop'
$scriptDirectory = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDirectory
$releaseExecutable = Join-Path $projectRoot 'target\release\tinyconnect.exe'

if (Test-Path -LiteralPath $releaseExecutable) {
    & $releaseExecutable @Arguments
    exit $LASTEXITCODE
}

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if ($null -ne $cargo) {
    Push-Location $projectRoot
    try {
        if ($Arguments.Count -gt 0) {
            & $cargo.Source run --release -- @Arguments
        } else {
            & $cargo.Source run --release
        }
        $exitCode = $LASTEXITCODE
    } finally {
        Pop-Location
    }
    exit $exitCode
}

throw "TinyConnect release executable not found at '$releaseExecutable', and cargo is not available. Build with Rust (cargo build --release) or obtain a release executable."
