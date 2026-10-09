import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkedDocker, completeBackup, nonce, ownerLabel, privateDirectory, projectName, validImage, verifyBackup } from './recovery-lib.mjs';

export function backupArguments(args) {
  const options = {};
  for (let index = 0; index < args.length; index++) {
    const argument = args[index];
    if (argument === '--writers-stopped' && !options.writersStopped) options.writersStopped = true;
    else if ((argument === '--project' || argument === '--output') && !options[argument.slice(2)]) {
      const value = args[++index];
      if (!value || value.startsWith('--')) throw new Error('Backup project/output requires a value.');
      options[argument.slice(2)] = value;
    } else throw new Error('Use only --project NAME --output NEW_DIRECTORY --writers-stopped.');
  }
  projectName(options.project);
  if (!options.output || !options.writersStopped) throw new Error('Backup requires --output NEW_DIRECTORY and --writers-stopped after stopping all application writers.');
  return options;
}

function inspectOne(text) {
  const values = JSON.parse(text);
  if (!Array.isArray(values) || values.length !== 1) throw new Error('Expected exactly one source Docker resource.');
  return values[0];
}

export function validateBackupSource({ postgres, sanctuary, shared, project }) {
  projectName(project);
  for (const [service, container] of Object.entries({ postgres, sanctuary })) {
    if (!/^[a-f0-9]{64}$/.test(container.Id ?? '') || container.Config?.Labels?.['com.docker.compose.project'] !== project ||
        container.Config?.Labels?.['com.docker.compose.service'] !== service)
      throw new Error('Source containers must have the selected project and service labels.');
  }
  if (postgres.State?.Running !== true || sanctuary.State?.Running !== false || !validImage(sanctuary.Image) || !validImage(postgres.Image))
    throw new Error('PostgreSQL must be running and Sanctuary must be stopped with a locally pinned image.');
  if (shared.Name !== `${project}_shared_data` || shared.Labels?.['com.docker.compose.project'] !== project ||
      shared.Labels?.['com.docker.compose.volume'] !== 'shared_data')
    throw new Error('Source shared_data volume must belong to the selected Compose project.');
  const mounts = sanctuary.Mounts?.filter(mount => mount.Destination === '/data') ?? [];
  if (mounts.length !== 1 || mounts[0].Type !== 'volume' || mounts[0].Name !== shared.Name)
    throw new Error('Source Sanctuary must use the selected project shared_data volume.');
  const pgMount = postgres.Mounts?.filter(mount => mount.Destination === '/var/lib/postgresql/data') ?? [];
  if (pgMount.length !== 1 || pgMount[0].Type !== 'volume' || pgMount[0].Name !== `${project}_postgres_data`)
    throw new Error('Source PostgreSQL must use its selected project PostgreSQL volume.');
}

export async function backupDeployment(options, { environment = process.env, runner, signal } = {}) {
  const { project, output, writersStopped } = options;
  projectName(project);
  if (!writersStopped) throw new Error('Application writers must be stopped before backup.');
  const docker = checkedDocker({ environment, runner, signal });
  const owner = nonce();
  const helperName = `illya-backup-${owner}`;
  const temporaryDump = `/tmp/illya-backup-${owner}.dump`;
  const temporaryArchive = `/tmp/illya-backup-${owner}.tar.gz`;
  let helperCreated = false;
  let dumpCreated = false;
  let source;
  const assertWritersStopped = async () => {
    const ids = (await docker(['ps', '-q', '--filter', `label=com.docker.compose.project=${project}`])).trim().split(/\s+/).filter(Boolean);
    if (ids.some(id => !/^[a-f0-9]{12,64}$/.test(id))) throw new Error('Unexpected source container identity.');
    if (ids.length) {
      const containers = JSON.parse(await docker(['inspect', ...ids]));
      if (containers.some(container => container.Config?.Labels?.['com.docker.compose.project'] !== project ||
          !['postgres', 'caddy', 'serendipity'].includes(container.Config?.Labels?.['com.docker.compose.service'])))
        throw new Error('Stop Sanctuary, Resonance and any other application writers before backup; the helper never stops them.');
    }
  };
  try {
    await assertWritersStopped();
    const containers = {};
    for (const service of ['postgres', 'sanctuary']) {
      const ids = (await docker(['ps', '-a', '-q', '--no-trunc', '--filter', `label=com.docker.compose.project=${project}`, '--filter', `label=com.docker.compose.service=${service}`])).trim();
      if (!/^[a-f0-9]{64}$/.test(ids)) throw new Error(`Expected one ${service} container in the selected source project.`);
      containers[service] = inspectOne(await docker(['inspect', ids]));
    }
    const shared = inspectOne(await docker(['volume', 'inspect', `${project}_shared_data`]));
    validateBackupSource({ ...containers, shared, project });
    source = containers.postgres.Id;
    const version = (await docker(['exec', source, 'psql', '-X', '-At', '-U', 'illya', '-d', 'illyaverse', '-c', 'SHOW server_version_num;'])).trim();
    if (!/^17[0-9]{4}$/.test(version)) throw new Error('Recovery backup currently requires PostgreSQL major version 17.');
    const directory = await privateDirectory(output, runner);
    const id = (await docker(['create', '--pull', 'never', '--name', helperName, '--label', `${ownerLabel}=${owner}`, '--network', 'none',
      '--mount', `type=volume,source=${shared.Name},target=/data,readonly,volume-nocopy`,
      '--user', '0:0', '--entrypoint', 'sleep', containers.sanctuary.Image, 'infinity'])).trim();
    helperCreated = true;
    if (!/^[a-f0-9]{64}$/.test(id)) throw new Error('Could not identify the owned archive helper.');
    await docker(['start', helperName]);
    const manifest = await completeBackup(directory, {
      metadata: { project, postgresMajor: 17, postgresImage: containers.postgres.Image, sanctuaryImage: containers.sanctuary.Image, includes: ['PostgreSQL database', 'entire /data'], excludes: ['deployment .env/secrets', 'Caddy volumes'] },
      produce: async (name, destination) => {
        await assertWritersStopped();
        if (name === 'illyaverse.dump') {
          dumpCreated = true;
          await docker(['exec', source, 'pg_dump', '-U', 'illya', '-d', 'illyaverse', '-Fc', '--file', temporaryDump]);
          await docker(['exec', source, 'pg_restore', '--list', temporaryDump]);
          await docker(['cp', `${source}:${temporaryDump}`, destination]);
        } else {
          await docker(['exec', helperName, 'tar', '-czf', temporaryArchive, '-C', '/data', '.']);
          await docker(['exec', helperName, 'sh', '-c', `tar -tzf ${temporaryArchive} >/dev/null`]);
          await docker(['cp', `${helperName}:${temporaryArchive}`, destination]);
        }
      },
      validate: assertWritersStopped,
      beforeComplete: assertWritersStopped,
    });
    await verifyBackup(directory);
    console.log(`Complete backup: ${directory}. Keep its manifest, private deployment secrets and separate Caddy snapshot together.`);
    return manifest;
  } finally {
    const failures = [];
    if (dumpCreated) {
      try { await docker(['exec', source, 'rm', '-f', temporaryDump], null); }
      catch { failures.push('temporary PostgreSQL dump removal'); }
    }
    if (helperCreated) {
      try {
        const helper = inspectOne(await docker(['inspect', helperName], null));
        if (helper.Name !== `/${helperName}` || helper.Config?.Labels?.[ownerLabel] !== owner)
          throw new Error('Refusing cleanup of an archive helper without exact nonce ownership.');
        await docker(['rm', '-f', helperName], null);
      } catch { failures.push(`owned archive helper cleanup (${helperName})`); }
    }
    if (failures.length) throw new Error(`Backup cleanup needs attention: ${failures.join('; ')}. Preserve this attempt's output and review its manifest before use.`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await backupDeployment(backupArguments(process.argv.slice(2))); }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
