param(
    [string]$NativeRoot = (Join-Path $env:LOCALAPPDATA 'osulazer/current'),
    [string]$NativeHost
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$proofRoot = $PSScriptRoot
$proofNative = [IO.Path]::GetFullPath($NativeRoot)
$proofOutput = Join-Path $proofRoot 'proof-output'
[IO.Directory]::CreateDirectory($proofOutput) | Out-Null
$proofLog = Join-Path $proofOutput 'contract-proof.txt'
Set-Content -LiteralPath $proofLog -Value '' -Encoding utf8NoBOM

function Invoke-ProofCommand([string]$Command, [string[]]$CommandArgs) {
    & $Command @CommandArgs | Tee-Object -FilePath $proofLog -Append
    if ($LASTEXITCODE -ne 0) { throw "Proof command failed ($LASTEXITCODE): $Command" }
}

& (Join-Path $proofRoot 'build.ps1') -NativeRoot $proofNative
Invoke-ProofCommand 'dotnet' @('build',(Join-Path $proofRoot 'Transfiguration.csproj'),'-c','Fixture','-p:Fixture=true',"-p:NativeRoot=$proofNative",'--nologo')
$proofRuntime = Join-Path $proofOutput 'runtime-10.0.12'
Invoke-ProofCommand 'dotnet' @('publish',(Join-Path $proofRoot 'tests/Proof.csproj'),'-c','Release','-r','win-x64','--self-contained','true','-p:UseAppHost=true','-p:RuntimeFrameworkVersion=10.0.12','-o',$proofRuntime,'--nologo')
$proofExe = Join-Path $proofRuntime 'Transfiguration.Proof.exe'
$proofFixture = Join-Path $proofRoot 'bin/Fixture/net10.0/osu.Game.Rulesets.Transfiguration.dll'
$proofProduction = Join-Path $proofRoot 'artifacts/osu.Game.Rulesets.Transfiguration.dll'
Invoke-ProofCommand $proofExe @('--policy',$proofProduction)
Invoke-ProofCommand $proofExe @($proofFixture,$proofNative,(Join-Path $proofOutput 'data-transfiguration-proof-native'))
foreach ($proofCase in 'nonce','ready-path','ready-exists','relative','unc') {
    Invoke-ProofCommand $proofExe @('--reject',$proofFixture,$proofNative,(Join-Path $proofOutput 'data-transfiguration-proof-reject'),$proofCase)
}

if ($NativeHost) {
    $proofHost = [IO.Path]::GetFullPath($NativeHost)
    if (!(Test-Path -LiteralPath $proofHost -PathType Leaf)) { throw 'Native runtime_probe executable missing; build its Kaleidoscope example first' }
    $proofData = Join-Path $proofOutput 'data-transfiguration-proof-native-host'
    [IO.Directory]::CreateDirectory($proofData) | Out-Null
    $proofNonce = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
    $proofEnvironment = @{
        ILLYAVERSE_LAZER_INSTALL_ROOT = $proofNative
        ILLYAVERSE_LAZER_DATA_ROOT = $proofData
        ILLYAVERSE_LAZER_SESSION = $proofNonce
        ILLYAVERSE_LAZER_READY_FILE = (Join-Path $proofData "session-ready-$proofNonce.json")
        ILLYAVERSE_TRANSFIGURATION_NATIVE_PROOF = '1'
        ILLYAVERSE_TRANSFIGURATION_PROOF_BRIDGE = $proofFixture
    }
    $proofPrevious = @{}
    try {
        foreach ($proofName in $proofEnvironment.Keys) {
            $proofPrevious[$proofName] = [Environment]::GetEnvironmentVariable($proofName)
            [Environment]::SetEnvironmentVariable($proofName,$proofEnvironment[$proofName])
        }
        Invoke-ProofCommand $proofHost @((Join-Path $proofRuntime 'Transfiguration.Proof.dll'),(Join-Path $proofNative 'hostfxr.dll'),$proofFixture)
    }
    finally {
        foreach ($proofName in $proofPrevious.Keys) { [Environment]::SetEnvironmentVariable($proofName,$proofPrevious[$proofName]) }
    }
}
Write-Output "Proof report: $proofLog"
