[CmdletBinding()]
param(
    [ValidateSet('Preview', 'Apply', 'Restore')][string]$Mode = 'Preview',
    [switch]$Write,
    [string]$ExpectedSha256,
    [string]$ExpectedThumbprint,
    [ValidateSet('CurrentUser', 'LocalMachine')][string]$TrustScope = 'CurrentUser',
    [string]$CertificatePath,
    [string]$HostsPath,
    [string]$BinaryPath
)
$ErrorActionPreference = 'Stop'
if ($Write -and $Mode -eq 'Preview') { throw 'Preview cannot be combined with -Write. Select Apply or Restore explicitly.' }
if ($env:OS -ne 'Windows_NT') { throw 'This connection wrapper is for Windows.' }
$taskRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $CertificatePath) { $CertificatePath = Join-Path $taskRoot 'data-client-test\caddy-root.crt' }
if (-not $HostsPath) { $HostsPath = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts' }
if (-not $BinaryPath) { $BinaryPath = Join-Path $taskRoot 'target\release\kaleidoscope.exe' }
$taskStatePath = Join-Path $taskRoot 'data-client-test\connection-state.json'
if ($Write) {
    $taskSystemHosts = [System.IO.Path]::GetFullPath((Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'))
    $taskNeedsElevation = [System.IO.Path]::GetFullPath($HostsPath).Equals($taskSystemHosts, [StringComparison]::OrdinalIgnoreCase) -or $TrustScope -eq 'LocalMachine'
    if ($Mode -eq 'Restore' -and (Test-Path -LiteralPath $taskStatePath)) {
        $taskPreviousScope = ([System.IO.File]::ReadAllText($taskStatePath) | ConvertFrom-Json).trustScope
        if ($taskPreviousScope -eq 'LocalMachine') { $taskNeedsElevation = $true }
    }
    if ($taskNeedsElevation) {
        $taskIdentity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
        try {
            $taskPrincipal = New-Object System.Security.Principal.WindowsPrincipal -ArgumentList $taskIdentity
            if (-not $taskPrincipal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
                throw 'Writing Windows system hosts or LocalMachine trust requires an elevated terminal. Preview needs no elevation. The helper will not elevate itself.'
            }
        } finally { $taskIdentity.Dispose() }
    }
}
Import-Module (Join-Path $PSScriptRoot 'client-test-connection.psm1') -Force
Invoke-ClientTestConnection -Mode $Mode -Write:$Write -CertificatePath $CertificatePath `
    -ExpectedSha256 $ExpectedSha256 -ExpectedThumbprint $ExpectedThumbprint -TrustScope $TrustScope `
    -HostsPath $HostsPath -BinaryPath $BinaryPath -StatePath $taskStatePath `
    -TrustProvider (New-WindowsConnectionTrustProvider)
