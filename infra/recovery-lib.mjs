import { createHash, randomBytes } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { chmod, lstat, mkdir, readFile, readdir, realpath, rename, rmdir, unlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { runCommand, workspace } from './protocol-smoke.mjs';

export const ownerLabel = 'io.illyaverse.recovery.owner';
export const artifactNames = ['illyaverse.dump', 'shared-data.tar.gz'];
export const exactMatch = (pattern, value) => typeof value === 'string' && pattern.exec(value)?.[0] === value;
export const validImage = value => exactMatch(/^sha256:[a-f0-9]{64}$/, value);
export const nonce = () => randomBytes(8).toString('hex');

export async function readSourceMigrations(directory = path.join(workspace, 'migrations')) {
  const files = (await readdir(directory)).filter(name => name.endsWith('.sql'));
  if (!files.length) throw new Error('Recovery requires at least one checked-in SQL migration.');
  const versions = new Set();
  const migrations = [];
  for (const filename of files) {
    if (!exactMatch(/^[0-9]{4}_[a-z0-9_]+\.sql$/, filename)) throw new Error('Unexpected source migration filename.');
    const version = Number(filename.slice(0, 4));
    if (version < 1 || versions.has(version)) throw new Error('Source migration versions must be positive and unique.');
    versions.add(version);
    const file = path.join(directory, filename);
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Source SQL migrations must be regular files without links.');
    migrations.push({ filename, version, checksum: createHash('sha384').update(await readFile(file)).digest('hex') });
  }
  return migrations.sort((left, right) => left.version - right.version);
}

export function projectName(value) {
  if (!exactMatch(/^[a-z][a-z0-9_-]{0,62}$/, value))
    throw new Error('Project name must be 1–63 lowercase letters, digits, underscores or hyphens, starting with a letter.');
  return value;
}

export function ownedProject(value, owner) {
  if (!exactMatch(/^[a-f0-9]{16}$/, owner) || !exactMatch(new RegExp(`^illya-recovery-${owner}-(source|restore)$`), value))
    throw new Error('Refusing a project outside this recovery drill nonce.');
  return value;
}

export function ownedResource(resource, { project, owner, kind, name }) {
  ownedProject(project, owner);
  if (!['container', 'volume', 'network'].includes(kind)) throw new Error('Refusing an unknown recovery resource kind.');
  const labels = resource.Labels ?? resource.Config?.Labels ?? {};
  if (labels[ownerLabel] !== owner || labels['com.docker.compose.project'] !== project)
    throw new Error('Refusing a recovery resource without both exact ownership labels.');
  if (kind === 'volume' && (!['postgres_data', 'shared_data'].includes(name) || resource.Name !== `${project}_${name}` || labels['com.docker.compose.volume'] !== name))
    throw new Error('Refusing an unexpected recovery volume.');
  if (kind === 'network' && (resource.Name !== `${project}_default` || labels['com.docker.compose.network'] !== 'default'))
    throw new Error('Refusing an unexpected recovery network.');
  if (kind === 'container' && (!exactMatch(new RegExp(`^/${project}[-_](postgres|sanctuary|archive)[-_](?:1|run[-_][a-f0-9]+)$`), resource.Name) ||
      !['postgres', 'sanctuary', 'archive'].includes(labels['com.docker.compose.service'])))
    throw new Error('Refusing an unexpected recovery container.');
}

export function dockerEnvironment(environment = process.env) {
  // A selected Docker context/host is respected. Compose interpolation and inherited
  // application secrets never select a project, override files or enter test services.
  return Object.fromEntries(Object.entries(environment).filter(([name]) => {
    const key = name.toUpperCase(); // Windows environment lookup is case-insensitive.
    return !key.startsWith('COMPOSE_') && !/^(POSTGRES_|DATABASE_URL$|SESSION_SECRET$|INTERNAL_SERVICE_KEY$|BOOTSTRAP_ADMIN_|MAIL_|SMTP_|ILLYA_)/.test(key);
  }));
}

export function checkedDocker({ environment = process.env, signal, runner = runCommand } = {}) {
  const env = dockerEnvironment(environment);
  return async (args, overrideSignal = signal) => {
    const result = await runner('docker', args, { env, signal: overrideSignal ?? undefined });
    if (result.code !== 0) throw new Error(`Recovery Docker operation ${args[0]} failed (exit ${result.code}).`);
    return result.stdout;
  };
}

export async function privateDirectory(target, runner = runCommand) {
  const absolute = path.resolve(target);
  const parent = await realpath(path.dirname(absolute));
  const directory = path.join(parent, path.basename(absolute));
  if (!path.basename(directory) || directory === parent) throw new Error('Backup requires a new directory under an existing private parent.');
  await mkdir(directory, { mode: 0o700 }); // EEXIST is intentionally fatal, including symlinks.
  if (process.platform === 'win32') {
    const identity = await runner('whoami.exe', ['/user', '/fo', 'csv', '/nh']);
    const sid = identity.code === 0 && /"(S-1-[0-9-]+)"/.exec(identity.stdout)?.[1];
    if (!sid) throw new Error('Could not identify the current Windows user for private backup ACLs.');
    const acl = await runner('icacls.exe', [directory, '/inheritance:r', '/grant:r', `*${sid}:(OI)(CI)F`]);
    if (acl.code !== 0) throw new Error('Could not restrict the new recovery directory to the current Windows user.');
  }
  return directory;
}

export async function digestFile(file) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size === 0) throw new Error('Recovery artifact must be a nonempty regular file.');
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(file)) hash.update(bytes);
  return { bytes: stat.size, sha256: hash.digest('hex') };
}

// Files may be copied only into a newly created private directory. Previous
// successful backups are never touched; failed runs leave inspectable partials.
export async function completeBackup(directory, { produce, validate, beforeComplete = async () => {}, metadata = {} }) {
  if ((await readdir(directory)).length) throw new Error('Refusing existing backup output; select a new empty private directory.');
  const artifacts = {};
  for (const name of artifactNames) {
    const partial = path.join(directory, `${name}.partial`);
    await produce(name, partial);
    const created = await lstat(partial);
    if (!created.isFile() || created.isSymbolicLink()) throw new Error('Recovery artifact must be a regular file.');
    await chmod(partial, 0o600);
    await validate(name, partial);
    artifacts[name] = await digestFile(partial);
  }
  await beforeComplete();
  for (const name of artifactNames) await rename(path.join(directory, `${name}.partial`), path.join(directory, name));
  const manifest = { ...metadata, format: 'illyaverse-recovery-v1', complete: true, createdAt: new Date().toISOString(), artifacts };
  await writeFile(path.join(directory, 'manifest.json.partial'), `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
  await rename(path.join(directory, 'manifest.json.partial'), path.join(directory, 'manifest.json'));
  return manifest;
}

export async function verifyBackup(directory) {
  const manifest = JSON.parse(await readFile(path.join(directory, 'manifest.json'), 'utf8'));
  if (manifest.format !== 'illyaverse-recovery-v1' || manifest.complete !== true ||
      Object.keys(manifest.artifacts ?? {}).sort().join() !== artifactNames.slice().sort().join())
    throw new Error('Backup has no recognized complete manifest.');
  for (const name of artifactNames) {
    const actual = await digestFile(path.join(directory, name));
    if (actual.bytes !== manifest.artifacts[name].bytes || actual.sha256 !== manifest.artifacts[name].sha256)
      throw new Error(`Backup checksum/size mismatch: ${name}.`);
  }
  return manifest;
}

export async function ownedTempDirectory(owner, runner) {
  if (!exactMatch(/^[a-f0-9]{16}$/, owner)) throw new Error('Invalid recovery temporary directory owner.');
  const root = await realpath(os.tmpdir());
  return { root, directory: await privateDirectory(path.join(root, `illya-recovery-${owner}`), runner) };
}

export async function cleanupTemp({ root, directory, owner, files }) {
  if (!exactMatch(/^[a-f0-9]{16}$/, owner) || path.dirname(directory) !== root || path.basename(directory) !== `illya-recovery-${owner}` ||
      await realpath(directory) !== directory || (await lstat(directory)).isSymbolicLink())
    throw new Error('Refusing cleanup outside the owned nonce recovery directory.');
  for (const name of files) {
    if (!exactMatch(/^[a-z][a-z0-9.-]*$/, name)) throw new Error('Refusing an unexpected recovery temporary filename.');
    const file = path.join(directory, name);
    await unlink(file).catch(error => { if (error.code !== 'ENOENT') throw error; });
  }
  await rmdir(directory); // Unexpected files cause failure; never recursive deletion.
}
