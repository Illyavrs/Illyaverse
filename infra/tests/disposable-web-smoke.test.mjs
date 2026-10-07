import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';
import { validateWebSmokeConfig, webSmokePort, webSmokeTestFiles } from '../disposable-web-smoke.mjs';
import { workspace } from '../protocol-smoke.mjs';

const project = 'illya-web-smoke-0123456789abcdef';
const port = 19000;
const caddyFile = path.join(workspace, 'infra/Caddyfile');
const password = 'generated-fixture-password';
const images = Object.fromEntries(['postgres', 'sanctuary', 'resonance', 'serendipity', 'caddy']
  .map((name, index) => [name, `sha256:${String(index + 1).repeat(64)}`]));
const settings = { project, port, caddyFile, password, images };

function config() {
  const service = name => ({ image: images[name], pull_policy: 'never', networks: { default: null } });
  const volume = (source, target) => ({ type: 'volume', source, target });
  const environment = { PUBLIC_ORIGIN: `http://localhost:${port}`, DATABASE_URL: `postgres://illya:${password}@postgres:5432/illyaverse` };
  return {
    name: project,
    networks: { default: { name: `${project}_default` } },
    volumes: Object.fromEntries(['postgres_data', 'shared_data', 'caddy_data', 'caddy_config'].map(name => [name, { name: `${project}_${name}` }])),
    services: {
      postgres: { ...service('postgres'), volumes: [volume('postgres_data', '/var/lib/postgresql/data')] },
      sanctuary: { ...service('sanctuary'), environment: { ...environment }, volumes: [volume('shared_data', '/data')] },
      resonance: { ...service('resonance'), environment: { ...environment }, volumes: [volume('shared_data', '/data')] },
      serendipity: service('serendipity'),
      caddy: { ...service('caddy'), ports: [{ host_ip: '127.0.0.1', published: String(port), target: 80, protocol: 'tcp' }], volumes: [
        { type: 'bind', source: caddyFile, target: '/etc/caddy/Caddyfile', read_only: true, bind: { create_host_path: false } },
        volume('caddy_data', '/data'), volume('caddy_config', '/config'),
      ] },
    },
  };
}

test('website helper rejects an absent guard before Docker, ports or environment files', () => {
  const env = { ...process.env, ILLYA_TEST_ENV_FILE: 'missing-web-guard.env', ILLYA_WEB_SMOKE_PORT: 'invalid' };
  delete env.ILLYA_ALLOW_DISPOSABLE_TEST;
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/disposable-web-smoke.mjs')], { env, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Set ILLYA_ALLOW_DISPOSABLE_TEST=1/);
  assert.doesNotMatch(result.stderr, /ILLYA_WEB_SMOKE_PORT|Docker operation|environment file/);
});

test('website port validation rejects unsafe values before starting a project', () => {
  assert.equal(webSmokePort(), 19000);
  assert.equal(webSmokePort('19001'), 19001);
  for (const value of ['', '0', '80', '1023', '65536', '19000.5', 'invalid']) assert.throws(() => webSmokePort(value), /integer/);
});

test('optional test selection defaults to the full suite and accepts up to eight basenames', () => {
  assert.deepEqual(webSmokeTestFiles(), []);
  assert.deepEqual(webSmokeTestFiles('catalog-workflows.spec.ts'), ['catalog-workflows.spec.ts']);
  assert.deepEqual(webSmokeTestFiles('catalog-workflows.spec.ts, profile-media.spec.ts'), ['catalog-workflows.spec.ts', 'profile-media.spec.ts']);
  const names = ['alpha', 'bravo', 'charlie', 'delta', 'echo', 'foxtrot', 'golf', 'hotel'].map(name => `${name}.spec.ts`);
  assert.deepEqual(webSmokeTestFiles(names.join(',')), names);
  assert.throws(() => webSmokeTestFiles([...names, 'india.spec.ts'].join(',')), /ILLYA_WEB_SMOKE_TEST_FILES/);
});

test('test selection rejects paths, options, malformed names and empty CSV entries', () => {
  for (const value of [
    '', ' ', ',', 'community.spec.ts,', ',community.spec.ts', '../community.spec.ts', 'tests/community.spec.ts',
    'tests\\community.spec.ts', '--list', 'community.spec.ts --list', '*.spec.ts', 'Community.spec.ts', 'score2.spec.ts', 'community.test.ts',
  ]) assert.throws(() => webSmokeTestFiles(value), /ILLYA_WEB_SMOKE_TEST_FILES/, value);
});

test('unsafe test selection is rejected before port checks, Docker or environment creation', () => {
  const env = {
    ...process.env, ILLYA_ALLOW_DISPOSABLE_TEST: '1', ILLYA_WEB_SMOKE_TEST_FILES: '../community.spec.ts',
    ILLYA_WEB_SMOKE_PORT: 'invalid', ILLYA_TEST_ENV_FILE: 'missing-web-filter.env',
  };
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/disposable-web-smoke.mjs')], { env, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ILLYA_WEB_SMOKE_TEST_FILES/);
  assert.doesNotMatch(result.stderr, /ILLYA_WEB_SMOKE_PORT|Docker operation|environment file/);
  assert.equal(result.stdout, '');
});

test('only the owned pinned website configuration is accepted', () => {
  assert.doesNotThrow(() => validateWebSmokeConfig(config(), settings));
});

for (const [name, change] of [
  ['external data volume', value => { value.volumes.postgres_data.external = true; }],
  ['main project data volume', value => { value.volumes.shared_data.name = 'illyaverse_shared_data'; }],
  ['shared network', value => { value.networks.default.name = 'illyaverse_default'; }],
  ['extra service network', value => { value.services.sanctuary.networks.main = null; }],
  ['database hostname override', value => { value.services.sanctuary.extra_hosts = { postgres: '172.30.91.20' }; }],
  ['build', value => { value.services.serendipity.build = { context: workspace }; }],
  ['mutable image tag', value => { value.services.serendipity.image = 'illyaverse/serendipity:local'; }],
  ['extra backend port', value => { value.services.sanctuary.ports = [{ host_ip: '127.0.0.1', target: 8080, published: '8080' }]; }],
  ['public website port', value => { value.services.caddy.ports[0].host_ip = '0.0.0.0'; }],
  ['different bind file', value => { value.services.caddy.volumes[0].source = path.join(workspace, '.env'); }],
  ['writable Caddyfile', value => { value.services.caddy.volumes[0].read_only = false; }],
  ['anonymous application volume', value => { value.services.sanctuary.volumes[0].source = undefined; }],
  ['main database URL', value => { value.services.sanctuary.environment.DATABASE_URL = 'postgres://fixture@host.docker.internal:15432/illyaverse'; }],
  ['different browser origin', value => { value.services.resonance.environment.PUBLIC_ORIGIN = 'http://localhost:3000'; }],
]) {
  test(`rendered configuration rejects ${name}`, () => {
    const value = config();
    change(value);
    assert.throws(() => validateWebSmokeConfig(value, settings), /Disposable|disposable/);
  });
}
