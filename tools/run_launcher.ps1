param([switch]$Check)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargoCommand=Get-Command cargo -ErrorAction SilentlyContinue
$cargoPath=if ($cargoCommand) {$cargoCommand.Source} else {Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'}
if (-not (Test-Path -LiteralPath $cargoPath)) {throw 'Rust/Cargo is required to build the launcher.'}
Push-Location $repo
try {
    $launchArgs=@('run','--locked','-p','rrising-launcher')
    if ($Check) {$launchArgs+=@('--','--check')}
    & $cargoPath @launchArgs
    if ($LASTEXITCODE -ne 0) {throw "Launcher failed with exit code $LASTEXITCODE"}
} finally {Pop-Location}
