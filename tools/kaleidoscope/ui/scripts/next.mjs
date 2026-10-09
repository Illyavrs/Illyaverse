import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
// Keep the launcher self-contained while retaining SWC's native integrity checks.
const env = { ...process.env };
if (process.platform === 'win32' && !env.SWC_NATIVE_BINDING_CACHE) {
  const cache = fileURLToPath(new URL('../.swc-cache/', import.meta.url));
  mkdirSync(cache, { recursive: true });
  env.SWC_NATIVE_BINDING_CACHE = cache;
}
const child = spawn(
  process.execPath,
  [require.resolve('next/dist/bin/next'), ...process.argv.slice(2)],
  { stdio: 'inherit', env },
);
child.on('exit', (code) => process.exit(code ?? 1));
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill(signal));
