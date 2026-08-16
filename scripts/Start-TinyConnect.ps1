[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo was not found on PATH. Install Rust from https://rustup.rs/.'
}

& cargo run --release
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
