param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $OutputPath) { $OutputPath = Join-Path $taskRoot '.env' }
$taskOutput = [System.IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $taskOutput) {
    Write-Output 'Existing environment file preserved. No secrets were changed.'
    exit 0
}
function New-TaskSecret {
    $taskBytes = New-Object byte[] 32
    $taskRng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $taskRng.GetBytes($taskBytes) } finally { $taskRng.Dispose() }
    return [System.BitConverter]::ToString($taskBytes).Replace('-', '').ToLowerInvariant()
}
$taskContent = [System.IO.File]::ReadAllText((Join-Path $taskRoot '.env.example'))
foreach ($taskKey in @('POSTGRES_PASSWORD', 'SESSION_SECRET', 'INTERNAL_SERVICE_KEY', 'BOOTSTRAP_ADMIN_PASSWORD')) {
    $taskContent = [System.Text.RegularExpressions.Regex]::Replace($taskContent, '(?m)^' + $taskKey + '=\s*$', $taskKey + '=' + (New-TaskSecret))
}
$taskStream = [System.IO.File]::Open($taskOutput, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
    $taskEncoding = New-Object System.Text.UTF8Encoding($false)
    $taskBytes = $taskEncoding.GetBytes($taskContent)
    $taskStream.Write($taskBytes, 0, $taskBytes.Length)
    $taskStream.Flush()
} finally { $taskStream.Dispose() }
Write-Output 'Created an environment file with independent random secrets. Read its initial administrator credentials locally.'
Write-Output 'Local start: docker compose up -d --build --wait'
