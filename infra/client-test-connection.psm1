Set-StrictMode -Version Latest

function Get-ConnectionSha256 {
    param([byte[]]$Bytes)
    $taskHasher = [System.Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($taskHasher.ComputeHash($Bytes)).Replace('-', '') }
    finally { $taskHasher.Dispose() }
}

function Assert-ConnectionPath {
    param([string]$Path, [switch]$MustExist)
    $taskPath = [System.IO.Path]::GetFullPath($Path)
    if ($MustExist -and -not [System.IO.File]::Exists($taskPath)) { throw "File is missing: $taskPath" }
    $taskCursor = $taskPath
    while ($taskCursor) {
        if (Test-Path -LiteralPath $taskCursor) {
            $taskItem = Get-Item -LiteralPath $taskCursor -Force
            if ($taskItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
                throw "Refusing a reparse point in the connection path: $taskCursor"
            }
        }
        $taskParent = [System.IO.Path]::GetDirectoryName($taskCursor)
        if ($taskParent -eq $taskCursor) { break }
        $taskCursor = $taskParent
    }
    return $taskPath
}

function Read-ConnectionHosts {
    param([string]$Path)
    [void](Assert-ConnectionPath -Path $Path -MustExist)
    $taskBytes = [System.IO.File]::ReadAllBytes($Path)
    if ($taskBytes.Length -gt 1048576) { throw 'Hosts exceeds the 1 MiB safety limit.' }
    $taskEncoding = New-Object System.Text.UTF8Encoding($false, $true)
    return $taskEncoding.GetString($taskBytes)
}

function Get-ConnectionBlockHash {
    param([string]$Text)
    $taskMatches = [regex]::Matches($Text, '(?ms)^\uFEFF?# BEGIN ILLYAVERSE KALEIDOSCOPE\r?\n.*?^# END ILLYAVERSE KALEIDOSCOPE(?=\r?$)')
    if ($taskMatches.Count -gt 1) { throw 'Multiple Kaleidoscope blocks are present.' }
    if ($taskMatches.Count -eq 0) { return $null }
    $taskBlock = $taskMatches[0].Value.TrimStart([char]0xFEFF).Replace("`r`n", "`n")
    return Get-ConnectionSha256 -Bytes ([System.Text.Encoding]::UTF8.GetBytes($taskBlock))
}

function Invoke-ConnectionKaleidoscope {
    param([string]$Binary, [string[]]$Arguments)
    $taskPreviousNativeErrors = $ErrorActionPreference
    try {
        # Native stderr is collected so a rejected plan cannot accidentally continue.
        $ErrorActionPreference = 'Continue'
        $taskOutput = @(& $Binary @Arguments 2>&1)
        $taskExitCode = $LASTEXITCODE
    } finally { $ErrorActionPreference = $taskPreviousNativeErrors }
    $taskText = ($taskOutput | ForEach-Object { $_.ToString() }) -join "`n"
    if ($taskExitCode -ne 0) { throw "Kaleidoscope rejected the operation:`n$taskText" }
    return $taskText
}

function Save-ConnectionState {
    param([string]$Path, $State)
    [void](Assert-ConnectionPath -Path $Path)
    $taskStaging = $Path + '.staging-' + [Guid]::NewGuid().ToString('N')
    $taskBytes = [System.Text.Encoding]::UTF8.GetBytes(($State | ConvertTo-Json -Depth 6))
    $taskStream = [System.IO.File]::Open($taskStaging, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $taskStream.Write($taskBytes, 0, $taskBytes.Length); $taskStream.Flush() }
    finally { $taskStream.Dispose() }
    try {
        if ([System.IO.File]::Exists($Path)) { [System.IO.File]::Replace($taskStaging, $Path, [System.Management.Automation.Language.NullString]::Value) }
        else { [System.IO.File]::Move($taskStaging, $Path) }
    } finally {
        if ([System.IO.File]::Exists($taskStaging)) { [System.IO.File]::Delete($taskStaging) }
    }
}

function Save-ConnectionSnapshot {
    param([string]$Path, [string]$Text)
    $taskBytes = [System.Text.Encoding]::UTF8.GetBytes($Text)
    $taskStream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $taskStream.Write($taskBytes, 0, $taskBytes.Length); $taskStream.Flush() }
    finally { $taskStream.Dispose() }
}

function Read-ConnectionState {
    param([string]$Path, [string]$HostsPath)
    if (-not [System.IO.File]::Exists($Path)) { return $null }
    [void](Assert-ConnectionPath -Path $Path -MustExist)
    if ((Get-Item -LiteralPath $Path).Length -gt 65536) { throw 'Connection journal is too large.' }
    $taskState = [System.IO.File]::ReadAllText($Path) | ConvertFrom-Json
    if ($taskState.schemaVersion -ne 1 -or $taskState.hostsPath -ne $HostsPath -or
        $taskState.trustScope -notin @('CurrentUser', 'LocalMachine') -or
        $taskState.certificateSha256 -notmatch '^[A-F0-9]{64}$' -or
        $taskState.certificateThumbprint -notmatch '^[A-F0-9]{40}$' -or
        $taskState.ownedBlockSha256 -notmatch '^[A-F0-9]{64}$' -or
        $taskState.status -notin @('applying', 'active', 'restoring', 'restored', 'incomplete')) {
        throw 'Connection journal is invalid or belongs to another hosts file.'
    }
    foreach ($taskProperty in @('trustOriginallyPresent', 'trustAdded', 'trustAddIntent', 'hostsApplyIntent', 'hostsApplied')) {
        if ($taskState.$taskProperty -isnot [bool]) { throw 'Connection journal contains an invalid flag.' }
    }
    return $taskState
}

function New-WindowsConnectionTrustProvider {
    return @{
        Get = {
            param($Thumbprint, $Scope)
            $taskStore = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root', $Scope)
            try {
                $taskStore.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly)
                $taskMatches = $taskStore.Certificates.Find('FindByThumbprint', $Thumbprint, $false)
                if ($taskMatches.Count -eq 0) { return $null }
                return [pscustomobject]@{
                    sha256 = Get-ConnectionSha256 -Bytes $taskMatches[0].RawData
                    thumbprint = $taskMatches[0].Thumbprint
                }
            } finally { $taskStore.Close() }
        }
        Add = {
            param($Certificate, $Scope)
            $taskStore = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root', $Scope)
            try {
                $taskStore.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                $taskMatches = $taskStore.Certificates.Find('FindByThumbprint', $Certificate.Thumbprint, $false)
                if ($taskMatches.Count -gt 0) {
                    if ((Get-ConnectionSha256 -Bytes $taskMatches[0].RawData) -ne (Get-ConnectionSha256 -Bytes $Certificate.RawData)) {
                        throw 'Existing certificate thumbprint has different contents.'
                    }
                    return $false
                }
                $taskStore.Add($Certificate)
                return $true
            } finally { $taskStore.Close() }
        }
        Remove = {
            param($Thumbprint, $ExpectedSha256, $Scope)
            $taskStore = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root', $Scope)
            try {
                $taskStore.Open([System.Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
                $taskMatches = $taskStore.Certificates.Find('FindByThumbprint', $Thumbprint, $false)
                foreach ($taskCertificate in $taskMatches) {
                    if ((Get-ConnectionSha256 -Bytes $taskCertificate.RawData) -ne $ExpectedSha256) {
                        throw 'Refusing to remove a certificate with different contents.'
                    }
                }
                foreach ($taskCertificate in $taskMatches) { $taskStore.Remove($taskCertificate) }
            } finally { $taskStore.Close() }
        }
    }
}

function Invoke-ClientTestConnection {
    [CmdletBinding()]
    param(
        [ValidateSet('Preview', 'Apply', 'Restore')][string]$Mode = 'Preview',
        [switch]$Write,
        [string]$CertificatePath,
        [string]$ExpectedSha256,
        [string]$ExpectedThumbprint,
        [ValidateSet('CurrentUser', 'LocalMachine')][string]$TrustScope = 'CurrentUser',
        [string]$HostsPath,
        [string]$BinaryPath,
        [string]$StatePath,
        [hashtable]$TrustProvider
    )
    $ErrorActionPreference = 'Stop'
    if ($Write -and $Mode -eq 'Preview') { throw 'Preview cannot be combined with -Write. Select Apply or Restore explicitly.' }
    foreach ($taskMethod in @('Get', 'Add', 'Remove')) {
        if ($TrustProvider[$taskMethod] -isnot [scriptblock]) { throw 'Invalid trust provider.' }
    }
    $HostsPath = Assert-ConnectionPath -Path $HostsPath -MustExist
    $BinaryPath = Assert-ConnectionPath -Path $BinaryPath -MustExist
    $StatePath = Assert-ConnectionPath -Path $StatePath
    $taskState = Read-ConnectionState -Path $StatePath -HostsPath $HostsPath
    $taskLock = $null
    $taskSnapshot = $null
    $taskCertificate = $null
    try {
        if ($Write) {
            [void][System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($StatePath))
            [void](Assert-ConnectionPath -Path $StatePath)
            $taskLock = [System.IO.File]::Open($StatePath + '.lock', [System.IO.FileMode]::CreateNew,
                [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
            # Re-read inside the lock in case another helper completed after preview.
            $taskState = Read-ConnectionState -Path $StatePath -HostsPath $HostsPath
        }
        $taskCurrentHosts = Read-ConnectionHosts -Path $HostsPath
        $taskCurrentBlock = Get-ConnectionBlockHash -Text $taskCurrentHosts
        if ($Mode -eq 'Restore') {
            if (-not $taskState -or $taskState.status -eq 'restored') {
                Write-Output 'No active helper operation to restore. Nothing changed.'
                return
            }
            if ($ExpectedSha256 -and $ExpectedSha256.Replace(':', '').Replace(' ', '').ToUpperInvariant() -ne $taskState.certificateSha256) {
                throw 'Restore SHA-256 does not match the recorded operation.'
            }
            if ($ExpectedThumbprint -and $ExpectedThumbprint.Replace(' ', '').ToUpperInvariant() -ne $taskState.certificateThumbprint) {
                throw 'Restore thumbprint does not match the recorded operation.'
            }
            if ($taskCurrentBlock -and $taskCurrentBlock -ne $taskState.ownedBlockSha256) {
                throw 'The managed hosts block changed after apply. Review it manually; nothing has been restored.'
            }
            $taskStoredTrust = & ($TrustProvider.Get) $taskState.certificateThumbprint $taskState.trustScope
            if ($taskStoredTrust -and $taskStoredTrust.sha256 -ne $taskState.certificateSha256) {
                throw 'Stored certificate does not match the reviewed SHA-256. Nothing has been restored.'
            }
            Write-Output "Restore scope: $($taskState.trustScope)\Root; certificate: $($taskState.certificateThumbprint)"
            $taskRemoveTrust = (-not $taskState.trustOriginallyPresent) -and ($taskState.trustAdded -or $taskState.trustAddIntent)
            Write-Output "Remove helper-added certificate: $taskRemoveTrust; preserve prior trust: $($taskState.trustOriginallyPresent)"
            Write-Output (Invoke-ConnectionKaleidoscope -Binary $BinaryPath -Arguments @('restore', '--hosts', $HostsPath))
            if (-not $Write) { Write-Output 'Restore preview only. Use -Mode Restore -Write to make these changes.'; return }
            $taskState.status = 'restoring'
            Save-ConnectionState -Path $StatePath -State $taskState
            if ($taskCurrentBlock) {
                $taskSnapshot = $StatePath + '.snapshot-' + [Guid]::NewGuid().ToString('N')
                Save-ConnectionSnapshot -Path $taskSnapshot -Text $taskCurrentHosts
                Write-Output (Invoke-ConnectionKaleidoscope -Binary $BinaryPath -Arguments @('restore', '--hosts', $HostsPath, '--expect-current', $taskSnapshot, '--write'))
            }
            $taskState.hostsApplied = $false
            $taskState.hostsApplyIntent = $false
            Save-ConnectionState -Path $StatePath -State $taskState
            if ($taskRemoveTrust) {
                & ($TrustProvider.Remove) $taskState.certificateThumbprint $taskState.certificateSha256 $taskState.trustScope
                $taskState.trustAdded = $false
                $taskState.trustAddIntent = $false
            }
            $taskState.status = 'restored'
            Save-ConnectionState -Path $StatePath -State $taskState
            Write-Output 'Restored this helper operation. Unrelated hosts records and prior certificate trust were preserved.'
            return
        }

        $CertificatePath = Assert-ConnectionPath -Path $CertificatePath -MustExist
        if ((Get-Item -LiteralPath $CertificatePath).Length -gt 65536) { throw 'Public CA certificate exceeds the 64 KiB safety limit.' }
        $taskPublicPem = [System.IO.File]::ReadAllText($CertificatePath)
        $taskPemMatch = [regex]::Match($taskPublicPem, '\A\s*-----BEGIN CERTIFICATE-----\s*([A-Za-z0-9+/=\r\n\t ]+)\s*-----END CERTIFICATE-----\s*\z')
        if ($taskPublicPem.Length -gt 65536 -or -not $taskPemMatch.Success) {
            throw 'Provide exactly one exported public PEM CA certificate, never a private key or bundle.'
        }
        $taskPublicDer = [Convert]::FromBase64String($taskPemMatch.Groups[1].Value)
        $taskCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 -ArgumentList (,$taskPublicDer)
        $taskCaExtensions = @($taskCertificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.19' })
        if ($taskCertificate.HasPrivateKey -or $taskCertificate.Subject -ne $taskCertificate.Issuer -or
            $taskCaExtensions.Count -ne 1 -or -not $taskCaExtensions[0].CertificateAuthority -or
            $taskCertificate.NotBefore -gt (Get-Date) -or $taskCertificate.NotAfter -le (Get-Date)) {
            throw 'Certificate must be a currently valid, self-issued public CA certificate.'
        }
        $taskSha256 = Get-ConnectionSha256 -Bytes $taskCertificate.RawData
        if ($ExpectedSha256) {
            $ExpectedSha256 = $ExpectedSha256.Replace(':', '').Replace(' ', '').ToUpperInvariant()
            if ($ExpectedSha256 -notmatch '^[A-F0-9]{64}$' -or $ExpectedSha256 -ne $taskSha256) { throw 'CA SHA-256 does not match the expected fingerprint.' }
        }
        if ($ExpectedThumbprint) {
            $ExpectedThumbprint = $ExpectedThumbprint.Replace(' ', '').ToUpperInvariant()
            if ($ExpectedThumbprint -notmatch '^[A-F0-9]{40}$' -or $ExpectedThumbprint -ne $taskCertificate.Thumbprint) { throw 'CA thumbprint does not match the expected thumbprint.' }
        }
        $taskPriorTrust = & ($TrustProvider.Get) $taskCertificate.Thumbprint $TrustScope
        if ($taskPriorTrust -and $taskPriorTrust.sha256 -ne $taskSha256) { throw 'Existing certificate contents differ from this CA.' }
        Write-Output "CA subject: $($taskCertificate.Subject)"
        Write-Output "CA valid from: $($taskCertificate.NotBefore.ToString('o'))"
        Write-Output "CA expires: $($taskCertificate.NotAfter.ToString('o'))"
        Write-Output "SHA-256: $taskSha256"
        Write-Output "Thumbprint: $($taskCertificate.Thumbprint)"
        Write-Output "Trust store: $TrustScope\Root; already trusted: $([bool]$taskPriorTrust)"
        Write-Output "Hosts file: $HostsPath"
        Write-Output 'Website: https://illya.test; API: https://osu.illya.test/api/v2'
        Write-Output 'SignalR: https://spectator.illya.test/{spectator,multiplayer,metadata}'
        Write-Output 'Stable launch preview: osu!.exe -devserver illya.test'
        Write-Output 'Lazer requires an independent source build with these endpoints; stock-client switching remains unverified. No clients will be launched.'
        if ($taskState -and $taskState.status -ne 'restored') {
            Write-Output "Existing operation: $($taskState.status). Restore it before applying again."
            if ($Write) { throw 'An active connection journal already exists.' }
            return
        }
        if ($taskCurrentBlock) { throw 'A Kaleidoscope block already exists without this active helper journal. Preserve it and review manually; this helper will not replace it.' }
        $taskDomainArguments = @('--ip', '127.0.0.1')
        foreach ($taskDomain in @('illya.test', 'osu.illya.test', 'c.illya.test', 'ce.illya.test', 'c4.illya.test', 'api.illya.test', 'spectator.illya.test', 'bss.illya.test', 'a.illya.test', 'b.illya.test')) {
            $taskDomainArguments += @('--domain', $taskDomain)
        }
        $taskPlan = Invoke-ConnectionKaleidoscope -Binary $BinaryPath -Arguments (@('apply', '--hosts', $HostsPath) + $taskDomainArguments)
        Write-Output $taskPlan
        if (-not $Write) { Write-Output 'Preview only. Apply requires -Mode Apply -Write -ExpectedSha256 with the inspected fingerprint.'; return }
        if (-not $ExpectedSha256) { throw 'Apply requires -ExpectedSha256 from the inspected public CA fingerprint.' }
        $taskPlannedBlock = Get-ConnectionBlockHash -Text $taskPlan
        if (-not $taskPlannedBlock) { throw 'Kaleidoscope preview did not contain the expected managed block.' }
        $taskState = [pscustomobject]@{
            schemaVersion = 1; status = 'applying'; hostsPath = $HostsPath; trustScope = $TrustScope
            certificateSha256 = $taskSha256; certificateThumbprint = $taskCertificate.Thumbprint
            trustOriginallyPresent = [bool]$taskPriorTrust; trustAdded = $false; trustAddIntent = $false
            hostsApplied = $false; hostsApplyIntent = $true; ownedBlockSha256 = $taskPlannedBlock
        }
        Save-ConnectionState -Path $StatePath -State $taskState
        $taskSnapshot = $StatePath + '.snapshot-' + [Guid]::NewGuid().ToString('N')
        Save-ConnectionSnapshot -Path $taskSnapshot -Text $taskCurrentHosts
        Write-Output (Invoke-ConnectionKaleidoscope -Binary $BinaryPath -Arguments (@('apply', '--hosts', $HostsPath, '--expect-current', $taskSnapshot, '--write') + $taskDomainArguments))
        if ((Get-ConnectionBlockHash -Text (Read-ConnectionHosts -Path $HostsPath)) -ne $taskState.ownedBlockSha256) { throw 'Applied hosts block did not match the reviewed plan.' }
        $taskState.hostsApplied = $true
        $taskState.hostsApplyIntent = $false
        if (-not $taskPriorTrust) { $taskState.trustAddIntent = $true }
        Save-ConnectionState -Path $StatePath -State $taskState
        if (-not $taskPriorTrust) { $taskState.trustAdded = [bool](& ($TrustProvider.Add) $taskCertificate $TrustScope) }
        $taskState.trustAddIntent = $false
        $taskState.status = 'active'
        Save-ConnectionState -Path $StatePath -State $taskState
        Write-Output 'Applied local hosts and the inspected public CA. No game client or account settings were changed.'
    } catch {
        if ($Write -and $taskState -and $taskState.status -in @('applying', 'restoring')) {
            $taskState.status = 'incomplete'
            Save-ConnectionState -Path $StatePath -State $taskState
            Write-Warning 'Operation incomplete. Its journal was preserved; inspect -Mode Restore, then explicitly restore it.'
        }
        throw
    } finally {
        if ($taskSnapshot -and [System.IO.File]::Exists($taskSnapshot)) { [System.IO.File]::Delete($taskSnapshot) }
        if ($taskLock) { $taskLock.Dispose(); [System.IO.File]::Delete($StatePath + '.lock') }
        if ($taskCertificate) { $taskCertificate.Dispose() }
    }
}

Export-ModuleMember -Function Invoke-ClientTestConnection, New-WindowsConnectionTrustProvider
