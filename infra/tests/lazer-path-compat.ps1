[CmdletBinding()]
param()

# Uses only an already cached image and nonce-owned containers. No retained service,
# real OAuth credential, player file, host certificate store, or image build is used.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$nonce = [guid]::NewGuid().ToString('N')
$image = 'sha256:f77f856a30f0004200b36b322d61da17fade31e24875699d77fb968399b9eb77'
$ownerLabel = 'illyaverse.lazer-path-fixture'
$outputDirectory = Join-Path $repoRoot ('.artifacts/tests/lazer-path-compat-' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ') + '-' + $nonce)
$null = New-Item -ItemType Directory -Path $outputDirectory
$networkName = 'illya-lazer-path-' + $nonce
$containerIds = [System.Collections.Generic.List[string]]::new()
$networkId = $null
$checks = [System.Collections.Generic.List[object]]::new()
$adaptations = [System.Collections.Generic.List[object]]::new()
$cleanup = [ordered]@{ containers = @(); network = $null }
$failure = $null
$reloadProof = $null
$utf8 = [System.Text.UTF8Encoding]::new($false)
$fixtureAuthorization = 'Bearer synthetic-lazer-path-fixture'
$fixtureQuery = 'key=username&fixture=a%2Bb&fixture=second'
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.AllowAutoRedirect = $false
$handler.UseProxy = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$client.Timeout = [TimeSpan]::FromSeconds(5)

function Write-JsonFile([string] $Path, $Value) {
    [System.IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 100), $utf8)
}

function Invoke-FixtureDocker([string[]] $Arguments) {
    $errorFile = Join-Path $outputDirectory ('docker-' + [guid]::NewGuid().ToString('N') + '.stderr')
    $result = & docker @Arguments 2> $errorFile
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "Docker fixture command failed ($exitCode); stderr: $errorFile"
    }
    return ($result -join "`n")
}

function Assert-Fixture([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
}

function Get-OwnedContainer([string] $Id) {
    $value = (Invoke-FixtureDocker @('inspect', $Id) | ConvertFrom-Json -AsHashtable -NoEnumerate)[0]
    Assert-Fixture ($value.Id -eq $Id) 'Container identity changed.'
    Assert-Fixture ($value.Config.Labels[$ownerLabel] -eq $nonce) 'Container ownership label mismatch.'
    return $value
}

function New-FixtureContainer([string] $Name, [string] $Config, [bool] $PublishHttp, [bool] $PublishTls, [bool] $Backend) {
    $arguments = @('run', '-d', '--pull', 'never', '--name', $Name, '--label', ($ownerLabel + '=' + $nonce), '--network', $networkName,
        '--mount', ('type=bind,src=' + $outputDirectory + ',dst=/fixture,readonly'), '--tmpfs', '/data', '--tmpfs', '/config', '--tmpfs', '/tmp')
    if ($Backend) { $arguments += @('--network-alias', 'resonance', '--network-alias', 'sanctuary', '--network-alias', 'serendipity') }
    if ($PublishHttp) { $arguments += @('-p', '127.0.0.1::8085') }
    if ($PublishTls) { $arguments += @('-p', '127.0.0.1::443') }
    $arguments += @($image, 'caddy', 'run', '--config', ('/fixture/' + $Config))
    $id = Invoke-FixtureDocker $arguments
    Assert-Fixture ($id -match '^[a-f0-9]{64}$') 'Unexpected container ID.'
    $containerIds.Add($id)
    return $id
}

function Get-FixturePort([string] $Id, [string] $ContainerPort) {
    $value = Get-OwnedContainer $Id
    $binding = $value.NetworkSettings.Ports[$ContainerPort][0]
    Assert-Fixture ($binding.HostIp -eq '127.0.0.1') 'Fixture port is not loopback-only.'
    return [int] $binding.HostPort
}

function Invoke-FixtureHttp([int] $Port, [string] $HostName, [string] $Method, [string] $Path, [string] $ExpectedPath, [string] $ExpectedUpstream, [int] $ExpectedStatus, [bool] $Record = $true) {
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::new($Method), ('http://127.0.0.1:' + $Port + $Path + '?' + $fixtureQuery))
    $request.Headers.Host = $HostName
    $request.Headers.TryAddWithoutValidation('Authorization', $fixtureAuthorization) | Out-Null
    $response = $null
    try {
        $response = $client.Send($request)
        $status = [int] $response.StatusCode
        $pathValue = @($response.Headers.GetValues('X-Fixture-Path'))[0]
        $queryValue = @($response.Headers.GetValues('X-Fixture-Query'))[0]
        $authorizationValue = @($response.Headers.GetValues('X-Fixture-Authorization'))[0]
        $methodValue = @($response.Headers.GetValues('X-Fixture-Method'))[0]
        $upstreamValue = @($response.Headers.GetValues('X-Fixture-Upstream'))[0]
        Assert-Fixture ($status -eq $ExpectedStatus) "$HostName $Method $Path status $status, expected $ExpectedStatus"
        Assert-Fixture (-not $response.Headers.Contains('Location')) "$HostName $Method $Path redirected."
        Assert-Fixture ($pathValue -eq $ExpectedPath) "$HostName $Method $Path changed outside the narrow alias."
        Assert-Fixture ($queryValue -eq $fixtureQuery) "$HostName $Method $Path query changed."
        Assert-Fixture ($authorizationValue -eq $fixtureAuthorization) "$HostName $Method $Path authorization changed."
        Assert-Fixture ($methodValue -eq $Method) "$HostName $Method $Path method changed."
        Assert-Fixture ($upstreamValue -eq $ExpectedUpstream) "$HostName $Method $Path upstream changed."
        if ($Record) {
            $checks.Add([ordered]@{ host = $HostName; method = $Method; path = $Path; upstream_path = $pathValue; upstream = $upstreamValue; status = $status; query_preserved = $true; authorization_preserved = $true; no_redirect = $true })
        }
    } finally {
        if ($null -ne $response) { $response.Dispose() }
        $request.Dispose()
    }
}

function Wait-FixtureHttp([int] $Port, [string] $HostName) {
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try { Invoke-FixtureHttp $Port $HostName 'GET' '/api/v2/me' '/api/v2/me' 'resonance' 200 $false; return }
        catch { Start-Sleep -Milliseconds 100 }
    }
    throw 'Fixture proxy did not become ready.'
}

try {
    Assert-Fixture ((Invoke-FixtureDocker @('image', 'inspect', $image, '--format', '{{.Id}}')) -eq $image) 'Required cached image unavailable.'
    $networkId = Invoke-FixtureDocker @('network', 'create', '--label', ($ownerLabel + '=' + $nonce), $networkName)
    $upstreamConfig = @'
{
    auto_https off
    admin off
}
(fixture_headers) {
    header X-Fixture-Path {http.request.uri.path}
    header X-Fixture-Query {http.request.uri.query}
    header X-Fixture-Authorization {http.request.header.Authorization}
    header X-Fixture-Method {http.request.method}
}
:8081 {
    import fixture_headers
    header X-Fixture-Upstream resonance
    @canonical path /api/v2/me /api/v2/users/42 /api/v2/users/fixture-player
    respond @canonical "canonical user endpoint" 200
    respond "unchanged noncanonical endpoint" 404
}
:8080 {
    import fixture_headers
    header X-Fixture-Upstream sanctuary
    respond "sanctuary fixture" 200
}
:3000 {
    import fixture_headers
    header X-Fixture-Upstream serendipity
    respond "website fixture" 200
}
'@
    [System.IO.File]::WriteAllText((Join-Path $outputDirectory 'Caddyfile.upstream'), $upstreamConfig, $utf8)
    $upstreamId = New-FixtureContainer ('illya-lazer-path-upstream-' + $nonce) 'Caddyfile.upstream' $false $false $true
    foreach ($file in @('Caddyfile', 'Caddyfile.client-test', 'Caddyfile.production')) {
        $sourcePath = Join-Path $repoRoot ('infra/' + $file)
        $sourceHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
        $arguments = @('run', '--rm', '--pull', 'never', '--network', 'none', '--read-only', '--tmpfs', '/tmp', '--tmpfs', '/data', '--tmpfs', '/config',
            '--label', ($ownerLabel + '=' + $nonce), '--mount', ('type=bind,src=' + (Join-Path $repoRoot 'infra') + ',dst=/source,readonly'),
            '-e', 'ILLYA_DOMAIN=fixture.test', '-e', 'ACME_EMAIL=fixture@example.invalid', $image,
            'caddy', 'adapt', '--config', ('/source/' + $file), '--pretty', '--validate')
        $adaptedText = Invoke-FixtureDocker $arguments
        [System.IO.File]::WriteAllText((Join-Path $outputDirectory ($file + '.adapted.json')), $adaptedText, $utf8)
        $configuration = $adaptedText | ConvertFrom-Json -AsHashtable
        Assert-Fixture ($configuration.admin.disabled -eq $true) "$file enabled the admin API."
        # The fixture changes only transport: run the exact adapted HTTP route graph
        # on loopback HTTP rather than issuing production certificates.
        $configuration.apps.Remove('tls') | Out-Null
        $configuration.apps.Remove('pki') | Out-Null
        $servers = $configuration.apps.http.servers
        foreach ($key in @($servers.Keys)) {
            $serverText = $servers[$key] | ConvertTo-Json -Depth 100 -Compress
            if (-not $serverText.Contains('lazer_user_alias')) { $servers.Remove($key); continue }
            $servers[$key].listen = @(':8085')
            $servers[$key].Remove('tls_connection_policies') | Out-Null
            $servers[$key].automatic_https = @{ disable = $true }
        }
        Assert-Fixture ($servers.Count -eq 1) "$file expected one route server."
        $httpConfigName = $file + '.http-fixture.json'
        Write-JsonFile (Join-Path $outputDirectory $httpConfigName) $configuration
        $adaptations.Add([ordered]@{ source = ('infra/' + $file); source_sha256 = $sourceHash; adapt_validate_exit = 0; admin_disabled = $true; transport_only_fixture = $true })
        $proxyId = New-FixtureContainer ('illya-lazer-path-proxy-' + $file.Replace('.', '-') + '-' + $nonce) $httpConfigName $true $false $false
        $port = Get-FixturePort $proxyId '8085/tcp'
        $domain = if ($file -eq 'Caddyfile.client-test') { 'illya.test' } else { 'fixture.test' }
        Wait-FixtureHttp $port $domain
        $hosts = @($domain, ('osu.' + $domain), ('c.' + $domain), ('ce.' + $domain), ('c4.' + $domain), ('api.' + $domain), ('spectator.' + $domain), ('bss.' + $domain), ('a.' + $domain), ('b.' + $domain))
        foreach ($hostName in $hosts) {
            foreach ($alias in @(@('/api/v2/me/', '/api/v2/me'), @('/api/v2/users/42/', '/api/v2/users/42'), @('/api/v2/users/fixture-player/', '/api/v2/users/fixture-player'))) {
                Invoke-FixtureHttp $port $hostName 'GET' $alias[0] $alias[1] 'resonance' 200
                Invoke-FixtureHttp $port $hostName 'HEAD' $alias[0] $alias[1] 'resonance' 200
            }
            Invoke-FixtureHttp $port $hostName 'GET' '/api/v2/me' '/api/v2/me' 'resonance' 200
            Invoke-FixtureHttp $port $hostName 'GET' '/api/v2/users/42' '/api/v2/users/42' 'resonance' 200
            foreach ($path in @('/api/v2/me/osu/', '/api/v2/users/42/osu/', '/api/v2/users/42/scores/best/', '/api/v2/beatmapsets/', '/api/v2/me//')) {
                Invoke-FixtureHttp $port $hostName 'GET' $path $path 'resonance' 404
            }
            foreach ($method in @('POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS')) {
                Invoke-FixtureHttp $port $hostName $method '/api/v2/me/' '/api/v2/me/' 'resonance' 404
                Invoke-FixtureHttp $port $hostName $method '/api/v2/users/42/' '/api/v2/users/42/' 'resonance' 404
            }
        }
        Invoke-FixtureHttp $port $domain 'GET' '/api/v1/account/' '/api/v1/account/' 'sanctuary' 200
        Invoke-FixtureHttp $port $domain 'GET' '/profile/' '/profile/' 'serendipity' 200
        Assert-Fixture ((Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant() -eq $sourceHash) "$file changed during fixture verification."
    }

    # Prove the same reload mechanism against the complete client-test Caddyfile,
    # including internal TLS and admin off, without recreating the proxy container.
    $clientTestText = [System.IO.File]::ReadAllText((Join-Path $repoRoot 'infra/Caddyfile.client-test'))
    $liveName = 'Caddyfile.signal-reload'
    $livePath = Join-Path $outputDirectory $liveName
    [System.IO.File]::WriteAllText($livePath, $clientTestText.Replace("`turi @lazer_user_alias strip_suffix /", '# Fixture starts before the alias rewrite.'), $utf8)
    $tlsProxyId = New-FixtureContainer ('illya-lazer-path-tls-' + $nonce) $liveName $false $true $false
    $tlsPort = Get-FixturePort $tlsProxyId '443/tcp'
    $rootCertificatePath = Join-Path $outputDirectory 'fixture-public-root.crt'
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        try {
            $publicCertificate = Invoke-FixtureDocker @('exec', $tlsProxyId, 'cat', '/data/caddy/pki/authorities/local/root.crt')
            Assert-Fixture ($publicCertificate.StartsWith('-----BEGIN CERTIFICATE-----')) 'Fixture public CA extraction failed.'
            [System.IO.File]::WriteAllText($rootCertificatePath, $publicCertificate + "`n", $utf8)
            break
        }
        catch { if ($attempt -eq 39) { throw }; Start-Sleep -Milliseconds 100 }
    }
    $curlHeaders = Join-Path $outputDirectory 'tls-before.headers'
    # Internal fixture certificates have no public CRL distribution endpoint.
    # Schannel still validates the supplied CA and hostname with this option.
    $curlArgs = @('--silent', '--show-error', '--noproxy', '*', '--ssl-revoke-best-effort', '--cacert', $rootCertificatePath, '--resolve', ('illya.test:' + $tlsPort + ':127.0.0.1'),
        '--header', ('Authorization: ' + $fixtureAuthorization), '--dump-header', $curlHeaders, '--output', (Join-Path $outputDirectory 'tls-before.body'),
        '--write-out', '%{http_code}', ('https://illya.test:' + $tlsPort + '/api/v2/me/?' + $fixtureQuery))
    $beforeStatus = & curl.exe @curlArgs
    Assert-Fixture ($LASTEXITCODE -eq 0 -and $beforeStatus -eq '404') 'TLS fixture did not reproduce the original missing alias.'
    $before = Get-OwnedContainer $tlsProxyId
    [System.IO.File]::WriteAllText($livePath, $clientTestText, $utf8)
    $null = Invoke-FixtureDocker @('kill', '--signal', 'SIGUSR1', $tlsProxyId)
    $afterStatus = $null
    $afterHeadersPath = Join-Path $outputDirectory 'tls-after.headers'
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        $afterArgs = @('--silent', '--show-error', '--noproxy', '*', '--ssl-revoke-best-effort', '--cacert', $rootCertificatePath, '--resolve', ('illya.test:' + $tlsPort + ':127.0.0.1'),
            '--header', ('Authorization: ' + $fixtureAuthorization), '--dump-header', $afterHeadersPath, '--output', (Join-Path $outputDirectory 'tls-after.body'),
            '--write-out', '%{http_code}', ('https://illya.test:' + $tlsPort + '/api/v2/me/?' + $fixtureQuery))
        $afterStatus = & curl.exe @afterArgs
        if ($LASTEXITCODE -eq 0 -and $afterStatus -eq '200') { break }
        Start-Sleep -Milliseconds 100
    }
    Assert-Fixture ($afterStatus -eq '200') 'SIGUSR1 did not activate the alias.'
    $headers = [System.IO.File]::ReadAllText($afterHeadersPath)
    Assert-Fixture ($headers -match '(?im)^X-Fixture-Path: /api/v2/me\r?$') 'TLS upstream path not canonical.'
    Assert-Fixture ($headers -match ('(?im)^X-Fixture-Query: ' + [regex]::Escape($fixtureQuery) + '\r?$')) 'TLS query changed.'
    Assert-Fixture ($headers -match ('(?im)^X-Fixture-Authorization: ' + [regex]::Escape($fixtureAuthorization) + '\r?$')) 'TLS Authorization changed.'
    Assert-Fixture (-not ($headers -match '(?im)^Location:')) 'TLS alias redirected.'
    $after = Get-OwnedContainer $tlsProxyId
    $null = Invoke-FixtureDocker @('exec', $tlsProxyId, 'sh', '-c', 'if wget -q -T 1 -O /dev/null http://127.0.0.1:2019/config/ >/dev/null 2>&1; then exit 1; else exit 0; fi')
    Assert-Fixture ($before.Id -eq $after.Id -and $before.State.StartedAt -eq $after.State.StartedAt) 'SIGUSR1 recreated or restarted the container.'
    Assert-Fixture (($before.Mounts | ConvertTo-Json -Depth 20 -Compress) -eq ($after.Mounts | ConvertTo-Json -Depth 20 -Compress)) 'SIGUSR1 changed mounts.'
    $logs = Invoke-FixtureDocker @('logs', $tlsProxyId)
    [System.IO.File]::WriteAllText((Join-Path $outputDirectory 'signal-reload.log'), $logs, $utf8)
    $reloadProof = [ordered]@{ container_id = $tlsProxyId; started_at_before = $before.State.StartedAt; started_at_after = $after.State.StartedAt; mounts_preserved = $true; before_status = $beforeStatus; after_status = $afterStatus; admin_disabled = $true; admin_endpoint_unreachable = $true; internal_tls = $true; public_root_only = $true; schannel_fixture_revocation_best_effort = $true; host_trust_store_modified = $false; signal = 'SIGUSR1'; no_redirect = $true; query_preserved = $true; authorization_preserved = $true }
} catch {
    $failure = $_.Exception.Message
} finally {
    $client.Dispose()
    $handler.Dispose()
    foreach ($id in $containerIds) {
        try {
            $null = Get-OwnedContainer $id
            & docker logs $id *> (Join-Path $outputDirectory ('container-' + $id + '.log'))
            $null = Invoke-FixtureDocker @('rm', '-f', $id)
            $remaining = & docker ps -aq --filter ('id=' + $id)
            Assert-Fixture ($LASTEXITCODE -eq 0 -and [string]::IsNullOrWhiteSpace(($remaining -join ''))) 'Fixture container remains.'
            $cleanup.containers += [ordered]@{ id = $id; removed = $true }
        } catch { $cleanup.containers += [ordered]@{ id = $id; removed = $false; error = $_.Exception.Message }; if ($null -eq $failure) { $failure = $_.Exception.Message } }
    }
    if ($null -ne $networkId) {
        try {
            $networkValue = (Invoke-FixtureDocker @('network', 'inspect', $networkId) | ConvertFrom-Json -AsHashtable -NoEnumerate)[0]
            Assert-Fixture ($networkValue.Id -eq $networkId -and $networkValue.Labels[$ownerLabel] -eq $nonce) 'Network ownership mismatch.'
            $null = Invoke-FixtureDocker @('network', 'rm', $networkId)
            $remaining = & docker network ls -q --filter ('id=' + $networkId)
            Assert-Fixture ($LASTEXITCODE -eq 0 -and [string]::IsNullOrWhiteSpace(($remaining -join ''))) 'Fixture network remains.'
            $cleanup.network = [ordered]@{ id = $networkId; removed = $true }
        } catch { $cleanup.network = [ordered]@{ id = $networkId; removed = $false; error = $_.Exception.Message }; if ($null -eq $failure) { $failure = $_.Exception.Message } }
    }
    $evidence = [ordered]@{ schema = 1; nonce = $nonce; cached_image = $image; fixture_script_sha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant(); passed = ($null -eq $failure); failure = $failure; retained_services_touched = $false; native_game_started = $false; host_trust_store_modified = $false; adaptations = @($adaptations); http_checks = @($checks); http_check_count = $checks.Count; signal_reload = $reloadProof; cleanup = $cleanup }
    Write-JsonFile (Join-Path $outputDirectory 'verification.json') $evidence
}
Write-Output $outputDirectory
if ($null -ne $failure) { throw $failure }
Write-Output ('PASS ' + $checks.Count + ' HTTP route checks, 3 adapted configurations, internal-TLS SIGUSR1 reload, and owned fixture cleanup.')
