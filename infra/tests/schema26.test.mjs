import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const image = 'sha256:1bea307dfb3ee30541a7acf7de14b58bcd6948da98e5d31a04c627c4d35ec64b';
const label = 'org.illyaverse.schema26.owner';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = readdirSync(path.join(workspace, 'migrations')).filter(name => /^\d{4}_.*\.sql$/.test(name) && Number(name.slice(0, 4)) <= 26)
  .sort().map(name => ({ name, version: Number(name.slice(0, 4)), bytes: readFileSync(path.join(workspace, 'migrations', name)) }));

const fixture = `
  CREATE FUNCTION schema26_assert(condition BOOLEAN, detail TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
    BEGIN IF condition IS NOT TRUE THEN RAISE EXCEPTION 'schema26 assertion: %', detail; END IF; END $$;
  INSERT INTO users(id,username,email,password_hash,password_md5_hash,is_restricted)
    SELECT id,'Schema26User'||id,'schema26-'||id||'@example.invalid','fixture','fixture',id=3
    FROM generate_series(1,8) id;
  INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES(1,4,'block'),(5,1,'block');
  INSERT INTO channels(id,name,kind,required_permission,dm_user_low,dm_user_high) VALUES
    (100,'schema26-visible','PM','chat.pm',1,2),
    (101,'schema26-restricted-peer','PM','chat.pm',1,3),
    (102,'schema26-reader-block','PM','chat.pm',1,4),
    (103,'schema26-peer-block','PM','chat.pm',1,5),
    (104,'schema26-outside-reader','PM','chat.pm',2,7),
    (105,'schema26-public','PUBLIC',NULL,NULL,NULL),
    (106,'schema26-reader-cascade','PM','chat.pm',7,8);
  INSERT INTO lazer_channel_members(user_id,channel_id,last_read_id) VALUES
    (1,100,1002),(6,100,1002),(1,101,1101),(1,102,1201),(1,103,1301),
    (1,104,1400),(1,105,1500),(8,106,0);
  INSERT INTO protocol_private_messages(id,sender_id,recipient_id,content,created_at,client_uuid)
    VALUES(9000,1,2,'Historical own bridged PM','2020-01-01','schema26-historical');
  INSERT INTO chat_messages(id,channel_id,user_id,content,created_at) VALUES
    (950,100,3,'Restricted sender in an accessible pair','2020-01-02'),
    (951,100,4,'Reader-blocked sender in an accessible pair','2020-01-03'),
    (952,100,5,'Sender-blocked reader in an accessible pair','2020-01-04'),
    (953,100,7,'Existing history predicate permits this sender','2020-01-05'),
    (1000,100,2,'Historical incoming PM','2020-01-06'),
    (1001,100,1,'Historical own PM','2020-01-07'),
    (1002,100,2,'Historical read marker PM','2020-01-08'),
    (1003,100,2,'Historical incoming beyond marker','2020-01-09'),
    (1100,101,3,'Restricted peer PM','2020-01-10'),
    (1101,101,7,'Clean sender with restricted peer','2020-01-11'),
    (1200,102,4,'Reader blocks peer PM','2020-01-12'),
    (1201,102,7,'Clean sender with reader-blocked peer','2020-01-13'),
    (1300,103,5,'Peer blocks reader PM','2020-01-14'),
    (1301,103,7,'Clean sender with peer-blocked reader','2020-01-15'),
    (1400,104,2,'Outside reader membership PM','2020-01-16'),
    (1500,105,2,'Public channel message','2020-01-17');
  CREATE VIEW schema26_current_history AS
    SELECT 'users'::TEXT AS table_name,id::TEXT AS row_key,to_jsonb(u) AS data FROM users u
    UNION ALL SELECT 'channels',id::TEXT,to_jsonb(c) FROM channels c
    UNION ALL SELECT 'chat_messages',id::TEXT,to_jsonb(m) FROM chat_messages m
    UNION ALL SELECT 'protocol_private_messages',id::TEXT,to_jsonb(p) FROM protocol_private_messages p
    UNION ALL SELECT 'user_relationships',user_id||':'||target_id||':'||relation_type,to_jsonb(r) FROM user_relationships r
    UNION ALL SELECT 'lazer_channel_members',user_id||':'||channel_id,to_jsonb(m) FROM lazer_channel_members m;
  CREATE TABLE schema26_historical AS SELECT * FROM schema26_current_history;
`;

test('schema 26 fresh and retained-version upgrades record visible PM rows atomically across commit order and membership changes', {
  skip: process.env.ILLYA_SCHEMA26_ALLOW_DISPOSABLE !== '1',
}, async t => {
  assert.equal(sources.at(-1).version, 26, 'this historical gate binds migration prefix 1..26');
  assert.equal(sources.length, 26);
  assert.deepEqual(sources.map(source => source.version), Array.from({ length: 26 }, (_, index) => index + 1));
  const owner = randomUUID();
  const name = `illya-schema26-${owner.replaceAll('-', '')}`;
  const artifact = path.join(workspace, '.artifacts', 'tests', `schema26-${new Date().toISOString().replaceAll(/[-:.]/g, '')}-${owner}`);
  mkdirSync(artifact, { recursive: true });
  const receipt = { version: 1, started_at: new Date().toISOString(), owner, image, container: name,
    source_files: sources.map(source => ({ path: `migrations/${source.name}`, sha256: digest(source.bytes),
      sqlx_sha384: createHash('sha384').update(source.bytes).digest('hex') })), cases: [], cleanup: null, outcome: 'running' };
  const save = () => writeFileSync(path.join(artifact, 'verification.json'), `${JSON.stringify(receipt, null, 2)}\n`);
  save();
  function docker(args, input, check = true) {
    const result = spawnSync('docker', args, { input, encoding: 'utf8', windowsHide: true, timeout: 60000, maxBuffer: 16 * 1024 * 1024 });
    assert.ifError(result.error);
    if (check) assert.equal(result.status, 0, `${args.slice(0, 2).join(' ')}: ${result.stderr}`);
    return result;
  }
  let created = false;
  let creationAttempted = false;
  let containerId;
  const sql = (db, text) => docker(['exec', '-i', name, 'psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres', '-d', db, '-Atq'], text).stdout.trim();
  function unchangedSources() {
    for (const source of sources) assert.equal(digest(readFileSync(path.join(workspace, 'migrations', source.name))), digest(source.bytes));
  }
  function ownedContainer(inspected) {
    assert.equal(inspected.Id, containerId);
    assert.equal(inspected.Image, image);
    assert.equal(inspected.Config.Labels[label], owner);
    assert.equal(inspected.Name, `/${name}`);
    assert.equal(inspected.HostConfig.NetworkMode, 'none');
    assert.equal(Object.keys(inspected.HostConfig.PortBindings ?? {}).length, 0);
    assert.equal(inspected.Mounts.length, 1);
    assert.equal(inspected.Mounts[0].Type, 'tmpfs');
    assert.equal(inspected.Mounts[0].Destination, '/var/lib/postgresql/data');
  }
  try {
    assert.equal(docker(['image', 'inspect', image, '--format', '{{.Id}}']).stdout.trim(), image);
    creationAttempted = true;
    const run = docker(['run', '-d', '--pull=never', '--name', name, '--label', `${label}=${owner}`, '--network', 'none',
      '--mount', 'type=tmpfs,destination=/var/lib/postgresql/data,tmpfs-size=268435456',
      '-e', `POSTGRES_PASSWORD=${randomBytes(32).toString('hex')}`, image], undefined, false);
    containerId = run.stdout.trim();
    if (/^[0-9a-f]{64}$/.test(containerId)) created = true;
    assert.equal(run.status, 0, `run -d: ${run.stderr}`);
    assert.match(containerId, /^[0-9a-f]{64}$/);
    created = true;
    ownedContainer(JSON.parse(docker(['inspect', name]).stdout)[0]);
    for (let attempt = 0; ; attempt++) {
      const ready = docker(['exec', name, 'pg_isready', '-U', 'postgres'], undefined, false);
      if (ready.status === 0) break;
      assert.ok(attempt < 45, 'owned PostgreSQL becomes ready');
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    const serverVersion = Number(sql('postgres', 'SHOW server_version_num;'));
    assert.ok(serverVersion >= 170000 && serverVersion < 180000, 'cached image runs PostgreSQL 17');
    receipt.server_version_num = serverVersion;
    for (const previous of [0, 24, 25]) {
        const caseReceipt = { previous_version: previous, passed: false };
        try {
          unchangedSources();
          const db = `schema26_${previous}`;
          sql('postgres', `CREATE DATABASE ${db};`);
          const apply = selected => sql(db, selected.map(source => source.bytes.toString('utf8')).join('\n'));
          apply(previous ? sources.filter(source => source.version <= previous) : sources);
          sql(db, fixture);
          if (previous) apply(sources.filter(source => source.version > previous));
          sql(db, `
            SELECT schema26_assert(NOT EXISTS(
              SELECT 1 FROM schema26_historical h FULL JOIN schema26_current_history c USING(table_name,row_key)
              WHERE h.data IS DISTINCT FROM c.data), 'historical PM/public rows, users, relationships, and read cursors unchanged');
            SELECT schema26_assert((SELECT COALESCE(array_agg(message_id ORDER BY message_id),ARRAY[]::BIGINT[])
              FROM pm_read_receipts WHERE reader_id=1)=${previous ? 'ARRAY[953,1000,1002]::BIGINT[]' : 'ARRAY[]::BIGINT[]'}, 'visible committed incoming backfill only');
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id<>1), 'backfill excludes outside memberships and zero cursors');
            UPDATE lazer_channel_members SET last_read_id=last_read_id WHERE user_id=1;
            UPDATE lazer_channel_members SET last_read_id=last_read_id WHERE user_id=6;
            SELECT schema26_assert((SELECT array_agg(message_id ORDER BY message_id) FROM pm_read_receipts WHERE reader_id=1)
              =ARRAY[953,1000,1002]::BIGINT[], 'equal-marker update records only accessible visible incoming PM rows');
            SELECT schema26_assert((SELECT count(*) FROM pm_read_receipts)=3, 'own, public, restricted, both block directions, peer access, and outside membership excluded');
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE read_at IS NULL), 'receipt timestamp populated');
            CREATE TABLE schema26_initial_receipts AS SELECT * FROM pm_read_receipts;
            UPDATE lazer_channel_members SET last_read_id=1002 WHERE user_id=1 AND channel_id=100;
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM schema26_initial_receipts h
              FULL JOIN pm_read_receipts r USING(reader_id,message_id) WHERE h.read_at IS DISTINCT FROM r.read_at), 'same-marker idempotency preserves receipt timestamp');
            INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(999,100,2,'Lower ID committed after the read marker');
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=999), 'message insertion alone is not a read');
            UPDATE lazer_channel_members SET last_read_id=1002 WHERE user_id=1 AND channel_id=100;
            SELECT schema26_assert((SELECT array_agg(message_id ORDER BY message_id) FROM pm_read_receipts WHERE reader_id=1)
              =ARRAY[953,999,1000,1002]::BIGINT[], 'same marker records newly committed lower-ID row');
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM schema26_initial_receipts h JOIN pm_read_receipts r USING(reader_id,message_id)
              WHERE h.read_at IS DISTINCT FROM r.read_at), 'late row preserves previous receipt timestamps');
            UPDATE lazer_channel_members SET last_read_id=1000 WHERE user_id=1 AND channel_id=100 AND last_read_id<=1000;
            SELECT schema26_assert((SELECT last_read_id=1002 FROM lazer_channel_members WHERE user_id=1 AND channel_id=100), 'guarded older marker remains monotonic');
            INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(998,100,2,'Late row during a decreasing marker update');
            UPDATE lazer_channel_members SET last_read_id=1001 WHERE user_id=1 AND channel_id=100;
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=998), 'decreasing raw marker emits no receipt');
            UPDATE lazer_channel_members SET last_read_id=1002 WHERE user_id=1 AND channel_id=100;
            SELECT schema26_assert(EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=998), 'restored marker records visible committed late row');
            UPDATE lazer_channel_members SET last_read_id=1003 WHERE user_id=1 AND channel_id=100 AND last_read_id<=1003;
            SELECT schema26_assert(EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=1003), 'forward marker records incoming row');
            INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(1004,100,2,'Committed row with rolled-back read');
            BEGIN;
              UPDATE lazer_channel_members SET last_read_id=1004 WHERE user_id=1 AND channel_id=100;
              SELECT schema26_assert(EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=1004), 'receipt appears inside marker transaction');
            ROLLBACK;
            SELECT schema26_assert((SELECT last_read_id=1003 FROM lazer_channel_members WHERE user_id=1 AND channel_id=100)
              AND NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=1004), 'marker and receipt roll back atomically');
            BEGIN;
              INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(997,100,2,'Rolled-back lower-ID row');
              UPDATE lazer_channel_members SET last_read_id=1003 WHERE user_id=1 AND channel_id=100;
              SELECT schema26_assert(EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=997), 'same-transaction inserted row can be read');
            ROLLBACK;
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM chat_messages WHERE id=997)
              AND NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE message_id=997), 'message and receipt rollback together');
            CREATE TABLE schema26_retained_receipts AS SELECT * FROM pm_read_receipts;
            DELETE FROM lazer_channel_members WHERE user_id=1 AND channel_id=100;
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM schema26_retained_receipts h
              FULL JOIN pm_read_receipts r USING(reader_id,message_id) WHERE h.read_at IS DISTINCT FROM r.read_at), 'membership deletion retains read rows');
            INSERT INTO lazer_channel_members(user_id,channel_id) VALUES(1,100);
            SELECT schema26_assert((SELECT last_read_id=0 FROM lazer_channel_members WHERE user_id=1 AND channel_id=100), 'rejoined membership starts with existing default marker');
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM schema26_retained_receipts h
              FULL JOIN pm_read_receipts r USING(reader_id,message_id) WHERE h.read_at IS DISTINCT FROM r.read_at), 'membership rejoin retains receipts without invented reads');
            SELECT schema26_assert((SELECT array_agg(m.id ORDER BY m.id) FROM chat_messages m JOIN users sender ON sender.id=m.user_id
              WHERE m.channel_id=100 AND m.user_id<>1 AND NOT sender.is_restricted
              AND NOT EXISTS(SELECT 1 FROM user_relationships b WHERE b.relation_type='block'
                AND ((b.user_id=1 AND b.target_id=m.user_id) OR (b.target_id=1 AND b.user_id=m.user_id)))
              AND NOT EXISTS(SELECT 1 FROM pm_read_receipts r WHERE r.reader_id=1 AND r.message_id=m.id))
              =ARRAY[1004]::BIGINT[], 'receipt anti-join preserves unread row across rejoin and lower-ID commits');
            DELETE FROM chat_messages WHERE id=999;
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE message_id=999), 'message deletion cascades its receipt');
            SELECT schema26_assert(EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=1 AND message_id=1000), 'message cascade preserves unrelated receipt');
            INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(1600,106,7,'Reader foreign-key cascade fixture');
            UPDATE lazer_channel_members SET last_read_id=1600 WHERE user_id=8 AND channel_id=106;
            SELECT schema26_assert(EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=8 AND message_id=1600), 'second reader receipt recorded');
            INSERT INTO pm_read_receipts(reader_id,message_id) VALUES(8,1000);
            DELETE FROM users WHERE id=8;
            SELECT schema26_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE reader_id=8), 'reader deletion cascades receipts');
          `);
          for (const statement of [
            'INSERT INTO pm_read_receipts(reader_id,message_id) VALUES(99999,1000)',
            'INSERT INTO pm_read_receipts(reader_id,message_id) VALUES(1,99999)',
          ]) sql(db, `DO $$ BEGIN BEGIN ${statement}; RAISE EXCEPTION 'Expected foreign-key rejection';
            EXCEPTION WHEN foreign_key_violation THEN NULL; END; END $$;`);
          unchangedSources();
          Object.assign(caseReceipt, { passed: true, negative_cases: 2,
            checks: ['historical rows/cursors preserved', 'visible incoming backfill', 'reader/peer and sender visibility',
              'own/public/nonparticipant exclusion', 'equal-marker insertion and timestamp idempotency', 'late committed lower-ID rows',
              'guarded monotonic marker and decreasing-update exclusion', 'marker/message/receipt rollback atomicity',
              'membership delete/rejoin retention', 'receipt anti-join unread rows', 'message/reader FK cascade and rejection'] });
        } catch (error) {
          caseReceipt.failure = { name: error.name, message: error.message };
          throw error;
        } finally {
          receipt.cases.push(caseReceipt);
          save();
        }
      t.diagnostic(`${previous === 0 ? 'Fresh installation' : `Upgrade from ${previous}`}: passed`);
    }
    receipt.outcome = receipt.cases.length === 3 && receipt.cases.every(entry => entry.passed) ? 'passed' : 'failed';
  } catch (error) {
    receipt.outcome = 'failed';
    receipt.failure = { name: error.name, message: error.message };
    throw error;
  } finally {
    let cleanupError;
    if (creationAttempted && !created) {
      try {
        const current = docker(['inspect', name], undefined, false);
        if (current.status === 0) {
          containerId = JSON.parse(current.stdout)[0].Id;
          created = true;
        } else assert.match(current.stderr, /No such (?:object|container)/, 'verify unique container is absent after failed creation');
      } catch (error) {
        receipt.outcome = 'failed';
        receipt.cleanup = { owned_container_absent: false, failure: { name: error.name, message: error.message } };
        cleanupError = error;
      }
    }
    if (created) {
      try {
        ownedContainer(JSON.parse(docker(['inspect', name]).stdout)[0]);
        docker(['rm', '-f', containerId]);
        assert.equal(docker(['ps', '-aq', '--filter', `label=${label}=${owner}`]).stdout.trim(), '');
        receipt.cleanup = { owned_container_absent: true, volumes_created: 0, networks_created: 0, ports_published: 0 };
      } catch (error) {
        receipt.outcome = 'failed';
        receipt.cleanup = { owned_container_absent: false, failure: { name: error.name, message: error.message } };
        cleanupError = error;
      }
    } else if (!cleanupError) receipt.cleanup = { owned_container_absent: true, container_created: false, volumes_created: 0, networks_created: 0, ports_published: 0 };
    receipt.finished_at = new Date().toISOString();
    save();
    t.diagnostic(`Receipt: ${path.relative(workspace, path.join(artifact, 'verification.json'))}`);
    if (cleanupError) throw cleanupError;
  }
});
