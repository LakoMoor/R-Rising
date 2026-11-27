param(
    [switch]$Check,
    [string]$Data = $(if ($env:VRISING_DATA) { $env:VRISING_DATA } else { Join-Path $PSScriptRoot '../../VRising_Data' }),
    [string]$Layout = $env:RRISING_LAYOUT_REPORT,
    [string]$Scene = $env:RRISING_SCENE,
    [string]$Position = $env:RRISING_PLAYER_POSITION,
    [string]$Python = 'python'
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$dataPath = (Resolve-Path -LiteralPath $Data).Path
$install = Split-Path $dataPath -Parent
$defaultScene = -not $Scene
if ($defaultScene) { $Scene = Join-Path $dataPath 'StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities' }
if ($defaultScene -and -not $Position) { $Position = '142.5,20,22.5' }
$expected = @{
    (Join-Path $install 'GameAssembly.dll') = 'a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56'
    (Join-Path $dataPath 'il2cpp_data/Metadata/global-metadata.dat') = 'b8f9f76ffb72a0c77079082aac8eb78da839094e9d090e8461b49230e8831271'
}
foreach ($path in $expected.Keys) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing required file: $path" }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $expected[$path]) { throw "Pinned build hash mismatch: $path" }
}
foreach ($path in @($Scene, (Join-Path $repo 'assets/typetrees.tpk'))) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing required file: $path" }
}
if ($Layout) {
    $layoutData = Get-Content -LiteralPath $Layout -Raw | ConvertFrom-Json
    if ($layoutData.binary_sha256 -ne $expected[(Join-Path $install 'GameAssembly.dll')] -or $layoutData.metadata_sha256 -ne $expected[(Join-Path $dataPath 'il2cpp_data/Metadata/global-metadata.dat')]) { throw 'Layout report does not match pinned client.' }
}
Write-Host 'Pinned client hashes verified.'
if ($Check) { Write-Host "Scene: $Scene"; return }
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo is required on PATH.' }
Push-Location $repo
try {
    if (-not $Layout) {
        if (-not (Get-Command $Python -ErrorAction SilentlyContinue)) { throw 'Python 3 is required; supply its path using -Python.' }
        New-Item -ItemType Directory -Force -Path target | Out-Null
        $Layout = Join-Path $repo 'target/client-layout.json'
        & $Python tools/probe_type_registry.py --install $install --check --output $Layout
        if ($LASTEXITCODE -ne 0) { throw 'Client layout generation failed.' }
    }
    Write-Host 'Experimental diagnostic: approximate movement, camera and face attachment; no collision, original animation state or verified spawn.'
    $arguments = @('--data', $dataPath, '--scene', $Scene, '--type-layouts', $Layout, '--archives', '100000', '--scene-live', '--spawn-player', '--experimental-motion', '--follow-player', '--animate-player')
    if ($Position) { $arguments += @('--player-position', $Position) }
    & cargo run --locked -p rrising-app -- @arguments
    if ($LASTEXITCODE -ne 0) { throw "Application failed with exit code $LASTEXITCODE." }
} finally { Pop-Location }
