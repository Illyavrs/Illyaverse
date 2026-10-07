// Original PostgreSQL boundary fixture. Uses only one new, labelled, tmpfs
// container; it cannot connect to retained community databases or media.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const image = process.env.ILLYA_AVAILABILITY_POSTGRES_IMAGE || 'postgres:17-alpine';
const nonce = randomUUID().replaceAll('-', '');
const name = `illya-availability-pg-${nonce}`;
const label = 'org.illyaverse.fixture.availability';
const children = new Set();
let ownedId;
let imageId;
let checks = 0;

function docker(args, input, allowFailure = false) {
  const result = spawnSync('docker', args, {
    cwd: root, encoding: 'utf8', input, windowsHide: true, maxBuffer: 16 * 1024 * 1024,
  });
  assert.ifError(result.error);
  if (!allowFailure) assert.equal(result.status, 0, result.stderr);
  return result;
}
function sql(statement, allowFailure = false) {
  return docker(['exec', '-i', ownedId, 'psql', '-X', '-U', 'fixture', '-d', 'fixture',
    '-A', '-t', '-q', '-v', 'ON_ERROR_STOP=1', '-v', 'VERBOSITY=verbose'], statement, allowFailure);
}
function scalar(statement) {
  return sql(statement).stdout.trim();
}
function json(statement) {
  return JSON.parse(scalar(statement));
}
function reject(statement, code) {
  const result = sql(statement, true);
  assert.notEqual(result.status, 0, 'the write must be refused');
  assert.ok(result.stderr.includes(code), result.stderr);
  checks += 1;
}
async function waitUntil(predicate, description) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 40));
  }
  throw new Error(`Timed out waiting for ${description}`);
}
function startSql(application, statement) {
  const child = spawn('docker', ['exec', '-i', '-e', `PGAPPNAME=${application}`, ownedId,
    'psql', '-X', '-U', 'fixture', '-d', 'fixture', '-A', '-t', '-q',
    '-v', 'ON_ERROR_STOP=1', '-v', 'VERBOSITY=verbose'], { cwd: root, windowsHide: true });
  children.add(child);
  const state = { child, stdout: '', stderr: '', terminal: false };
  child.stdout.on('data', (chunk) => { state.stdout += chunk.toString(); });
  child.stderr.on('data', (chunk) => { state.stderr += chunk.toString(); });
  state.done = new Promise((resolve, rejectPromise) => {
    child.once('error', rejectPromise);
    child.once('close', (code) => {
      state.terminal = true;
      children.delete(child);
      resolve({ code, stdout: state.stdout, stderr: state.stderr });
    });
  });
  if (statement) child.stdin.end(statement);
  return state;
}
async function holder(setId) {
  const session = startSql(`availability-holder-${nonce}`);
  session.child.stdin.write(`BEGIN;
    SELECT pg_advisory_xact_lock(1229737811,${setId});
    SELECT 'fixture-ready';
  `);
  await waitUntil(() => {
    assert.equal(session.terminal, false, session.stderr);
    return session.stdout.includes('fixture-ready');
  }, 'owned holder transaction');
  return session;
}
async function blocked(application, session) {
  await waitUntil(() => {
    assert.equal(session.terminal, false, session.stderr);
    return scalar(`SELECT count(*) FROM pg_stat_activity
      WHERE application_name='${application}' AND wait_event_type='Lock'
      AND wait_event='advisory';`) === '1';
  }, 'the specific PostgreSQL advisory waiter');
}
const retainedTables = ['users', 'beatmaps', 'beatmap_packages', 'official_beatmapset_archives',
  'official_beatmapset_archive_members', 'beatmap_filename_aliases', 'scores',
  'lazer_score_tokens', 'audit_events', 'lazer_beatmap_updates'];
function fingerprints() {
  return json(`SELECT json_build_object(${retainedTables.map((table) =>
    `'${table}',(SELECT md5(coalesce(string_agg(to_jsonb(t)::text,E'\\n'
      ORDER BY to_jsonb(t)::text),'')) FROM ${table} t)`).join(',')});`);
}
function transition(setId, withdrawn, revision) {
  const stateWrite = revision === 1
    ? `INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id)
       VALUES(${setId},true,1,'Fixture withdrawal',11);`
    : `UPDATE beatmapset_availability SET withdrawn=${withdrawn},revision=${revision},
       reason='Fixture ${withdrawn ? 'withdrawal' : 'restoration'}',changed_at=clock_timestamp(),actor_id=11
       WHERE native_set_id=${setId};`;
  sql(`BEGIN; SELECT pg_advisory_xact_lock(1229737811,${setId}); ${stateWrite}
    INSERT INTO audit_events(actor_id,action,target_type,target_id,details)
      VALUES(11,'beatmaps.${withdrawn ? 'withdraw' : 'restore'}','beatmapset','${setId}',
      jsonb_build_object('withdrawn',${withdrawn},'revision',${revision})); COMMIT;`);
}
const scoreInsert = (map, key, user = 11) => `INSERT INTO scores(user_id,beatmap_id,mode,score,
  accuracy,max_combo,pp,client,ingestion_key,verification)
  VALUES(${user},${map},0,12345,100,2,10,'stable','${key}','verified')`;
const retry = (map, key, user = 11) => `${scoreInsert(map, key, user)}
  ON CONFLICT(ingestion_key) DO UPDATE SET ingestion_key=scores.ingestion_key
  WHERE scores.user_id=excluded.user_id AND scores.beatmap_id=excluded.beatmap_id
    AND scores.mode=excluded.mode AND scores.score_variant=excluded.score_variant
  RETURNING id;`;

try {
  assert.match(image, /^(?:sha256:[a-f0-9]{64}|postgres:17-alpine)$/);
  imageId = docker(['image', 'inspect', image, '--format', '{{.Id}}']).stdout.trim();
  const absent = docker(['container', 'inspect', name], undefined, true);
  assert.notEqual(absent.status, 0, 'the generated fixture name must be unused');
  ownedId = docker(['run', '-d', '--name', name, '--label', `${label}=${nonce}`,
    '--mount', 'type=tmpfs,destination=/var/lib/postgresql/data',
    '-e', 'POSTGRES_USER=fixture', '-e', 'POSTGRES_DB=fixture',
    '-e', `POSTGRES_PASSWORD=owned-fixture-${nonce}`, imageId]).stdout.trim();
  assert.match(ownedId, /^[a-f0-9]{64}$/);
  await waitUntil(() => docker(['exec', ownedId, 'pg_isready', '-U', 'fixture', '-d', 'fixture'],
    undefined, true).status === 0, 'owned PostgreSQL readiness');
  const migrations = readdirSync(path.join(root, 'migrations')).filter((entry) => entry.endsWith('.sql')).sort();
  assert.equal(migrations.length, 20);
  for (const filename of migrations.slice(0, 19)) sql(readFileSync(path.join(root, 'migrations', filename), 'utf8'));
  sql(`BEGIN;
    INSERT INTO users(id,username,email,password_hash,password_md5_hash) VALUES
      (11,'Availability owner','availability@fixture.invalid','unused','unused'),
      (12,'Other availability owner','other@fixture.invalid','unused','unused');
    INSERT INTO beatmap_packages(id,uploaded_by,kind,sha256,byte_length,member_md5,archive_path)
      VALUES('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',11,'osz','${'a'.repeat(64)}',100,
      ARRAY['${'1'.repeat(32)}','${'2'.repeat(32)}','${'3'.repeat(32)}','${'4'.repeat(32)}'],
      'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa/package.osz');
    ${['ranked', 'loved', 'pending', 'unranked'].map((status, index) => `
      INSERT INTO beatmaps(id,uploaded_by,title,artist,creator,version,mode,status,stars,md5,
        osu_path,archive_path,local_package_id)
        VALUES(${1001 + index},11,'Local fixture','Fixture','Fixture','${status}',0,'${status}',1,
        '${String(index + 1).repeat(32)}','local-${index}.osu',
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa/package.osz','aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');
      INSERT INTO beatmaps(id,official_id,official_set_id,uploaded_by,title,artist,creator,version,
        mode,status,source,stars,md5,osu_path)
        VALUES(${2001 + index},${8001 + index},701,11,'Official fixture','Fixture','Fixture',
        '${status}',0,'${status}','official',1,'${String(index + 5).repeat(32)}','official-${index}.osu');
    `).join('')}
    INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by)
      VALUES(701,'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb',
      'official-package-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb/package.osz',
      '${'b'.repeat(64)}',100,ARRAY['${'5'.repeat(32)}','${'6'.repeat(32)}','${'7'.repeat(32)}','${'8'.repeat(32)}'],11);
    INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id)
      SELECT 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb',id FROM beatmaps WHERE native_set_id=701;
    INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES('Fixture [Ranked].osu',1001);
    INSERT INTO scores(id,user_id,beatmap_id,mode,score,accuracy,max_combo,pp,replay_path,ingestion_key,verification)
      VALUES(21,11,1001,0,45678,100,2,100,'retained.osr','saved-fixture','verified');
    INSERT INTO lazer_score_tokens(id,user_id,beatmap_id,ruleset_id,version_hash,expires_at,score_id)
      VALUES(31,11,1001,0,'retained-version',now()+interval '1 hour',21);
    COMMIT;`);
  const before = fingerprints();
  sql(readFileSync(path.join(root, 'migrations', migrations[19]), 'utf8'));
  assert.deepEqual(fingerprints(), before, 'additive migration must preserve all retained fixture rows');
  checks += 1;
  assert.equal(scalar('SELECT count(*) FROM active_beatmaps;'), '8');
  transition(2_000_000_000, true, 1);
  assert.equal(scalar('SELECT count(*) FROM active_beatmaps;'), '4');
  assert.equal(scalar('SELECT count(*) FROM beatmaps;'), '8');
  assert.equal(scalar("SELECT replay_path FROM scores WHERE id=21;"), 'retained.osr');
  assert.equal(scalar("SELECT count(*) FROM beatmap_filename_aliases;"), '1');
  assert.equal(scalar("SELECT count(*) FROM lazer_score_tokens WHERE id=31 AND score_id=21;"), '1');
  assert.equal(scalar("SELECT count(*) FROM lazer_beatmap_updates;"), '10');
  checks += 1;
  reject(`${scoreInsert(1001, 'unknown-withdrawn')};`, 'P2001');
  reject(retry(1001, 'saved-fixture', 12), 'P2001');
  reject(retry(1002, 'saved-fixture'), 'P2001');
  assert.equal(scalar(retry(1001, 'saved-fixture')), '21');
  checks += 1;
  reject("UPDATE beatmapset_availability SET withdrawn=false,revision=9 WHERE native_set_id=2000000000;", '23514');
  reject("DELETE FROM beatmapset_availability WHERE native_set_id=2000000000;", '23514');
  reject("UPDATE beatmapset_availability SET reason='Changed reason' WHERE native_set_id=2000000000;", '23514');
  transition(2_000_000_000, false, 2);
  assert.equal(scalar('SELECT count(*) FROM active_beatmaps;'), '8');
  assert.equal(scalar('SELECT revision FROM beatmapset_availability WHERE native_set_id=2000000000;'), '2');
  assert.equal(scalar('SELECT count(*) FROM lazer_beatmap_updates;'), '11');
  sql(`${scoreInsert(1001, 'after-restoration')};`);
  checks += 1;
  transition(702, true, 1);
  reject(`INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,source,stars,md5,osu_path)
    VALUES(9001,702,'Uncached','Fixture','Fixture','Test',0,'official',1,'${'9'.repeat(32)}','uncached.osu');`, 'P2001');
  reject(`INSERT INTO official_beatmapset_archives(official_set_id,archive_path,sha256,byte_length,member_md5,uploaded_by)
    VALUES(702,'official-package-cccccccc-cccc-cccc-cccc-cccccccccccc/package.osz',
    '${'c'.repeat(64)}',100,ARRAY['${'9'.repeat(32)}'],11);`, 'P2001');
  transition(2_147_483_630, true, 1);
  reject(`INSERT INTO beatmap_packages(native_set_id,kind,member_md5,legacy)
    VALUES(2147483630,'osu',ARRAY['${'a'.repeat(32)}'],true);`, 'P2001');
  const rollbackEvents = scalar('SELECT count(*) FROM lazer_beatmap_updates;');
  sql(`BEGIN; SELECT pg_advisory_xact_lock(1229737811,703);
    INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id)
      VALUES(703,true,1,'Rolled back transition',11); ROLLBACK;`);
  assert.equal(scalar('SELECT count(*) FROM lazer_beatmap_updates;'), rollbackEvents);
  assert.equal(scalar('SELECT illya_beatmapset_active(703);'), 't');
  checks += 1;

  // The INSERT statement starts while the set is active, then waits for a
  // withdrawal transaction. Its volatile trigger must see the committed state.
  let owner = await holder(701);
  let application = `availability-new-score-${nonce}`;
  let contender = startSql(application, `${scoreInsert(2001, 'racing-new-score')};`);
  await blocked(application, contender);
  owner.child.stdin.end(`INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id)
    VALUES(701,true,1,'Concurrent withdrawal',11); COMMIT;
  `);
  assert.equal((await owner.done).code, 0);
  let result = await contender.done;
  assert.notEqual(result.code, 0);
  assert.ok(result.stderr.includes('P2001'), result.stderr);
  assert.equal(scalar("SELECT count(*) FROM scores WHERE ingestion_key='racing-new-score';"), '0');
  checks += 1;

  // A saved exact retry appears while the contender waits. It remains
  // idempotent even when that same holder also withdraws the set before commit.
  owner = await holder(701);
  application = `availability-saved-retry-${nonce}`;
  contender = startSql(application, retry(2001, 'saved-during-wait'));
  await blocked(application, contender);
  owner.child.stdin.end(`UPDATE beatmapset_availability SET withdrawn=false,revision=2,
    reason='Concurrent restoration' WHERE native_set_id=701;
    ${scoreInsert(2001, 'saved-during-wait')};
    UPDATE beatmapset_availability SET withdrawn=true,revision=3,
    reason='Concurrent withdrawal again' WHERE native_set_id=701; COMMIT;
  `);
  assert.equal((await owner.done).code, 0);
  result = await contender.done;
  assert.equal(result.code, 0, result.stderr);
  assert.equal(scalar("SELECT count(*) FROM scores WHERE ingestion_key='saved-during-wait';"), '1');
  assert.equal(scalar('SELECT illya_beatmapset_active(701);'), 'f');
  checks += 1;

  // Admission already owning the shared fence commits first. Withdrawal waits
  // for that specific transaction, and then forbids a new result.
  owner = startSql(`availability-admission-${nonce}`);
  owner.child.stdin.write(`BEGIN; SELECT illya_lock_active_beatmapset(2000000000);
    SELECT 'fixture-ready';
  `);
  await waitUntil(() => owner.stdout.includes('fixture-ready'), 'shared admission holder');
  application = `availability-writer-${nonce}`;
  contender = startSql(application, `BEGIN; SELECT pg_advisory_xact_lock(1229737811,2000000000);
    UPDATE beatmapset_availability SET withdrawn=true,revision=3,reason='Wait for prior admission'
      WHERE native_set_id=2000000000; COMMIT;`);
  await blocked(application, contender);
  owner.child.stdin.end(`${scoreInsert(1001, 'saved-before-withdrawal')}; COMMIT;
  `);
  assert.equal((await owner.done).code, 0);
  assert.equal((await contender.done).code, 0);
  reject(`${scoreInsert(1001, 'new-after-withdrawal')};`, 'P2001');
  assert.match(scalar(retry(1001, 'saved-before-withdrawal')), /^\d+$/);
  checks += 1;
  for (const table of retainedTables.filter((entry) => !['scores', 'audit_events', 'lazer_beatmap_updates'].includes(entry))) {
    assert.equal(fingerprints()[table], before[table], `retained ${table} remains byte-identical`);
  }
  checks += 1;
  console.log(`Availability PostgreSQL boundaries passed: ${checks} assertions, schema 0019→0020, actual lock waiters and retained identity/history.`);
} finally {
  for (const child of children) {
    if (child.stdin.writable) child.stdin.end('ROLLBACK;\n');
  }
  if (ownedId) {
    const inspection = JSON.parse(docker(['inspect', ownedId]).stdout)[0];
    assert.equal(inspection.Id, ownedId);
    assert.equal(inspection.Name, `/${name}`);
    assert.equal(inspection.Image, imageId);
    assert.equal(inspection.Config.Labels[label], nonce);
    assert.ok(inspection.Mounts.every((mount) => mount.Type === 'tmpfs'));
    docker(['rm', '-f', ownedId]);
    assert.notEqual(docker(['container', 'inspect', ownedId], undefined, true).status, 0);
    console.log('Verified removal of this invocation’s labelled tmpfs PostgreSQL container.');
  }
}
