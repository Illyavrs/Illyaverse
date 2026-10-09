param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../..')).Path
Import-Module (Join-Path $PSScriptRoot '../scripts/launcher-package.psm1') -Force
$identity = [guid]::NewGuid().ToString('N')
$fixtureRoot = Assert-PackageWorkspacePath (Join-Path $repo ('data-kaleidoscope-package-fixtures-' + $identity)) $repo
$checks = 0

function Write-Fixture([string]$Path, [string]$Text) {
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)) | Out-Null
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
function Write-FixtureJson([string]$Path, $Value) { Write-Fixture $Path ($Value | ConvertTo-Json -Depth 20) }
function Assert-Fixture([bool]$Condition, [string]$Message) { if (!$Condition) { throw $Message } }
function Expect-Refusal([string]$Name, [scriptblock]$Action, [string]$MessagePattern = '*') {
    try { $null = & $Action } catch {
        if ($_.Exception.Message -notlike $MessagePattern) { throw "Unexpected failure in ${Name}: $($_.Exception.Message)" }
        $script:checks++
        Write-Output "PASS $Name"
        return
    }
    throw "Integrity boundary did not refuse: $Name"
}
function New-FixtureSource([string]$Name) {
    $source = Join-Path $fixtureRoot ('source-' + $Name)
    Copy-Item -LiteralPath (Join-Path $fixtureRoot 'source-baseline') -Destination $source -Recurse
    return $source
}
function New-FixturePackage([string]$Source, [string]$Name, [hashtable]$Evidence) {
    if (!$Evidence) { $Evidence = @{ kind = 'synthetic_package_integrity'; launcher_sha256 = (Get-FileHash -LiteralPath (Join-Path $Source 'Kaleidoscope.exe') -Algorithm SHA256).Hash; exit_code = $null } }
    return New-KaleidoscopePackage -WorkspaceRoot $repo -LauncherPath (Join-Path $Source 'Kaleidoscope.exe') -BridgeDirectory (Join-Path $Source 'bridge') -LicensePath (Join-Path $Source 'LICENSE') -ExpectedCompatibilityPath (Join-Path $fixtureRoot 'expected-compatibility.json') -OutputDirectory (Join-Path $fixtureRoot ('output-' + $Name)) -InterfaceEvidence $Evidence
}
function Copy-FixturePackage([string]$Name) {
    $copy = Join-Path $fixtureRoot ('tamper-' + $Name)
    Copy-Item -LiteralPath $script:valid.Directory -Destination $copy -Recurse
    return $copy
}

try {
    [IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
    $owner = Join-Path $fixtureRoot '.owner.json'
    $ownerStream = [IO.File]::Open($owner, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes((@{ identity = $identity; root = $fixtureRoot } | ConvertTo-Json))
        $ownerStream.Write($bytes, 0, $bytes.Length)
    } finally { $ownerStream.Dispose() }
    $baseline = Join-Path $fixtureRoot 'source-baseline'
    # These bytes are deliberately not a runnable EXE or DLL. No process is started.
    Write-Fixture (Join-Path $baseline 'Kaleidoscope.exe') 'synthetic launcher bytes; not executable'
    Write-Fixture (Join-Path $baseline 'bridge/osu.Game.Rulesets.Transfiguration.dll') 'synthetic bridge bytes; not executable'
    Write-Fixture (Join-Path $baseline 'LICENSE') 'synthetic repository license text'
    Write-Fixture (Join-Path $baseline 'bridge/README.md') 'synthetic bridge documentation'
    Write-Fixture (Join-Path $baseline 'bridge/THIRD-PARTY-NOTICES.md') 'synthetic third-party notices'
    foreach ($name in @('DotNet.txt', 'Harmony.txt', 'Iced.txt', 'Mono.Cecil.txt', 'MonoMod.txt')) { Write-Fixture (Join-Path $baseline "bridge/licenses/$name") "synthetic license $name" }
    $compatibility = Get-Content -LiteralPath (Join-Path $repo 'tools/transfiguration/compatibility.json') -Raw
    Write-Fixture (Join-Path $baseline 'bridge/compatibility.json') $compatibility
    Write-Fixture (Join-Path $fixtureRoot 'expected-compatibility.json') $compatibility
    Write-FixtureJson (Join-Path $baseline 'bridge/artifact.json') @{ protocol = 1; artifact = 'osu.Game.Rulesets.Transfiguration.dll'; sha256 = (Get-FileHash -LiteralPath (Join-Path $baseline 'bridge/osu.Game.Rulesets.Transfiguration.dll') -Algorithm SHA256).Hash.ToLowerInvariant(); endpoint = 'https://illya.test'; fixture = $false }

    $script:valid = New-FixturePackage $baseline 'valid'
    $manifest = Test-KaleidoscopePackage -Directory $valid.Directory -AllowSyntheticFixtures
    $null = Test-KaleidoscopePackageArchive -ArchivePath $valid.Archive -Directory $valid.Directory -AllowSyntheticFixtures
    $directoryCli = & (Join-Path $PSScriptRoot '../scripts/verify-launcher-package.ps1') -Directory $valid.Directory -AllowSyntheticFixtures
    $archiveCli = & (Join-Path $PSScriptRoot '../scripts/verify-launcher-package.ps1') -ArchivePath $valid.Archive -AllowSyntheticFixtures
    Assert-Fixture ($directoryCli -like 'PASS complete package integrity*' -and $archiveCli -like 'PASS complete package integrity*') 'Package verifier CLI did not pass both parameter sets.'
    Assert-Fixture ($manifest.verification_kind -ceq 'synthetic_package_integrity' -and $manifest.embedded_interface_verified -eq $false -and $manifest.native_gui_verified -eq $false) 'Synthetic package claimed native evidence.'
    $checks++
    Write-Output 'PASS complete synthetic directory and matching ZIP; all required files hashed'
    Expect-Refusal 'synthetic package is refused by production verifier' { Test-KaleidoscopePackage -Directory $valid.Directory } '*Synthetic*'

    foreach ($required in @('artifact.json', 'THIRD-PARTY-NOTICES.md', 'README.md', 'licenses/Harmony.txt')) {
        $name = $required.Replace('/', '-').Replace('.', '-')
        $source = New-FixtureSource "missing-$name"
        Remove-Item -LiteralPath (Join-Path $source "bridge/$required")
        Expect-Refusal "missing required source $required" { New-FixturePackage $source "missing-$name" }
        Assert-Fixture (!(Test-Path -LiteralPath (Join-Path $fixtureRoot "output-missing-$name"))) 'Failed source gate created a final delivery.'
    }
    foreach ($case in @('hash', 'fixture', 'endpoint', 'protocol-type')) {
        $source = New-FixtureSource "artifact-$case"
        $path = Join-Path $source 'bridge/artifact.json'
        $artifact = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
        switch ($case) {
            'hash' { $artifact.sha256 = ('0' * 64) }
            'fixture' { $artifact.fixture = $true }
            'endpoint' { $artifact.endpoint = 'https://osu.ppy.sh' }
            'protocol-type' { $artifact.protocol = '1' }
        }
        Write-FixtureJson $path $artifact
        Expect-Refusal "artifact rejects $case" { New-FixturePackage $source "artifact-$case" }
    }
    foreach ($case in @('endpoint', 'assembly-hash', 'missing-assembly', 'runtime', 'contracts', 'missing-account-isolation-contract', 'missing-api-constructor-contract', 'missing-account-isolation-proof', 'account-proof-overclaim', 'worker-proof-overclaim', 'proof-state', 'fixture-type', 'source-mismatch')) {
        $source = New-FixtureSource "compatibility-$case"
        $path = Join-Path $source 'bridge/compatibility.json'
        $value = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
        switch ($case) {
            'endpoint' { $value.endpoint = 'https://illya.test:444' }
            'assembly-hash' { $value.assemblies['osu.Game.dll'].sha256 = 'invalid' }
            'missing-assembly' { $value.assemblies.Remove('System.Net.Http.dll') }
            'runtime' { $value.runtime = 'unknown' }
            'contracts' { $value.contracts = @('OsuGameBase.CreateEndpoints') }
            'missing-account-isolation-contract' { $value.contracts = @($value.contracts | Where-Object { $_ -cne 'IniConfigManager<OsuSetting>.Filename(exact OsuConfigManager only)' }) }
            'missing-api-constructor-contract' { $value.contracts = @($value.contracts | Where-Object { $_ -cne 'APIAccess.ctor(OsuGameBase,OsuConfigManager,EndpointConfiguration,string)' }) }
            'missing-account-isolation-proof' { $value.verification.Remove('private_configuration_before_api_constructor') }
            'account-proof-overclaim' { $value.verification.private_credential_save_logout_remember_flags = 'passed_native_login' }
            'worker-proof-overclaim' { $value.verification.real_api_worker_refresh_and_login = 'passed' }
            'proof-state' { $value.verification.native_gui_and_login = 'passed' }
            'fixture-type' { $value.fixture = 'false' }
            'source-mismatch' { $value.runtime = '10.0.13' }
        }
        Write-FixtureJson $path $value
        Expect-Refusal "compatibility rejects $case" { New-FixturePackage $source "compatibility-$case" }
    }
    $source = New-FixtureSource 'truncated-json'
    Write-Fixture (Join-Path $source 'bridge/artifact.json') '{"protocol":1,'
    Expect-Refusal 'truncated source JSON' { New-FixturePackage $source 'truncated-json' } '*Invalid package JSON*'
    $source = New-FixtureSource 'duplicate-json-field'
    $path = Join-Path $source 'bridge/artifact.json'
    $text = [IO.File]::ReadAllText($path).Replace('"protocol": 1', '"protocol": 1, "protocol": 1')
    Write-Fixture $path $text
    Expect-Refusal 'duplicate JSON field' { New-FixturePackage $source 'duplicate-json-field' } '*Invalid package JSON*'
    $source = New-FixtureSource 'endpoint-array'
    $path = Join-Path $source 'bridge/artifact.json'
    $value = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
    $value.endpoint = @('https://illya.test')
    Write-FixtureJson $path $value
    Expect-Refusal 'endpoint array instead of literal string' { New-FixturePackage $source 'endpoint-array' } '*Invalid package value*'
    Expect-Refusal 'changed launcher after verification' { New-FixturePackage $baseline 'changed-launcher' @{ kind = 'synthetic_package_integrity'; launcher_sha256 = ('0' * 64); exit_code = $null } } '*Launcher changed*'
    Expect-Refusal 'failed interface command evidence' { New-FixturePackage $baseline 'failed-proof' @{ kind = 'embedded_interface_command'; launcher_sha256 = (Get-FileHash -LiteralPath (Join-Path $baseline 'Kaleidoscope.exe') -Algorithm SHA256).Hash; exit_code = 7 } } '*did not pass*'

    $existing = Join-Path $fixtureRoot 'output-existing-directory'
    Write-Fixture (Join-Path $existing 'keep.txt') 'old delivery remains'
    Expect-Refusal 'existing output directory' { New-FixturePackage $baseline 'existing-directory' } '*already exists*'
    Assert-Fixture ([IO.File]::ReadAllText((Join-Path $existing 'keep.txt')) -ceq 'old delivery remains') 'Existing output changed.'
    Write-Fixture (Join-Path $fixtureRoot 'output-existing-zip.zip') 'old ZIP remains'
    Expect-Refusal 'existing output archive' { New-FixturePackage $baseline 'existing-zip' } '*already exists*'
    Assert-Fixture ([IO.File]::ReadAllText((Join-Path $fixtureRoot 'output-existing-zip.zip')) -ceq 'old ZIP remains') 'Existing archive changed.'
    Expect-Refusal 'output outside workspace' { Assert-PackageWorkspacePath (Join-Path ([IO.Path]::GetDirectoryName($repo)) 'outside-package-test') $repo } '*inside the repository*'

    foreach ($case in @('missing-license', 'missing-completion', 'truncated-completion', 'changed-launcher', 'unexpected-file')) {
        $copy = Copy-FixturePackage $case
        switch ($case) {
            'missing-license' { Remove-Item -LiteralPath (Join-Path $copy 'licenses/DotNet.txt') }
            'missing-completion' { Remove-Item -LiteralPath (Join-Path $copy 'package.json') }
            'truncated-completion' { Write-Fixture (Join-Path $copy 'package.json') '{"status":' }
            'changed-launcher' { Write-Fixture (Join-Path $copy 'Kaleidoscope.exe') 'changed payload bytes' }
            'unexpected-file' { Write-Fixture (Join-Path $copy 'osu.Game.dll') 'official assembly must never be packaged' }
        }
        Expect-Refusal "assembled package rejects $case" { Test-KaleidoscopePackage -Directory $copy -AllowSyntheticFixtures }
    }

    foreach ($case in @('changed-content', 'duplicate-entry', 'unsafe-entry', 'truncated-zip')) {
        $zipPath = Join-Path $fixtureRoot "tamper-$case.zip"
        Copy-Item -LiteralPath $valid.Archive -Destination $zipPath
        if ($case -ceq 'truncated-zip') {
            $file = [IO.File]::Open($zipPath, [IO.FileMode]::Open, [IO.FileAccess]::Write)
            try { $file.SetLength(32) } finally { $file.Dispose() }
        } else {
            $zip = [IO.Compression.ZipFile]::Open($zipPath, [IO.Compression.ZipArchiveMode]::Update)
            try {
                $entryName = 'LICENSE'
                if ($case -ceq 'changed-content') { $zip.GetEntry($entryName).Delete() }
                if ($case -ceq 'unsafe-entry') { $entryName = '../escape.txt' }
                $writer = [IO.StreamWriter]::new($zip.CreateEntry($entryName).Open())
                try { $writer.Write('tampered synthetic archive content') } finally { $writer.Dispose() }
            } finally { $zip.Dispose() }
        }
        Expect-Refusal "ZIP integrity rejects $case" { Test-KaleidoscopePackageArchive -ArchivePath $zipPath -AllowSyntheticFixtures }
    }
    $copy = Copy-FixturePackage 'zip-directory-mismatch'
    $path = Join-Path $copy 'package.json'
    $value = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable
    $value.completed_at_utc = '2026-01-01T00:00:00.0000000Z'
    Write-FixtureJson $path $value
    Expect-Refusal 'ZIP completion metadata differs from directory' { Test-KaleidoscopePackageArchive -ArchivePath $valid.Archive -Directory $copy -AllowSyntheticFixtures } '*does not match assembled*'
    Write-Output "PASS $checks synthetic package integrity checks. No EXE/DLL executed; no embedded-interface or native gameplay proof claimed."
} finally {
    if (Test-Path -LiteralPath $fixtureRoot) {
        # Only this invocation's exact workspace-contained, identity-marked fixtures may be removed.
        $resolvedFixture = Assert-PackageWorkspacePath (Resolve-Path -LiteralPath $fixtureRoot).Path $repo
        $ownership = Get-Content -LiteralPath (Join-Path $resolvedFixture '.owner.json') -Raw | ConvertFrom-Json -AsHashtable
        if ([IO.Path]::GetDirectoryName($resolvedFixture) -ne $repo -or [IO.Path]::GetFileName($resolvedFixture) -cne ('data-kaleidoscope-package-fixtures-' + $identity) -or $ownership.identity -cne $identity -or $ownership.root -cne $fixtureRoot) { throw 'Refusing fixture cleanup: ownership identity differs.' }
        if (@(Get-ChildItem -LiteralPath $resolvedFixture -Recurse -Force | Where-Object { $_.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint) }).Count -ne 0) { throw 'Refusing fixture cleanup: a reparse point is present.' }
        Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
    }
}
