import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';

// Capture rendered config in memory: it contains secrets and must never be printed.
function config(files) {
  const args = ['compose', ...files.flatMap(file => ['-f', file]), 'config', '--format', 'json'];
  try {
    return JSON.parse(execFileSync('docker', args, {encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe']}));
  } catch {
    throw new Error('Compose could not be resolved. Create .env with infra/setup first and run docker compose config --quiet.');
  }
}

function checkSecrets(services) {
  for (const key of ['SESSION_SECRET', 'INTERNAL_SERVICE_KEY']) {
    assert.ok(services.sanctuary.environment[key]?.length >= 32, `${key} must have at least 32 characters`);
  }
  assert.ok(services.sanctuary.environment.INTERNAL_SERVICE_KEY === services.resonance.environment.INTERNAL_SERVICE_KEY,
    'Sanctuary and Resonance must use the same internal service key');
  assert.ok(services.sanctuary.environment.DATABASE_URL === services.resonance.environment.DATABASE_URL,
    'Both services must use the same database');
  for (const name of ['sanctuary', 'resonance']) {
    assert.ok(services[name].volumes.some(volume => volume.type === 'volume' && volume.target === '/data'),
      `${name} must mount the shared data at the same absolute path`);
    assert.ok(services[name].healthcheck?.test, `${name} must have a readiness healthcheck`);
  }
  assert.equal(services.sanctuary.volumes.find(volume => volume.target === '/data').source,
    services.resonance.volumes.find(volume => volume.target === '/data').source,
    'Sanctuary and Resonance must use the same data volume');
}

const local = config(['compose.yaml']);
checkSecrets(local.services);
for (const [name, service] of Object.entries(local.services)) {
  for (const port of service.ports ?? []) {
    assert.equal(port.host_ip, '127.0.0.1', `Local ${name} port must bind to loopback`);
  }
}
for (const name of ['postgres', 'sanctuary', 'serendipity']) {
  assert.equal(local.services[name].ports?.length ?? 0, 0, `Local ${name} must stay behind the gateway`);
}

const production = config(['compose.yaml', 'compose.production.yaml']);
checkSecrets(production.services);
for (const name of ['postgres', 'sanctuary', 'resonance', 'serendipity']) {
  assert.equal(production.services[name].ports?.length ?? 0, 0, `Production ${name} must not publish a host port`);
}
assert.equal(production.services.sanctuary.environment.COOKIE_SECURE, 'true', 'Production cookies must require HTTPS');
assert.match(production.services.sanctuary.environment.PUBLIC_ORIGIN, /^https:\/\//, 'Production origin must use HTTPS');
const listeners = production.services.caddy.ports.map(port => `${port.published}/${port.protocol}`).sort();
assert.deepEqual(listeners, ['443/tcp', '443/udp', '80/tcp'], 'Production must replace the local Caddy port mappings');
console.log('Compose security and deployment invariants verified. Rendered secrets were not printed.');
