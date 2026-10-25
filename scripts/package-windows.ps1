# Builds the Windows release: a portable zip and a per-user MSI (ADR 0017).
#
#   scripts/package-windows.ps1 [-SkipBuild] [-SkipMsi]
#
# Output goes to target/release-artifacts. The MSI needs the WiX v5 tool on PATH
# (`dotnet tool install --global wix --version 5.0.2`); -SkipMsi makes only the zip.
param(
    [switch]$SkipBuild,
    [switch]$SkipMsi
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# The version all crates share, from [workspace.package] in Cargo.toml.
$manifest = Get-Content -Raw Cargo.toml
if ($manifest -notmatch '(?ms)^\[workspace\.package\].*?^version = "([^"]+)"') {
    throw 'No version in [workspace.package] of Cargo.toml'
}
$version = $Matches[1]

if (-not $SkipBuild) {
    cargo build --release --locked -p focus-daemon -p focus-ui -p focus-nmh
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}

$out = Join-Path $root 'target\release-artifacts'
$name = "Focus-$version-windows-x64"
$stage = Join-Path $out $name
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $stage | Out-Null

foreach ($exe in 'focus-daemon', 'focus-ui', 'focus-nmh') {
    Copy-Item "target\release\$exe.exe" $stage
}
Copy-Item LICENSE-MIT, LICENSE-APACHE $stage
# The bundled IBM Plex fonts are compiled into focus-ui.exe; their licence ships beside it.
Copy-Item apps\focus-ui\fonts\OFL.txt $stage

# The zip holds one folder, so unzipping never scatters files.
Compress-Archive -Path $stage -DestinationPath (Join-Path $out "$name.zip")

if (-not $SkipMsi) {
    $msi = Join-Path $out "$name.msi"
    wix build installer\windows\focus.wxs -arch x64 -d "Version=$version" -d "Stage=$stage" -o $msi
    if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }
    Remove-Item (Join-Path $out "$name.wixpdb") -ErrorAction SilentlyContinue
}

Get-ChildItem $out -File | ForEach-Object { "$($_.Name)  $([math]::Round($_.Length / 1MB, 1)) MB" }
