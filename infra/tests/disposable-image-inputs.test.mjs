import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { access, readFile, readdir } from 'node:fs/promises';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { disposableSmoke } from '../disposable-protocol-smoke.mjs';
import { disposableWebSmoke } from '../disposable-web-smoke.mjs';
import { workspace } from '../protocol-smoke.mjs';

const roles = ['postgres', 'sanctuary', 'resonance', 'serendipity', 'caddy'];
const tags = {
  postgres: 'postgres:17-alpine', sanctuary: 'illyaverse/sanctuary:local',
  resonance: 'illyaverse/resonance:local', serendipity: 'illyaverse/serendipity:local', caddy: 'caddy:2-alpine',
};
const id = digit => `sha256:${digit.repeat(64)}`;
const pinned = Object.fromEntries(roles.map((role, index) => [role, id(String(index + 1))]));
const overrides = Object.fromEntries(roles.map((role, index) => [role, id(String(index + 5))]));
const suites = [
  { name: 'protocol', prefix: 'ILLYA_SMOKE', roles: roles.slice(0, 3), invoke: disposableSmoke, script: 'disposable-protocol-smoke.mjs' },
  { name: 'website', prefix: 'ILLYA_WEB_SMOKE', roles, invoke: disposableWebSmoke, script: 'disposable-web-smoke.mjs' },
];
const result = stdout => ({ code: 0, stdout, stderr: '' });
const ownedTemporaryNames = async () => (await readdir(os.tmpdir()))
  .filter(name => /^illya-(?:web-)?smoke-/.test(name)).sort();

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const port = server.address().port;
      server.close(error => error ? reject(error) : resolve(port));
    });
  });
}

async function ports(suite) {
  if (suite.name === 'website') return { ILLYA_WEB_SMOKE_PORT: String(await freePort()) };
  const first = await freePort();
  let second = await freePort();
  while (first === second) second = await freePort();
  return { ILLYA_SMOKE_SANCTUARY_PORT: String(first), ILLYA_SMOKE_RESONANCE_PORT: String(second) };
}

function renderConfig(suite, project, env, images) {
  const service = name => ({ image: images[name], pull_policy: 'never', networks: { default: null } });
  const volume = (source, target) => ({ type: 'volume', source, target });
  const config = {
    name: project, networks: { default: { name: `${project}_default` } },
    volumes: Object.fromEntries((suite.name === 'website'
      ? ['postgres_data', 'shared_data', 'caddy_data', 'caddy_config']
      : ['postgres_data', 'shared_data']).map(name => [name, { name: `${project}_${name}` }])),
    services: {
      postgres: { ...service('postgres'), volumes: [volume('postgres_data', '/var/lib/postgresql/data')] },
      sanctuary: { ...service('sanctuary'), volumes: [volume('shared_data', '/data')] },
      resonance: { ...service('resonance'), volumes: [volume('shared_data', '/data')] },
    },
  };
  if (suite.name === 'website') {
    const port = env.ILLYA_WEB_SMOKE_PORT;
    const environment = {
      PUBLIC_ORIGIN: `http://localhost:${port}`, DATABASE_URL: `postgres://illya:${env.POSTGRES_PASSWORD}@postgres:5432/illyaverse`,
    };
    config.services.sanctuary.environment = { ...environment };
    config.services.resonance.environment = { ...environment };
    config.services.serendipity = service('serendipity');
    config.services.caddy = {
      ...service('caddy'), ports: [{ host_ip: '127.0.0.1', published: port, target: 80, protocol: 'tcp' }],
      volumes: [
        { type: 'bind', source: path.join(workspace, 'infra/Caddyfile'), target: '/etc/caddy/Caddyfile',
          read_only: true, bind: { create_host_path: false } },
        volume('caddy_data', '/data'), volume('caddy_config', '/config'),
      ],
    };
  } else {
    config.services.sanctuary.ports = [{ host_ip: '127.0.0.1' }];
    config.services.resonance.ports = [{ host_ip: '127.0.0.1' }];
  }
  return config;
}

function fakeFlow(suite, environment, { mismatch, missing, changeConfig } = {}) {
  const selected = Object.fromEntries(suite.roles.map(role => [role,
    environment[`${suite.prefix}_${role.toUpperCase()}_IMAGE`] ?? pinned[role]]));
  const references = suite.roles.map(role => environment[`${suite.prefix}_${role.toUpperCase()}_IMAGE`] ?? tags[role]);
  const observed = { inspected: [], operations: [], cleanupChecks: 0, envFiles: [], testCalls: 0 };
  const verifyImages = env => {
    for (const role of suite.roles) assert.equal(env[`${suite.prefix}_${role.toUpperCase()}_IMAGE`], selected[role]);
  };
  const runner = async (command, args, { env }) => {
    if (command !== 'docker') {
      if (args[0] === '-c') {
        assert.deepEqual(args, ['-c', 'import py3rijndael']);
        return result('');
      }
      assert.equal(command, process.execPath);
      assert.deepEqual(args, [path.join(workspace, 'infra/protocol-smoke.mjs')]);
      verifyImages(env);
      await access(env.ILLYA_TEST_ENV_FILE);
      observed.testCalls++;
      return result('');
    }
    if (args[0] === 'image') {
      const role = suite.roles[observed.inspected.length];
      assert.deepEqual(args, ['image', 'inspect', references[observed.inspected.length], '--format', '{{.Id}}']);
      observed.inspected.push(args[2]);
      if (missing === role) return { code: 1, stdout: '', stderr: '' };
      return result(`${mismatch?.role === role ? mismatch.image : selected[role]}\n`);
    }
    if (args[0] !== 'compose') {
      const expected = [['ps', '-aq'], ['volume', 'ls', '-q'], ['network', 'ls', '-q']][observed.cleanupChecks];
      assert.deepEqual(args.slice(0, -2), expected);
      assert.equal(args.at(-2), '--filter');
      assert.equal(args.at(-1), `label=com.docker.compose.project=${env.COMPOSE_PROJECT_NAME}`);
      observed.cleanupChecks++;
      return result('');
    }
    const project = args[args.indexOf('--project-name') + 1];
    const envFile = args[args.indexOf('--env-file') + 1];
    assert.equal(project, env.COMPOSE_PROJECT_NAME);
    assert.match(path.basename(path.dirname(envFile)), /^illya-(?:web-)?smoke-/);
    assert.equal(path.basename(envFile), 'test.env');
    verifyImages(env);
    const values = Object.fromEntries((await readFile(envFile, 'utf8')).trim().split('\n').map(line => {
      const at = line.indexOf('=');
      return [line.slice(0, at), line.slice(at + 1)];
    }));
    verifyImages(values);
    // Assertions expose only booleans, never generated credential contents.
    assert.equal(/^[a-f0-9]{64}$/.test(values.POSTGRES_PASSWORD), true);
    if (!observed.envFiles.includes(envFile)) observed.envFiles.push(envFile);
    const operation = args[args.indexOf('--file') + 2];
    observed.operations.push(operation);
    if (operation === 'config') {
      assert.deepEqual(args.slice(-3), ['config', '--format', 'json']);
      const config = renderConfig(suite, project, values, selected);
      changeConfig?.(config);
      return result(JSON.stringify(config));
    }
    if (operation === 'up') {
      assert.deepEqual(args.slice(-6), ['up', '-d', '--no-build', '--wait', '--wait-timeout', '120']);
      return result('');
    }
    if (operation === 'down') {
      assert.deepEqual(args.slice(-5), ['down', '--volumes', '--remove-orphans', '--timeout', '10']);
      return result('');
    }
    assert.fail('Unexpected fake Docker operation.');
  };
  const browserRunner = async (env, signal, files) => {
    assert.equal(suite.name, 'website');
    verifyImages(env);
    assert.deepEqual(files, ['community.spec.ts']);
    assert.equal(signal.aborted, false);
    await access(env.ILLYA_TEST_ENV_FILE);
    observed.testCalls++;
    return result('');
  };
  return { runner, browserRunner, observed, references };
}

for (const suite of suites) {
  for (const role of suite.roles) {
    test(`${suite.name} rejects invalid ${role} image inputs before resources or commands`, async () => {
      const before = await ownedTemporaryNames();
      let commands = 0;
      let browserCalls = 0;
      const invalidValues = [
        null, 1, false, {}, [], '', 'illyaverse/sanctuary:local', 'sha256:abc', id('A'), id('a').toUpperCase(),
        ` ${id('a')}`, `${id('a')} `, `${id('a')}\n`, `${id('a')}\r\n`, `${id('a')}\0`,
      ];
      for (const invalid of invalidValues) {
        const environment = { ILLYA_ALLOW_DISPOSABLE_TEST: '1', ILLYA_WEB_SMOKE_PORT: 'invalid',
          ILLYA_SMOKE_SANCTUARY_PORT: 'invalid', [`${suite.prefix}_${role.toUpperCase()}_IMAGE`]: invalid };
        for (const earlier of suite.roles.slice(0, suite.roles.indexOf(role)))
          environment[`${suite.prefix}_${earlier.toUpperCase()}_IMAGE`] = overrides[earlier];
        await assert.rejects(suite.invoke(environment, {
          runner: async () => { commands++; throw new Error('Unexpected command.'); },
          browserRunner: async () => { browserCalls++; throw new Error('Unexpected browser.'); },
        }),
          new RegExp(`${suite.prefix}_${role.toUpperCase()}_IMAGE must be an exact lowercase sha256 Docker image ID`));
      }
      assert.equal(commands, 0);
      assert.equal(browserCalls, 0);
      assert.deepEqual(await ownedTemporaryNames(), before);
    });
  }

  test(`${suite.name} CLI rejects a trailing-newline image before port validation`, () => {
    const env = { ...process.env, ILLYA_ALLOW_DISPOSABLE_TEST: '1', ILLYA_WEB_SMOKE_PORT: 'invalid',
      ILLYA_SMOKE_SANCTUARY_PORT: 'invalid', [`${suite.prefix}_RESONANCE_IMAGE`]: `${id('a')}\n` };
    const child = spawnSync(process.execPath, [path.join(workspace, 'infra', suite.script)], { env, encoding: 'utf8', windowsHide: true });
    assert.equal(child.status, 1);
    assert.match(child.stderr, new RegExp(`${suite.prefix}_RESONANCE_IMAGE must be an exact lowercase sha256 Docker image ID`));
    assert.equal(child.stdout, '');
    assert.doesNotMatch(child.stderr, /invalid|sha256:[a-f0-9]{64}|port|Docker operation failed/);
  });

  for (const mode of ['defaults', 'all overrides', 'mixed overrides']) {
    test(`${suite.name} ${mode} select actual configured immutable images without retags or pulls`, async () => {
      const environment = { ILLYA_ALLOW_DISPOSABLE_TEST: '1', ...await ports(suite), ILLYA_WEB_SMOKE_TEST_FILES: 'community.spec.ts' };
      for (const [index, role] of suite.roles.entries()) {
        if (mode === 'all overrides' || mode === 'mixed overrides' && index % 2 === 1)
          environment[`${suite.prefix}_${role.toUpperCase()}_IMAGE`] = overrides[role];
      }
      const fake = fakeFlow(suite, environment);
      await suite.invoke(environment, fake);
      assert.deepEqual(fake.observed.inspected, fake.references);
      assert.deepEqual(fake.observed.operations, ['config', 'up', 'down']);
      assert.equal(fake.observed.testCalls, 1);
      assert.equal(fake.observed.cleanupChecks, 3);
      for (const file of fake.observed.envFiles) {
        await assert.rejects(access(file), { code: 'ENOENT' });
        await assert.rejects(access(path.dirname(file)), { code: 'ENOENT' });
      }
    });
  }

  test(`${suite.name} rejects a mismatched explicit inspection without writing an envfile or starting Compose`, async () => {
    const before = await ownedTemporaryNames();
    const environment = { ILLYA_ALLOW_DISPOSABLE_TEST: '1', ...await ports(suite), [`${suite.prefix}_RESONANCE_IMAGE`]: overrides.resonance };
    const fake = fakeFlow(suite, environment, { mismatch: { role: 'resonance', image: id('a') } });
    await assert.rejects(suite.invoke(environment, fake), /does not match the explicit immutable ID/);
    assert.deepEqual(fake.observed.operations, []);
    assert.deepEqual(fake.observed.envFiles, []);
    assert.equal(fake.observed.testCalls, 0);
    assert.equal(fake.observed.cleanupChecks, 0);
    assert.deepEqual(await ownedTemporaryNames(), before);
  });

  test(`${suite.name} rejects an unavailable local image without falling back or pulling`, async () => {
    const before = await ownedTemporaryNames();
    const environment = { ILLYA_ALLOW_DISPOSABLE_TEST: '1', ...await ports(suite), [`${suite.prefix}_RESONANCE_IMAGE`]: overrides.resonance };
    const fake = fakeFlow(suite, environment, { missing: 'resonance' });
    await assert.rejects(suite.invoke(environment, fake), /operation failed with exit code 1/);
    assert.deepEqual(fake.observed.inspected, fake.references.slice(0, 3));
    assert.deepEqual(fake.observed.operations, []);
    assert.deepEqual(fake.observed.envFiles, []);
    assert.equal(fake.observed.testCalls, 0);
    assert.equal(fake.observed.cleanupChecks, 0);
    assert.deepEqual(await ownedTemporaryNames(), before);
  });

  for (const [name, changeConfig] of [
    ['different image', config => { config.services.resonance.image = id('a'); }],
    ['pull policy', config => { config.services.resonance.pull_policy = 'always'; }],
  ]) {
    test(`${suite.name} rejects rendered ${name} before Compose up and removes its envfile`, async () => {
      const environment = { ILLYA_ALLOW_DISPOSABLE_TEST: '1', ...await ports(suite), [`${suite.prefix}_RESONANCE_IMAGE`]: overrides.resonance };
      const fake = fakeFlow(suite, environment, { changeConfig });
      await assert.rejects(suite.invoke(environment, fake), /pinned existing images/);
      assert.deepEqual(fake.observed.operations, ['config']);
      assert.equal(fake.observed.testCalls, 0);
      assert.equal(fake.observed.cleanupChecks, 0);
      for (const file of fake.observed.envFiles) {
        await assert.rejects(access(file), { code: 'ENOENT' });
        await assert.rejects(access(path.dirname(file)), { code: 'ENOENT' });
      }
    });
  }
}
