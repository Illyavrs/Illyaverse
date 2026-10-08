param(
    [string]$NativeRoot = (Join-Path $env:LOCALAPPDATA 'osulazer/current'),
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module (Join-Path $PSScriptRoot 'launcher-package.psm1') -Force
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
$native = (Resolve-Path -LiteralPath $NativeRoot).Path
if (!$OutputDirectory) {
    $OutputDirectory = Join-Path $repo ('data-kaleidoscope-package-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N'))
}
$output = Assert-PackageWorkspacePath $OutputDirectory $repo
if ((Test-Path -LiteralPath $output) -or (Test-Path -LiteralPath ($output + '.zip'))) { throw 'Package output or ZIP already exists; choose a new output directory.' }
if (!(Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($output)) -PathType Container)) { throw 'Package parent directory must already exist.' }
foreach ($required in @('osu.Game.dll','osu.Framework.dll','osu!.dll','hostfxr.dll')) {
    if (!(Test-Path -LiteralPath (Join-Path $native $required) -PathType Leaf)) { throw "Selected lazer installation is missing $required" }
}
$previousHash = [Environment]::GetEnvironmentVariable('ILLYA_BRIDGE_SHA256', 'Process')
$previousIncremental = [Environment]::GetEnvironmentVariable('CARGO_INCREMENTAL', 'Process')
$previousTarget = [Environment]::GetEnvironmentVariable('CARGO_TARGET_DIR', 'Process')
function Assert-Build([string]$step) {
    if ($LASTEXITCODE -ne 0) { throw "$step failed; no launcher is ready to distribute." }
}
Push-Location $repo
try {
    $env:CARGO_INCREMENTAL = '0'
    $env:CARGO_TARGET_DIR = Join-Path $repo 'target'
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
    $launcher = Join-Path $repo 'target/release/kaleidoscope-desktop.exe'
    $launcherHash = (Get-FileHash -LiteralPath $launcher -Algorithm SHA256).Hash.ToLowerInvariant()
    $interfaceProof = Start-Process -FilePath $launcher -ArgumentList '--verify-embedded-interface' -WindowStyle Hidden -Wait -PassThru
    if ($interfaceProof.ExitCode -ne 0) { throw 'Embedded production interface verification failed; no launcher is ready to distribute.' }
    $package = New-KaleidoscopePackage -WorkspaceRoot $repo -LauncherPath $launcher -BridgeDirectory $bridgeOutput -LicensePath (Join-Path $repo 'LICENSE') -ExpectedCompatibilityPath (Join-Path $repo 'tools/transfiguration/compatibility.json') -OutputDirectory $output -InterfaceEvidence @{ kind = 'embedded_interface_command'; launcher_sha256 = $launcherHash; exit_code = $interfaceProof.ExitCode }
    Write-Output "Validated local test package: $($package.Directory)"
    Write-Output "Validated local ZIP: $($package.Archive)"
    Write-Output 'Uses the player installed client; no official client/resources are redistributed. No game was launched.'
} finally {
    Pop-Location
    [Environment]::SetEnvironmentVariable('ILLYA_BRIDGE_SHA256', $previousHash, 'Process')
    [Environment]::SetEnvironmentVariable('CARGO_INCREMENTAL', $previousIncremental, 'Process')
    [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', $previousTarget, 'Process')
}
