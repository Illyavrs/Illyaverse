[CmdletBinding()]
param([switch]$Prepare, [switch]$Build)
$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
$taskCheckout = Join-Path $taskRoot 'data-lazer-client\upstream'
$taskCommit = '6cbcae7fbb764839a66c1adf564c2e1679242072'
$taskProject = Join-Path $taskCheckout 'osu.Desktop\osu.Desktop.csproj'
$taskAdapter = Join-Path $PSScriptRoot 'IllyaverseTestGame.cs'
Write-Output "Official upstream: https://github.com/ppy/osu.git @ $taskCommit (2026.1005.1-tachyon)"
Write-Output "Ignored checkout: $taskCheckout"
Write-Output 'Original adapter: illya.test endpoints, separate game name and IPC, public local OAuth identity, no update checks or automatic stable import.'
if (-not $Prepare -and -not $Build) {
    Write-Output 'Preview only. -Prepare downloads a new pinned checkout and applies the original adapter. -Build compiles that prepared checkout; no mode launches a client.'
    return
}
$taskAncestor = $taskCheckout
while ($taskAncestor) {
    if ((Test-Path -LiteralPath $taskAncestor) -and
        ((Get-Item -LiteralPath $taskAncestor -Force).Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing a reparse point in the independent client checkout path: $taskAncestor"
    }
    $taskAncestor = [System.IO.Path]::GetDirectoryName($taskAncestor)
}
function Invoke-TaskGit {
    param([string[]]$Arguments)
    & git @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Git command failed; no installed client files were changed.' }
}
function Replace-TaskContract {
    param([string]$Text, [string]$Before, [string]$After)
    if (($Text.Split([string[]]@($Before), [StringSplitOptions]::None)).Length -ne 2) {
        throw "Pinned bootstrap contract changed or is ambiguous: $Before"
    }
    return $Text.Replace($Before, $After)
}
if ($Prepare) {
    if (Test-Path -LiteralPath $taskCheckout) { throw 'Checkout already exists. It is preserved; do not reset it. Use -Build on a prepared checkout.' }
    [void][System.IO.Directory]::CreateDirectory($taskCheckout)
    Invoke-TaskGit @('init', $taskCheckout)
    Invoke-TaskGit @('-C', $taskCheckout, 'remote', 'add', 'origin', 'https://github.com/ppy/osu.git')
    Invoke-TaskGit @('-C', $taskCheckout, 'fetch', '--depth=1', 'origin', $taskCommit)
    Invoke-TaskGit @('-C', $taskCheckout, 'checkout', '--detach', $taskCommit)
    $taskProgramPath = Join-Path $taskCheckout 'osu.Desktop\Program.cs'
    $taskProgram = [System.IO.File]::ReadAllText($taskProgramPath)
    $taskProgram = Replace-TaskContract $taskProgram 'new OsuGameDesktop(args)' 'new IllyaverseTestGame(args)'
    $taskProgram = Replace-TaskContract $taskProgram '@"osu-development"' '@"illyaverse-lazer-test"'
    $taskProgram = Replace-TaskContract $taskProgram '@"osu"' '@"illyaverse-lazer-test"'
    $taskProgram = Replace-TaskContract $taskProgram 'OsuGame.IPC_PIPE_NAME' '"illyaverse-lazer-test-ipc"'
    $taskProgram = Replace-TaskContract $taskProgram 'setupVelopack(args);' ('Environment.SetEnvironmentVariable("OSU_EXTERNAL_UPDATE_PROVIDER", "Illyaverse source test");' + [Environment]::NewLine + '            setupVelopack(args);')
    $taskProgram = Replace-TaskContract $taskProgram 'legacyIpc.Bind();' '// Illyaverse test client does not bind the official legacy IPC port.'
    [System.IO.File]::WriteAllText($taskProgramPath, $taskProgram, (New-Object System.Text.UTF8Encoding($false)))
    Copy-Item -LiteralPath $taskAdapter -Destination (Join-Path $taskCheckout 'osu.Desktop\IllyaverseTestGame.cs')
    [System.IO.File]::WriteAllText((Join-Path $taskCheckout '.illyaverse-client-prepared'), $taskCommit)
    Write-Output 'Prepared an independent source checkout. Upstream source remains ignored; installed DLLs and account configurations were untouched.'
}
if ($Build) {
    if (-not (Test-Path -LiteralPath (Join-Path $taskCheckout '.illyaverse-client-prepared'))) { throw 'Run -Prepare first.' }
    $taskHead = (& git -C $taskCheckout rev-parse HEAD | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $taskHead -ne $taskCommit) { throw 'Build refused: unexpected upstream commit.' }
    $taskPreparedAdapter = Join-Path $taskCheckout 'osu.Desktop\IllyaverseTestGame.cs'
    if ((Get-FileHash -LiteralPath $taskPreparedAdapter).Hash -ne (Get-FileHash -LiteralPath $taskAdapter).Hash) { throw 'Build refused: original adapter changed.' }
    & dotnet build $taskProject -c Debug
    if ($LASTEXITCODE -ne 0) { throw 'Client build failed. The independent checkout was preserved for diagnostics.' }
    Write-Output 'Built only; no client was launched. Output: data-lazer-client/upstream/osu.Desktop/bin/Debug/net10.0/osu!.exe'
}
