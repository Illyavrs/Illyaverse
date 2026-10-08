Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:bridgeName = 'osu.Game.Rulesets.Transfiguration.dll'
$script:licenseNames = @('DotNet.txt', 'Harmony.txt', 'Iced.txt', 'Mono.Cecil.txt', 'MonoMod.txt')
$script:payloadNames = @('Kaleidoscope.exe', $script:bridgeName, 'artifact.json', 'compatibility.json', 'THIRD-PARTY-NOTICES.md', 'LICENSE', 'README.txt', 'Transfiguration-README.md') + @($script:licenseNames | ForEach-Object { "licenses/$_" })
$script:allNames = $script:payloadNames + @('package.json')

function Assert-PackageWorkspacePath([string]$Path, [string]$WorkspaceRoot) {
    $full = [IO.Path]::GetFullPath($Path)
    $root = [IO.Path]::GetFullPath($WorkspaceRoot).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    if (!$full.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Package target must be inside the repository workspace.' }
    $cursor = $full
    while ($cursor) {
        if ((Test-Path -LiteralPath $cursor) -and (Get-Item -LiteralPath $cursor -Force).Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) { throw "Package path contains a reparse point: $cursor" }
        if ($cursor.Equals($root, [StringComparison]::OrdinalIgnoreCase)) { break }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $full
}

function Read-PackageJson([string]$Text, [string]$Name) {
    $document = $null
    try {
        $document = [Text.Json.JsonDocument]::Parse($Text)
        Assert-PackageJsonProperties $document.RootElement
    } catch { throw "Invalid package JSON: $Name" } finally { if ($document) { $document.Dispose() } }
    $options = @{ InputObject = $Text; AsHashtable = $true; ErrorAction = 'Stop' }
    if ((Get-Command ConvertFrom-Json).Parameters.ContainsKey('DateKind')) { $options.DateKind = 'String' }
    try { $value = ConvertFrom-Json @options } catch { throw "Invalid package JSON: $Name" }
    if ($value -isnot [System.Collections.IDictionary]) { throw "Package JSON must be an object: $Name" }
    return $value
}

function Assert-PackageJsonProperties([Text.Json.JsonElement]$Element) {
    if ($Element.ValueKind -eq [Text.Json.JsonValueKind]::Object) {
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($property in $Element.EnumerateObject()) {
            if (!$names.Add($property.Name)) { throw 'Duplicate package JSON field.' }
            Assert-PackageJsonProperties $property.Value
        }
    } elseif ($Element.ValueKind -eq [Text.Json.JsonValueKind]::Array) {
        foreach ($child in $Element.EnumerateArray()) { Assert-PackageJsonProperties $child }
    }
}

function Assert-PackageKeys($Value, [string[]]$Names, [string]$Name) {
    if ($Value -isnot [System.Collections.IDictionary] -or $Value.Count -ne $Names.Count) { throw "Invalid fields in $Name" }
    foreach ($key in $Names) { if (@($Value.Keys) -cnotcontains $key) { throw "Missing $Name field: $key" } }
}

function Assert-PackageLiteral($Value, [string]$Expected, [string]$Name) {
    if ($Value -isnot [string] -or $Value -cne $Expected) { throw "Invalid package value: $Name" }
}

function Assert-PackageBoolean($Value, [bool]$Expected, [string]$Name) {
    if ($Value -isnot [bool] -or $Value -ne $Expected) { throw "Invalid package boolean: $Name" }
}

function Assert-PackageHash($Value, [string]$Name) {
    if ($Value -isnot [string] -or $Value -cnotmatch '^[0-9a-fA-F]{64}$') { throw "Invalid SHA256: $Name" }
}

function Assert-PackageProtocol($Value, [string]$Name) {
    if (($Value -isnot [long] -and $Value -isnot [int]) -or $Value -ne 1) { throw "Unsupported package protocol: $Name" }
}

function Assert-BridgePackageMetadata($Artifact, $Compatibility, [string]$BridgeHash) {
    Assert-PackageKeys $Artifact @('protocol', 'artifact', 'sha256', 'endpoint', 'fixture') 'artifact.json'
    Assert-PackageProtocol $Artifact.protocol 'artifact.json'
    Assert-PackageBoolean $Artifact.fixture $false 'artifact.json.fixture'
    Assert-PackageHash $Artifact.sha256 'artifact.json.sha256'
    Assert-PackageLiteral $Artifact.artifact $script:bridgeName 'artifact.json.artifact'
    Assert-PackageLiteral $Artifact.endpoint 'https://illya.test' 'artifact.json.endpoint'
    if ($Artifact.sha256 -ine $BridgeHash) { throw 'Production bridge artifact metadata or hash does not match.' }
    Assert-PackageKeys $Compatibility @('protocol', 'artifact', 'endpoint', 'fixture', 'platform', 'runtime', 'ruleset_api', 'harmony', 'assemblies', 'contracts', 'verification') 'compatibility.json'
    Assert-PackageProtocol $Compatibility.protocol 'compatibility.json'
    Assert-PackageBoolean $Compatibility.fixture $false 'compatibility.json.fixture'
    Assert-PackageLiteral $Compatibility.artifact $script:bridgeName 'compatibility.json.artifact'
    Assert-PackageLiteral $Compatibility.endpoint 'https://illya.test' 'compatibility.json.endpoint'
    Assert-PackageLiteral $Compatibility.platform 'windows-x64' 'compatibility.json.platform'
    foreach ($key in @('runtime', 'ruleset_api', 'harmony')) {
        if ($Compatibility[$key] -isnot [string] -or $Compatibility[$key] -cnotmatch '^\d+\.\d+\.\d+$') { throw "Invalid compatibility version: $key" }
    }
    $assemblies = @('osu.Game.dll', 'osu.Framework.dll', 'osu!.dll', 'System.Net.Http.dll', 'System.Net.WebSockets.Client.dll')
    Assert-PackageKeys $Compatibility.assemblies $assemblies 'compatibility.json.assemblies'
    foreach ($key in $assemblies) {
        $assembly = $Compatibility.assemblies[$key]
        Assert-PackageKeys $assembly @('version', 'sha256') "compatibility assembly $key"
        if ($assembly.version -isnot [string] -or $assembly.version -cnotmatch '^\d+\.\d+\.\d+\.\d+$') { throw "Invalid assembly version: $key" }
        Assert-PackageHash $assembly.sha256 "compatibility assembly $key"
    }
    if ($Compatibility.contracts -isnot [array] -or $Compatibility.contracts.Count -eq 0) { throw 'Missing compatibility contracts.' }
    $contracts = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($contract in $Compatibility.contracts) {
        if ($contract -isnot [string] -or [string]::IsNullOrWhiteSpace($contract) -or !$contracts.Add($contract)) { throw 'Invalid or duplicate compatibility contract.' }
    }
    foreach ($contract in @('OsuGameBase.CreateEndpoints', 'Host.GetSuitableDesktopHost', 'DesktopGameHost.GetDefaultGameStorage', 'RulesetStore.addRuleset', 'SocketsHttpHandler.SendAsync', 'ClientWebSocket.ConnectAsync(Uri,CancellationToken)')) {
        if (!$contracts.Contains($contract)) { throw "Missing mandatory compatibility contract: $contract" }
    }
    $verification = [ordered]@{
        native_metadata_and_endpoints = 'passed'; http_websocket_and_redirects = 'passed'; mandatory_bootstrap_before_headless_main = 'passed'; production_destination_policy = 'passed'
        rx_native_request_context = 'passed_headless'; rx_statistics_generation_cache = 'passed_headless'; native_gui_and_login = 'pending_human_test'
    }
    Assert-PackageKeys $Compatibility.verification @($verification.Keys) 'compatibility.json.verification'
    foreach ($key in $verification.Keys) { Assert-PackageLiteral $Compatibility.verification[$key] $verification[$key] "compatibility.json.verification.$key" }
}

function Get-PackageFileRecords([string]$Directory) {
    if (!(Test-Path -LiteralPath $Directory -PathType Container)) { throw 'Assembled package directory is missing.' }
    $root = [IO.Path]::GetFullPath($Directory)
    if ((Get-Item -LiteralPath $root -Force).Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) { throw 'Package directory is a reparse point.' }
    $records = [ordered]@{}
    foreach ($item in Get-ChildItem -LiteralPath $root -Recurse -Force) {
        if ($item.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) { throw 'Package contains a reparse point.' }
        if ($item.PSIsContainer) {
            if ([IO.Path]::GetRelativePath($root, $item.FullName).Replace('\', '/') -cne 'licenses') { throw 'Package contains an unexpected directory.' }
            continue
        }
        $name = [IO.Path]::GetRelativePath($root, $item.FullName).Replace('\', '/')
        if ($script:allNames -cnotcontains $name -or $item.Length -eq 0) { throw "Unexpected or empty package file: $name" }
        $records[$name] = @{ sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant(); text = $null }
        if ($name.EndsWith('.json', [StringComparison]::Ordinal)) {
            if ($item.Length -gt 1MB) { throw 'Package JSON exceeds the integrity verifier size limit.' }
            $records[$name].text = [IO.File]::ReadAllText($item.FullName)
        }
    }
    return $records
}

function Assert-PackageRecords($Records, [switch]$AllowSyntheticFixtures) {
    Assert-PackageKeys $Records $script:allNames 'assembled package files'
    $artifact = Read-PackageJson $Records['artifact.json'].text 'artifact.json'
    $compatibility = Read-PackageJson $Records['compatibility.json'].text 'compatibility.json'
    Assert-BridgePackageMetadata $artifact $compatibility $Records[$script:bridgeName].sha256
    $package = Read-PackageJson $Records['package.json'].text 'package.json'
    Assert-PackageKeys $package @('version', 'status', 'server', 'fixture', 'bridge_sha256', 'embedded_interface_verified', 'native_gui_verified', 'verification_kind', 'completed_at_utc', 'content_sha256') 'package.json'
    Assert-PackageProtocol $package.version 'package.json'
    Assert-PackageBoolean $package.fixture $false 'package.json.fixture'
    Assert-PackageBoolean $package.native_gui_verified $false 'package.json.native_gui_verified'
    Assert-PackageLiteral $package.status 'complete' 'package.json.status'
    Assert-PackageLiteral $package.server 'https://illya.test' 'package.json.server'
    Assert-PackageHash $package.bridge_sha256 'package.json.bridge_sha256'
    if ($package.bridge_sha256 -ine $Records[$script:bridgeName].sha256) { throw 'Package bridge hash does not match.' }
    if ($package.verification_kind -isnot [string]) { throw 'Unsupported package verification evidence.' }
    if ($package.verification_kind -ceq 'synthetic_package_integrity') {
        if (!$AllowSyntheticFixtures) { throw 'Synthetic package integrity evidence is not production interface evidence.' }
        Assert-PackageBoolean $package.embedded_interface_verified $false 'synthetic embedded_interface_verified'
    } elseif ($package.verification_kind -ceq 'embedded_interface_command') {
        Assert-PackageBoolean $package.embedded_interface_verified $true 'embedded_interface_verified'
    } else { throw 'Unsupported package verification evidence.' }
    # Earlier PowerShell versions deserialize ISO timestamps as DateTime automatically.
    if ($package.completed_at_utc -is [DateTime] -and $package.completed_at_utc.Kind -eq [DateTimeKind]::Utc) { $package.completed_at_utc = $package.completed_at_utc.ToString('o') }
    if ($package.completed_at_utc -isnot [string] -or $package.completed_at_utc -cnotmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{7}Z$') { throw 'Invalid package completion timestamp.' }
    $parsedTimestamp = [DateTime]::MinValue
    if (![DateTime]::TryParse($package.completed_at_utc, [ref]$parsedTimestamp)) { throw 'Invalid package completion timestamp.' }
    Assert-PackageKeys $package.content_sha256 $script:payloadNames 'package.json.content_sha256'
    foreach ($name in $script:payloadNames) {
        Assert-PackageHash $package.content_sha256[$name] "package content $name"
        if ($package.content_sha256[$name] -ine $Records[$name].sha256) { throw "Package content hash does not match: $name" }
    }
    return $package
}

function Test-KaleidoscopePackage {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Directory, [switch]$AllowSyntheticFixtures)
    return Assert-PackageRecords (Get-PackageFileRecords $Directory) -AllowSyntheticFixtures:$AllowSyntheticFixtures
}

function Test-KaleidoscopePackageArchive {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ArchivePath, [string]$Directory, [switch]$AllowSyntheticFixtures)
    $records = [ordered]@{}
    $zip = [IO.Compression.ZipFile]::OpenRead([IO.Path]::GetFullPath($ArchivePath))
    try {
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName
            if ($script:allNames -cnotcontains $name -or !$names.Add($name) -or $entry.Length -eq 0) { throw "Unexpected, duplicate or empty ZIP entry: $name" }
            if ($entry.Length -gt 256MB) { throw 'Package ZIP entry exceeds the integrity verifier size limit.' }
            $stream = $entry.Open()
            try {
                $hash = [Security.Cryptography.SHA256]::Create()
                try { $digest = [Convert]::ToHexString($hash.ComputeHash($stream)).ToLowerInvariant() } finally { $hash.Dispose() }
            } finally { $stream.Dispose() }
            $records[$name] = @{ sha256 = $digest; text = $null }
            if ($name.EndsWith('.json', [StringComparison]::Ordinal)) {
                if ($entry.Length -gt 1MB) { throw 'Package JSON exceeds the integrity verifier size limit.' }
                $reader = [IO.StreamReader]::new($entry.Open())
                try { $records[$name].text = $reader.ReadToEnd() } finally { $reader.Dispose() }
            }
        }
    } finally { $zip.Dispose() }
    $package = Assert-PackageRecords $records -AllowSyntheticFixtures:$AllowSyntheticFixtures
    if ($Directory) {
        $files = Get-PackageFileRecords $Directory
        $null = Assert-PackageRecords $files -AllowSyntheticFixtures:$AllowSyntheticFixtures
        foreach ($name in $script:allNames) { if ($records[$name].sha256 -ine $files[$name].sha256) { throw "ZIP does not match assembled package: $name" } }
    }
    return $package
}

function New-KaleidoscopePackage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$WorkspaceRoot,
        [Parameter(Mandatory)][string]$LauncherPath,
        [Parameter(Mandatory)][string]$BridgeDirectory,
        [Parameter(Mandatory)][string]$LicensePath,
        [Parameter(Mandatory)][string]$ExpectedCompatibilityPath,
        [Parameter(Mandatory)][string]$OutputDirectory,
        [Parameter(Mandatory)][hashtable]$InterfaceEvidence
    )
    $output = Assert-PackageWorkspacePath $OutputDirectory $WorkspaceRoot
    $archive = Assert-PackageWorkspacePath ($output + '.zip') $WorkspaceRoot
    if ((Test-Path -LiteralPath $output) -or (Test-Path -LiteralPath $archive)) { throw 'Package output or ZIP already exists; choose a new output directory.' }
    $sources = [ordered]@{
        'Kaleidoscope.exe' = $LauncherPath; $script:bridgeName = (Join-Path $BridgeDirectory $script:bridgeName)
        'artifact.json' = (Join-Path $BridgeDirectory 'artifact.json'); 'compatibility.json' = (Join-Path $BridgeDirectory 'compatibility.json')
        'THIRD-PARTY-NOTICES.md' = (Join-Path $BridgeDirectory 'THIRD-PARTY-NOTICES.md'); 'LICENSE' = $LicensePath
        'Transfiguration-README.md' = (Join-Path $BridgeDirectory 'README.md')
    }
    foreach ($name in $script:licenseNames) { $sources["licenses/$name"] = Join-Path $BridgeDirectory "licenses/$name" }
    foreach ($name in $sources.Keys) {
        $source = Get-Item -LiteralPath $sources[$name] -Force
        if ($source.PSIsContainer -or $source.Length -eq 0 -or $source.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint)) { throw "Missing or invalid package source: $name" }
    }
    $bridgeHash = (Get-FileHash -LiteralPath $sources[$script:bridgeName] -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-BridgePackageMetadata (Read-PackageJson ([IO.File]::ReadAllText($sources['artifact.json'])) 'artifact.json') (Read-PackageJson ([IO.File]::ReadAllText($sources['compatibility.json'])) 'compatibility.json') $bridgeHash
    $expectedCompatibilityHash = (Get-FileHash -LiteralPath $ExpectedCompatibilityPath -Algorithm SHA256).Hash
    if ((Get-FileHash -LiteralPath $sources['compatibility.json'] -Algorithm SHA256).Hash -ine $expectedCompatibilityHash) { throw 'Bridge artifact compatibility differs from the tested repository manifest.' }
    Assert-PackageKeys $InterfaceEvidence @('kind', 'launcher_sha256', 'exit_code') 'interface evidence'
    if ($InterfaceEvidence.kind -isnot [string]) { throw 'Unsupported interface evidence kind.' }
    Assert-PackageHash $InterfaceEvidence.launcher_sha256 'interface evidence launcher_sha256'
    if ($InterfaceEvidence.launcher_sha256 -ine (Get-FileHash -LiteralPath $LauncherPath -Algorithm SHA256).Hash) { throw 'Launcher changed after interface verification.' }
    $synthetic = $InterfaceEvidence.kind -ceq 'synthetic_package_integrity'
    if (!$synthetic -and $InterfaceEvidence.kind -cne 'embedded_interface_command') { throw 'Unsupported interface evidence kind.' }
    if ($synthetic) {
        if ($null -ne $InterfaceEvidence.exit_code) { throw 'Synthetic integrity evidence must not claim a native command result.' }
    } elseif (($InterfaceEvidence.exit_code -isnot [int] -and $InterfaceEvidence.exit_code -isnot [long]) -or $InterfaceEvidence.exit_code -ne 0) { throw 'Embedded production interface verification did not pass.' }
    $parent = [IO.Path]::GetDirectoryName($output)
    if (!(Test-Path -LiteralPath $parent -PathType Container)) { throw 'Package parent directory must already exist.' }
    $stage = Assert-PackageWorkspacePath (Join-Path $parent ('.kaleidoscope-package-staging-' + [guid]::NewGuid().ToString('N'))) $WorkspaceRoot
    $stageZip = Assert-PackageWorkspacePath ($stage + '.zip') $WorkspaceRoot
    [IO.Directory]::CreateDirectory($stage) | Out-Null
    [IO.Directory]::CreateDirectory((Join-Path $stage 'licenses')) | Out-Null
    foreach ($name in $sources.Keys) { Copy-Item -LiteralPath $sources[$name] -Destination (Join-Path $stage $name) }
    $readme = @'
Kaleidoscope — Illyaverse local test package

Run Kaleidoscope.exe, choose the installed stable/lazer osu!.exe, then launch.
This package is fixed to https://illya.test and requires the documented local
DNS/CA setup. It is a local test artifact; public download is not promised.
Lazer updates outside the tested build stop before Main. Update the complete
launcher and bridge package together; do not copy this DLL into official rulesets.
Close the game to remove the temporary private-profile DLL. The independent
guardian survives closing this window. After an interrupted guardian or power
loss, reopen the launcher and use cleanup. Foreign or changed files are kept.

Native GUI/gameplay/login remains pending user-operated verification. Package
integrity and embedded-interface evidence do not prove a real game session.
See Transfiguration-README.md, compatibility.json, and THIRD-PARTY-NOTICES.md.
'@
    [IO.File]::WriteAllText((Join-Path $stage 'README.txt'), $readme, [Text.UTF8Encoding]::new($false))
    $hashes = [ordered]@{}
    foreach ($name in $script:payloadNames) { $hashes[$name] = (Get-FileHash -LiteralPath (Join-Path $stage $name) -Algorithm SHA256).Hash.ToLowerInvariant() }
    if ($hashes['Kaleidoscope.exe'] -ine $InterfaceEvidence.launcher_sha256 -or $hashes['compatibility.json'] -ine $expectedCompatibilityHash) { throw 'Verified launcher or compatibility changed during package assembly.' }
    # Completion metadata is the final file written, after every required payload file.
    $manifest = [ordered]@{
        version = 1; status = 'complete'; server = 'https://illya.test'; fixture = $false; bridge_sha256 = $bridgeHash
        embedded_interface_verified = !$synthetic; native_gui_verified = $false; verification_kind = $InterfaceEvidence.kind
        completed_at_utc = [DateTime]::UtcNow.ToString('o'); content_sha256 = $hashes
    }
    [IO.File]::WriteAllText((Join-Path $stage 'package.json'), ($manifest | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
    $null = Test-KaleidoscopePackage -Directory $stage -AllowSyntheticFixtures:$synthetic
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $stageZip, [IO.Compression.CompressionLevel]::Optimal, $false)
    $null = Test-KaleidoscopePackageArchive -ArchivePath $stageZip -Directory $stage -AllowSyntheticFixtures:$synthetic
    # Directory.Move/File.Move refuse existing targets; no old delivery is overwritten.
    $null = Assert-PackageWorkspacePath $stage $WorkspaceRoot
    $null = Assert-PackageWorkspacePath $output $WorkspaceRoot
    [IO.Directory]::Move($stage, $output)
    $null = Assert-PackageWorkspacePath $stageZip $WorkspaceRoot
    $null = Assert-PackageWorkspacePath $archive $WorkspaceRoot
    [IO.File]::Move($stageZip, $archive)
    return [pscustomobject]@{ Directory = $output; Archive = $archive; VerificationKind = $InterfaceEvidence.kind }
}

Export-ModuleMember -Function Assert-PackageWorkspacePath, Test-KaleidoscopePackage, Test-KaleidoscopePackageArchive, New-KaleidoscopePackage
