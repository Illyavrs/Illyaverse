import { readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function localOrigin(value, label) {
  // Check spelling too: URL() normalizes alternative integer/octal IP forms.
  const match = /^http:\/\/(localhost|127\.0\.0\.1)(?::([1-9][0-9]{0,4}))?\/?$/i.exec(value);
  if (!match || (match[2] && Number(match[2]) > 65535)) {
    throw new Error(`${label} must be an http localhost/127.0.0.1 origin without credentials, path, query or fragment.`);
  }
  return new URL(value).origin;
}

export function requireDisposable(environment) {
  if (environment.ILLYA_ALLOW_DISPOSABLE_TEST !== '1') {
    throw new Error('Set ILLYA_ALLOW_DISPOSABLE_TEST=1 only for a disposable local Compose database.');
  }
}

export function redact(value, secrets) {
  for (const secret of secrets.filter(Boolean).sort((a, b) => b.length - a.length)) value = value.replaceAll(secret, '[redacted]');
  return value
    .replace(/((?:["']?(?:access_token|refresh_token|cho-token|osu-token|password|password_hash|session_secret|internal_service_key)["']?)\s*[:=]\s*["'])[^"'\r\n]*(["'])/gi, '$1[redacted]$2')
    .replace(/Bearer\s+[A-Za-z0-9._-]+/gi, 'Bearer [redacted]');
}

export function runCommand(command, args, { env = process.env, signal } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: workspace, env, signal, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    const output = { stdout: '', stderr: '' };
    let bytes = 0;
    for (const name of ['stdout', 'stderr']) {
      child[name].setEncoding('utf8');
      child[name].on('data', chunk => {
        bytes += Buffer.byteLength(chunk);
        if (bytes > 2 * 1024 * 1024) {
          child.kill();
          reject(new Error('Test subprocess output exceeded the safety limit.'));
        } else output[name] += chunk;
      });
    }
    child.once('error', () => reject(new Error(signal?.aborted ? 'Disposable smoke interrupted.' : 'Could not start a required test subprocess.')));
    child.once('close', code => resolve({ ...output, code: code ?? 1 }));
  });
}

export function pythonRuntime(environment = process.env) {
  const venv = path.join(workspace, '.protocol-venv', process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
  const python = environment.ILLYA_TEST_PYTHON ?? (existsSync(venv) ? venv : process.platform === 'win32' ? 'python' : 'python3');
  const env = { ...environment, PYTHONDONTWRITEBYTECODE: '1' };
  const dependencies = path.join(workspace, '.test-deps');
  if (!existsSync(venv) && existsSync(dependencies)) env.PYTHONPATH = [dependencies, environment.PYTHONPATH].filter(Boolean).join(path.delimiter);
  return { python, env };
}

export function protocolScript(value) {
  if (value === undefined || value === 'protocol') return 'services/resonance/tests/protocol_smoke.py';
  if (value === 'official-archives') return 'services/resonance/tests/official_archive_smoke.py';
  if (value === 'official-set-metadata') return 'services/resonance/tests/official_set_metadata_smoke.py';
  if (value === 'official-search') return 'services/resonance/tests/official_search_smoke.py';
  throw new Error('ILLYA_PROTOCOL_SMOKE_SUITE must be protocol, official-archives, official-set-metadata or official-search.');
}

export async function launchSmoke(environment = process.env, signal) {
  requireDisposable(environment);
  // Reject unsafe destinations before reading credentials or performing network requests.
  const sanctuary = localOrigin(environment.TEST_SANCTUARY_URL ?? 'http://localhost:3000', 'TEST_SANCTUARY_URL');
  const resonance = localOrigin(environment.TEST_RESONANCE_URL ?? 'http://127.0.0.1:8081', 'TEST_RESONANCE_URL');
  const script = protocolScript(environment.ILLYA_PROTOCOL_SMOKE_SUITE);
  let contents;
  try { contents = await readFile(path.resolve(workspace, environment.ILLYA_TEST_ENV_FILE ?? '.env'), 'utf8'); }
  catch { throw new Error('The disposable test environment file could not be read.'); }
  const values = Object.create(null);
  for (const raw of contents.split(/\r?\n/)) {
    const line = raw.trim();
    const at = line.indexOf('=');
    if (at > 0 && /^[A-Z_][A-Z0-9_]*$/.test(line.slice(0, at))) values[line.slice(0, at)] = line.slice(at + 1);
  }
  if (!values.BOOTSTRAP_ADMIN_USERNAME || !values.BOOTSTRAP_ADMIN_PASSWORD) throw new Error('This disposable test requires its bootstrap account.');
  const runtime = pythonRuntime(environment);
  const pythonCheck = await runCommand(runtime.python, ['-c', 'import py3rijndael'], { env: runtime.env, signal });
  if (pythonCheck.code !== 0) throw new Error('Install the protocol test requirements in the selected Python environment.');
  const username = `protocol_${randomBytes(4).toString('hex')}`;
  const password = randomBytes(24).toString('hex');
  const registration = await fetch(`${sanctuary}/api/v1/auth/register`, {
    method: 'POST', headers: { 'Content-Type': 'application/json', Origin: 'http://localhost:3000' },
    body: JSON.stringify({ username, password, email: `${username}@example.test` }),
    redirect: 'error', signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(15000)]) : AbortSignal.timeout(15000),
  });
  if (!registration.ok) throw new Error(`Fixture registration failed: HTTP ${registration.status}`);
  const result = await runCommand(runtime.python, [script], {
    env: { ...runtime.env,
      TEST_ADMIN_USERNAME: values.BOOTSTRAP_ADMIN_USERNAME, TEST_ADMIN_PASSWORD: values.BOOTSTRAP_ADMIN_PASSWORD,
      TEST_PLAYER_USERNAME: username, TEST_PLAYER_PASSWORD: password,
      TEST_SANCTUARY_URL: sanctuary, TEST_RESONANCE_URL: resonance,
    },
    signal,
  });
  const secrets = [password, ...Object.entries(values).filter(([key]) => /PASSWORD|SECRET|KEY/.test(key)).map(([, value]) => value)];
  process.stdout.write(redact(result.stdout, secrets));
  process.stderr.write(redact(result.stderr, secrets));
  if (result.code !== 0) throw new Error(`Protocol smoke failed with exit code ${result.code}.`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const controller = new AbortController();
  const interrupt = () => controller.abort();
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', interrupt);
  try { await launchSmoke(process.env, controller.signal); }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
  finally {
    process.removeListener('SIGINT', interrupt);
    process.removeListener('SIGTERM', interrupt);
  }
}
