import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const markerName = 'fixture-owner.json';
const fixtureType = 'migration-source';
const canonical = (name) => realpathSync.native(name);
const samePath = (left, right) => process.platform === 'win32'
  ? left.toLowerCase() === right.toLowerCase() : left === right;

function ancestors(target) {
  const result = [];
  for (let current = path.resolve(target);;) {
    result.unshift(current);
    const parent = path.dirname(current);
    if (parent === current) return result;
    current = parent;
  }
}

function noReparse(paths) {
  for (const name of paths) {
    assert.equal(lstatSync(name).isSymbolicLink(), false, `Refuse redirected fixture path: ${name}`);
    assert.ok(samePath(canonical(name), path.resolve(name)), `Refuse noncanonical fixture path: ${name}`);
  }
  if (process.platform !== 'win32') return;
  // Inspect native attributes too: not every NTFS reparse point is a symbolic link.
  // Paths are JSON on stdin, never interpolated into shell code.
  const result = spawnSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command',
    "$ErrorActionPreference = 'Stop'; foreach ($fixturePath in (ConvertFrom-Json ([Console]::In.ReadToEnd()))) { $fixtureItem = Get-Item -LiteralPath $fixturePath -Force; if (($fixtureItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Refuse fixture reparse point' } }; [Console]::Out.Write('verified')"],
  { input: JSON.stringify(paths), encoding: 'utf8', windowsHide: true });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `Native fixture path verification failed: ${result.stderr}`);
  assert.equal(result.stdout, 'verified');
}

function directory(target) {
  const parent = path.dirname(target);
  noReparse(ancestors(parent));
  try { mkdirSync(target); } catch (error) { if (error.code !== 'EEXIST') throw error; }
  noReparse([target]);
  assert.ok(lstatSync(target).isDirectory(), `Fixture directory required: ${target}`);
  return canonical(target);
}

function regularFile(target) {
  noReparse(ancestors(path.dirname(target)));
  const stat = lstatSync(target);
  assert.ok(stat.isFile(), `Fixture marker must be a regular file: ${target}`);
  assert.equal(stat.nlink, 1, `Refuse linked fixture marker: ${target}`);
  noReparse([target]);
}

function tree(target) {
  const entries = [];
  let pending = [target];
  while (pending.length) {
    const inspected = pending.map((name) => {
      const stat = lstatSync(name);
      assert.equal(stat.isSymbolicLink(), false, `Refuse redirected fixture entry: ${name}`);
      assert.ok(stat.isFile() || stat.isDirectory(), `Refuse special fixture entry: ${name}`);
      return { name, stat };
    });
    // Validate native NTFS reparse attributes before enumerating this level.
    // Batch each level so Git object directories do not need separate shells.
    noReparse(pending);
    pending = [];
    for (const { name, stat } of inspected) {
      entries.push(name);
      if (stat.isDirectory()) pending.push(...readdirSync(name).map((child) => path.join(name, child)));
    }
  }
  return entries;
}

/**
 * Only freshly created runs from this process can be removed by this helper.
 * The test process owns these random directories; callers must not share or
 * concurrently rename them. Pathname checks are not an OS filesystem lock.
 */
export function createMigrationSourceFixture(workspacePath) {
  const requestedWorkspace = path.resolve(workspacePath);
  noReparse(ancestors(requestedWorkspace));
  const workspace = canonical(requestedWorkspace);
  const artifacts = directory(path.join(workspace, '.artifacts'));
  const tests = directory(path.join(artifacts, 'tests'));
  const artifactRoot = directory(path.join(tests, fixtureType));
  const receipts = directory(path.join(artifactRoot, 'receipts'));
  const root = canonical(mkdtempSync(path.join(artifactRoot, 'run-')));
  const markerPath = path.join(root, markerName);
  const receiptPath = path.join(receipts, `${path.basename(root)}.json`);
  const rootStat = lstatSync(root, { bigint: true });
  const identity = {
    version: 1, type: fixtureType, run_id: path.basename(root), owner_nonce: randomUUID(),
    canonical_workspace: workspace, canonical_artifact_root: artifactRoot, canonical_run_root: root,
    run_directory_identity: { dev: String(rootStat.dev), ino: String(rootStat.ino) },
    created_at: new Date().toISOString(),
  };
  let marker = { ...identity, completed_at: null, outcome: 'running' };
  let finished = false;
  writeFileSync(markerPath, `${JSON.stringify(marker, null, 2)}\n`, { flag: 'wx' });
  const markerStat = lstatSync(markerPath, { bigint: true });

  function verifyOwner() {
    const relative = path.relative(artifactRoot, root);
    assert.ok(relative.startsWith('run-') && !relative.includes(path.sep) && !path.isAbsolute(relative),
      'Fixture removal must remain one owned run below its canonical artifact root');
    noReparse(ancestors(root));
    assert.ok(samePath(canonical(workspace), workspace));
    assert.ok(samePath(canonical(artifactRoot), artifactRoot));
    assert.ok(samePath(canonical(root), root));
    const currentRoot = lstatSync(root, { bigint: true });
    assert.equal(currentRoot.dev, rootStat.dev, 'Fixture root device must be unchanged');
    assert.equal(currentRoot.ino, rootStat.ino, 'Fixture root identity must be unchanged');
    regularFile(markerPath);
    const currentMarker = lstatSync(markerPath, { bigint: true });
    assert.equal(currentMarker.dev, markerStat.dev, 'Fixture marker device must be unchanged');
    assert.equal(currentMarker.ino, markerStat.ino, 'Fixture marker identity must be unchanged');
    assert.deepEqual(JSON.parse(readFileSync(markerPath, 'utf8')), marker, 'Fixture ownership marker must be unchanged');
  }

  function finish(outcome) {
    assert.equal(finished, false, 'Fixture run can finish only once');
    verifyOwner();
    if (outcome === 'passed') noReparse(tree(root));
    marker = { ...marker, completed_at: new Date().toISOString(), outcome };
    writeFileSync(markerPath, `${JSON.stringify(marker, null, 2)}\n`);
    noReparse(ancestors(receipts));
    const receipt = { ...marker, cleanup: outcome === 'passed' ? 'verified-removal-pending' : 'retained-failure' };
    writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { flag: 'wx' });
    if (outcome === 'passed') {
      verifyOwner();
      const entries = tree(root);
      noReparse(entries);
      // Recheck ownership and all ancestors immediately before recursive removal.
      verifyOwner();
      try {
        rmSync(root, { recursive: true, force: false, maxRetries: 2 });
      } catch (error) {
        regularFile(receiptPath);
        assert.deepEqual(JSON.parse(readFileSync(receiptPath, 'utf8')), receipt, 'Cleanup receipt must be unchanged');
        writeFileSync(receiptPath, `${JSON.stringify({ ...receipt, cleanup: 'removal-failed', cleanup_error_code: error.code ?? 'unknown' }, null, 2)}\n`);
        throw error;
      }
      regularFile(receiptPath);
      assert.deepEqual(JSON.parse(readFileSync(receiptPath, 'utf8')), receipt, 'Cleanup receipt must be unchanged');
      writeFileSync(receiptPath, `${JSON.stringify({ ...receipt, cleanup: 'removed', cleanup_completed_at: new Date().toISOString() }, null, 2)}\n`);
    }
    finished = true;
    return { root, receiptPath, outcome };
  }

  return { root, markerPath, receiptPath, complete: () => finish('passed'), fail: () => finish('failed') };
}
