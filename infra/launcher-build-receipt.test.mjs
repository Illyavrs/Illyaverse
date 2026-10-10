import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import test from 'node:test';
import { promisify } from 'node:util';
import { detachedDiagnostic, detachedEnvironment, interfaceDiagnostic, publicBuildConfiguration, sourceIdentity, validateBeforeSnapshot, validateBundledProofs } from './launcher-build-receipt.mjs';

const execute = promisify(execFile);
async function git(cwd, ...args) {
  const result = await execute('git', ['-c', 'commit.gpgsign=false', '-c', 'core.autocrlf=false', '-c', 'core.eol=lf', '-c', 'user.name=Build receipt fixture', '-c', 'user.email=fixture@example.invalid', ...args], { cwd, windowsHide: true, timeout: 30_000 });
  return result.stdout.trim();
}

const environment = () => ({
  ILLYAVERSE_LAZER_SERVER_ENDPOINT: 'https://illya.test',
  KALEIDOSCOPE_UPDATE_REPOSITORY: 'Illyavrs/Kaleidoscope',
  KALEIDOSCOPE_UPDATE_PUBLIC_KEY: 'a'.repeat(64),
  KALEIDOSCOPE_UPDATE_VERSION: '1.2.3',
  TRANSFIGURATION_UPDATE_REPOSITORY: 'Illyavrs/Transfiguration',
  TRANSFIGURATION_UPDATE_PUBLIC_KEY: 'b'.repeat(64),
  TRANSFIGURATION_BUNDLED_VERSION: '0.2.0',
});

test('build configuration binds both independent public channels and stable versions', () => {
  const configuration = publicBuildConfiguration(environment());
  assert.equal(configuration.repository, 'Illyavrs/Kaleidoscope');
  assert.equal(configuration.bridge_repository, 'Illyavrs/Transfiguration');
  assert.equal(configuration.version, '1.2.3');
  assert.equal(configuration.bridge_version, '0.2.0');
  assert.notEqual(configuration.public_key, configuration.bridge_public_key);
});

test('build job rejects signing material, missing keys, alternate repositories and invalid versions', () => {
  const patches = [
    { KALEIDOSCOPE_UPDATE_REPOSITORY: 'other/Kaleidoscope' },
    { TRANSFIGURATION_UPDATE_REPOSITORY: 'Illyavrs/Kaleidoscope' },
    { KALEIDOSCOPE_UPDATE_PUBLIC_KEY: '' },
    { TRANSFIGURATION_UPDATE_PUBLIC_KEY: '0'.repeat(64) },
    { TRANSFIGURATION_UPDATE_PUBLIC_KEY: 'B'.repeat(64) },
    { TRANSFIGURATION_UPDATE_PUBLIC_KEY: `${'b'.repeat(64)}\n` },
    { KALEIDOSCOPE_UPDATE_VERSION: '01.2.3' },
    { TRANSFIGURATION_BUNDLED_VERSION: '0.2.0-beta' },
    { ILLYAVERSE_LAZER_SERVER_ENDPOINT: 'https://osu.ppy.sh' },
    { ILLYAVERSE_LAZER_SERVER_ENDPOINT: undefined },
    { KALEIDOSCOPE_UPDATE_SIGNING_KEY_PEM: 'synthetic private material' },
    { TRANSFIGURATION_UPDATE_SIGNING_KEY_FILE: 'synthetic-private-file' },
  ];
  for (const patch of patches) assert.throws(() => publicBuildConfiguration({ ...environment(), ...patch }));
});

test('before and after receipt rejects changed commits or build channel input', () => {
  const source = { parent_commit: 'a'.repeat(40), repositories: { '.': 'a'.repeat(40), 'tools/kaleidoscope': 'b'.repeat(40) } };
  const configuration = publicBuildConfiguration(environment());
  const before = { protocol: 1, kind: 'kaleidoscope-build-source', source, public_build_configuration: configuration };
  assert.doesNotThrow(() => validateBeforeSnapshot(before, source, configuration));
  assert.throws(() => validateBeforeSnapshot(before, { ...source, parent_commit: 'c'.repeat(40) }, configuration), /changed/);
  assert.throws(() => validateBeforeSnapshot(before, source, { ...configuration, bridge_public_key: 'c'.repeat(64) }), /changed/);
  assert.throws(() => validateBeforeSnapshot({ ...before, kind: 'synthetic-proof' }, source, configuration));
});

test('embedded production diagnostic requires exact positive asset, script and stylesheet counts', () => {
  assert.deepEqual(interfaceDiagnostic('Production interface verified: 451 embedded assets, 7 script references, 3 stylesheet references\r\n'), {
    command: '--verify-embedded-interface', exit_code: 0, embedded_assets: 451, script_references: 7, stylesheet_references: 3,
  });
  for (const output of ['PASS', 'Production interface verified: 1 embedded assets, 0 script references, 1 stylesheet references', 'Production interface verified: 1 embedded assets, 1 script references, 1 stylesheet references\nextra', 'Production interface verified: 1 embedded assets, 1 script references, 1 stylesheet references\n\n', 'Production interface verified: 9007199254740992 embedded assets, 1 script references, 1 stylesheet references']) assert.throws(() => interfaceDiagnostic(output));
});

test('all detached modes require their successful compiled production summary', () => {
  const summaries = [
    ['--production-browser-only', 'PASS 41 compiled production browser policy checks on .NET 10.0.12; no bootstrap, shell, GameHost, player data, network or fixture bridge', 41],
    ['--production-logout-contracts', 'PASS 63 compiled production explicit logout contract checks; metadata and synthetic provider only, no game constructors, config, auth, Realm database, shell or network', 63],
    ['--production-welcome-contracts', 'PASS 17 compiled production welcome checks; pinned metadata and synthetic callbacks only, no game constructors, notification UI, auth, player data, Realm, shell or network', 17],
    ['--production-relax-contracts', 'PASS 29 compiled production RX request-builder checks; pinned metadata and inert builders only, no bootstrap/GameHost/API worker/Perform/Queue/config/player auth/Realm database/network', 29],
  ];
  for (const [mode, summary, count] of summaries) {
    assert.equal(detachedDiagnostic(mode, `PASS preceding synthetic assertion\r\n${summary}\r\n`), count);
    assert.throws(() => detachedDiagnostic(mode, `${summary}\nFAIL synthetic failed assertion`));
    assert.throws(() => detachedDiagnostic(mode, summary.replace(`PASS ${count}`, 'PASS 0')));
  }
  assert.throws(() => detachedDiagnostic('--browser-only', summaries[0][1]));
  assert.throws(() => detachedDiagnostic('--production-welcome-contracts', summaries[0][1]));
});

test('bundled proofs reject missing welcome or relax, alternate DLLs, fixture modes and changed native bindings', () => {
  const expected = { bridgeHash: 'a'.repeat(64), nativeHash: 'b'.repeat(64), proofExe: 'synthetic-proof.exe', bridgePath: 'synthetic-bundled.dll', nativeRoot: 'synthetic-native', frozenPath: 'synthetic-frozen.json' };
  const commands = ['--production-browser-only', '--production-logout-contracts', '--production-welcome-contracts', '--production-relax-contracts'].map((mode, index) => ({
    command: expected.proofExe,
    args: index === 0 ? [mode, expected.bridgePath] : [mode, expected.bridgePath, expected.nativeRoot, expected.frozenPath],
    exit_code: 0, checks: 17, stdout_sha256: 'c'.repeat(64),
  }));
  const proofs = { bridge_sha256: expected.bridgeHash, native_inputs_sha256: expected.nativeHash, commands };
  assert.doesNotThrow(() => validateBundledProofs(proofs, expected));
  assert.throws(() => validateBundledProofs({ ...proofs, commands: commands.slice(0, 2) }, expected));
  assert.throws(() => validateBundledProofs({ ...proofs, commands: commands.slice(0, 3) }, expected));
  assert.throws(() => validateBundledProofs({ ...proofs, bridge_sha256: 'd'.repeat(64) }, expected));
  assert.throws(() => validateBundledProofs({ ...proofs, native_inputs_sha256: 'd'.repeat(64) }, expected));
  for (const patch of [{ exit_code: 1 }, { args: ['--production-welcome-contracts', 'different.dll', expected.nativeRoot, expected.frozenPath] }, { args: ['--welcome-only', expected.bridgePath] }, { command: 'different.exe' }, { checks: 0 }]) {
    assert.throws(() => validateBundledProofs({ ...proofs, commands: [...commands.slice(0, 2), { ...commands[2], ...patch }, commands[3]] }, expected));
  }
});

test('detached proof environment rejects inherited runtime hooks and active profiling', () => {
  assert.doesNotThrow(() => detachedEnvironment({ CORECLR_ENABLE_PROFILING: '0' }));
  for (const name of ['DOTNET_STARTUP_HOOKS', 'DOTNET_ADDITIONAL_DEPS', 'DOTNET_SHARED_STORE', 'CORECLR_ENABLE_PROFILING', 'COR_ENABLE_PROFILING']) assert.throws(() => detachedEnvironment({ [name]: 'synthetic-active-override' }));
});

test('actual Git identity rejects a different parent, dirty files and changed submodule checkouts', async () => {
  const prefix = join(resolve(tmpdir()), 'illyaverse-build-source-fixture-');
  const fixture = await mkdtemp(prefix);
  try {
    const parent = join(fixture, 'parent');
    await mkdir(parent);
    await git(parent, 'init', '--initial-branch=main');
    await writeFile(join(parent, 'README.md'), 'Synthetic parent source\n');
    const folders = ['apps/serendipity', 'services/sanctuary', 'services/resonance', 'tools/kaleidoscope', 'tools/transfiguration'];
    for (const [index, folder] of folders.entries()) {
      const child = join(fixture, `source-${index}`);
      await mkdir(child);
      await git(child, 'init', '--initial-branch=main');
      await writeFile(join(child, 'README.md'), `Synthetic component ${index}\n`);
      await git(child, 'add', 'README.md');
      await git(child, 'commit', '-m', 'chore(fixture): seed synthetic source');
      await git(parent, '-c', 'protocol.file.allow=always', 'submodule', 'add', '--', child, folder);
    }
    await git(parent, 'add', 'README.md', '.gitmodules', ...folders);
    await git(parent, 'commit', '-m', 'chore(fixture): pin synthetic components');
    const head = await git(parent, 'rev-parse', 'HEAD');
    const actual = await sourceIdentity(parent, head);
    assert.equal(actual.parent_commit, head);
    assert.equal(Object.keys(actual.repositories).length, 6);
    assert.equal(actual.recursive_submodules.split('\n').length, 5);
    await assert.rejects(sourceIdentity(parent, '0'.repeat(40)), /differs/);
    await assert.rejects(sourceIdentity(parent, `${head}\n`), /Invalid/);
    await writeFile(join(parent, 'README.md'), 'Dirty tracked fixture\n');
    await assert.rejects(sourceIdentity(parent, head), /clean/);
    await writeFile(join(parent, 'README.md'), 'Synthetic parent source\n');
    const checkout = join(parent, 'tools/kaleidoscope');
    await writeFile(join(checkout, 'untracked.fixture'), 'Untracked fixture\n');
    await assert.rejects(sourceIdentity(parent, head), /clean/);
    await git(checkout, 'add', 'untracked.fixture');
    await git(checkout, 'commit', '-m', 'chore(fixture): change component pin');
    await assert.rejects(sourceIdentity(parent, head), /clean|gitlink/);
  } finally {
    // Only this exact owned temporary Git fixture crosses the cleanup boundary.
    assert(fixture.startsWith(prefix) && dirname(fixture) === resolve(tmpdir()));
    await rm(fixture, { recursive: true, force: true });
  }
});
