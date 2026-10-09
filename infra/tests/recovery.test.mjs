import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  artifactNames, checkedDocker, cleanupTemp, completeBackup, dockerEnvironment, nonce, ownedProject, ownedResource,
  ownedTempDirectory, ownerLabel, privateDirectory, projectName, readSourceMigrations, validImage, verifyBackup,
} from '../recovery-lib.mjs';
import { backupArguments, backupDeployment, validateBackupSource } from '../recovery-backup.mjs';
import {
  checkMigrations, cleanupProject, databaseSnapshot, drillArguments, recoveryConfig, recoveryDrill, requireFreshProject, sameDatabase, sanctuaryImageReference, validateRecoveryConfig,
} from '../disposable-recovery-drill.mjs';
import { workspace } from '../protocol-smoke.mjs';

const owner = '0123456789abcdef';
const project = `illya-recovery-${owner}-source`;
const images = { postgres: `sha256:${'1'.repeat(64)}`, sanctuary: `sha256:${'2'.repeat(64)}` };
const secrets = { password: '3'.repeat(64), sessionSecret: '4'.repeat(64), internalKey: '5'.repeat(64) };
const windowsSid = 'S-1-5-21-1234-5678-9012-1001';
const backupBytes = {
  'illyaverse.dump': Buffer.from([80, 71, 68, 77, 80, 0, 255, 128, 13, 10]),
  'shared-data.tar.gz': Buffer.from([31, 139, 8, 0, 255, 128, 13, 10]),
};

async function temporaryRoot(t) {
  const parent = path.resolve(os.tmpdir());
  const directory = await mkdtemp(path.join(parent, 'illya-recovery-offline-'));
  t.after(async () => {
    // Only remove the new fixture directory made by this test, under its fixed parent.
    assert.equal(path.dirname(path.resolve(directory)), parent);
    assert.match(path.basename(directory), /^illya-recovery-offline-/);
    await rm(directory, { recursive: true, force: true });
  });
  return directory;
}

function privateRunner() {
  const calls = [];
  const runner = async (command, args) => {
    calls.push({ command, args: [...args] });
    if (command === 'whoami.exe') return { code: 0, stdout: `"fixture\\owner","${windowsSid}"\r\n`, stderr: '' };
    if (command === 'icacls.exe') return { code: 0, stdout: 'Successfully processed 1 files; Failed processing 0 files\r\n', stderr: '' };
    assert.fail(`Offline fixture attempted an unexpected command: ${command}`);
  };
  return { calls, runner };
}

function recordingDocker(respond = () => '') {
  const calls = [];
  const docker = async (args, signal) => {
    calls.push({ args: [...args], signal });
    return await respond(args, calls.length);
  };
  return { calls, docker };
}

function labels(service) {
  return {
    [ownerLabel]: owner,
    'com.docker.compose.project': project,
    ...(service ? { 'com.docker.compose.service': service } : {}),
  };
}

function resource(kind, name = kind === 'volume' ? 'shared_data' : 'sanctuary') {
  if (kind === 'container') return { Id: 'a'.repeat(64), Name: `/${project}-${name}-1`, Config: { Labels: labels(name) } };
  return {
    Name: `${project}_${kind === 'network' ? 'default' : name}`,
    Labels: { ...labels(), [`com.docker.compose.${kind}`]: kind === 'network' ? 'default' : name },
  };
}

function backupSource() {
  const sourceProject = 'offline-source';
  const sourceLabels = service => ({ 'com.docker.compose.project': sourceProject, 'com.docker.compose.service': service });
  return {
    project: sourceProject,
    postgres: {
      Id: 'a'.repeat(64), Image: images.postgres, Config: { Labels: sourceLabels('postgres') }, State: { Running: true },
      Mounts: [{ Type: 'volume', Name: `${sourceProject}_postgres_data`, Destination: '/var/lib/postgresql/data' }],
    },
    sanctuary: {
      Id: 'b'.repeat(64), Image: images.sanctuary, Config: { Labels: sourceLabels('sanctuary') }, State: { Running: false },
      Mounts: [{ Type: 'volume', Name: `${sourceProject}_shared_data`, Destination: '/data' }],
    },
    shared: {
      Name: `${sourceProject}_shared_data`,
      Labels: { 'com.docker.compose.project': sourceProject, 'com.docker.compose.volume': 'shared_data' },
    },
  };
}

function config(restore = false) {
  return recoveryConfig({ project: restore ? project.replace('-source', '-restore') : project, owner, images, ...secrets, restore });
}

async function writeCompleteBackup(directory, options = {}) {
  return completeBackup(directory, {
    produce: async (name, destination) => writeFile(destination, backupBytes[name]),
    validate: async (name, file) => assert.deepEqual(await readFile(file), backupBytes[name]),
    metadata: { project: 'offline-source', postgresMajor: 17 },
    ...options,
  });
}

function backupLifecycleFixture(output, { failDumpCleanup = false } = {}) {
  const privacy = privateRunner();
  const controller = new AbortController();
  const calls = [];
  const state = { writerChecks: 0, helperRemoved: false, dumpRemoved: false };
  const postgres = {
    ...resource('container', 'postgres'), Image: images.postgres, State: { Running: true },
    Mounts: [{ Type: 'volume', Name: `${project}_postgres_data`, Destination: '/var/lib/postgresql/data' }],
  };
  const sanctuary = {
    ...resource('container'), Id: 'b'.repeat(64), Image: images.sanctuary, State: { Running: false },
    Mounts: [{ Type: 'volume', Name: `${project}_shared_data`, Destination: '/data' }],
  };
  const environment = {
    DOCKER_CONTEXT: 'offline-fixture', COMPOSE_PROJECT_NAME: 'production', COMPOSE_FILE: 'production.yaml',
    DATABASE_URL: 'production-database', SESSION_SECRET: 'production-session', ILLYA_ALLOW_DISPOSABLE_TEST: '1',
  };
  const success = stdout => ({ code: 0, stdout, stderr: '' });
  const runner = async (command, args, options) => {
    if (command !== 'docker') return privacy.runner(command, args);
    calls.push({ args: [...args], signal: options.signal, env: options.env });
    assert.deepEqual(options.env, { DOCKER_CONTEXT: 'offline-fixture' });
    const cleanup = args[0] === 'rm' || (args[0] === 'inspect' && args[1] === state.helperName) || (args[0] === 'exec' && args[2] === 'rm');
    assert.equal(options.signal, cleanup ? undefined : controller.signal);
    if (cleanup) assert.equal(controller.signal.aborted, true, 'Cleanup must still run after cancellation');
    if (args[0] === 'ps') {
      assert.equal(args.includes(`label=com.docker.compose.project=${project}`), true);
      if (!args.includes('-a')) {
        assert.deepEqual(args, ['ps', '-q', '--filter', `label=com.docker.compose.project=${project}`]);
        state.writerChecks++;
        return success(`${postgres.Id}\n`);
      }
      const service = args.find(value => value.startsWith('label=com.docker.compose.service='))?.split('=').at(-1);
      assert.equal(['postgres', 'sanctuary'].includes(service), true);
      return success(`${service === 'postgres' ? postgres.Id : sanctuary.Id}\n`);
    }
    if (args[0] === 'inspect') {
      if (args[1] === postgres.Id) {
        // The sixth running-writer inspection is the final pre-promotion check.
        // Cancel after it succeeds so cleanup receives an already aborted signal.
        if (state.writerChecks === 6) controller.abort();
        return success(JSON.stringify([postgres]));
      }
      if (args[1] === sanctuary.Id) return success(JSON.stringify([sanctuary]));
      assert.equal(args[1], state.helperName);
      return success(JSON.stringify([{ Name: `/${state.helperName}`, Config: { Labels: { [ownerLabel]: state.helperOwner } } }]));
    }
    if (args[0] === 'volume') {
      assert.deepEqual(args, ['volume', 'inspect', `${project}_shared_data`]);
      return success(JSON.stringify([resource('volume')]));
    }
    if (args[0] === 'create') {
      state.helperName = args[args.indexOf('--name') + 1];
      assert.match(state.helperName, /^illya-backup-[a-f0-9]{16}$/);
      state.helperOwner = state.helperName.slice('illya-backup-'.length);
      state.dump = `/tmp/illya-backup-${state.helperOwner}.dump`;
      state.archive = `/tmp/illya-backup-${state.helperOwner}.tar.gz`;
      assert.deepEqual(args, [
        'create', '--pull', 'never', '--name', state.helperName, '--label', `${ownerLabel}=${state.helperOwner}`, '--network', 'none',
        '--mount', `type=volume,source=${project}_shared_data,target=/data,readonly,volume-nocopy`,
        '--user', '0:0', '--entrypoint', 'sleep', images.sanctuary, 'infinity',
      ]);
      return success(`${'c'.repeat(64)}\n`);
    }
    if (args[0] === 'start') {
      assert.deepEqual(args, ['start', state.helperName]);
      return success('');
    }
    if (args[0] === 'exec') {
      const operation = args[2];
      if (operation === 'psql') {
        assert.deepEqual(args, ['exec', postgres.Id, 'psql', '-X', '-At', '-U', 'illya', '-d', 'illyaverse', '-c', 'SHOW server_version_num;']);
        return success('170006\n');
      }
      if (operation === 'pg_dump') assert.deepEqual(args, ['exec', postgres.Id, 'pg_dump', '-U', 'illya', '-d', 'illyaverse', '-Fc', '--file', state.dump]);
      else if (operation === 'pg_restore') assert.deepEqual(args, ['exec', postgres.Id, 'pg_restore', '--list', state.dump]);
      else if (operation === 'tar') assert.deepEqual(args, ['exec', state.helperName, 'tar', '-czf', state.archive, '-C', '/data', '.']);
      else if (operation === 'sh') assert.deepEqual(args, ['exec', state.helperName, 'sh', '-c', `tar -tzf ${state.archive} >/dev/null`]);
      else if (operation === 'rm') {
        assert.deepEqual(args, ['exec', postgres.Id, 'rm', '-f', state.dump]);
        if (failDumpCleanup) return { code: 9, stdout: '', stderr: 'fixture dump removal failure' };
        state.dumpRemoved = true;
      } else assert.fail(`Unexpected backup exec operation: ${JSON.stringify(args)}`);
      return success('');
    }
    if (args[0] === 'cp') {
      const name = args[1] === `${postgres.Id}:${state.dump}` ? 'illyaverse.dump' : 'shared-data.tar.gz';
      assert.equal(args[1], name === 'illyaverse.dump' ? `${postgres.Id}:${state.dump}` : `${state.helperName}:${state.archive}`);
      assert.equal(args[2], path.join(output, `${name}.partial`));
      await writeFile(args[2], backupBytes[name]);
      return success('');
    }
    if (args[0] === 'rm') {
      assert.deepEqual(args, ['rm', '-f', state.helperName]);
      state.helperRemoved = true;
      return success('');
    }
    assert.fail(`Offline backup fixture received an unexpected Docker operation: ${JSON.stringify(args)}`);
  };
  return { calls, controller, environment, privacy, runner, state };
}

test('production backup lifecycle validates and copies binary artifacts then cleans its owned helper despite cancellation', async t => {
  const root = await temporaryRoot(t);
  const output = path.join(root, 'production-backup');
  const fixture = backupLifecycleFixture(output);
  const manifest = await backupDeployment({ project, output, writersStopped: true }, {
    environment: fixture.environment, runner: fixture.runner, signal: fixture.controller.signal,
  });
  assert.equal(manifest.project, project);
  assert.equal(manifest.postgresMajor, 17);
  assert.equal(manifest.postgresImage, images.postgres);
  assert.equal(manifest.sanctuaryImage, images.sanctuary);
  assert.equal(manifest.complete, true);
  assert.deepEqual(await verifyBackup(output), manifest);
  for (const name of artifactNames) {
    assert.deepEqual(await readFile(path.join(output, name)), backupBytes[name]);
    assert.deepEqual(manifest.artifacts[name], {
      bytes: backupBytes[name].length, sha256: createHash('sha256').update(backupBytes[name]).digest('hex'),
    });
  }
  assert.equal(fixture.state.writerChecks, 6);
  assert.equal(fixture.controller.signal.aborted, true);
  assert.equal(fixture.state.dumpRemoved, true);
  assert.equal(fixture.state.helperRemoved, true);
  const operations = fixture.calls.map(call => call.args);
  const dumpList = operations.findIndex(args => args[2] === 'pg_restore' && args[3] === '--list');
  const archiveList = operations.findIndex(args => args[2] === 'sh' && args[4].startsWith('tar -tzf '));
  const copies = operations.map((args, index) => args[0] === 'cp' ? index : -1).filter(index => index !== -1);
  assert.equal(copies.length, 2);
  assert.equal(dumpList < copies[0], true);
  assert.equal(archiveList < copies[1], true);
  assert.deepEqual(operations.slice(-3), [
    ['exec', 'a'.repeat(64), 'rm', '-f', fixture.state.dump], ['inspect', fixture.state.helperName], ['rm', '-f', fixture.state.helperName],
  ]);
  assert.deepEqual((await readdir(output)).sort(), [...artifactNames, 'manifest.json'].sort());
});

test('production backup still removes its owned archive helper when temporary dump cleanup fails', async t => {
  const root = await temporaryRoot(t);
  const output = path.join(root, 'cleanup-failure');
  const fixture = backupLifecycleFixture(output, { failDumpCleanup: true });
  await assert.rejects(() => backupDeployment({ project, output, writersStopped: true }, {
    environment: fixture.environment, runner: fixture.runner, signal: fixture.controller.signal,
  }), /Backup cleanup needs attention: temporary PostgreSQL dump removal/);
  assert.equal(fixture.controller.signal.aborted, true);
  assert.equal(fixture.state.dumpRemoved, false);
  assert.equal(fixture.state.helperRemoved, true);
  assert.equal((await verifyBackup(output)).complete, true);
  assert.deepEqual(fixture.calls.slice(-3).map(call => call.signal), [undefined, undefined, undefined]);
  assert.deepEqual(fixture.calls.slice(-2).map(call => call.args), [['inspect', fixture.state.helperName], ['rm', '-f', fixture.state.helperName]]);
});

const originalSchemaFixture = 'CREATE TABLE public.users (id integer, CONSTRAINT fixture_check CHECK (((id > 0) AND (id < 10)) AND (id <> 5)));\n';
const parsedSchemaFixture = 'CREATE TABLE public.users (id integer, CONSTRAINT fixture_check CHECK ((id > 0) AND (id < 10) AND (id <> 5)));\n';
const snapshotRows = '_sqlx_migrations:[{"version":1,"success":true}]\nusers:[{"id":1}]\n';
const snapshotSequences = 'public|users_id_seq|1|1|2147483647|1|f|1|1\npublic|fixture_id_seq|1|1|2147483647|1|f|1|2\n';
const snapshotMigrations = `1|t|${'a'.repeat(96)}`;
const fingerprint = value => createHash('sha256').update(value).digest('hex');

function snapshotFixture(base, { rawSchema = originalSchemaFixture, parsedSchema = parsedSchemaFixture, existingReference = false } = {}) {
  const state = { reference: null, referenceRestored: false, dumpedDatabase: null };
  const fixture = recordingDocker(args => {
    assert.deepEqual(args.slice(0, base.length + 3), [...base, 'exec', '-T', 'postgres']);
    const operation = args.slice(base.length + 3);
    if (operation[0] === 'psql') {
      assert.deepEqual(operation.slice(0, -1), ['psql', '-X', '-At', '--set', 'ON_ERROR_STOP=1', '-U', 'illya', '-d', 'illyaverse', '-c']);
      const query = operation.at(-1);
      if (query === "SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename;") return '_sqlx_migrations\nusers\n';
      if (query.startsWith("SELECT '_sqlx_migrations:'") && query.includes('FROM public.users t')) return snapshotRows;
      if (query.startsWith('SELECT schemaname,sequencename,')) return snapshotSequences;
      assert.equal(query, "SELECT version,success,encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version;");
      return `${snapshotMigrations}\n`;
    }
    if (operation[0] === 'pg_dump') {
      const database = operation[operation.indexOf('-d') + 1];
      if (operation.includes('-Fc')) {
        assert.deepEqual(operation, ['pg_dump', '-U', 'illya', '-d', 'illyaverse', '--schema-only', '-Fc', '--file', '/tmp/recovery-schema-reference.dump']);
      } else {
        assert.deepEqual(operation, ['pg_dump', '-U', 'illya', '-d', database, '--schema-only', '--no-owner', '--no-acl', '--file', '/tmp/recovery-schema.sql']);
        assert.equal(database === 'illyaverse' || database === state.reference, true);
        if (database === state.reference) assert.equal(state.referenceRestored, true);
        state.dumpedDatabase = database;
      }
      return '';
    }
    if (operation[0] === 'createdb') {
      state.reference = operation.at(-1);
      assert.match(state.reference, /^recovery_schema_[a-f0-9]{16}$/);
      assert.deepEqual(operation, ['createdb', '-U', 'illya', state.reference]);
      if (existingReference) throw new Error('fixture reference database already exists');
      return '';
    }
    if (operation[0] === 'pg_restore') {
      assert.notEqual(state.reference, null);
      assert.deepEqual(operation, ['pg_restore', '--exit-on-error', '--no-owner', '--no-acl', '-U', 'illya', '-d', state.reference, '/tmp/recovery-schema-reference.dump']);
      state.referenceRestored = true;
      return '';
    }
    assert.deepEqual(operation, ['cat', '/tmp/recovery-schema.sql']);
    return state.dumpedDatabase === 'illyaverse' ? rawSchema : parsedSchema;
  });
  return { ...fixture, state };
}

test('source snapshot normalizes schema through a new owned reference database and keeps original schema evidence', async () => {
  const base = ['compose', '--project-name', project, '--file', 'source.json'];
  const fixture = snapshotFixture(base);
  const source = await databaseSnapshot(fixture.docker, base, { canonicalizeSchema: true, owner });
  assert.equal(source.schema, fingerprint(parsedSchemaFixture));
  assert.equal(source.schemaDefinition, parsedSchemaFixture);
  assert.equal(source.originalSchema, fingerprint(originalSchemaFixture));
  assert.notEqual(source.schema, source.originalSchema);
  assert.equal(source.rows, fingerprint(snapshotRows));
  assert.equal(source.sequences, fingerprint(snapshotSequences));
  assert.equal(source.migrations, snapshotMigrations);
  assert.deepEqual(source.rowCounts, { _sqlx_migrations: 1, users: 1 });
  assert.equal(source.tableCount, 2);
  assert.equal(source.sequenceCount, 2);
  const operations = fixture.calls.map(call => call.args.slice(base.length + 3));
  const created = operations.findIndex(args => args[0] === 'createdb');
  const restored = operations.findIndex(args => args[0] === 'pg_restore');
  assert.equal(created > 0 && restored > created, true);
  assert.equal(operations.filter(args => args[0] === 'createdb').length, 1);
  assert.equal(operations.some(args => args.includes('--clean') || args.includes('dropdb') || args.some(value => /\bDROP\s+(?:DATABASE|TABLE|SCHEMA)\b/i.test(value))), false);

  const restoreBase = ['compose', '--project-name', project.replace('-source', '-restore'), '--file', 'restore.json'];
  const restoreFixture = snapshotFixture(restoreBase, { rawSchema: parsedSchemaFixture });
  const restoredSnapshot = await databaseSnapshot(restoreFixture.docker, restoreBase);
  assert.equal(sameDatabase(source, restoredSnapshot), true);
  assert.notEqual(source.originalSchema, restoredSnapshot.originalSchema);
  assert.equal(restoreFixture.calls.some(call => call.args.includes('createdb') || call.args.includes('pg_restore') || call.args.includes('-Fc')), false);
});

test('schema normalization refuses unowned, missing-owner and restore projects before fake Docker', async () => {
  for (const [selectedProject, selectedOwner] of [
    ['illyaverse', owner], [project, undefined], [project, 'fedcba9876543210'], [project.replace('-source', '-restore'), owner],
  ]) {
    const fixture = recordingDocker(() => assert.fail('Schema ownership guard ran after Docker'));
    const base = ['compose', '--project-name', selectedProject];
    await assert.rejects(() => databaseSnapshot(fixture.docker, base, { canonicalizeSchema: true, owner: selectedOwner }), /nonce|owned source/);
    assert.deepEqual(fixture.calls, []);
  }
});

test('an existing schema reference database fails without cleaning, dropping or reusing it', async () => {
  const base = ['compose', '--project-name', project];
  const fixture = snapshotFixture(base, { existingReference: true });
  await assert.rejects(() => databaseSnapshot(fixture.docker, base, { canonicalizeSchema: true, owner }), /already exists/);
  assert.equal(fixture.calls.at(-1).args.includes('createdb'), true);
  assert.equal(fixture.calls.some(call => call.args.includes('pg_restore') || call.args.includes('--clean') || call.args.includes('dropdb')), false);
});

test('database equality gates schema, all rows, sequences, migration checksums and counts while preserving independent original hashes', () => {
  assert.equal(sameDatabase({}, {}), false);
  const source = {
    schema: fingerprint(parsedSchemaFixture), originalSchema: fingerprint(originalSchemaFixture), schemaDefinition: parsedSchemaFixture,
    rows: fingerprint(snapshotRows), sequences: fingerprint(snapshotSequences), migrations: snapshotMigrations,
    rowCounts: { _sqlx_migrations: 1, users: 1 }, tableCount: 2, sequenceCount: 2,
  };
  const restored = { ...structuredClone(source), originalSchema: fingerprint(parsedSchemaFixture) };
  assert.equal(sameDatabase(source, restored), true);
  assert.equal(sameDatabase(source, {}), false);
  for (const [key, changed] of [
    ['schema', fingerprint(parsedSchemaFixture.replace('id <> 5', 'id <> 6'))],
    ['rows', fingerprint(snapshotRows.replace('"id":1', '"id":2'))],
    ['sequences', fingerprint(snapshotSequences.replace('|f|1|2', '|f|1|3'))],
    ['migrations', snapshotMigrations.replace('a'.repeat(96), 'b'.repeat(96))],
    ['rowCounts', { _sqlx_migrations: 1, users: 2 }], ['tableCount', 3], ['sequenceCount', 3],
  ]) assert.equal(sameDatabase(source, { ...structuredClone(restored), [key]: changed }), false, key);
});

const migrationFilename = version => `${String(version).padStart(4, '0')}_fixture.sql`;
const migrationBytes = version => Buffer.from(`-- exact bytes café migration ${version}\r\nSELECT ${version};\r\n`, 'utf8');
const migrationChecksum = bytes => createHash('sha384').update(bytes).digest('hex');
const migrationLedger = records => records.map(record => `${record.version}|t|${record.checksum}`).join('\n');

async function migrationFixture(directory, count) {
  await mkdir(directory);
  // Create out of order to ensure discovery chooses numeric migration order.
  for (let version = count; version >= 1; version--) await writeFile(path.join(directory, migrationFilename(version)), migrationBytes(version));
  return Array.from({ length: count }, (_, index) => ({
    filename: migrationFilename(index + 1), version: index + 1, checksum: migrationChecksum(migrationBytes(index + 1)),
  }));
}

test('migration discovery hashes exact bytes in numeric order and an appended 23rd migration preserves the first 22 records', async t => {
  const root = await temporaryRoot(t);
  const directory = path.join(root, 'migrations');
  const expected22 = await migrationFixture(directory, 22);
  await writeFile(path.join(directory, 'README.txt'), 'Non-SQL files do not enter the migration ledger.');
  const captured22 = await readSourceMigrations(directory);
  assert.deepEqual(captured22, expected22);
  assert.equal(await checkMigrations({ migrations: migrationLedger(captured22) }, captured22), 22);
  assert.notEqual(captured22[0].checksum, migrationChecksum(migrationBytes(1).toString('utf8').replaceAll('\r\n', '\n')));
  await writeFile(path.join(directory, migrationFilename(23)), migrationBytes(23));
  const captured23 = await readSourceMigrations(directory);
  assert.equal(captured23.length, 23);
  assert.deepEqual(captured23.slice(0, 22), captured22);
  assert.deepEqual(captured23.at(-1), { filename: migrationFilename(23), version: 23, checksum: migrationChecksum(migrationBytes(23)) });
  // The old captured expectation remains usable without rediscovering the source.
  assert.equal(await checkMigrations({ migrations: migrationLedger(captured22) }, captured22), 22);
  assert.equal(await checkMigrations({ migrations: migrationLedger(captured23) }, captured23), 23);
  await assert.rejects(async () => checkMigrations({ migrations: migrationLedger(captured22) }, captured23), /migration|checksum/i);
});

test('migration validation refuses failed, extra, missing, duplicate, reordered and changed ledger entries', async t => {
  const root = await temporaryRoot(t);
  const directory = path.join(root, 'migrations');
  const expected = await migrationFixture(directory, 23);
  const records = await readSourceMigrations(directory);
  assert.deepEqual(records, expected);
  const lines = migrationLedger(records).split('\n');
  for (const [name, change] of [
    ['failed migration', value => { value[4] = value[4].replace('|t|', '|f|'); }],
    ['extra migration', value => { value.push(`24|t|${migrationChecksum(migrationBytes(24))}`); }],
    ['missing migration', value => { value.splice(4, 1); }],
    ['duplicate migration', value => { value.push(value[4]); }],
    ['changed version', value => { value[4] = value[4].replace(/^5\|/, '25|'); }],
    ['changed checksum', value => { value[4] = `5|t|${'0'.repeat(96)}`; }],
    ['reordered migration', value => { value.reverse(); }],
  ]) {
    const changed = [...lines];
    change(changed);
    await assert.rejects(async () => checkMigrations({ migrations: changed.join('\n') }, records), /migration|checksum/i, name);
  }
  await assert.rejects(async () => checkMigrations({ migrations: '' }, records), /migration|checksum/i);
  await assert.rejects(async () => checkMigrations({ migrations: `${lines.join('\n')}\n` }, records), /migration|checksum/i);
  await writeFile(path.join(directory, migrationFilename(5)), Buffer.concat([migrationBytes(5), Buffer.from('-- changed exact bytes\n')]));
  const changedSource = await readSourceMigrations(directory);
  assert.notEqual(changedSource[4].checksum, records[4].checksum);
  await assert.rejects(async () => checkMigrations({ migrations: migrationLedger(records) }, changedSource), /migration|checksum/i);
});

test('migration discovery refuses an empty SQL set, unsupported SQL filenames and duplicate numeric versions', async t => {
  const root = await temporaryRoot(t);
  const empty = path.join(root, 'empty');
  await mkdir(empty);
  await writeFile(path.join(empty, 'README.txt'), 'No SQL files.');
  await assert.rejects(() => readSourceMigrations(empty), /migration|SQL|empty/i);
  for (const [index, filename] of ['unexpected.sql', '0001-bad.sql'].entries()) {
    const directory = path.join(root, `unsupported-${index}`);
    await mkdir(directory);
    await writeFile(path.join(directory, filename), 'SELECT 1;\n');
    await assert.rejects(() => readSourceMigrations(directory), /migration|SQL|filename/i);
  }
  const duplicate = path.join(root, 'duplicate');
  await mkdir(duplicate);
  await writeFile(path.join(duplicate, '0001_first.sql'), 'SELECT 1;\n');
  await writeFile(path.join(duplicate, '0001_second.sql'), 'SELECT 2;\n');
  await assert.rejects(() => readSourceMigrations(duplicate), /duplicate|migration|version/i);
});

test('migration discovery refuses nonregular and linked SQL entries', async t => {
  const root = await temporaryRoot(t);
  const nonregular = path.join(root, 'nonregular');
  await mkdir(nonregular);
  await mkdir(path.join(nonregular, '0001_directory.sql'));
  await assert.rejects(() => readSourceMigrations(nonregular), /migration|regular|SQL/i);
  const linked = path.join(root, 'linked');
  const target = path.join(root, 'link-target');
  await mkdir(linked);
  await mkdir(target);
  // Windows junctions exercise linked-entry refusal without elevated symlink rights.
  await symlink(target, path.join(linked, '0001_linked.sql'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(() => readSourceMigrations(linked), /migration|regular|link|SQL/i);
});

test('repository migration expectations derive their numeric versions and checksums from the current source files', async () => {
  const records = await readSourceMigrations();
  assert.equal(records.length > 0, true);
  assert.deepEqual(records.map(record => record.version), records.map(record => record.version).sort((left, right) => left - right));
  assert.equal(new Set(records.map(record => record.version)).size, records.length);
  for (const record of records) {
    assert.equal(record.version, Number(record.filename.split('_')[0]));
    assert.equal(record.checksum, migrationChecksum(await readFile(path.join(workspace, 'migrations', record.filename))));
  }
  assert.equal(await checkMigrations({ migrations: migrationLedger(records) }, records), records.length);
});

test('project, nonce and image identities use strict full-string allowlists', () => {
  for (const value of ['a', 'offline_source-1', `a${'1'.repeat(62)}`]) assert.equal(projectName(value), value);
  for (const value of [undefined, null, 1, '', '1project', 'UPPER', '-project', 'a.b', 'a/b', 'a\\b', 'a b', 'safe\n', `a${'1'.repeat(63)}`])
    assert.throws(() => projectName(value), /Project name/, String(value));
  for (const role of ['source', 'restore']) assert.equal(ownedProject(`illya-recovery-${owner}-${role}`, owner), `illya-recovery-${owner}-${role}`);
  for (const [value, selectedOwner] of [
    ['illyaverse', owner], [project.replace(owner, 'fedcba9876543210'), owner], [`${project}\n`, owner],
    [project.replace('-source', '-other'), owner], [project, owner.toUpperCase()], [project, '123'], [project, undefined],
  ]) assert.throws(() => ownedProject(value, selectedOwner), /nonce/);
  assert.equal(validImage(images.postgres), true);
  for (const value of ['postgres:17-alpine', images.postgres.toUpperCase(), `${images.postgres}\n`, 'sha256:123', undefined]) assert.equal(validImage(value), false);
  const generated = nonce();
  assert.equal(generated.length, 16);
  assert.match(generated, /^[a-f0-9]{16}$/);
});

test('resource ownership requires the exact nonce and project labels plus the expected resource identity', () => {
  for (const kind of ['container', 'volume', 'network']) {
    assert.doesNotThrow(() => ownedResource(resource(kind), { project, owner, kind, name: 'shared_data' }));
    for (const label of [ownerLabel, 'com.docker.compose.project']) {
      const value = resource(kind);
      const selectedLabels = kind === 'container' ? value.Config.Labels : value.Labels;
      delete selectedLabels[label];
      assert.throws(() => ownedResource(value, { project, owner, kind, name: 'shared_data' }), /ownership/);
      selectedLabels[label] = 'other-owner-or-project';
      assert.throws(() => ownedResource(value, { project, owner, kind, name: 'shared_data' }), /ownership/);
    }
  }
  assert.throws(() => ownedResource(resource('volume', 'extra'), { project, owner, kind: 'volume', name: 'extra' }), /volume/);
  for (const value of [
    { ...resource('volume'), Name: 'illyaverse_shared_data' },
    { ...resource('network'), Name: 'illyaverse_default' },
    { ...resource('container'), Name: '/illyaverse-sanctuary-1' },
    { ...resource('container'), Name: `${resource('container').Name}\n` },
    resource('container', 'resonance'),
  ]) assert.throws(() => ownedResource(value, { project, owner, kind: value.Config ? 'container' : value.Labels['com.docker.compose.volume'] ? 'volume' : 'network', name: 'shared_data' }), /unexpected|ownership/);
});

test('backup CLI accepts only a single explicit project, new output and stopped-writers acknowledgement', () => {
  assert.deepEqual(backupArguments(['--project', 'offline-source', '--output', 'new-backup', '--writers-stopped']), {
    project: 'offline-source', output: 'new-backup', writersStopped: true,
  });
  assert.deepEqual(backupArguments(['--writers-stopped', '--output', 'new-backup', '--project', 'offline-source']), {
    project: 'offline-source', output: 'new-backup', writersStopped: true,
  });
  for (const args of [
    [], ['--project', 'offline-source'], ['--project', 'offline-source', '--output', 'new-backup'],
    ['--project', '--writers-stopped'], ['--output'], ['--project', 'unsafe/name', '--output', 'new-backup', '--writers-stopped'],
    ['--project', 'offline-source', '--output', 'new-backup', '--writers-stopped', '--writers-stopped'],
    ['--project', 'offline-source', '--project', 'other', '--output', 'new-backup', '--writers-stopped'],
    ['--project', 'offline-source', '--output', 'new-backup', '--output', 'other', '--writers-stopped'],
    ['--project', 'offline-source', '--output', 'new-backup', '--writers-stopped', '--file', 'compose.yaml'],
    ['--project', 'offline-source', '--output', 'new-backup', '--writers-stopped', '--context', 'production'],
    ['--project=offline-source', '--output', 'new-backup', '--writers-stopped'], ['--help'], ['--dry-run'],
  ]) assert.throws(() => backupArguments(args), /requires|Use only|Project name/);
});

test('backup source accepts running PostgreSQL and explicitly stopped Sanctuary on the selected named volumes', () => {
  assert.doesNotThrow(() => validateBackupSource(backupSource()));
});

for (const [name, change] of [
  ['missing source project label', value => { delete value.postgres.Config.Labels['com.docker.compose.project']; }],
  ['different source service label', value => { value.sanctuary.Config.Labels['com.docker.compose.service'] = 'resonance'; }],
  ['malformed container identity', value => { value.postgres.Id = 'short-id'; }],
  ['stopped PostgreSQL', value => { value.postgres.State.Running = false; }],
  ['running Sanctuary writer', value => { value.sanctuary.State.Running = true; }],
  ['missing Sanctuary stopped state', value => { delete value.sanctuary.State; }],
  ['tagged Sanctuary image', value => { value.sanctuary.Image = 'illyaverse/sanctuary:local'; }],
  ['foreign shared volume name', value => { value.shared.Name = 'illyaverse_shared_data'; }],
  ['unlabelled shared volume', value => { delete value.shared.Labels['com.docker.compose.project']; }],
  ['different shared volume label', value => { value.shared.Labels['com.docker.compose.volume'] = 'postgres_data'; }],
  ['Sanctuary bind mount', value => { value.sanctuary.Mounts[0].Type = 'bind'; }],
  ['Sanctuary foreign data volume', value => { value.sanctuary.Mounts[0].Name = 'illyaverse_shared_data'; }],
  ['missing Sanctuary data mount', value => { value.sanctuary.Mounts = []; }],
  ['duplicate Sanctuary data mounts', value => { value.sanctuary.Mounts.push({ ...value.sanctuary.Mounts[0] }); }],
  ['PostgreSQL bind mount', value => { value.postgres.Mounts[0].Type = 'bind'; }],
  ['PostgreSQL foreign data volume', value => { value.postgres.Mounts[0].Name = 'illyaverse_postgres_data'; }],
  ['missing PostgreSQL data mount', value => { value.postgres.Mounts = []; }],
]) test(`backup refuses ${name}`, () => {
  const value = backupSource();
  change(value);
  assert.throws(() => validateBackupSource(value), /Source|PostgreSQL|Sanctuary/);
});

test('backup refuses missing writers acknowledgement before any external command', async () => {
  const { calls, runner } = privateRunner();
  await assert.rejects(() => backupDeployment({ project: 'offline-source', output: 'unused', writersStopped: false }, { runner }), /writers must be stopped/);
  assert.deepEqual(calls, []);
});

test('a new backup directory is private and an existing directory or file is always refused', async t => {
  const root = await temporaryRoot(t);
  const { calls, runner } = privateRunner();
  const directory = await privateDirectory(path.join(root, 'backup'), runner);
  assert.equal(directory, path.join(root, 'backup'));
  assert.deepEqual(await readdir(directory), []);
  if (process.platform === 'win32') assert.deepEqual(calls, [
    { command: 'whoami.exe', args: ['/user', '/fo', 'csv', '/nh'] },
    { command: 'icacls.exe', args: [directory, '/inheritance:r', '/grant:r', `*${windowsSid}:(OI)(CI)F`] },
  ]);
  else assert.deepEqual(calls, []);
  const callCount = calls.length;
  await assert.rejects(() => privateDirectory(directory, runner), { code: 'EEXIST' });
  const existingFile = path.join(root, 'existing-file');
  await writeFile(existingFile, 'previous backup');
  await assert.rejects(() => privateDirectory(existingFile, runner), { code: 'EEXIST' });
  assert.equal(await readFile(existingFile, 'utf8'), 'previous backup');
  assert.equal(calls.length, callCount);
});

test('Windows backup privacy fails closed when user identity or ACL restriction fails', { skip: process.platform !== 'win32' }, async t => {
  const root = await temporaryRoot(t);
  for (const [name, runner] of [
    ['identity', async () => ({ code: 0, stdout: 'missing user SID', stderr: '' })],
    ['acl', async command => command === 'whoami.exe'
      ? { code: 0, stdout: `"fixture\\owner","${windowsSid}"`, stderr: '' }
      : { code: 1, stdout: '', stderr: 'denied' }],
  ]) await assert.rejects(() => privateDirectory(path.join(root, name), runner), /Windows user|restrict/);
});

test('temporary directory ownership refuses malformed nonces before creating files', async () => {
  const { calls, runner } = privateRunner();
  for (const value of ['', undefined, '123', owner.toUpperCase(), `${owner}\n`, '../0123456789abcdef'])
    await assert.rejects(() => ownedTempDirectory(value, runner), /owner/);
  assert.deepEqual(calls, []);
});

test('complete backup validates both binary artifacts before publishing a complete checksum manifest', async t => {
  const root = await temporaryRoot(t);
  const directory = await privateDirectory(path.join(root, 'backup'), privateRunner().runner);
  const validationOrder = [];
  const manifest = await writeCompleteBackup(directory, {
    validate: async (name, file) => {
      validationOrder.push(name);
      assert.equal(path.basename(file), `${name}.partial`);
      assert.deepEqual(await readFile(file), backupBytes[name]);
      assert.equal((await readdir(directory)).includes('manifest.json'), false);
    },
  });
  assert.deepEqual(validationOrder, artifactNames);
  assert.equal(manifest.complete, true);
  assert.equal(manifest.format, 'illyaverse-recovery-v1');
  assert.equal(manifest.postgresMajor, 17);
  assert.deepEqual((await readdir(directory)).sort(), [...artifactNames, 'manifest.json'].sort());
  assert.deepEqual(await verifyBackup(directory), manifest);
});

test('a failed backup leaves partials, never marks completion and preserves a previous successful backup', async t => {
  const root = await temporaryRoot(t);
  const runner = privateRunner().runner;
  const previous = await privateDirectory(path.join(root, 'previous'), runner);
  const priorManifest = await writeCompleteBackup(previous);
  const priorManifestBytes = await readFile(path.join(previous, 'manifest.json'));
  await assert.rejects(() => writeCompleteBackup(previous), /existing backup output/);
  for (const failure of ['produce', 'validate', 'beforeComplete']) {
    const directory = await privateDirectory(path.join(root, failure), runner);
    const options = failure === 'produce' ? {
      produce: async (name, destination) => {
        await writeFile(destination, backupBytes[name]);
        if (name === 'shared-data.tar.gz') throw new Error('fixture copy failed');
      },
    } : failure === 'validate' ? {
      validate: async name => { if (name === 'shared-data.tar.gz') throw new Error('fixture archive unreadable'); },
    } : { beforeComplete: async () => { throw new Error('fixture writers restarted'); } };
    await assert.rejects(() => writeCompleteBackup(directory, options), /fixture/);
    assert.deepEqual((await readdir(directory)).sort(), artifactNames.map(name => `${name}.partial`).sort());
    await assert.rejects(() => verifyBackup(directory), { code: 'ENOENT' });
  }
  assert.deepEqual(await readFile(path.join(previous, 'manifest.json')), priorManifestBytes);
  assert.deepEqual(await verifyBackup(previous), priorManifest);
  for (const name of artifactNames) assert.deepEqual(await readFile(path.join(previous, name)), backupBytes[name]);
});

test('verification refuses changed bytes even when the artifact size is unchanged', async t => {
  const root = await temporaryRoot(t);
  const directory = await privateDirectory(path.join(root, 'backup'), privateRunner().runner);
  await writeCompleteBackup(directory);
  const changed = Buffer.from(backupBytes['illyaverse.dump']);
  changed[changed.length - 1] ^= 1;
  await writeFile(path.join(directory, 'illyaverse.dump'), changed);
  await assert.rejects(() => verifyBackup(directory), /checksum\/size mismatch: illyaverse.dump/);
});

for (const failure of ['missing artifact', 'empty artifact', 'directory artifact', 'missing manifest', 'invalid manifest', 'incomplete manifest', 'wrong artifact allowlist']) {
  test(`verification refuses ${failure}`, async t => {
    const root = await temporaryRoot(t);
    const directory = await privateDirectory(path.join(root, 'backup'), privateRunner().runner);
    const manifest = await writeCompleteBackup(directory);
    const artifact = path.join(directory, 'shared-data.tar.gz');
    const manifestFile = path.join(directory, 'manifest.json');
    if (failure === 'missing artifact') await rm(artifact);
    if (failure === 'empty artifact') await writeFile(artifact, '');
    if (failure === 'directory artifact') { await rm(artifact); await mkdir(artifact); }
    if (failure === 'missing manifest') await rm(manifestFile);
    if (failure === 'invalid manifest') await writeFile(manifestFile, '{ unreadable json');
    if (failure === 'incomplete manifest') await writeFile(manifestFile, JSON.stringify({ ...manifest, complete: false }));
    if (failure === 'wrong artifact allowlist') await writeFile(manifestFile, JSON.stringify({ ...manifest, artifacts: { ...manifest.artifacts, '.env': {} } }));
    await assert.rejects(() => verifyBackup(directory));
  });
}

test('Docker runner keeps context selection but removes inherited Compose selectors and application secrets', async () => {
  const environment = {
    PATH: 'fixture-path', DOCKER_HOST: 'fixture-host', DOCKER_CONTEXT: 'fixture-context', DOCKER_TLS_VERIFY: '1',
    COMPOSE_PROJECT_NAME: 'production', COMPOSE_FILE: 'production.yaml', COMPOSE_PROFILES: 'production', COMPOSE_ENV_FILES: 'secret.env',
    POSTGRES_PASSWORD: 'production-password', POSTGRES_USER: 'production-user', DATABASE_URL: 'production-db',
    SESSION_SECRET: 'production-session', INTERNAL_SERVICE_KEY: 'production-key', BOOTSTRAP_ADMIN_PASSWORD: 'admin-secret',
    MAIL_ENCRYPTION_KEY: 'mail-secret', SMTP_PASSWORD: 'smtp-secret', ILLYA_DOMAIN: 'production-domain', ILLYA_ALLOW_DISPOSABLE_TEST: '1',
    compose_file: 'case-insensitive-production.yaml', Database_Url: 'case-insensitive-secret',
  };
  const expected = { PATH: 'fixture-path', DOCKER_HOST: 'fixture-host', DOCKER_CONTEXT: 'fixture-context', DOCKER_TLS_VERIFY: '1' };
  assert.deepEqual(dockerEnvironment(environment), expected);
  assert.equal(environment.COMPOSE_PROJECT_NAME, 'production');
  const signal = new AbortController().signal;
  const calls = [];
  const docker = checkedDocker({ environment, signal, runner: async (command, args, options) => {
    calls.push({ command, args, options });
    return { code: 0, stdout: 'fixture output', stderr: '' };
  } });
  assert.equal(await docker(['image', 'inspect', 'fixture']), 'fixture output');
  assert.equal(calls[0].command, 'docker');
  assert.deepEqual(calls[0].options.env, expected);
  assert.equal(calls[0].options.signal, signal);
  await docker(['ps'], null);
  assert.equal(calls[1].options.signal, undefined);
  const failedDocker = checkedDocker({ runner: async () => ({ code: 19, stdout: '', stderr: 'secret fixture stderr' }) });
  await assert.rejects(() => failedDocker(['inspect', 'fixture']), /operation inspect failed \(exit 19\)/);
});

test('temporary cleanup refuses foreign paths and filenames, and never recursively removes unexpected files', async () => {
  const own = nonce();
  const temporary = await ownedTempDirectory(own, privateRunner().runner);
  const safe = path.join(temporary.directory, 'safe.txt');
  const unexpected = path.join(temporary.directory, 'unexpected.txt');
  try {
    await writeFile(safe, 'owned fixture');
    await writeFile(unexpected, 'must remain');
    await assert.rejects(() => cleanupTemp({ ...temporary, owner: 'f'.repeat(16), files: ['safe.txt'] }), /outside/);
    await assert.rejects(() => cleanupTemp({ ...temporary, owner: own, files: ['../safe.txt'] }), /filename/);
    assert.equal(await readFile(safe, 'utf8'), 'owned fixture');
    await assert.rejects(() => cleanupTemp({ ...temporary, owner: own, files: ['safe.txt'] }));
    assert.equal(await readFile(unexpected, 'utf8'), 'must remain');
  } finally {
    await cleanupTemp({ ...temporary, owner: own, files: ['safe.txt', 'unexpected.txt'] });
  }
});

test('the generated source and restore configurations pass the rendered configuration guard', () => {
  for (const restore of [false, true]) {
    const expected = config(restore);
    const rendered = structuredClone(expected);
    rendered.services.sanctuary.environment = Object.fromEntries(Object.entries(rendered.services.sanctuary.environment).sort());
    assert.doesNotThrow(() => validateRecoveryConfig(rendered, expected));
    assert.equal(expected.services.archive.volumes[0].read_only, !restore);
  }
  assert.throws(() => recoveryConfig({ project, owner, images: { ...images, postgres: 'postgres:17-alpine' }, ...secrets }), /pinned/);
  assert.throws(() => recoveryConfig({ project, owner, images, ...secrets, password: 'production-password' }), /generated hex/);
});

test('rendered recovery configuration accepts empty IPAM and null image-default commands emitted by Compose', () => {
  for (const restore of [false, true]) {
    const expected = config(restore);
    const rendered = structuredClone(expected);
    rendered.networks.default.ipam = {};
    for (const service of ['postgres', 'sanctuary']) {
      rendered.services[service].command = null;
      rendered.services[service].entrypoint = null;
    }
    assert.doesNotThrow(() => validateRecoveryConfig(rendered, expected));
  }
});

for (const [name, change] of [
  ['external data volume', value => { value.volumes.postgres_data.external = true; }],
  ['existing production volume', value => { value.volumes.shared_data.name = 'illyaverse_shared_data'; }],
  ['volume driver', value => { value.volumes.shared_data.driver = 'local'; }],
  ['missing nonce volume label', value => { delete value.volumes.shared_data.labels[ownerLabel]; }],
  ['bind mount', value => { value.services.sanctuary.volumes[0].type = 'bind'; }],
  ['extra mount', value => { value.services.sanctuary.volumes.push({ type: 'bind', source: workspace, target: '/workspace' }); }],
  ['volume subpath', value => { value.services.archive.volumes[0].volume = { subpath: 'media' }; }],
  ['writable source archive', value => { value.services.archive.volumes[0].read_only = false; }],
  ['published port', value => { value.services.postgres.ports = [{ published: '15432', target: 5432 }]; }],
  ['build instructions', value => { value.services.sanctuary.build = { context: workspace }; }],
  ['unpinned image', value => { value.services.sanctuary.image = 'illyaverse/sanctuary:local'; }],
  ['different pinned image', value => { value.services.sanctuary.image = images.postgres; }],
  ['pull policy', value => { value.services.postgres.pull_policy = 'always'; }],
  ['host networking', value => { value.services.archive.network_mode = 'host'; }],
  ['additional service network', value => { value.services.sanctuary.networks.production = null; }],
  ['external network', value => { value.networks.default.external = true; }],
  ['custom network subnet', value => { value.networks.default.ipam = { config: [{ subnet: '172.30.0.0/16' }] }; }],
  ['noninternal network', value => { value.networks.default.internal = false; }],
  ['production network name', value => { value.networks.default.name = 'illyaverse_default'; }],
  ['database hostname override', value => { value.services.sanctuary.extra_hosts = { postgres: 'host.docker.internal' }; }],
  ['archive command override', value => { value.services.archive.command = ['rm', '-rf', '/data']; }],
  ['Sanctuary command override', value => { value.services.sanctuary.command = ['fixture']; }],
  ['health command override', value => { value.services.postgres.healthcheck.test = ['CMD', 'fixture']; }],
  ['entrypoint override', value => { value.services.archive.entrypoint = ['sh']; }],
  ['production database environment', value => { value.services.sanctuary.environment.DATABASE_URL = 'postgres://production/illyaverse'; }],
  ['inherited application environment', value => { value.services.sanctuary.environment.SMTP_PASSWORD = 'production-secret'; }],
  ['additional service', value => { value.services.production = { image: images.postgres }; }],
]) test(`rendered recovery configuration refuses ${name}`, () => {
  const expected = config();
  const rendered = structuredClone(expected);
  change(rendered);
  assert.throws(() => validateRecoveryConfig(rendered, expected), /recovery|Recovery|Unexpected/);
});

test('fresh project discovery uses project labels and exact names to reject unlabelled existing resources', async () => {
  const { calls, docker } = recordingDocker();
  await requireFreshProject(docker, project, owner);
  assert.equal(calls.length, 5);
  for (const call of calls.slice(0, 3)) assert.equal(call.args.at(-1), `label=com.docker.compose.project=${project}`);
  assert.deepEqual(calls.slice(3).map(call => call.args), [
    ['volume', 'ls', '--format', '{{.Name}}'], ['network', 'ls', '--format', '{{.Name}}'],
  ]);
  for (const [selectedCall, existing] of [[1, 'existing-resource'], [2, 'existing-resource'], [3, 'existing-resource'], [4, `${project}_postgres_data`], [4, `${project}_shared_data`], [5, `${project}_default`]]) {
    const fixture = recordingDocker((args, call) => call === selectedCall ? `${existing}\r\n` : '');
    await assert.rejects(() => requireFreshProject(fixture.docker, project, owner), /existing recovery/);
    assert.equal(fixture.calls.length, selectedCall);
  }
  const similarNames = recordingDocker(args => args.includes('{{.Name}}') ? `${project}_shared_data_other\nother_${project}_default\n` : '');
  await assert.doesNotReject(() => requireFreshProject(similarNames.docker, project, owner));
  const invalid = recordingDocker(() => assert.fail('Project guard must run before Docker'));
  await assert.rejects(() => requireFreshProject(invalid.docker, 'illyaverse', owner), /nonce/);
  assert.deepEqual(invalid.calls, []);
});

function cleanupFixture(change, hiddenKind) {
  const resources = {
    container: resource('container'), volume: resource('volume'), network: resource('network'),
  };
  if (change) change(resources);
  let removed = false;
  return recordingDocker(args => {
    if (args[0] === 'compose') { assert.equal(args.includes('down'), true); removed = true; return ''; }
    if (removed) return '';
    if (args[0] === 'inspect') return JSON.stringify([resources.container]);
    if (args[1] === 'inspect') return JSON.stringify([resources[args[0]]]);
    if (args[0] === hiddenKind && args.includes('--filter')) return '';
    if (args[0] === 'ps') return `${'a'.repeat(64)}\n`;
    if (args[0] === 'volume') return `${resources.volume.Name}\n`;
    if (args[0] === 'network') return `${resources.network.Name}\n`;
    assert.fail(`Unexpected fake Docker arguments: ${JSON.stringify(args)}`);
  });
}

test('cleanup audits every owned resource before down and verifies the project is gone', async () => {
  const base = ['compose', '--project-name', project, '--file', 'fixture.json'];
  const fixture = cleanupFixture();
  await cleanupProject(fixture.docker, base, project, owner);
  const downIndex = fixture.calls.findIndex(call => call.args.includes('down'));
  assert.equal(downIndex, 10);
  assert.deepEqual(fixture.calls[downIndex].args, [...base, 'down', '--volumes', '--timeout', '10']);
  assert.equal(fixture.calls[downIndex].signal, null);
  assert.equal(fixture.calls.length, downIndex + 1 + 5);
});

for (const kind of ['container', 'volume', 'network']) test(`cleanup refuses a ${kind} without ownership before down`, async () => {
  const fixture = cleanupFixture(resources => {
    delete (kind === 'container' ? resources[kind].Config.Labels : resources[kind].Labels)[ownerLabel];
  });
  await assert.rejects(() => cleanupProject(fixture.docker, ['compose', '--project-name', project], project, owner), /ownership/);
  assert.equal(fixture.calls.some(call => call.args.includes('down')), false);
});

for (const kind of ['volume', 'network']) test(`cleanup finds an unlabelled ${kind} by its exact configured name before down`, async () => {
  const fixture = cleanupFixture(resources => { delete resources[kind].Labels['com.docker.compose.project']; }, kind);
  await assert.rejects(() => cleanupProject(fixture.docker, ['compose', '--project-name', project], project, owner), /ownership/);
  assert.equal(fixture.calls.some(call => call.args.includes('down')), false);
  assert.equal(fixture.calls.some(call => call.args[0] === kind && call.args.includes('{{.Name}}')), true);
});

test('cleanup refuses an unexpected owned volume before down', async () => {
  const fixture = cleanupFixture(resources => { resources.volume = resource('volume', 'extra'); });
  await assert.rejects(() => cleanupProject(fixture.docker, ['compose', '--project-name', project], project, owner), /volume/);
  assert.equal(fixture.calls.some(call => call.args.includes('down')), false);
});

test('drill arguments allow only no arguments or dry run', () => {
  assert.deepEqual(drillArguments([]), { dryRun: false });
  assert.deepEqual(drillArguments(['--dry-run']), { dryRun: true });
  for (const args of [['--project', 'illyaverse'], ['--dry-run', '--dry-run'], ['--file', 'compose.yaml'], ['--build'], ['--help']])
    assert.throws(() => drillArguments(args), /accepts only/);
});

const invalidSanctuaryImages = [
  null, '', 1, {}, 'illyaverse/sanctuary:local', 'illyaverse/sanctuary:approved', '--pull=always', '--context production',
  ` ${images.sanctuary}`, `${images.sanctuary}\n`, `${images.sanctuary} --pull always`, `sha256:${'A'.repeat(64)}`,
  `sha256:${'2'.repeat(63)}`, `sha256:${'2'.repeat(65)}`, images.sanctuary.toUpperCase(),
];

test('Sanctuary image selection defaults to the local tag and permits only explicit immutable lower-case SHA IDs', () => {
  assert.equal(sanctuaryImageReference(), 'illyaverse/sanctuary:local');
  assert.equal(sanctuaryImageReference(images.sanctuary), images.sanctuary);
  for (const value of invalidSanctuaryImages) assert.throws(() => sanctuaryImageReference(value), /immutable sha256 image ID/);
});

test('live drill refuses invalid explicit Sanctuary images before invoking any external runner', async () => {
  const calls = [];
  for (const sanctuaryImage of invalidSanctuaryImages) {
    await assert.rejects(() => recoveryDrill({
      environment: { ILLYA_ALLOW_DISPOSABLE_TEST: '1' }, sanctuaryImage,
      runner: async (...args) => { calls.push(args); assert.fail('Invalid image override invoked an external command'); },
    }), /immutable sha256 image ID/);
  }
  assert.deepEqual(calls, []);
});

test('live drill inspects the explicit immutable image ID and rejects a different result before project discovery or file creation', async () => {
  const calls = [];
  const runner = async (command, args) => {
    calls.push({ command, args: [...args] });
    assert.equal(command, 'docker');
    assert.equal(calls.length <= 2, true, 'Mismatched image inspection reached project resources or privacy commands');
    assert.deepEqual(args, ['image', 'inspect', calls.length === 1 ? 'postgres:17-alpine' : images.sanctuary, '--format', '{{.Id}}']);
    return { code: 0, stdout: `${images.postgres}\n`, stderr: '' };
  };
  await assert.rejects(() => recoveryDrill({
    environment: { ILLYA_ALLOW_DISPOSABLE_TEST: '1' }, sanctuaryImage: images.sanctuary, runner,
  }), /differs from the explicitly approved immutable image ID/);
  assert.deepEqual(calls, [
    { command: 'docker', args: ['image', 'inspect', 'postgres:17-alpine', '--format', '{{.Id}}'] },
    { command: 'docker', args: ['image', 'inspect', images.sanctuary, '--format', '{{.Id}}'] },
  ]);
});

function guardlessEnvironment() {
  const environment = { ...process.env, NODE_OPTIONS: '' };
  for (const name of Object.keys(environment)) if (name.toUpperCase() === 'ILLYA_ALLOW_DISPOSABLE_TEST' || name.toUpperCase() === 'PATH') delete environment[name];
  // An absent executable path means these CLI cases cannot accidentally invoke Docker.
  environment.PATH = '';
  return environment;
}

test('dry run works without the disposable guard and never calls an external runner', async () => {
  const environment = guardlessEnvironment();
  await recoveryDrill({ dryRun: true, environment, runner: async () => assert.fail('Dry run invoked an external command') });
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/disposable-recovery-drill.mjs'), '--dry-run'], {
    env: environment, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /dry run: no Docker calls or files created/);
  assert.equal(result.stderr, '');
});

test('live drill refuses the missing disposable guard before Docker in both API and CLI', async () => {
  const environment = guardlessEnvironment();
  const calls = [];
  await assert.rejects(() => recoveryDrill({ environment, runner: async (...args) => { calls.push(args); assert.fail('Guardless drill invoked Docker'); } }), /Set ILLYA_ALLOW_DISPOSABLE_TEST=1/);
  assert.deepEqual(calls, []);
  const result = spawnSync(process.execPath, [path.join(workspace, 'infra/disposable-recovery-drill.mjs')], {
    env: environment, encoding: 'utf8', windowsHide: true,
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Set ILLYA_ALLOW_DISPOSABLE_TEST=1/);
  assert.doesNotMatch(result.stderr, /Docker operation|subprocess|environment file/);
  assert.equal(result.stdout, '');
});
