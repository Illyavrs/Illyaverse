[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$taskRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$taskOutput = Join-Path $taskRoot '.env.client-test.local'

if (Test-Path -LiteralPath $taskOutput) {
    Write-Output 'Existing .env.client-test.local preserved. No secrets were changed.'
    exit 0
}

# Report conflicts without stopping processes or changing the firewall.
$taskListeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
    Where-Object { $_.LocalPort -in @(80, 443) })
if ($taskListeners.Count -gt 0) {
    $taskListeners | Select-Object LocalAddress, LocalPort, OwningProcess | Format-Table | Out-Host
    throw 'Port 80 or 443 is already in use. Keep the existing service intact and resolve the port allocation before starting this test stack.'
}

# Windows listener inventory can omit services forwarded from WSL.
foreach ($taskPort in @(80, 443)) {
    $taskSocket = New-Object System.Net.Sockets.TcpClient
    try {
        $taskConnection = $taskSocket.ConnectAsync('127.0.0.1', $taskPort)
        try { $taskCompleted = $taskConnection.Wait(1000) }
        catch [System.AggregateException] {
            $taskSocketError = $_.Exception.InnerException
            if ($taskSocketError -is [System.Net.Sockets.SocketException] -and
                $taskSocketError.SocketErrorCode -eq [System.Net.Sockets.SocketError]::ConnectionRefused) {
                continue
            }
            throw
        }
        if (-not $taskCompleted) { throw "Could not establish whether localhost:$taskPort is available." }
        if ($taskSocket.Connected) {
            throw "localhost:$taskPort already accepts connections, including a possible WSL service. Keep it intact and resolve the port allocation before starting this stack."
        }
    } finally { $taskSocket.Dispose() }
}

function New-ClientTestSecret {
    $taskBytes = New-Object byte[] 32
    $taskRng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try { $taskRng.GetBytes($taskBytes) } finally { $taskRng.Dispose() }
    return [System.BitConverter]::ToString($taskBytes).Replace('-', '').ToLowerInvariant()
}

$taskContent = [System.IO.File]::ReadAllText((Join-Path $taskRoot '.env.example'))
foreach ($taskKey in @('POSTGRES_PASSWORD', 'SESSION_SECRET', 'INTERNAL_SERVICE_KEY', 'BOOTSTRAP_ADMIN_PASSWORD')) {
    $taskContent = [System.Text.RegularExpressions.Regex]::Replace(
        $taskContent, '(?m)^' + $taskKey + '=\s*$', $taskKey + '=' + (New-ClientTestSecret))
}
$taskSettings = @{
    PUBLIC_ORIGIN = 'https://illya.test'
    COOKIE_SECURE = 'true'
    ILLYA_DOMAIN = 'illya.test'
    BOOTSTRAP_ADMIN_USERNAME = 'illya_client_admin'
    BOOTSTRAP_ADMIN_EMAIL = 'admin@illya.test'
}
foreach ($taskKey in $taskSettings.Keys) {
    $taskContent = [System.Text.RegularExpressions.Regex]::Replace(
        $taskContent, '(?m)^' + $taskKey + '=.*$', $taskKey + '=' + $taskSettings[$taskKey])
}
$taskStream = [System.IO.File]::Open($taskOutput, [System.IO.FileMode]::CreateNew,
    [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
    $taskEncoding = New-Object System.Text.UTF8Encoding($false)
    $taskBytes = $taskEncoding.GetBytes($taskContent)
    $taskStream.Write($taskBytes, 0, $taskBytes.Length)
    $taskStream.Flush()
} finally { $taskStream.Dispose() }

Write-Output 'Created .env.client-test.local with independent random secrets for illya.test.'
Write-Output 'Read the initial test administrator credentials locally; do not publish this file.'
Write-Output 'No containers, system hosts, certificate trust, firewall rules, or game clients were changed.'
Write-Output 'Validate: docker compose --env-file .env.client-test.local -p illyaverse-client-test -f compose.yaml -f compose.client-test.yaml config --quiet'
