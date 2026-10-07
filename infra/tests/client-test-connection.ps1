# Original integration checks: temporary hosts + in-memory trust, never OS trust/hosts.
[CmdletBinding()]
param([string]$CertificatePath)
$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
if (-not $CertificatePath) { $CertificatePath = Join-Path $taskRoot 'data-client-test\caddy-root.crt' }
Import-Module (Join-Path $taskRoot 'infra\client-test-connection.psm1') -Force
$taskCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 -ArgumentList $CertificatePath
$taskHasher = [System.Security.Cryptography.SHA256]::Create()
try { $taskSha256 = [BitConverter]::ToString($taskHasher.ComputeHash($taskCertificate.RawData)).Replace('-', '') }
finally { $taskHasher.Dispose() }
$taskThumbprint = $taskCertificate.Thumbprint
$taskCertificate.Dispose()
$taskFixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('illyaverse-connection-test-' + [Guid]::NewGuid().ToString('N'))
[void][System.IO.Directory]::CreateDirectory($taskFixtureRoot)
$taskChecks = 0

function Assert-Task {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw "Assertion failed: $Message" }
}
function Assert-TaskFailure {
    param([scriptblock]$Action, [string]$Pattern)
    $taskFailed = $false
    try { & $Action | Out-Null }
    catch { $taskFailed = $true; Assert-Task ($_.Exception.Message -like "*$Pattern*") "Expected error '$Pattern', got $($_.Exception.Message)" }
    Assert-Task $taskFailed "Expected operation to reject '$Pattern'"
}
function New-TaskFixture {
    param([switch]$AlreadyTrusted)
    $taskMockFingerprint = $script:taskSha256
    $taskDirectory = Join-Path $taskFixtureRoot ([Guid]::NewGuid().ToString('N'))
    [void][System.IO.Directory]::CreateDirectory($taskDirectory)
    $taskHosts = Join-Path $taskDirectory 'hosts'
    $taskOriginal = [char]0xFEFF + "# preserve`r`n127.0.0.1 localhost"
    [System.IO.File]::WriteAllBytes($taskHosts, [System.Text.Encoding]::UTF8.GetBytes($taskOriginal))
    $taskStore = @{}
    if ($AlreadyTrusted) { $taskStore['CurrentUser:' + $taskThumbprint] = [pscustomobject]@{sha256 = $taskSha256; thumbprint = $taskThumbprint} }
    $taskControl = @{ adds = 0; removes = 0; failAdd = $false; failRemove = $false; mutateOnGet = $false; gets = 0 }
    $taskProvider = @{
        Get = {
            param($Thumbprint, $Scope)
            $taskControl.gets++
            if ($taskControl.mutateOnGet -and $taskControl.gets -eq 1) {
                [System.IO.File]::AppendAllText($taskHosts, "`r`n10.0.0.2 raced.test`r`n", [System.Text.Encoding]::UTF8)
            }
            return $taskStore[$Scope + ':' + $Thumbprint]
        }.GetNewClosure()
        Add = {
            param($Certificate, $Scope)
            $taskControl.adds++
            if ($taskControl.failAdd) { throw 'Injected trust-add failure' }
            $taskKey = $Scope + ':' + $Certificate.Thumbprint
            if ($taskStore.ContainsKey($taskKey)) { return $false }
            $taskStore[$taskKey] = [pscustomobject]@{sha256 = $taskMockFingerprint; thumbprint = $Certificate.Thumbprint}
            return $true
        }.GetNewClosure()
        Remove = {
            param($Thumbprint, $ExpectedSha256, $Scope)
            $taskControl.removes++
            if ($taskControl.failRemove) { throw 'Injected trust-remove failure' }
            $taskKey = $Scope + ':' + $Thumbprint
            if ($taskStore.ContainsKey($taskKey) -and $taskStore[$taskKey].sha256 -ne $ExpectedSha256) { throw 'Mock certificate fingerprint mismatch' }
            $taskStore.Remove($taskKey)
        }.GetNewClosure()
    }
    return [pscustomobject]@{
        directory = $taskDirectory; hosts = $taskHosts; original = $taskOriginal; store = $taskStore; control = $taskControl
        parameters = @{
            CertificatePath = $CertificatePath; HostsPath = $taskHosts
            BinaryPath = Join-Path $taskRoot 'target\release\kaleidoscope.exe'
            StatePath = Join-Path $taskDirectory 'journal.json'; TrustProvider = $taskProvider
        }
    }
}
function Read-TaskHosts {
    param($Fixture)
    return [System.Text.Encoding]::UTF8.GetString([System.IO.File]::ReadAllBytes($Fixture.hosts))
}
function Invoke-TaskApply {
    param($Fixture)
    $taskParameters = $Fixture.parameters
    Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -ExpectedSha256 $taskSha256 | Out-Null
}
function Invoke-TaskRestore {
    param($Fixture)
    $taskParameters = $Fixture.parameters
    Invoke-ClientTestConnection @taskParameters -Mode Restore -Write | Out-Null
}

try {
    $taskFixture = New-TaskFixture
    $taskParameters = $taskFixture.parameters
    foreach ($taskMode in @('Preview', 'Apply', 'Restore')) { Invoke-ClientTestConnection @taskParameters -Mode $taskMode | Out-Null }
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskFixture.original) 'Preview preserves every hosts byte including BOM'
    Assert-Task ((Get-ChildItem -LiteralPath $taskFixture.directory).Count -eq 1) 'Preview creates no journal, lock or backup'
    Assert-Task ($taskFixture.control.adds -eq 0 -and $taskFixture.control.removes -eq 0) 'Preview never writes trust'
    $taskChecks++

    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -ExpectedSha256 ('0' * 64) } 'SHA-256 does not match'
    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Apply -Write } 'requires -ExpectedSha256'
    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -ExpectedSha256 $taskSha256 -ExpectedThumbprint ('0' * 40) } 'thumbprint does not match'
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskFixture.original) 'Rejected fingerprints do not alter hosts'
    Assert-Task ($taskFixture.store.Count -eq 0) 'Rejected fingerprints do not alter trust'
    $taskChecks++

    $taskBundle = Join-Path $taskFixture.directory 'bundle.pem'
    [System.IO.File]::WriteAllText($taskBundle, ([System.IO.File]::ReadAllText($CertificatePath) * 2))
    $taskParameters.CertificatePath = $taskBundle
    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -ExpectedSha256 $taskSha256 } 'exactly one exported public PEM'
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskFixture.original -and $taskFixture.store.Count -eq 0) 'Certificate bundles are rejected before mutation'
    $taskParameters.CertificatePath = $CertificatePath
    $taskChecks++

    Invoke-TaskApply $taskFixture
    $taskInstalled = Read-TaskHosts $taskFixture
    Assert-Task ($taskFixture.control.adds -eq 1 -and $taskFixture.store.Count -eq 1) 'Apply imports exactly one public CA'
    Assert-Task ($taskInstalled.Contains('127.0.0.1' + "`t" + 'b.illya.test')) 'Apply includes all planned domains'
    Assert-TaskFailure { Invoke-TaskApply $taskFixture } 'active connection journal'
    Invoke-ClientTestConnection @taskParameters -Mode Restore | Out-Null
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskInstalled) 'Restore preview is read-only'
    Invoke-TaskRestore $taskFixture
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskFixture.original) 'Restore exactly recovers original BOM and missing final newline'
    Assert-Task ($taskFixture.control.removes -eq 1 -and $taskFixture.store.Count -eq 0) 'Restore removes only helper-added CA'
    Invoke-TaskRestore $taskFixture
    Assert-Task ($taskFixture.control.removes -eq 1) 'Repeated restore is idempotent'
    $taskChecks++

    $taskFixture = New-TaskFixture -AlreadyTrusted
    $taskParameters = $taskFixture.parameters
    Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -TrustScope LocalMachine -ExpectedSha256 $taskSha256 | Out-Null
    Assert-Task ($taskFixture.store.Count -eq 2) 'LocalMachine apply does not replace separate CurrentUser trust'
    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Restore -Write -ExpectedSha256 ('0' * 64) } 'Restore SHA-256'
    Assert-Task ($taskFixture.control.removes -eq 0) 'Mismatched restore fingerprint is rejected'
    # Restore does not need the current export: the Caddy CA may have been regenerated.
    $taskParameters.CertificatePath = Join-Path $taskFixture.directory 'no-longer-exported.pem'
    Invoke-TaskRestore $taskFixture
    Assert-Task ($taskFixture.store.ContainsKey('CurrentUser:' + $taskThumbprint) -and -not $taskFixture.store.ContainsKey('LocalMachine:' + $taskThumbprint)) 'Restore follows journal scope and preserves pre-existing trust'
    $taskChecks++

    $taskFixture = New-TaskFixture -AlreadyTrusted
    Invoke-TaskApply $taskFixture
    [System.IO.File]::AppendAllText($taskFixture.hosts, "10.0.0.3 keep.test`r`n", [System.Text.Encoding]::UTF8)
    Invoke-TaskRestore $taskFixture
    Assert-Task ($taskFixture.control.adds -eq 0 -and $taskFixture.control.removes -eq 0 -and $taskFixture.store.Count -eq 1) 'Existing trust is preserved'
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq ($taskFixture.original + "`r`n10.0.0.3 keep.test`r`n")) 'Unrelated hosts edits survive restore'
    $taskChecks++

    $taskFixture = New-TaskFixture
    Invoke-TaskApply $taskFixture
    $taskInstalled = Read-TaskHosts $taskFixture
    $taskTampered = $taskInstalled.Replace("127.0.0.1`tapi.illya.test", "127.0.0.2`tapi.illya.test")
    [System.IO.File]::WriteAllBytes($taskFixture.hosts, [System.Text.Encoding]::UTF8.GetBytes($taskTampered))
    Assert-TaskFailure { Invoke-TaskRestore $taskFixture } 'managed hosts block changed'
    Assert-Task ($taskFixture.control.removes -eq 0 -and (Read-TaskHosts $taskFixture) -ceq $taskTampered) 'Changed managed block is not removed'
    [System.IO.File]::WriteAllBytes($taskFixture.hosts, [System.Text.Encoding]::UTF8.GetBytes($taskInstalled))
    Invoke-TaskRestore $taskFixture
    $taskChecks++

    $taskFixture = New-TaskFixture
    Invoke-TaskApply $taskFixture
    $taskFixture.store['CurrentUser:' + $taskThumbprint].sha256 = '0' * 64
    Assert-TaskFailure { Invoke-TaskRestore $taskFixture } 'Stored certificate does not match'
    Assert-Task ($taskFixture.control.removes -eq 0) 'Changed trust certificate contents are not removed'
    $taskFixture.store['CurrentUser:' + $taskThumbprint].sha256 = $taskSha256
    Invoke-TaskRestore $taskFixture
    $taskChecks++

    $taskFixture = New-TaskFixture
    $taskParameters = $taskFixture.parameters
    [System.IO.File]::AppendAllText($taskFixture.hosts, "`r`n# BEGIN ILLYAVERSE KALEIDOSCOPE`r`n127.0.0.1 previous.test`r`n# END ILLYAVERSE KALEIDOSCOPE`r`n")
    $taskBefore = Read-TaskHosts $taskFixture
    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -ExpectedSha256 $taskSha256 } 'block already exists'
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskBefore -and $taskFixture.store.Count -eq 0) 'Pre-existing managed block is preserved'
    $taskChecks++

    $taskFixture = New-TaskFixture
    $taskParameters = $taskFixture.parameters
    [System.IO.File]::AppendAllText($taskFixture.hosts, "`r`n127.0.0.1 osu.illya.test`r`n")
    Assert-TaskFailure { Invoke-ClientTestConnection @taskParameters -Mode Apply -Write -ExpectedSha256 $taskSha256 } 'outside the Kaleidoscope block'
    Assert-Task ($taskFixture.store.Count -eq 0 -and -not (Test-Path -LiteralPath $taskParameters.StatePath)) 'Conflicting unmanaged entry is rejected before mutation'
    $taskChecks++

    $taskFixture = New-TaskFixture
    $taskFixture.control.failAdd = $true
    Assert-TaskFailure { Invoke-TaskApply $taskFixture } 'Injected trust-add failure'
    $taskState = Get-Content -LiteralPath $taskFixture.parameters.StatePath -Raw | ConvertFrom-Json
    Assert-Task ($taskState.status -eq 'incomplete' -and $taskState.hostsApplied) 'Partial apply keeps recoverable journal'
    Invoke-TaskRestore $taskFixture
    Assert-Task ((Read-TaskHosts $taskFixture) -ceq $taskFixture.original) 'Partial apply can restore hosts without importing trust'
    $taskChecks++

    $taskFixture = New-TaskFixture
    Invoke-TaskApply $taskFixture
    $taskFixture.control.failRemove = $true
    Assert-TaskFailure { Invoke-TaskRestore $taskFixture } 'Injected trust-remove failure'
    $taskFixture.control.failRemove = $false
    Invoke-TaskRestore $taskFixture
    Assert-Task ($taskFixture.store.Count -eq 0 -and (Read-TaskHosts $taskFixture) -ceq $taskFixture.original) 'Partial restore can retry safely'
    $taskChecks++

    $taskFixture = New-TaskFixture
    $taskFixture.control.mutateOnGet = $true
    Assert-TaskFailure { Invoke-TaskApply $taskFixture } 'reviewed snapshot'
    Assert-Task ($taskFixture.store.Count -eq 0 -and (Read-TaskHosts $taskFixture).Contains('raced.test')) 'Race rejects hosts write and preserves the external edit'
    Invoke-TaskRestore $taskFixture
    Assert-Task ((Read-TaskHosts $taskFixture).Contains('raced.test')) 'Race recovery retains the external edit'
    $taskChecks++

    Write-Output "PASS: $taskChecks connection helper scenarios; temporary hosts and in-memory trust only."
} finally {
    $taskResolvedFixture = [System.IO.Path]::GetFullPath($taskFixtureRoot)
    $taskResolvedTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\', '/')
    if ([System.IO.Path]::GetDirectoryName($taskResolvedFixture) -ne $taskResolvedTemp -or
        -not [System.IO.Path]::GetFileName($taskResolvedFixture).StartsWith('illyaverse-connection-test-')) {
        throw 'Refusing to clean up an unexpected fixture path.'
    }
    Remove-Item -LiteralPath $taskResolvedFixture -Recurse -Force
}
