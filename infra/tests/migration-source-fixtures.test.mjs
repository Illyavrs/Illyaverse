import assert from 'node:assert/strict';
import { existsSync, linkSync, mkdirSync, readFileSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createMigrationSourceFixture } from './migration-source-fixtures.mjs';

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

test('successful fixture removes only its owned run and retains a completed receipt', (t) => {
  const owned = createMigrationSourceFixture(workspace);
  writeFileSync(path.join(owned.root, 'owned.txt'), 'owned fixture');
  const marker = JSON.parse(readFileSync(owned.markerPath, 'utf8'));
  assert.equal(marker.type, 'migration-source');
  assert.equal(marker.canonical_run_root, owned.root);
  assert.equal(marker.completed_at, null);
  assert.equal(marker.outcome, 'running');
  owned.complete();
  assert.equal(existsSync(owned.root), false);
  const receipt = JSON.parse(readFileSync(owned.receiptPath, 'utf8'));
  assert.equal(receipt.owner_nonce, marker.owner_nonce);
  assert.equal(receipt.run_id, marker.run_id);
  assert.equal(receipt.outcome, 'passed');
  assert.equal(receipt.cleanup, 'removed');
  assert.ok(Number.isFinite(Date.parse(receipt.created_at)));
  assert.ok(Number.isFinite(Date.parse(receipt.completed_at)));
  assert.ok(Number.isFinite(Date.parse(receipt.cleanup_completed_at)));
  assert.throws(() => owned.complete(), /finish only once/);
  t.diagnostic(`Completed cleanup receipt: ${path.relative(workspace, owned.receiptPath)}`);
});

test('failed fixture preserves its repository evidence and completed failure marker', (t) => {
  const owned = createMigrationSourceFixture(workspace);
  writeFileSync(path.join(owned.root, 'failure-evidence.txt'), 'intentional lifecycle retention proof');
  owned.fail();
  assert.equal(existsSync(owned.root), true);
  assert.equal(readFileSync(path.join(owned.root, 'failure-evidence.txt'), 'utf8'), 'intentional lifecycle retention proof');
  const marker = JSON.parse(readFileSync(owned.markerPath, 'utf8'));
  const receipt = JSON.parse(readFileSync(owned.receiptPath, 'utf8'));
  assert.equal(marker.outcome, 'failed');
  assert.ok(Number.isFinite(Date.parse(marker.completed_at)));
  assert.equal(receipt.cleanup, 'retained-failure');
  assert.equal(receipt.owner_nonce, marker.owner_nonce);
  t.diagnostic(`Intentional failure proof retained: ${path.relative(workspace, owned.root)}`);
});

test('altered ownership marker cannot authorize fixture removal', () => {
  const owned = createMigrationSourceFixture(workspace);
  const original = readFileSync(owned.markerPath);
  const marker = JSON.parse(original.toString('utf8'));
  writeFileSync(owned.markerPath, JSON.stringify({ ...marker, canonical_run_root: workspace }));
  assert.throws(() => owned.complete(), /ownership marker must be unchanged/);
  assert.equal(existsSync(owned.root), true);
  assert.equal(existsSync(owned.receiptPath), false);
  writeFileSync(owned.markerPath, original);
  owned.complete();
});

test('linked ownership marker is refused before any marker update or removal', () => {
  const owned = createMigrationSourceFixture(workspace);
  const second = path.join(owned.root, 'linked-owner.json');
  const original = readFileSync(owned.markerPath);
  linkSync(owned.markerPath, second);
  assert.throws(() => owned.complete(), /linked fixture marker/);
  assert.deepEqual(readFileSync(second), original);
  assert.equal(existsSync(owned.root), true);
  unlinkSync(second);
  owned.complete();
});

test('descendant junction or symlink cannot redirect fixture cleanup into another owned run', () => {
  const owned = createMigrationSourceFixture(workspace);
  const destination = createMigrationSourceFixture(workspace);
  const sentinel = path.join(destination.root, 'sentinel.txt');
  const link = path.join(owned.root, 'redirected');
  writeFileSync(sentinel, 'destination must survive refusal');
  symlinkSync(destination.root, link, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => owned.complete(), /redirected fixture entry/);
  assert.equal(readFileSync(sentinel, 'utf8'), 'destination must survive refusal');
  assert.equal(existsSync(owned.receiptPath), false);
  unlinkSync(link);
  owned.complete();
  destination.complete();
});

test('redirected artifact ancestor is rejected before producing a run at its destination', () => {
  const owner = createMigrationSourceFixture(workspace);
  const destination = createMigrationSourceFixture(workspace);
  const fakeWorkspace = path.join(owner.root, 'workspace');
  const link = path.join(fakeWorkspace, '.artifacts');
  mkdirSync(fakeWorkspace);
  symlinkSync(destination.root, link, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => createMigrationSourceFixture(fakeWorkspace), /redirected fixture path/);
  assert.equal(existsSync(path.join(destination.root, 'tests')), false);
  unlinkSync(link);
  const redirectedWorkspace = path.join(owner.root, 'workspace-alias');
  symlinkSync(fakeWorkspace, redirectedWorkspace, process.platform === 'win32' ? 'junction' : 'dir');
  assert.throws(() => createMigrationSourceFixture(redirectedWorkspace), /redirected fixture path/);
  assert.equal(existsSync(path.join(fakeWorkspace, '.artifacts')), false);
  unlinkSync(redirectedWorkspace);
  owner.complete();
  destination.complete();
});
