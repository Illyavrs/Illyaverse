import { randomBytes } from 'node:crypto';
import { mkdtemp, realpath, rmdir, unlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { pythonRuntime, redact, requireDisposable, runCommand, workspace } from './protocol-smoke.mjs';
import { pinExistingSmokeImages, smokeImageReferences } from './existing-smoke-images.mjs';

const imageTags = {
  postgres: 'postgres:17-alpine', sanctuary: 'illyaverse/sanctuary:local', resonance: 'illyaverse/resonance:local',
};

function testPort(value, fallback) {
  const port = value === undefined ? fallback : Number(value);
  if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Disposable test ports must be integers from 1024 to 65535.');
  return port;
}
async function requireFreePort(port) {
  await new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', () => reject(new Error(`Disposable test port ${port} is already in use; select another ILLYA_SMOKE_*_PORT.`)));
    server.listen(port, '127.0.0.1', () => server.close(error => error ? reject(error) : resolve()));
  });
}

export async function disposableSmoke(environment = process.env, { runner = runCommand } = {}) {
  requireDisposable(environment);
  const references = smokeImageReferences(environment, 'ILLYA_SMOKE', imageTags);
  const sanctuaryPort = testPort(environment.ILLYA_SMOKE_SANCTUARY_PORT, 18082);
  const resonancePort = testPort(environment.ILLYA_SMOKE_RESONANCE_PORT, 18083);
  if (sanctuaryPort === resonancePort) throw new Error('Disposable service ports must differ.');
  await requireFreePort(sanctuaryPort);
  await requireFreePort(resonancePort);
  const runtime = pythonRuntime(environment);
  if ((await runner(runtime.python, ['-c', 'import py3rijndael'], { env: runtime.env })).code !== 0) throw new Error('Install the protocol test requirements before starting the disposable helper.');
  const values = {
    POSTGRES_PASSWORD: randomBytes(32).toString('hex'), SESSION_SECRET: randomBytes(32).toString('hex'),
    INTERNAL_SERVICE_KEY: randomBytes(32).toString('hex'),
    BOOTSTRAP_ADMIN_USERNAME: `smoke_admin_${randomBytes(4).toString('hex')}`,
    BOOTSTRAP_ADMIN_PASSWORD: randomBytes(32).toString('hex'), BOOTSTRAP_ADMIN_EMAIL: 'smoke-admin@example.test',
    ILLYA_SMOKE_SANCTUARY_PORT: String(sanctuaryPort), ILLYA_SMOKE_RESONANCE_PORT: String(resonancePort),
  };
  const secrets = Object.entries(values).filter(([key]) => /PASSWORD|SECRET|KEY/.test(key)).map(([, value]) => value);
  const project = `illya-smoke-${randomBytes(8).toString('hex')}`;
  const tempRoot = await realpath(os.tmpdir());
  const temp = await mkdtemp(path.join(tempRoot, 'illya-smoke-'));
  const envFile = path.join(temp, 'test.env');
  const env = { ...environment, ...values, COMPOSE_PROJECT_NAME: project };
  const controller = new AbortController();
  const interrupt = () => controller.abort();
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', interrupt);
  const base = ['compose', '--project-name', project, '--env-file', envFile, '--file', path.join(workspace, 'infra/compose.smoke.yaml')];
  let started = false;
  let success = false;
  async function checked(command, args, signal = controller.signal) {
    const result = await runner(command, args, { env, signal: signal ?? undefined });
    if (result.code !== 0) {
      process.stderr.write(redact(result.stderr, secrets));
      throw new Error(`${command} test operation failed with exit code ${result.code}.`);
    }
    return result.stdout;
  }
  try {
    const images = await pinExistingSmokeImages(references, reference =>
      checked('docker', ['image', 'inspect', reference, '--format', '{{.Id}}']));
    for (const { name, key } of references) {
      values[key] = images[name];
      env[key] = images[name];
    }
    await writeFile(envFile, Object.entries(values).map(([key, value]) => `${key}=${value}`).join('\n') + '\n', { mode: 0o600, flag: 'wx' });
    // Render into memory only: this JSON includes credentials and must never be printed.
    const config = JSON.parse(await checked('docker', [...base, 'config', '--format', 'json']));
    if (config.name !== project || Object.keys(config.services).sort().join() !== 'postgres,resonance,sanctuary') throw new Error('Unexpected disposable Compose services.');
    if (Object.values(config.volumes).some(volume => volume.external || !volume.name.startsWith(`${project}_`)) || Object.values(config.networks).some(network => network.external || !network.name.startsWith(`${project}_`))) throw new Error('Disposable resources must belong exclusively to this random project.');
    for (const [name, service] of Object.entries(config.services)) {
      if (service.image !== images[name] || service.pull_policy !== 'never') throw new Error('Disposable services must use the pinned existing images without pulls.');
      if (service.build || service.volumes?.some(volume => volume.type !== 'volume') || service.ports?.some(port => port.host_ip !== '127.0.0.1')) throw new Error('Disposable Compose may only use its own volumes and loopback ports.');
    }
    console.log(`Starting isolated PostgreSQL 17 protocol smoke: ${project}, ports ${sanctuaryPort}/${resonancePort}.`);
    console.log(`Application images: Sanctuary ${values.ILLYA_SMOKE_SANCTUARY_IMAGE}; Resonance ${values.ILLYA_SMOKE_RESONANCE_IMAGE}.`);
    started = true;
    await checked('docker', [...base, 'up', '-d', '--no-build', '--wait', '--wait-timeout', '120']);
    const result = await runner(process.execPath, [path.join(workspace, 'infra/protocol-smoke.mjs')], {
      env: { ...runtime.env, ...env, ILLYA_TEST_ENV_FILE: envFile, TEST_SANCTUARY_URL: `http://127.0.0.1:${sanctuaryPort}`, TEST_RESONANCE_URL: `http://127.0.0.1:${resonancePort}` }, signal: controller.signal,
    });
    process.stdout.write(redact(result.stdout, secrets));
    process.stderr.write(redact(result.stderr, secrets));
    if (result.code !== 0) throw new Error('The isolated final-image protocol smoke failed.');
    success = true;
  } catch (error) {
    if (started) {
      const diagnostics = await runner('docker', [...base, 'logs', '--no-color', '--tail', '20', 'sanctuary', 'resonance'], { env });
      process.stderr.write(redact(diagnostics.stdout + diagnostics.stderr, secrets));
    }
    throw error;
  } finally {
    process.removeListener('SIGINT', interrupt);
    process.removeListener('SIGTERM', interrupt);
    if (started) {
      await checked('docker', [...base, 'down', '--volumes', '--remove-orphans', '--timeout', '10'], null);
      for (const args of [['ps', '-aq'], ['volume', 'ls', '-q'], ['network', 'ls', '-q']]) {
        const remaining = (await checked('docker', [...args, '--filter', `label=com.docker.compose.project=${project}`], null)).trim();
        if (remaining) throw new Error(`Disposable resources remain for ${project}; cleanup needs attention.`);
      }
    }
    // No recursive deletion: verify the exact directory, then remove only our file.
    const resolved = await realpath(temp);
    if (path.dirname(resolved) !== tempRoot || !path.basename(resolved).startsWith('illya-smoke-') || path.dirname(envFile) !== temp) throw new Error('Refusing cleanup outside the owned smoke directory.');
    await unlink(envFile).catch(error => { if (error.code !== 'ENOENT') throw error; });
    await rmdir(temp);
    console.log(`Removed disposable containers, network, volumes and temporary environment for ${project}.`);
  }
  if (success) console.log('Final-image isolated HTTP protocol smoke passed. Main Illyaverse services and data were not used.');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await disposableSmoke(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
