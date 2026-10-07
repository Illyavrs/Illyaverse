import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import test from 'node:test';
import { localOrigin, protocolScript, redact, workspace } from './protocol-smoke.mjs';

test('only explicit local HTTP origins are accepted', () => {
  for (const origin of ['http://localhost:3000', 'http://127.0.0.1:18082/', 'http://localhost']) assert.match(localOrigin(origin, 'URL'), /^http:\/\//);
  for (const origin of [
    'https://localhost:3000', 'http://example.test', 'http://localhost.evil.test', 'http://localhost@evil.test',
    'http://user:password@localhost', 'http://127.1', 'http://2130706433', 'http://0177.0.0.1',
    'http://[::1]', 'http://localhost:65536', 'http://localhost:0', 'http://localhost/path',
    'http://localhost?url=http://evil.test', 'http://localhost#fragment', ' http://localhost',
  ]) assert.throws(() => localOrigin(origin, 'URL'), /must be an http localhost/);
});
test('launcher denies absent guard before credentials or network access', () => {
  const env = { ...process.env, ILLYA_TEST_ENV_FILE: 'missing-guard-test.env' };
  delete env.ILLYA_ALLOW_DISPOSABLE_TEST;
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/protocol-smoke.mjs')], { env, encoding: 'utf8', windowsHide: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Set ILLYA_ALLOW_DISPOSABLE_TEST=1/);
  assert.doesNotMatch(result.stderr, /environment file could not be read/);
});
test('launcher denies remote destinations before reading an environment file', () => {
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/protocol-smoke.mjs')], {
    env: { ...process.env, ILLYA_ALLOW_DISPOSABLE_TEST: '1', ILLYA_TEST_ENV_FILE: 'missing-url-test.env', TEST_SANCTUARY_URL: 'http://example.test' }, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /TEST_SANCTUARY_URL must be an http localhost/);
  assert.doesNotMatch(result.stderr, /environment file could not be read/);
});
test('subprocess output masks generated credentials', () => {
  assert.equal(redact('secretabc secret secretabc', ['secret', 'secretabc']), '[redacted] [redacted] [redacted]');
  assert.equal(redact('{"access_token":"opaque-token","refresh_token":"opaque-refresh"} Bearer bearer-token', []), '{"access_token":"[redacted]","refresh_token":"[redacted]"} Bearer [redacted]');
});

test('smoke suites select only fixed owned scripts', () => {
  assert.equal(protocolScript(undefined), 'services/resonance/tests/protocol_smoke.py');
  assert.equal(protocolScript('protocol'), 'services/resonance/tests/protocol_smoke.py');
  assert.equal(protocolScript('official-archives'), 'services/resonance/tests/official_archive_smoke.py');
  for (const value of ['', '../external.py', 'official-archives.py', 'PROTOCOL', null])
    assert.throws(() => protocolScript(value), /must be protocol or official-archives/);
});

test('invalid suite is rejected before credential-file access', () => {
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/protocol-smoke.mjs')], {
    env: { ...process.env, ILLYA_ALLOW_DISPOSABLE_TEST: '1', ILLYA_TEST_ENV_FILE: 'missing-suite-test.env',
      TEST_SANCTUARY_URL: 'http://127.0.0.1:18082', TEST_RESONANCE_URL: 'http://127.0.0.1:18083',
      ILLYA_PROTOCOL_SMOKE_SUITE: '../external.py' }, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /must be protocol or official-archives/);
  assert.doesNotMatch(result.stderr, /environment file could not be read/);
});
