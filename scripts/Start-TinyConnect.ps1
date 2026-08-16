[CmdletBinding()]
param(
    [switch] $Inner,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $Arguments
)

$ErrorActionPreference = 'Stop'
$scriptDirectory = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectRoot = Split-Path -Parent $scriptDirectory
$releaseExecutable = Join-Path $projectRoot 'target\release\tinyconnect.exe'

if (-not $Inner) {
    $wt = Get-Command wt.exe -ErrorAction SilentlyContinue
    if ($null -ne $wt) {
        $shellPath = (Get-Process -Id $PID).Path
        if ([string]::IsNullOrWhiteSpace($shellPath)) {
            $shellPath = if ($PSEdition -eq 'Core') { 'pwsh.exe' } else { 'powershell.exe' }
        }

        & $wt.Source '--window' 'new' '--size' '108,27' 'new-tab' $shellPath '-NoLogo' '-NoProfile' '-ExecutionPolicy' 'Bypass' '-File' $PSCommandPath '-Inner' @Arguments
        if ($LASTEXITCODE -eq 0) {
            exit 0
        }
    }
}

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
