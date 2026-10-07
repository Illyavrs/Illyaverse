param([string]$NativeRoot = (Join-Path $env:LOCALAPPDATA 'osulazer/current'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$bridgeRoot = $PSScriptRoot
$bridgeNative = [IO.Path]::GetFullPath($NativeRoot)
$bridgeManifest = Get-Content -LiteralPath (Join-Path $bridgeRoot 'compatibility.json') -Raw | ConvertFrom-Json
foreach ($assembly in $bridgeManifest.assemblies.PSObject.Properties) {
    $assemblyPath = Join-Path $bridgeNative $assembly.Name
    if (!(Test-Path -LiteralPath $assemblyPath -PathType Leaf)) { throw "Missing tested client assembly: $($assembly.Name)" }
    if ((Get-FileHash -LiteralPath $assemblyPath -Algorithm SHA256).Hash -ne $assembly.Value.sha256) { throw "Untested client assembly: $($assembly.Name). Update the small bridge compatibility gate after its proofs pass." }
}
& dotnet build (Join-Path $bridgeRoot 'Transfiguration.csproj') -c Release '-p:Fixture=false' "-p:NativeRoot=$bridgeNative" --nologo
if ($LASTEXITCODE -ne 0) { throw 'Transfiguration compilation failed' }

$bridgeOutput = Join-Path $bridgeRoot 'artifacts'
[IO.Directory]::CreateDirectory($bridgeOutput) | Out-Null
$bridgeDll = Join-Path $bridgeRoot 'bin/Release/net10.0/osu.Game.Rulesets.Transfiguration.dll'
Copy-Item -LiteralPath $bridgeDll -Destination (Join-Path $bridgeOutput $bridgeManifest.artifact) -Force
Copy-Item -LiteralPath (Join-Path $bridgeRoot 'compatibility.json'),(Join-Path $bridgeRoot 'THIRD-PARTY-NOTICES.md'),(Join-Path $bridgeRoot 'README.md') -Destination $bridgeOutput -Force
$bridgeLicenseOutput = Join-Path $bridgeOutput 'licenses'
[IO.Directory]::CreateDirectory($bridgeLicenseOutput) | Out-Null
Get-ChildItem -LiteralPath (Join-Path $bridgeRoot 'licenses') -File | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $bridgeLicenseOutput -Force }
$bridgeHash = (Get-FileHash -LiteralPath $bridgeDll -Algorithm SHA256).Hash.ToLowerInvariant()
[ordered]@{ protocol = 1; artifact = $bridgeManifest.artifact; sha256 = $bridgeHash; endpoint = $bridgeManifest.endpoint; fixture = $false } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $bridgeOutput 'artifact.json') -Encoding utf8NoBOM
Write-Output "Production bridge: $bridgeOutput"
Write-Output "SHA256: $bridgeHash"
