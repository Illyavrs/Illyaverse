import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { access, mkdtemp, realpath, rmdir, unlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { redact, requireDisposable, runCommand, workspace } from './protocol-smoke.mjs';
import { pinExistingSmokeImages, smokeImageReferences } from './existing-smoke-images.mjs';

const services = ['caddy', 'postgres', 'resonance', 'sanctuary', 'serendipity'];
const volumes = ['caddy_config', 'caddy_data', 'postgres_data', 'shared_data'];
const imageTags = {
  postgres: 'postgres:17-alpine', sanctuary: 'illyaverse/sanctuary:local',
  resonance: 'illyaverse/resonance:local', serendipity: 'illyaverse/serendipity:local', caddy: 'caddy:2-alpine',
};

export function webSmokePort(value) {
  const port = value === undefined ? 19000 : Number(value);
  if (!/^[1-9][0-9]{3,4}$/.test(String(port)) || !Number.isInteger(port) || port < 1024 || port > 65535)
    throw new Error('ILLYA_WEB_SMOKE_PORT must be an integer from 1024 to 65535.');
  return port;
}

export function webSmokeTestFiles(value) {
  if (value === undefined) return [];
  const files = typeof value === 'string' ? value.split(',').map(name => name.trim()) : [];
  if (files.length < 1 || files.length > 8 || files.some(name => !/^[a-z][a-z-]*\.spec\.ts$/.test(name)))
    throw new Error('ILLYA_WEB_SMOKE_TEST_FILES must contain 1–8 comma-separated test basenames such as catalog-workflows.spec.ts.');
  return files;
}

const keys = (value) => Object.keys(value ?? {}).sort().join();
const samePath = (left, right) => process.platform === 'win32'
  ? path.resolve(left).toLowerCase() === path.resolve(right).toLowerCase() : path.resolve(left) === path.resolve(right);

// Validate the rendered configuration before any containers or data volumes exist.
export function validateWebSmokeConfig(config, { project, port, images, caddyFile, password }) {
  if (!/^illya-web-smoke-[a-f0-9]{16}$/.test(project) || config.name !== project || keys(config.services) !== services.join())
    throw new Error('Unexpected disposable website Compose project or services.');
  if (keys(config.volumes) !== volumes.join() || keys(config.networks) !== 'default')
    throw new Error('Unexpected disposable website resources.');
  for (const [key, volume] of Object.entries(config.volumes)) {
    if (volume.external || volume.name !== `${project}_${key}`)
      throw new Error('Disposable website volumes must belong exclusively to this project.');
  }
  if (config.networks.default.external || config.networks.default.name !== `${project}_default`)
    throw new Error('Disposable website network must belong exclusively to this project.');
  const mounts = {
    postgres: [['postgres_data', '/var/lib/postgresql/data']], sanctuary: [['shared_data', '/data']],
    resonance: [['shared_data', '/data']], serendipity: [], caddy: [['caddy_data', '/data'], ['caddy_config', '/config']],
  };
  let binds = 0;
  for (const [name, service] of Object.entries(config.services)) {
    if (service.build || service.container_name || service.network_mode || service.privileged || service.devices?.length ||
        service.extra_hosts || service.links || service.volumes_from ||
        service.env_file?.length || service.configs?.length || service.secrets?.length || keys(service.networks) !== 'default')
      throw new Error('Disposable website services may only use their own Compose resources.');
    if (!/^sha256:[a-f0-9]{64}$/.test(images[name] ?? '') || service.image !== images[name] || service.pull_policy !== 'never')
      throw new Error('Disposable website services must use the pinned existing images without builds or pulls.');
    const actual = [];
    for (const volume of service.volumes ?? []) {
      if (volume.type === 'volume' && mounts[name].some(([source, target]) => source === volume.source && target === volume.target)) {
        actual.push([volume.source, volume.target]);
      } else if (name === 'caddy' && volume.type === 'bind' && volume.read_only === true &&
          volume.target === '/etc/caddy/Caddyfile' && typeof volume.source === 'string' && samePath(volume.source, caddyFile) &&
          volume.bind?.create_host_path !== true) {
        binds++;
      } else throw new Error(`Disposable website may bind only the exact read-only infra/Caddyfile (service ${name}, expected ${caddyFile}, got ${JSON.stringify(volume)}).`);
    }
    if (JSON.stringify(actual.sort()) !== JSON.stringify(mounts[name].slice().sort()))
      throw new Error('Disposable website data mounts must be its own expected volumes.');
    const ports = service.ports ?? [];
    if (name === 'caddy') {
      if (ports.length !== 1 || ports[0].host_ip !== '127.0.0.1' || String(ports[0].published) !== String(port) ||
          ports[0].target !== 80 || ports[0].protocol !== 'tcp')
        throw new Error('Disposable website must publish only its selected loopback HTTP port.');
    } else if (ports.length) throw new Error('Only the disposable website HTTP port may be published.');
  }
  if (binds !== 1) throw new Error('Disposable website requires its exact read-only Caddyfile bind.');
  const origin = `http://localhost:${port}`;
  for (const name of ['sanctuary', 'resonance']) {
    if (config.services[name].environment?.PUBLIC_ORIGIN !== origin ||
        config.services[name].environment?.DATABASE_URL !== `postgres://illya:${password}@postgres:5432/illyaverse`)
      throw new Error('Disposable application origin and database must target this fresh local project.');
  }
}

async function requireFreePort(port) {
  await new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', () => reject(new Error(`Disposable website port ${port} is in use; choose another ILLYA_WEB_SMOKE_PORT.`)));
    server.listen(port, '127.0.0.1', () => server.close(error => error ? reject(error) : resolve()));
  });
}

// Use the installed CLI directly so Windows does not need an npx.cmd shell.
function playwright(env, signal, files) {
  return new Promise((resolve, reject) => {
    const require = createRequire(path.join(workspace, 'apps/serendipity/package.json'));
    const cli = path.join(path.dirname(require.resolve('@playwright/test/package.json')), 'cli.js');
    const child = spawn(process.execPath, [cli, 'test', ...files], {
      cwd: path.join(workspace, 'apps/serendipity'), env, signal, windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const result = { stdout: '', stderr: '' };
    let bytes = 0;
    for (const stream of ['stdout', 'stderr']) {
      child[stream].setEncoding('utf8');
      child[stream].on('data', chunk => {
        bytes += Buffer.byteLength(chunk);
        if (bytes > 2 * 1024 * 1024) { child.kill(); reject(new Error('Website test output exceeded the safety limit.')); }
        else result[stream] += chunk;
      });
    }
    child.once('error', () => reject(new Error(signal.aborted ? 'Disposable website smoke interrupted.' : 'Could not start installed Playwright.')));
    child.once('close', code => resolve({ ...result, code: code ?? 1 }));
  });
}

async function requireAbsent(target) {
  try { await access(target); } catch (error) { if (error.code === 'ENOENT') return; throw error; }
  throw new Error('Owned temporary website environment remains after cleanup.');
}

export async function disposableWebSmoke(environment = process.env, { runner = runCommand, browserRunner = playwright } = {}) {
  requireDisposable(environment);
  const references = smokeImageReferences(environment, 'ILLYA_WEB_SMOKE', imageTags);
  const files = webSmokeTestFiles(environment.ILLYA_WEB_SMOKE_TEST_FILES);
  const port = webSmokePort(environment.ILLYA_WEB_SMOKE_PORT);
  await requireFreePort(port);
  const values = {
    POSTGRES_PASSWORD: randomBytes(32).toString('hex'), SESSION_SECRET: randomBytes(32).toString('hex'),
    INTERNAL_SERVICE_KEY: randomBytes(32).toString('hex'),
    BOOTSTRAP_ADMIN_USERNAME: `web_smoke_admin_${randomBytes(4).toString('hex')}`,
    BOOTSTRAP_ADMIN_PASSWORD: randomBytes(32).toString('hex'), BOOTSTRAP_ADMIN_EMAIL: 'web-smoke-admin@example.test',
    ILLYA_WEB_SMOKE_PORT: String(port),
  };
  const secrets = Object.entries(values).filter(([key]) => /PASSWORD|SECRET|KEY/.test(key)).map(([, value]) => value);
  const project = `illya-web-smoke-${randomBytes(8).toString('hex')}`;
  const tempRoot = await realpath(os.tmpdir());
  const temp = await mkdtemp(path.join(tempRoot, 'illya-web-smoke-'));
  const envFile = path.join(temp, 'test.env');
  const env = { ...environment, ...values, COMPOSE_PROJECT_NAME: project };
  const controller = new AbortController();
  const interrupt = () => controller.abort();
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', interrupt);
  const base = ['compose', '--project-name', project, '--env-file', envFile, '--file', path.join(workspace, 'infra/compose.web-smoke.yaml')];
  let started = false;
  let success = false;
  async function checked(args, signal = controller.signal) {
    const result = await runner('docker', args, { env, signal: signal ?? undefined });
    if (result.code !== 0) {
      process.stderr.write(redact(result.stderr, secrets));
      throw new Error(`Disposable website Docker operation failed with exit code ${result.code}.`);
    }
    return result.stdout;
  }
  try {
    const images = await pinExistingSmokeImages(references, reference =>
      checked(['image', 'inspect', reference, '--format', '{{.Id}}']));
    for (const { name, key } of references) {
      env[key] = values[key] = images[name];
    }
    await writeFile(envFile, Object.entries(values).map(([key, value]) => `${key}=${value}`).join('\n') + '\n', { mode: 0o600, flag: 'wx' });
    // Rendered JSON contains generated secrets. Never print it or load the actual .env.
    let config;
    const rendered = await checked([...base, 'config', '--format', 'json']);
    try { config = JSON.parse(rendered); } catch { throw new Error('Could not parse the disposable website Compose configuration.'); }
    validateWebSmokeConfig(config, {
      project, port, images, caddyFile: await realpath(path.join(workspace, 'infra/Caddyfile')), password: values.POSTGRES_PASSWORD,
    });
    console.log(`Starting isolated PostgreSQL 17 website smoke: ${project}, http://localhost:${port}.`);
    for (const [name, image] of Object.entries(images)) console.log(`Pinned ${name} image: ${image}.`);
    started = true;
    await checked([...base, 'up', '-d', '--no-build', '--wait', '--wait-timeout', '120']);
    const capture = environment.ILLYA_WEB_SMOKE_CAPTURE === '1' ? Object.fromEntries(
      ['RBAC', 'RECOVERY', 'RELAX', 'SCORE', 'WORKFLOW'].map(name => [`ILLYA_CAPTURE_${name}_REVIEW`, '1']),
    ) : {};
    const result = await browserRunner({
      ...env, ...capture, ILLYA_WEB_URL: `http://localhost:${port}`, ILLYA_TEST_ENV_FILE: envFile, ILLYA_ALLOW_DISPOSABLE_TEST: '1',
    }, controller.signal, files);
    process.stdout.write(redact(result.stdout, secrets));
    process.stderr.write(redact(result.stderr, secrets));
    if (result.code !== 0) throw new Error('The isolated final-image website browser gate failed.');
    success = true;
  } catch (error) {
    if (started) {
      try {
        const logs = await checked([...base, 'logs', '--no-color', '--tail', '20'], null);
        process.stderr.write(redact(logs, secrets));
      } catch { process.stderr.write(`Could not read startup diagnostics for ${project}.\n`); }
    }
    throw error;
  } finally {
    process.removeListener('SIGINT', interrupt);
    process.removeListener('SIGTERM', interrupt);
    const failures = [];
    if (started) {
      try { await checked([...base, 'down', '--volumes', '--remove-orphans', '--timeout', '10'], null); }
      catch { failures.push('Docker project removal'); }
      for (const args of [['ps', '-aq'], ['volume', 'ls', '-q'], ['network', 'ls', '-q']]) {
        try {
          if ((await checked([...args, '--filter', `label=com.docker.compose.project=${project}`], null)).trim())
            failures.push(`remaining ${args[0]} resources`);
        } catch { failures.push(`${args[0]} cleanup verification`); }
      }
    }
    // Always clean the exact private file, even if Docker cleanup needs attention.
    try {
      const resolved = await realpath(temp);
      if (path.dirname(resolved) !== tempRoot || !path.basename(resolved).startsWith('illya-web-smoke-') || path.dirname(envFile) !== temp)
        throw new Error('Refusing cleanup outside the owned website smoke directory.');
      await unlink(envFile).catch(error => { if (error.code !== 'ENOENT') throw error; });
      await rmdir(temp);
      await requireAbsent(envFile);
      await requireAbsent(temp);
    } catch { failures.push(`temporary environment cleanup at ${temp}`); }
    if (failures.length) throw new Error(`Cleanup needs attention for ${project}: ${failures.join('; ')}.`);
    console.log(`Verified removal of disposable containers, networks, volumes and temporary environment for ${project}.`);
  }
  if (success) console.log('Final-image isolated website browser gate passed.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await disposableWebSmoke(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
