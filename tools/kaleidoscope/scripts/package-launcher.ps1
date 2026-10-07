param(
    [string]$NativeRoot = (Join-Path $env:LOCALAPPDATA 'osulazer/current'),
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
$native = (Resolve-Path -LiteralPath $NativeRoot).Path
if (!$OutputDirectory) {
    $OutputDirectory = Join-Path $repo ('data-kaleidoscope-package/' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N'))
}
$output = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $output) { throw 'Package output already exists; choose a new output directory.' }
foreach ($required in @('osu.Game.dll','osu.Framework.dll','osu!.dll','hostfxr.dll')) {
    if (!(Test-Path -LiteralPath (Join-Path $native $required) -PathType Leaf)) { throw "Selected lazer installation is missing $required" }
}
$previousHash = [Environment]::GetEnvironmentVariable('ILLYA_BRIDGE_SHA256', 'Process')
function Assert-Build([string]$step) {
    if ($LASTEXITCODE -ne 0) { throw "$step failed; no launcher is ready to distribute." }
}
New-Item -ItemType Directory -Path $output | Out-Null
Push-Location $repo
try {
    $bridgeOutput = Join-Path $repo 'tools/transfiguration/artifacts'
    & ./tools/transfiguration/build.ps1 -NativeRoot $native
    Assert-Build 'Original connection bridge build'
    $bridge = Join-Path $bridgeOutput 'osu.Game.Rulesets.Transfiguration.dll'
    $env:ILLYA_BRIDGE_SHA256 = (Get-FileHash -LiteralPath $bridge -Algorithm SHA256).Hash.ToLowerInvariant()
    $bridgeManifest = Get-Content -LiteralPath (Join-Path $bridgeOutput 'artifact.json') -Raw | ConvertFrom-Json
    if ($bridgeManifest.protocol -ne 1 -or $bridgeManifest.fixture -ne $false -or $bridgeManifest.endpoint -ne 'https://illya.test' -or $bridgeManifest.sha256 -ne $env:ILLYA_BRIDGE_SHA256) { throw 'Production bridge manifest validation failed.' }
    & npm run build --workspace '@illyaverse/kaleidoscope'
    Assert-Build 'Launcher interface build'
    & cargo build --manifest-path apps/kaleidoscope/src-tauri/Cargo.toml --release --locked --features custom-protocol
    Assert-Build 'Native launcher build'
    $launcher = Join-Path $repo 'apps/kaleidoscope/src-tauri/target/release/kaleidoscope-desktop.exe'
    $interfaceProof = Start-Process -FilePath $launcher -ArgumentList '--verify-embedded-interface' -WindowStyle Hidden -Wait -PassThru
    if ($interfaceProof.ExitCode -ne 0) { throw 'Embedded production interface verification failed; no launcher is ready to distribute.' }
    Copy-Item -LiteralPath $launcher -Destination (Join-Path $output 'Kaleidoscope.exe')
    Copy-Item -LiteralPath $bridge -Destination (Join-Path $output 'osu.Game.Rulesets.Transfiguration.dll')
    $manifest = @{ version = 1; server = 'https://illya.test'; bridge_sha256 = $env:ILLYA_BRIDGE_SHA256; embedded_interface_verified = $true; native_gui_verified = $false }
    $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'package.json') -Encoding utf8
    if (Test-Path -LiteralPath 'tools/transfiguration/THIRD-PARTY-NOTICES.md') {
        Copy-Item -LiteralPath 'tools/transfiguration/THIRD-PARTY-NOTICES.md' -Destination (Join-Path $output 'THIRD-PARTY-NOTICES.md')
    }
    Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $output 'LICENSE')
    Copy-Item -LiteralPath (Join-Path $bridgeOutput 'licenses') -Destination (Join-Path $output 'licenses') -Recurse
    Copy-Item -LiteralPath 'tools/transfiguration/compatibility.json' -Destination (Join-Path $output 'compatibility.json')
    $readme = @'
Kaleidoscope — Illyaverse

Run Kaleidoscope.exe, choose the installed stable/lazer osu!.exe, then launch.
This build is fixed to https://illya.test for local development. It still needs
the documented local DNS/CA setup; public releases use real DNS and HTTPS.
Lazer updates outside the tested build stop before Main until a compatible
launcher and bridge package is published. Update the complete package together;
never manually copy this DLL into official rulesets.
Close the game to remove the temporary private-profile DLL. The independent
guardian survives closing this window. After an interrupted guardian or power
loss, reopen the launcher and use cleanup. Foreign or changed files are kept.

Native GUI/gameplay remains a user-operated verification step. Source and
headless test evidence is documented in docs/verification.md in the repository.
'@
    Set-Content -LiteralPath (Join-Path $output 'README.txt') -Value $readme -Encoding utf8
    Write-Output "Portable launcher package: $output"
    Write-Output 'Uses the player installed client; no official client/resources are redistributed. No game was launched.'
} finally {
    Pop-Location
    [Environment]::SetEnvironmentVariable('ILLYA_BRIDGE_SHA256', $previousHash, 'Process')
}
