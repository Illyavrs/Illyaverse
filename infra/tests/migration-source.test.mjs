import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createMigrationSourceFixture } from './migration-source-fixtures.mjs';

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const migrations = readdirSync(path.join(workspace, 'migrations'))
  .filter((name) => name.endsWith('.sql')).sort()
  .map((name) => ({ name: `migrations/${name}`, bytes: readFileSync(path.join(workspace, 'migrations', name)) }));
const sha384 = (bytes) => createHash('sha384').update(bytes).digest('hex');

function runGit(cwd, args, env = process.env, config = []) {
  const result = spawnSync('git', [...config, ...args], { cwd, env, windowsHide: true, maxBuffer: 16 * 1024 * 1024 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `git ${args.join(' ')}: ${result.stderr.toString('utf8')}`);
  return result.stdout;
}

function attributes(output) {
  const fields = output.toString('utf8').split('\0');
  assert.equal(fields.pop(), '');
  assert.equal(fields.length % 3, 0);
  const values = new Map();
  for (let index = 0; index < fields.length; index += 3) {
    values.set(`${fields[index]}:${fields[index + 1]}`, fields[index + 2]);
  }
  return values;
}

function assertLf(bytes, label) {
  assert.equal(bytes.includes(0x0d), false, `${label} must use LF, without CR bytes`);
  assert.equal(bytes.subarray(0, 3).equals(Buffer.from([0xef, 0xbb, 0xbf])), false, `${label} must not have a UTF-8 BOM`);
  assert.equal(bytes.includes(0x0a), true, `${label} must contain LF line endings`);
  assert.doesNotThrow(() => new TextDecoder('utf-8', { fatal: true }).decode(bytes), `${label} must be UTF-8`);
}

// Each fixture is a new, owned repository. It never checks out or changes this repository.
// Successful owned repositories are removed after verification; failures retain
// their marker and durable receipt. Existing data-* evidence is never inspected.
function withFixture(t, attributeBytes, sources, verify) {
  const lifecycle = createMigrationSourceFixture(workspace);
  const { root } = lifecycle;
  t.diagnostic(`Owned fixture: ${path.relative(workspace, root)}`);
  try {
    const empty = path.join(root, 'empty');
    const source = path.join(root, 'source');
    const checkout = path.join(root, 'checkout');
    mkdirSync(empty);
    mkdirSync(source);
    writeFileSync(path.join(empty, 'gitconfig'), '');
    writeFileSync(path.join(empty, 'attributes'), '');
    const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_')));
    Object.assign(env, {
      GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: path.join(empty, 'gitconfig'), GIT_ATTR_NOSYSTEM: '1',
      GIT_AUTHOR_NAME: 'Migration source test', GIT_AUTHOR_EMAIL: 'migration-test@example.invalid',
      GIT_COMMITTER_NAME: 'Migration source test', GIT_COMMITTER_EMAIL: 'migration-test@example.invalid',
      GIT_AUTHOR_DATE: '2000-01-01T00:00:00Z', GIT_COMMITTER_DATE: '2000-01-01T00:00:00Z',
    });
    function git(cwd, args, autocrlf = 'false', eol = 'lf') {
      return runGit(cwd, args, env, [
        '-c', `core.autocrlf=${autocrlf}`, '-c', `core.eol=${eol}`, '-c', 'core.safecrlf=false',
        '-c', `core.attributesFile=${path.join(empty, 'attributes')}`, '-c', `core.hooksPath=${empty}`,
        '-c', 'commit.gpgsign=false',
      ]);
    }
    git(source, ['init', '--quiet', '--initial-branch=fixture', `--template=${empty}`]);
    mkdirSync(path.join(source, 'migrations'));
    writeFileSync(path.join(source, '.gitattributes'), attributeBytes);
    for (const migration of sources) writeFileSync(path.join(source, migration.name), migration.bytes);
    git(source, ['add', '--', '.gitattributes', 'migrations']);
    const tree = git(source, ['write-tree']).toString('utf8').trim();
    const commit = git(source, ['commit-tree', tree, '-m', 'Owned migration source fixture']).toString('utf8').trim();
    git(source, ['update-ref', 'refs/heads/fixture', commit]);
    const owned = {
      git,
      source,
      checkout,
      clone(autocrlf) {
        git(source, ['clone', '--quiet', '--no-checkout', '--no-hardlinks', '--local', `--template=${empty}`, source, checkout], autocrlf, 'crlf');
        git(checkout, ['checkout', '--quiet', '--force', 'HEAD'], autocrlf, 'crlf');
      },
    };
    assert.equal(verify(owned) instanceof Promise, false, 'Fixture verification must finish synchronously');
  } catch (error) {
    lifecycle.fail();
    t.diagnostic(`Failed fixture retained; receipt: ${path.relative(workspace, lifecycle.receiptPath)}`);
    throw error;
  }
  lifecycle.complete();
  t.diagnostic(`Verified fixture removed; receipt: ${path.relative(workspace, lifecycle.receiptPath)}`);
}

// Read the short, regular-file entries emitted by Git's tar archive in memory.
// No external archive tool or extraction into another filesystem tree is needed.
function archiveFiles(tar) {
  const files = new Map();
  const string = (bytes) => bytes.toString('utf8').replace(/\0.*$/s, '');
  for (let offset = 0; offset + 512 <= tar.length;) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const size = Number.parseInt(string(header.subarray(124, 136)).trim(), 8);
    assert.ok(Number.isSafeInteger(size) && size >= 0, 'valid tar entry size');
    assert.ok(offset + 512 + size <= tar.length, 'complete tar entry');
    const prefix = string(header.subarray(345, 500));
    const name = `${prefix ? `${prefix}/` : ''}${string(header.subarray(0, 100))}`;
    if (header[156] === 0 || header[156] === 0x30) {
      assert.equal(files.has(name), false, `duplicate tar entry ${name}`);
      files.set(name, tar.subarray(offset + 512, offset + 512 + size));
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return files;
}

test('repository migration attributes force text and LF for every SQL file', () => {
  assert.ok(migrations.length >= 15, 'include all fifteen existing migrations');
  const names = migrations.map(({ name }) => name);
  const effective = attributes(runGit(workspace, ['check-attr', '-z', 'text', 'eol', '--', ...names]));
  for (const name of names) {
    assert.equal(effective.get(`${name}:text`), 'set', `${name} text attribute`);
    assert.equal(effective.get(`${name}:eol`), 'lf', `${name} eol attribute`);
  }
});

test('current migration bytes are UTF-8 LF without BOM and match their Git blobs', (t) => {
  const tracked = runGit(workspace, ['ls-files', '-z', '--', 'migrations/*.sql']).toString('utf8').split('\0').filter(Boolean).sort();
  assert.deepEqual(migrations.map(({ name }) => name), tracked, 'every SQL migration is tracked');
  for (const { name, bytes } of migrations) {
    assertLf(bytes, name);
    const blob = runGit(workspace, ['show', `:${name}`]);
    assert.deepEqual(bytes, blob, `${name} working bytes must match the immutable Git blob`);
    assert.equal(sha384(bytes), sha384(blob), `${name} SQLx SHA-384 checksum`);
  }
  t.diagnostic(`Verified ${migrations.length} migration SHA-384 checksums against Git blobs`);
});

for (const autocrlf of ['true', 'false', 'input']) {
  test(`owned Git checkout and archive preserve migration checksums with core.autocrlf=${autocrlf}`, (t) => {
    withFixture(t, readFileSync(path.join(workspace, '.gitattributes')), migrations, (owned) => {
      owned.clone(autocrlf);
      const archived = archiveFiles(owned.git(owned.checkout, ['archive', '--format=tar', 'HEAD', 'migrations'], autocrlf, 'crlf'));
      assert.deepEqual([...archived.keys()].sort(), migrations.map(({ name }) => name));
      for (const { name, bytes } of migrations) {
        const blob = owned.git(owned.checkout, ['show', `HEAD:${name}`], autocrlf, 'crlf');
        const checkedOut = readFileSync(path.join(owned.checkout, name));
        assertLf(checkedOut, `${autocrlf} checkout ${name}`);
        assert.deepEqual(blob, bytes, `${autocrlf} fixture Git blob ${name}`);
        assert.deepEqual(checkedOut, bytes, `${autocrlf} checkout bytes ${name}`);
        assert.deepEqual(archived.get(name), bytes, `${autocrlf} archive bytes ${name}`);
        assert.equal(sha384(checkedOut), sha384(bytes), `${autocrlf} checkout checksum ${name}`);
        assert.equal(sha384(archived.get(name)), sha384(bytes), `${autocrlf} archive checksum ${name}`);
      }
      t.diagnostic(`All ${migrations.length} migrations retain their Git/LF SHA-384 checksums in checkout and archive`);
    });
  });
}

test('independent fixture without the LF attribute reproduces the Windows-style checksum mismatch', (t) => {
  const name = 'migrations/0001_checksum_fixture.sql';
  const lf = Buffer.from('-- portable migration fixture\nCREATE TABLE checksum_fixture (id BIGINT PRIMARY KEY);\n', 'utf8');
  const crlf = Buffer.from('-- portable migration fixture\r\nCREATE TABLE checksum_fixture (id BIGINT PRIMARY KEY);\r\n', 'utf8');
  withFixture(t, Buffer.from('* text=auto\n'), [{ name, bytes: lf }], (owned) => {
    const lfArchive = archiveFiles(owned.git(owned.source, ['archive', '--format=tar', 'HEAD', 'migrations']));
    assert.deepEqual(lfArchive.get(name), lf, 'an LF-configured archive matches the Git blob');
    owned.clone('true');
    const effective = attributes(owned.git(owned.checkout, ['check-attr', '-z', 'eol', '--', name], 'true', 'crlf'));
    assert.equal(effective.get(`${name}:eol`), 'unspecified');
    const checkedOut = readFileSync(path.join(owned.checkout, name));
    const blob = owned.git(owned.checkout, ['show', `HEAD:${name}`], 'true', 'crlf');
    const archived = archiveFiles(owned.git(owned.checkout, ['archive', '--format=tar', 'HEAD', 'migrations'], 'true', 'crlf'));
    assert.deepEqual(blob, lf, 'the committed SQL stays LF');
    assert.deepEqual(archived.get(name), crlf, 'the Windows-style archive also converts SQL to CRLF');
    assert.deepEqual(checkedOut, crlf, 'core.autocrlf=true converts checkout bytes to CRLF');
    assert.notEqual(sha384(checkedOut), sha384(blob), 'line endings alone change the SQLx SHA-384 checksum');
    assert.notEqual(sha384(archived.get(name)), sha384(blob), 'archive line endings alone change the checksum');
    t.diagnostic(`Control SHA-384 LF=${sha384(lf)} CRLF=${sha384(checkedOut)}`);
  });
});
