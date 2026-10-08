param(
    [string]$NativeRoot = (Join-Path $env:LOCALAPPDATA 'osulazer/current'),
    [switch]$InspectOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$archiveRepo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$archiveNative = (Resolve-Path -LiteralPath $NativeRoot).Path
$archiveWorkspace = [IO.Path]::GetFullPath((Join-Path $archiveRepo 'data-archive-compatibility'))
$archiveIdentity = [guid]::NewGuid().ToString('N')
$archiveOutput = [IO.Path]::GetFullPath((Join-Path $archiveWorkspace $archiveIdentity))
if (![string]::Equals([IO.Path]::GetDirectoryName($archiveOutput), $archiveWorkspace, [StringComparison]::OrdinalIgnoreCase)) { throw 'Proof output is outside its workspace' }
if (Test-Path -LiteralPath $archiveOutput) { throw 'Proof UUID already exists' }
foreach ($archiveVariable in @('DOTNET_STARTUP_HOOKS', 'DOTNET_ADDITIONAL_DEPS')) {
    if ([Environment]::GetEnvironmentVariable($archiveVariable, 'Process')) { throw "Offline probe requires no $archiveVariable" }
}
[IO.Directory]::CreateDirectory($archiveOutput) | Out-Null
$archiveObject = Join-Path $archiveOutput 'obj/'
$archiveBinary = Join-Path $archiveOutput 'bin/'
$archiveProject = Join-Path $PSScriptRoot 'ArchiveCompatibility.csproj'
$archiveManifest = Join-Path $archiveRepo 'tools/transfiguration/compatibility.json'
$archiveBuildArgs = @("-p:NativeRoot=$archiveNative", "-p:BaseIntermediateOutputPath=$archiveObject", "-p:MSBuildProjectExtensionsPath=$archiveObject", "-p:OutputPath=$archiveBinary", "-p:RestorePackagesPath=$(Join-Path $archiveOutput 'packages')", '-p:NuGetAudit=false')
$archiveLog = Join-Path $archiveOutput 'proof.txt'
$archiveTemporary = Join-Path $archiveOutput 'tmp'
[IO.Directory]::CreateDirectory($archiveTemporary) | Out-Null
$archiveEnvironment = @{
    DOTNET_CLI_HOME = (Join-Path $archiveOutput 'cli-home')
    DOTNET_CLI_TELEMETRY_OPTOUT = '1'
    DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE = 'true'
    DOTNET_GENERATE_ASPNET_CERTIFICATE = 'false'
    DOTNET_ADD_GLOBAL_TOOLS_TO_PATH = 'false'
    TEMP = $archiveTemporary
    TMP = $archiveTemporary
}
$archivePreviousEnvironment = @{}

function Invoke-ArchiveCommand([string[]]$CommandArgs) {
    & dotnet @CommandArgs 2>&1 | Tee-Object -FilePath $archiveLog -Append
    if ($LASTEXITCODE -ne 0) { throw "Archive probe command failed with exit code $LASTEXITCODE; output retained at $archiveOutput" }
}

try {
    foreach ($archiveName in $archiveEnvironment.Keys) {
        $archivePreviousEnvironment[$archiveName] = [Environment]::GetEnvironmentVariable($archiveName, 'Process')
        [Environment]::SetEnvironmentVariable($archiveName, $archiveEnvironment[$archiveName], 'Process')
    }
    # Clear package sources and disable SDK telemetry/workload checking. Every
    # build/temp/CLI output stays in the unique proof workspace.
    Invoke-ArchiveCommand (@('restore', $archiveProject, '--configfile', (Join-Path $PSScriptRoot 'NuGet.Config'), '--nologo') + $archiveBuildArgs)
    Invoke-ArchiveCommand (@('build', $archiveProject, '-c', 'Release', '--no-restore', '--nologo') + $archiveBuildArgs)
    $archiveDll = Join-Path $archiveBinary 'Illyaverse.ArchiveCompatibility.dll'
    Get-ChildItem -LiteralPath $archiveBinary -Filter '*.dll' -File | ForEach-Object {
        if ($_.Name -ne 'Illyaverse.ArchiveCompatibility.dll') { throw "Unexpected copied assembly: $($_.Name)" }
    }
    if ($InspectOnly) {
        foreach ($archiveAssembly in @('osu.Game.dll', 'osu.Framework.dll', 'Realm.dll', 'SharpCompress.dll')) {
            Invoke-ArchiveCommand @($archiveDll, '--inspect-module', (Join-Path $archiveNative $archiveAssembly))
        }
        Invoke-ArchiveCommand @($archiveDll, '--inspect-type', (Join-Path $archiveNative 'Realm.dll'), 'RealmSchema')
        Write-Output "Read-only PE metadata inspection: $archiveLog"
        return
    }
    Invoke-ArchiveCommand @($archiveDll, $archiveNative, $archiveManifest, $archiveOutput)
    $archiveReport = Join-Path $archiveOutput 'report.json'
    Write-Output "Report SHA256: $((Get-FileHash -LiteralPath $archiveReport -Algorithm SHA256).Hash)"
    Write-Output "Offline archive-reader report: $archiveReport"
}
finally {
    foreach ($archiveName in $archivePreviousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($archiveName, $archivePreviousEnvironment[$archiveName], 'Process')
    }
}
