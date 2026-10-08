[CmdletBinding(DefaultParameterSetName = 'Directory')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Directory')][string]$Directory,
    [Parameter(Mandatory, ParameterSetName = 'Archive')][string]$ArchivePath,
    [switch]$AllowSyntheticFixtures
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'launcher-package.psm1') -Force
if ($PSCmdlet.ParameterSetName -eq 'Archive') {
    $manifest = Test-KaleidoscopePackageArchive -ArchivePath $ArchivePath -AllowSyntheticFixtures:$AllowSyntheticFixtures
} else {
    $manifest = Test-KaleidoscopePackage -Directory $Directory -AllowSyntheticFixtures:$AllowSyntheticFixtures
}
Write-Output "PASS complete package integrity ($($manifest.verification_kind)); native GUI/login remains pending."
