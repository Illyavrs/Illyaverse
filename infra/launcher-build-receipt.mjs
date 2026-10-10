import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { lstat, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MAX_ARCHIVE_BYTES, assertUnlinkedPath, collectPackageHashes, validateCompiledUpdateConfig, validateVersion } from '../tools/kaleidoscope/scripts/create-update-release.mjs';
import { snapshotNative, snapshotRuntime, validateFrozenNative } from '../tools/transfiguration/scripts/production-evidence.mjs';

const COMPONENTS = ['apps/serendipity', 'services/sanctuary', 'services/resonance', 'tools/kaleidoscope', 'tools/transfiguration'];
const assert = (condition, message) => { if (!condition) throw new Error(message); };
const commit = (value) => { assert(typeof value === 'string' && value.length === 40 && /^[0-9a-f]{40}$/.test(value), 'Invalid immutable commit.'); return value; };
const BRIDGE = 'osu.Game.Rulesets.Transfiguration.dll';
const PROOF_MODES = ['--production-browser-only', '--production-logout-contracts', '--production-welcome-contracts', '--production-relax-contracts'];
const digest = (value) => createHash('sha256').update(value).digest('hex');

export function detachedEnvironment(environment) {
  for (const name of ['DOTNET_STARTUP_HOOKS', 'DOTNET_ADDITIONAL_DEPS', 'DOTNET_SHARED_STORE']) assert(!environment[name], 'Inherited .NET hooks or dependency overrides are not allowed in detached proofs.');
  for (const name of ['CORECLR_ENABLE_PROFILING', 'COR_ENABLE_PROFILING']) assert(!environment[name] || environment[name] === '0', 'Inherited profiling is not allowed in detached proofs.');
  return environment;
}

function nativeRoot(environment) {
  const value = environment.ILLYAVERSE_NATIVE_ROOT;
  assert(typeof value === 'string' && isAbsolute(value) && !value.startsWith('\\\\') && !value.startsWith('//'), 'Configure an absolute local native reference directory.');
  return resolve(value);
}

function ownedLocations(options, stage) {
  const root = resolve(options.workspaceRoot, '.artifacts/ci-launcher');
  const expected = {
    outputFile: resolve(root, `provenance/${stage === 'snapshot' ? 'source-before' : 'build-receipt'}.json`),
    frozenNative: resolve(root, 'provenance/frozen-native.json'),
    before: resolve(root, 'provenance/source-before.json'),
    proofRuntime: resolve(root, 'proof-runtime'),
    packageDirectory: resolve(root, 'payload/package'),
    archive: resolve(root, 'payload/package.zip'),
  };
  for (const [name, path] of Object.entries(expected)) if (options[name] !== undefined) assert(resolve(options[name]) === path, 'Producer inputs and outputs must stay in the exact owned CI build directories.');
}

export function publicBuildConfiguration(environment) {
  detachedEnvironment(environment);
  assert(environment.ILLYAVERSE_LAZER_SERVER_ENDPOINT === 'https://illya.test', 'Detached proofs require the explicit default private HTTPS task origin.');
  for (const name of ['KALEIDOSCOPE_UPDATE_SIGNING_KEY_PEM', 'KALEIDOSCOPE_UPDATE_SIGNING_KEY_FILE', 'TRANSFIGURATION_UPDATE_SIGNING_KEY_PEM', 'TRANSFIGURATION_UPDATE_SIGNING_KEY_FILE']) {
    assert(!environment[name], 'Signing keys must not enter the build job.');
  }
  assert(environment.KALEIDOSCOPE_UPDATE_REPOSITORY === 'Illyavrs/Kaleidoscope', 'Configure the canonical Kaleidoscope repository.');
  assert(environment.TRANSFIGURATION_UPDATE_REPOSITORY === 'Illyavrs/Transfiguration', 'Configure the canonical Transfiguration repository.');
  for (const name of ['KALEIDOSCOPE_UPDATE_PUBLIC_KEY', 'TRANSFIGURATION_UPDATE_PUBLIC_KEY']) {
    assert(typeof environment[name] === 'string' && environment[name].length === 64 && /^[0-9a-f]{64}$/.test(environment[name]) && environment[name] !== '0'.repeat(64), 'Configure nonzero raw lowercase Ed25519 public keys.');
  }
  return {
    version: validateVersion(environment.KALEIDOSCOPE_UPDATE_VERSION),
    bridge_version: validateVersion(environment.TRANSFIGURATION_BUNDLED_VERSION),
    repository: environment.KALEIDOSCOPE_UPDATE_REPOSITORY,
    public_key: environment.KALEIDOSCOPE_UPDATE_PUBLIC_KEY,
    bridge_repository: environment.TRANSFIGURATION_UPDATE_REPOSITORY,
    bridge_public_key: environment.TRANSFIGURATION_UPDATE_PUBLIC_KEY,
    detached_proof_endpoint: environment.ILLYAVERSE_LAZER_SERVER_ENDPOINT,
  };
}

async function command(program, args, options = {}) {
  return await new Promise((accept, reject) => {
    const child = spawn(program, args, { shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], ...options });
    let output = '';
    let count = 0;
    let failure;
    const timer = setTimeout(() => { failure = new Error('Build verification command timed out.'); child.kill(); }, 60_000);
    function consume(chunk, keep) {
      count += chunk.length;
      if (count > 65_536) { failure = new Error('Build verification output exceeded its limit.'); child.kill(); }
      if (keep && count <= 65_536) output += chunk.toString('utf8');
    }
    child.stdout.on('data', (chunk) => consume(chunk, true));
    child.stderr.on('data', (chunk) => consume(chunk, false));
    child.on('error', (error) => { clearTimeout(timer); reject(error); });
    child.on('close', (code) => {
      clearTimeout(timer);
      if (failure || code !== 0) reject(failure ?? new Error('Build verification command failed.'));
      else accept(output);
    });
  });
}

export async function sourceIdentity(workspaceRoot, parentCommit) {
  const root = await assertUnlinkedPath(workspaceRoot);
  const requested = commit(parentCommit);
  const repositories = {};
  for (const folder of ['.', ...COMPONENTS]) {
    const cwd = await assertUnlinkedPath(resolve(root, folder));
    const head = commit((await command('git', ['rev-parse', 'HEAD'], { cwd })).trim());
    assert((await command('git', ['status', '--porcelain', '--untracked-files=all'], { cwd })).trim() === '', 'All source checkouts must be committed and clean.');
    await command('git', ['diff', '--exit-code', 'HEAD', '--'], { cwd });
    if (folder === '.') assert(head === requested, 'Parent checkout differs from the selected workflow commit.');
    else {
      const tree = (await command('git', ['ls-tree', 'HEAD', '--', folder], { cwd: root })).trim();
      assert(tree === `160000 commit ${head}\t${folder}`, 'Component checkout differs from its parent gitlink.');
    }
    repositories[folder] = head;
  }
  const recursive = (await command('git', ['submodule', 'status', '--recursive'], { cwd: root })).trimEnd();
  assert(recursive.length > 0 && recursive.split(/\r?\n/).every((line) => /^ [0-9a-f]{40} /.test(line)), 'Recursive submodules must match their committed pins.');
  return { parent_commit: requested, repositories, recursive_submodules: recursive };
}

async function hash(path) {
  const value = createHash('sha256');
  for await (const chunk of createReadStream(path)) value.update(chunk);
  return value.digest('hex');
}

async function jsonFile(path) {
  const full = await assertUnlinkedPath(path);
  const item = await lstat(full);
  assert(item.isFile() && item.size > 0 && item.size <= 1_048_576, 'Invalid build evidence file.');
  return JSON.parse(await readFile(full, 'utf8'));
}

export function validateBeforeSnapshot(before, source, configuration) {
  assert(before?.protocol === 1 && before.kind === 'kaleidoscope-build-source', 'Invalid pre-build source evidence.');
  assert(JSON.stringify(before.source) === JSON.stringify(source), 'Source checkouts changed during the production build.');
  assert(JSON.stringify(before.public_build_configuration) === JSON.stringify(configuration), 'Public build configuration changed during the build.');
}

export function interfaceDiagnostic(output) {
  const match = /^Production interface verified: ([1-9][0-9]*) embedded assets, ([1-9][0-9]*) script references, ([1-9][0-9]*) stylesheet references\r?\n?$/.exec(output);
  assert(match !== null && match[0] === output, 'Invalid embedded production interface diagnostic.');
  const [assets, scripts, stylesheets] = match.slice(1).map(Number);
  assert([assets, scripts, stylesheets].every(Number.isSafeInteger), 'Invalid embedded interface counts.');
  return { command: '--verify-embedded-interface', exit_code: 0, embedded_assets: assets, script_references: scripts, stylesheet_references: stylesheets };
}

export function detachedDiagnostic(mode, output) {
  const summaries = {
    '--production-browser-only': /^PASS ([1-9][0-9]*) compiled production browser policy checks on \.NET 10\.0\.12; no bootstrap, shell, GameHost, player data, network or fixture bridge$/,
    '--production-logout-contracts': /^PASS ([1-9][0-9]*) compiled production explicit logout contract checks; metadata and synthetic provider only, no game constructors, config, auth, Realm database, shell or network$/,
    '--production-welcome-contracts': /^PASS ([1-9][0-9]*) compiled production welcome checks; pinned metadata and synthetic callbacks only, no game constructors, notification UI, auth, player data, Realm, shell or network$/,
    '--production-relax-contracts': /^PASS ([1-9][0-9]*) compiled production RX request-builder checks; pinned metadata and inert builders only, no bootstrap\/GameHost\/API worker\/Perform\/Queue\/config\/player auth\/Realm database\/network$/,
  };
  assert(PROOF_MODES.includes(mode), 'Only the four detached production proof modes are allowed.');
  const lines = output.trimEnd().split(/\r?\n/);
  assert(lines.every((line) => line.startsWith('PASS ')), 'Detached proof output contains an unexpected result.');
  const match = summaries[mode].exec(lines.at(-1));
  assert(match !== null && Number.isSafeInteger(Number(match[1])), 'Missing successful detached production proof summary.');
  return Number(match[1]);
}

export function validateBundledProofs(proofs, expected) {
  assert(proofs?.bridge_sha256 === expected.bridgeHash && proofs.native_inputs_sha256 === expected.nativeHash && Array.isArray(proofs.commands) && proofs.commands.length === PROOF_MODES.length, 'All four proofs must bind the exact bundled DLL and frozen native inputs.');
  for (const [index, result] of proofs.commands.entries()) {
    const mode = PROOF_MODES[index];
    const args = mode === PROOF_MODES[0] ? [mode, expected.bridgePath] : [mode, expected.bridgePath, expected.nativeRoot, expected.frozenPath];
    assert(result.command === expected.proofExe && JSON.stringify(result.args) === JSON.stringify(args) && result.exit_code === 0 && Number.isSafeInteger(result.checks) && result.checks > 0 && /^[0-9a-f]{64}$/.test(result.stdout_sha256) && result.stdout_sha256.length === 64, 'Unexpected bundled bridge command, unsuccessful proof or fixture/bootstrap invocation.');
  }
  return proofs;
}

export async function buildReceipt(options, environment = process.env) {
  assert(process.platform === 'win32', 'Production package diagnostics require Windows.');
  ownedLocations(options, 'complete');
  const configuration = publicBuildConfiguration(environment);
  const before = await jsonFile(options.before);
  const source = await sourceIdentity(options.workspaceRoot, options.parentCommit);
  validateBeforeSnapshot(before, source, configuration);
  const compatibility = await jsonFile(resolve(options.workspaceRoot, 'tools/transfiguration/compatibility.json'));
  const native = nativeRoot(environment);
  const frozenPath = await assertUnlinkedPath(options.frozenNative);
  const frozen = validateFrozenNative(await jsonFile(frozenPath), compatibility);
  const frozenHash = await hash(frozenPath);
  assert(before.frozen_native_sha256 === frozenHash && JSON.stringify(await snapshotNative(native, compatibility)) === JSON.stringify(frozen), 'Native references differ from the pre-build frozen bytes.');
  const directory = await assertUnlinkedPath(options.packageDirectory);
  const archive = await assertUnlinkedPath(options.archive);
  const container = dirname(directory);
  assert(directory === resolve(container, 'package') && archive === resolve(container, 'package.zip'), 'The payload must contain package/ and package.zip.');
  const names = (await readdir(container)).sort();
  assert(JSON.stringify(names) === JSON.stringify(['package', 'package.zip']), 'The payload must contain exactly package/ and package.zip.');
  const archiveItem = await lstat(archive);
  assert(archiveItem.isFile() && archiveItem.size > 0 && archiveItem.size <= MAX_ARCHIVE_BYTES, 'Invalid production package ZIP.');
  const files = await collectPackageHashes(directory);
  const archiveHash = await hash(archive);
  const metadata = await jsonFile(resolve(directory, 'package.json'));
  assert(metadata.build_profile === 'release' && metadata.fixture === false && metadata.status === 'complete' && metadata.embedded_interface_verified === true && metadata.native_gui_verified === false, 'Require a complete production Release GUI package with honest native verification state.');
  const verifier = await assertUnlinkedPath(resolve(options.workspaceRoot, 'tools/kaleidoscope/scripts/launcher-package.psm1'));
  await command('pwsh', ['-NoProfile', '-NonInteractive', '-Command', "$ErrorActionPreference='Stop'; Import-Module $env:ILLYA_RECEIPT_VERIFIER -Force; Test-KaleidoscopePackage -Directory $env:ILLYA_RECEIPT_PACKAGE | Out-Null; Test-KaleidoscopePackageArchive -ArchivePath $env:ILLYA_RECEIPT_ARCHIVE -Directory $env:ILLYA_RECEIPT_PACKAGE | Out-Null"], {
    env: { ...environment, ILLYA_RECEIPT_VERIFIER: verifier, ILLYA_RECEIPT_PACKAGE: directory, ILLYA_RECEIPT_ARCHIVE: archive },
  });
  const launcher = resolve(directory, 'Kaleidoscope.exe');
  const compiled = JSON.parse(await command(launcher, ['--verify-update-config']));
  validateCompiledUpdateConfig(compiled, { version: configuration.version, bridgeVersion: configuration.bridge_version, repository: configuration.repository, publicKeyHex: configuration.public_key });
  const embedded = interfaceDiagnostic(await command(launcher, ['--verify-embedded-interface']));
  const proofRoot = await assertUnlinkedPath(options.proofRuntime);
  const proofRuntime = await snapshotRuntime(proofRoot);
  const proofExe = resolve(proofRoot, 'Transfiguration.Proof.exe');
  const bridgePath = resolve(directory, BRIDGE);
  const proofs = { bridge_sha256: files[BRIDGE], native_inputs_sha256: frozenHash, proof_runtime_sha256: proofRuntime, commands: [] };
  for (const mode of PROOF_MODES) {
    const args = mode === PROOF_MODES[0] ? [mode, bridgePath] : [mode, bridgePath, native, frozenPath];
    const output = await command(proofExe, args, { env: detachedEnvironment(environment) });
    const checks = detachedDiagnostic(mode, output);
    const logName = `${mode.slice(2)}.log`;
    await writeFile(resolve(dirname(options.outputFile), logName), output, { encoding: 'utf8', flag: 'wx' });
    proofs.commands.push({ command: proofExe, args, exit_code: 0, checks, stdout_sha256: digest(output), stdout_file: logName });
  }
  validateBundledProofs(proofs, { bridgeHash: files[BRIDGE], nativeHash: frozenHash, proofExe, bridgePath, nativeRoot: native, frozenPath });
  const after = await sourceIdentity(options.workspaceRoot, options.parentCommit);
  validateBeforeSnapshot(before, after, publicBuildConfiguration(environment));
  assert(JSON.stringify(files) === JSON.stringify(await collectPackageHashes(directory)) && archiveHash === await hash(archive) && archiveItem.size === (await lstat(archive)).size, 'Package bytes changed during production verification.');
  assert(frozenHash === await hash(frozenPath) && JSON.stringify(await snapshotNative(native, compatibility)) === JSON.stringify(frozen) && JSON.stringify(await snapshotRuntime(proofRoot)) === JSON.stringify(proofRuntime), 'Native reference bytes or detached proof runtime changed during verification.');
  return {
    protocol: 1,
    kind: 'kaleidoscope-production-build',
    completed_at_utc: new Date().toISOString(),
    build_profile: 'release',
    source_before: before.source,
    source_after: after,
    public_build_configuration: configuration,
    archive: { name: 'package.zip', bytes: archiveItem.size, sha256: archiveHash },
    package_files_sha256: files,
    compiled_update_configuration: { command: '--verify-update-config', exit_code: 0, result: compiled },
    embedded_interface: embedded,
    bundled_bridge_proofs: proofs,
    verification: {
      package_and_zip_integrity: 'passed',
      immutable_source_pins: 'passed',
      compiled_launcher_channel_and_bundled_version: 'passed',
      exact_bundled_bridge_browser_logout_welcome_and_relax: 'passed_detached_compiled_proofs',
      frozen_native_reference_bytes: 'unchanged_before_build_and_after_proofs',
      bridge_update_repository_and_key: 'recorded_build_inputs; compiled_diagnostic_not_available',
      native_gui: 'pending',
      native_login: 'pending',
      game_launched: false,
    },
  };
}

function argumentsFrom(argv) {
  const [stage, ...args] = argv;
  assert(stage === 'snapshot' || stage === 'complete', 'Use snapshot or complete.');
  const allowed = stage === 'snapshot' ? ['workspace-root', 'parent-commit', 'output-file', 'frozen-native'] : ['workspace-root', 'parent-commit', 'output-file', 'before', 'frozen-native', 'proof-runtime', 'package-directory', 'archive'];
  const values = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i]?.startsWith('--') ? args[i].slice(2) : '';
    assert(allowed.includes(key) && args[i + 1] && !Object.hasOwn(values, key), 'Invalid or duplicate receipt arguments.');
    values[key] = args[i + 1];
  }
  assert(allowed.every((key) => Object.hasOwn(values, key)), 'Missing required receipt arguments.');
  return { stage, workspaceRoot: values['workspace-root'], parentCommit: values['parent-commit'], outputFile: values['output-file'], before: values.before, frozenNative: values['frozen-native'], proofRuntime: values['proof-runtime'], packageDirectory: values['package-directory'], archive: values.archive };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = argumentsFrom(process.argv.slice(2));
    let value;
    if (options.stage === 'snapshot') {
      assert(process.platform === 'win32', 'Native inventory freezing requires the dedicated Windows runner.');
      ownedLocations(options, 'snapshot');
      const configuration = publicBuildConfiguration(process.env);
      const source = await sourceIdentity(options.workspaceRoot, options.parentCommit);
      const compatibility = await jsonFile(resolve(options.workspaceRoot, 'tools/transfiguration/compatibility.json'));
      const frozen = await snapshotNative(nativeRoot(process.env), compatibility);
      const frozenPath = await assertUnlinkedPath(options.frozenNative, { exists: false });
      await writeFile(frozenPath, `${JSON.stringify(frozen, null, 2)}\n`, { encoding: 'utf8', flag: 'wx' });
      value = { protocol: 1, kind: 'kaleidoscope-build-source', source, public_build_configuration: configuration, frozen_native_sha256: await hash(frozenPath) };
    } else value = await buildReceipt(options);
    const destination = await assertUnlinkedPath(options.outputFile, { exists: false });
    await writeFile(destination, `${JSON.stringify(value, null, 2)}\n`, { encoding: 'utf8', flag: 'wx' });
    process.stdout.write(`PASS ${value.kind}; native GUI/login remains pending.\n`);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
