import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const filename = fileURLToPath(import.meta.url);
const workspace = path.resolve(path.dirname(filename), '../..');
const image = 'sha256:1bea307dfb3ee30541a7acf7de14b58bcd6948da98e5d31a04c627c4d35ec64b';
const label = 'org.illyaverse.schema27.owner';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = readdirSync(path.join(workspace, 'migrations')).filter(name => /^\d{4}_.*\.sql$/.test(name))
  .sort().map(name => ({ name, version: Number(name.slice(0, 4)), bytes: readFileSync(path.join(workspace, 'migrations', name)) }));
// Original LF bytes frozen at the schema-25 release. Keep this gate independent of private receipts.
const historicalSha256 = [
  'f55df89df34e50e8e3b939a7710b5dfd91db20a49015058b9ee19f3a6dbd2815',
  'df2526a06662891b50336dd9a37e9fb03089ca720087c71b3e4829900484a938',
  'dadbb6449a438bae04f76e03370630988f0fe699dbbd0eae4ee72f48da46cbfc',
  '48661d44d28b2b311e064037afc321da25ff51ca3596ca0d30703dc2cf0bef68',
  '5f52c084e54bde2f5ee5118b12b270f023493f218bd62f97904cfbf6abdda59e',
  '16dbbbc9edf9ae94fa546b06b146a37b38718c2946a6907afef4ce2424a8fbff',
  '8e5be2e5971a089d8d9959294d1355e7d68ffcabaacaece083107db7cc19e219',
  '9d9381d396706a939feff55d770f1bc28776425319875dae0a65ae11dd8616ae',
  '4735145e22a70147b1957dc2460fca01d3f61c59dd2ccb27ce85c72fd796e305',
  '5dee0014d2b2ce53d02b0603a3da02a4956ea57c5fd1dec71f1c6f9ffe29eb24',
  '013ddbe4dec6c30120ce1b66bc5bb3f5445650f68cb642d622301f57fecaa463',
  'caf2f00542757cc248fba1bf11faba76d87c867d283d6b2105910899c74395d2',
  'fee4688dc7d9cc0172a8c8dd8b76a08b96e3bc8412bf838993c242287ed7a3dd',
  '1c13658ee9b10aa58df88c1fc0fe8a041720e31dc52e5fa75f7861d79d196598',
  'c8bdda87ccac9706a823424a5d55f202373cb936b787d6c2883b7905985c4128',
  'ae6c6d425b24ed9a38769ac24b7a9e94ce84b588d20c7c978b506adb275225a0',
  '1672325ad2a21c3b21075f205a671d2e5086e2995bd0a0d31b1cd5c347864119',
  '60c3cb1644f77273221c28f032191a371af87c81afa28a236726ae444b90750e',
  '7f261b922d22617061207c92f90ab146fc76b99473cc6de1990492491d780fbf',
  'a4ce65e2b048a863e39fd993fbb8750cd8414eb7b77fb4fd945ff6109550b6cf',
  '385a823f225bf9283908d300d1ca362cc32f80f1bb0c1489baa0195532fc21db',
  '29ff415d96e5d7f41614bde4b90da9c5b7adbe4e8d0cc0d51aff09550c86cc68',
  '3ee1200c57665dcd0ff110c9456a9449fdd8155025eb7194227575c6c1646584',
  '1a982c8598f8c9ae586820d872c66e31003bb1538dd8ae7b16653f820c337a90',
  'da059622057d1df2a44e304242cca78cb06c403e7b05671e091d1b6cae46e0fb',
];

const fixture = `
  CREATE FUNCTION schema27_assert(condition BOOLEAN, detail TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
    BEGIN IF condition IS NOT TRUE THEN RAISE EXCEPTION 'schema27 assertion: %', detail; END IF; END $$;
  INSERT INTO users(id,username,email,password_hash,password_md5_hash,is_restricted)
    SELECT id,'Schema27User'||id,'schema27-'||id||'@example.invalid','fixture','fixture',id=3
    FROM generate_series(1,4) id;
  INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES(1,4,'block');
  INSERT INTO channels(id,name,kind,required_permission,dm_user_low,dm_user_high) VALUES
    (100,'schema27-pm','PM','chat.pm',1,2),
    (101,'schema27-public','PUBLIC',NULL,NULL,NULL),
    (102,'schema27-restricted-peer','PM','chat.pm',1,3);
  INSERT INTO lazer_channel_members(user_id,channel_id,last_read_id) VALUES(1,100,20),(1,101,50),(1,102,60);
  INSERT INTO chat_messages(id,channel_id,user_id,content,created_at) VALUES
    (9,100,1,'Own retained PM','2020-01-01'),(10,100,2,'Read retained PM','2020-01-02'),
    (11,100,3,'Restricted sender','2020-01-03'),(12,100,4,'Blocked sender','2020-01-04'),
    (20,100,2,'Read marker retained PM','2020-01-05'),(21,100,2,'Unread retained PM','2020-01-06'),
    (50,101,2,'Public message','2020-01-07'),(60,102,3,'Restricted peer message','2020-01-08');
  INSERT INTO protocol_private_messages(id,sender_id,recipient_id,content,created_at,client_uuid)
    VALUES(100,1,2,'Retained bridge message','2020-01-01','schema27-retained-bridge');
  INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path)
    VALUES(101,100,'Historical official','Fixture','Fixture','Official',0,'pending','official',1,
      '11111111111111111111111111111111','official-101.osu');
  INSERT INTO beatmaps(title,artist,creator,version,mode,status,source,stars,md5,osu_path)
    VALUES('Historical local','Fixture','Fixture','Local',0,'pending','local',1,
      '22222222222222222222222222222222','local.osu');
  CREATE VIEW schema27_pm_history AS
    SELECT 'users'::TEXT table_name,id::TEXT row_key,to_jsonb(u) data FROM users u
    UNION ALL SELECT 'channels',id::TEXT,to_jsonb(c) FROM channels c
    UNION ALL SELECT 'chat_messages',id::TEXT,to_jsonb(m) FROM chat_messages m
    UNION ALL SELECT 'protocol_private_messages',id::TEXT,to_jsonb(p) FROM protocol_private_messages p
    UNION ALL SELECT 'user_relationships',user_id||':'||target_id||':'||relation_type,to_jsonb(r) FROM user_relationships r
    UNION ALL SELECT 'lazer_channel_members',user_id||':'||channel_id,to_jsonb(m) FROM lazer_channel_members m;
  CREATE TABLE schema27_retained_pm AS SELECT * FROM schema27_pm_history;
  CREATE VIEW schema27_content_history AS
    SELECT 'beatmaps'::TEXT table_name,id::TEXT row_key,to_jsonb(b) data FROM beatmaps b
    UNION ALL SELECT 'beatmap_packages',id::TEXT,to_jsonb(p) FROM beatmap_packages p;
  CREATE TABLE schema27_retained_content AS SELECT * FROM schema27_content_history;
`;

const pmChecks = `
  SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_retained_pm h FULL JOIN schema27_pm_history c USING(table_name,row_key)
    WHERE h.data IS DISTINCT FROM c.data), 'PM messages, bridge, users, blocks, memberships and markers survive catalogue migration');
  SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_retained_content h FULL JOIN schema27_content_history c USING(table_name,row_key)
    WHERE h.data IS DISTINCT FROM c.data), 'original immutable map and package rows survive catalogue migration');
  SELECT schema27_assert((SELECT array_agg(message_id ORDER BY message_id) FROM pm_read_receipts WHERE reader_id=1)
    =ARRAY[10,20]::BIGINT[], 'PM26 visible incoming read receipts survive catalogue migration');
  SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_retained_receipts h FULL JOIN pm_read_receipts r USING(reader_id,message_id)
    WHERE h.read_at IS DISTINCT FROM r.read_at), 'catalogue migration preserves original read timestamps');
  INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(19,100,2,'Late lower-ID PM after catalogue migration');
  SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM pm_read_receipts WHERE message_id=19), 'late insertion is not an acknowledgement');
  UPDATE lazer_channel_members SET last_read_id=20 WHERE user_id=1 AND channel_id=100;
  SELECT schema27_assert((SELECT array_agg(message_id ORDER BY message_id) FROM pm_read_receipts WHERE reader_id=1)
    =ARRAY[10,19,20]::BIGINT[], 'PM26 equal marker acknowledges late committed row after catalogue migration');
  SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_retained_receipts h JOIN pm_read_receipts r USING(reader_id,message_id)
    WHERE h.read_at IS DISTINCT FROM r.read_at), 'equal marker retains earlier receipt timestamps');
  DELETE FROM lazer_channel_members WHERE user_id=1 AND channel_id=100;
  INSERT INTO lazer_channel_members(user_id,channel_id) VALUES(1,100);
  SELECT schema27_assert((SELECT last_read_id=0 FROM lazer_channel_members WHERE user_id=1 AND channel_id=100)
    AND (SELECT array_agg(message_id ORDER BY message_id) FROM pm_read_receipts WHERE reader_id=1)=ARRAY[10,19,20]::BIGINT[],
    'PM26 receipt retention and reset membership marker still compose with catalogue');
  SELECT schema27_assert((SELECT array_agg(m.id ORDER BY m.id) FROM chat_messages m JOIN users sender ON sender.id=m.user_id
    WHERE m.channel_id=100 AND m.user_id<>1 AND NOT sender.is_restricted
    AND NOT EXISTS(SELECT 1 FROM user_relationships b WHERE b.relation_type='block'
      AND ((b.user_id=1 AND b.target_id=m.user_id) OR (b.target_id=1 AND b.user_id=m.user_id)))
    AND NOT EXISTS(SELECT 1 FROM pm_read_receipts r WHERE r.reader_id=1 AND r.message_id=m.id))=ARRAY[21]::BIGINT[],
    'existing row-based unread predicate retains unread PM after rejoin');
`;

test('schema 27 composes catalogue storage and generation with PM26 on fresh installation and upgrades from 25/26', {
  skip: process.env.ILLYA_SCHEMA27_ALLOW_DISPOSABLE !== '1',
}, async t => {
  const owner = randomUUID();
  const name = `illya-schema27-${owner.replaceAll('-', '')}`;
  const artifact = path.join(workspace, '.artifacts', 'tests', `schema27-${new Date().toISOString().replaceAll(/[-:.]/g, '')}-${owner}`);
  mkdirSync(artifact, { recursive: true });
  const receipt = { version: 1, started_at: new Date().toISOString(), owner, image, container: name,
    gate_file: { path: 'infra/tests/schema27.test.mjs', sha256: digest(readFileSync(filename)) },
    historical_1_to_25_matches_frozen_lf: false,
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
    assert.equal(digest(readFileSync(filename)), receipt.gate_file.sha256, 'composition gate is unchanged while running');
    for (const source of sources) assert.equal(digest(readFileSync(path.join(workspace, 'migrations', source.name))), digest(source.bytes), `${source.name} is unchanged while running`);
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
    assert.deepEqual(sources.map(source => source.version), Array.from({ length: 27 }, (_, index) => index + 1), 'gate binds contiguous migrations 1..27');
    for (const source of sources) {
      assert.equal(source.bytes.includes(13), false, `${source.name} uses original LF bytes`);
      assert.equal(source.bytes.at(-1), 10, `${source.name} retains its final newline`);
      if (source.version <= 25) assert.equal(digest(source.bytes), historicalSha256[source.version - 1], `${source.name} matches frozen schema-25 bytes`);
    }
    receipt.historical_1_to_25_matches_frozen_lf = true;
    save();
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
    for (const previous of [0, 25, 26]) {
        const caseReceipt = { previous_version: previous, passed: false };
        try {
          unchangedSources();
          const db = `schema27_${previous}`;
          sql('postgres', `CREATE DATABASE ${db};`);
          sql(db, `CREATE TABLE _sqlx_migrations (
            version BIGINT PRIMARY KEY, description TEXT NOT NULL, installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
            success BOOLEAN NOT NULL, checksum BYTEA NOT NULL, execution_time BIGINT NOT NULL);`);
          // SQLx records SHA384 of the exact migration bytes, each migration committed with its ledger row.
          const apply = selected => sql(db, selected.map(source => `BEGIN;\n${source.bytes.toString('utf8')}\n
            INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time)
              VALUES(${source.version},'${source.name.slice(5, -4).replaceAll('_', ' ')}',TRUE,
                decode('${createHash('sha384').update(source.bytes).digest('hex')}','hex'),0);\nCOMMIT;`).join('\n'));
          apply(sources.filter(source => source.version <= (previous || 27)));
          if (previous) {
            sql(db, fixture);
            sql(db, 'CREATE TABLE schema27_previous_ledger AS SELECT * FROM _sqlx_migrations;');
            if (previous === 25) apply(sources.filter(source => source.version === 26));
            else sql(db, 'UPDATE lazer_channel_members SET last_read_id=last_read_id WHERE user_id=1;');
            sql(db, 'CREATE TABLE schema27_retained_receipts AS SELECT * FROM pm_read_receipts;');
            apply(sources.filter(source => source.version === 27));
          }
          assert.equal(sql(db, 'SELECT epoch FROM server_catalogue_revision WHERE singleton;'), '0', 'new catalogue generation starts at zero without touching retained rows');
          if (!previous) {
            sql(db, fixture);
            sql(db, `UPDATE lazer_channel_members SET last_read_id=last_read_id WHERE user_id=1;
              CREATE TABLE schema27_retained_receipts AS SELECT * FROM pm_read_receipts;`);
          }
          const ledger = JSON.parse(sql(db, `SELECT json_agg(json_build_object('version',version,'success',success,
            'checksum',encode(checksum,'hex')) ORDER BY version) FROM _sqlx_migrations;`));
          assert.deepEqual(ledger, sources.map(source => ({ version: source.version, success: true,
            checksum: createHash('sha384').update(source.bytes).digest('hex') })), 'all 27 SQLx-compatible successful ledger checksums match exact source bytes');
          if (previous) sql(db, `SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_previous_ledger h
            FULL JOIN (SELECT * FROM _sqlx_migrations WHERE version<=${previous}) c USING(version)
            WHERE to_jsonb(h) IS DISTINCT FROM to_jsonb(c)), 'upgrade retains original SQLx ledger rows');`);
          sql(db, pmChecks);
          sql(db, 'CREATE TABLE schema27_receipts_after_pm AS SELECT * FROM pm_read_receipts;');
          const epoch = () => Number(sql(db, 'SELECT epoch FROM server_catalogue_revision WHERE singleton;'));
          const advances = (description, statement, expectedDelta = 1) => {
            const before = epoch();
            sql(db, statement);
            assert.equal(epoch() - before, expectedDelta, description);
          };
          advances('metadata publication invalidates native continuation', `
            INSERT INTO beatmap_catalogue_metadata(native_set_id,metadata,revision)
              VALUES(100,'{"title":"Verified title","beatmaps":[{"id":101}]}',900);
            SELECT schema27_assert((SELECT revision=0 FROM beatmap_catalogue_metadata WHERE native_set_id=100), 'metadata insert revision cannot be forged');
            CREATE TABLE schema27_initial_metadata AS SELECT * FROM beatmap_catalogue_metadata;`);
          advances('metadata reverification invalidates native continuation', `
            SELECT pg_sleep(0.01);
            UPDATE beatmap_catalogue_metadata SET revision=900,created_at='2000-01-01',verified_at='2000-01-01' WHERE native_set_id=100;
            SELECT schema27_assert((SELECT m.revision=0 AND m.created_at=h.created_at AND m.verified_at>h.verified_at
              FROM beatmap_catalogue_metadata m JOIN schema27_initial_metadata h USING(native_set_id)), 'equal metadata keeps revision/creation and refreshes verification time');`);
          advances('metadata edit invalidates native continuation', `
            UPDATE beatmap_catalogue_metadata SET metadata='{"title":"Changed title"}',revision=0 WHERE native_set_id=100;
            UPDATE beatmap_catalogue_metadata SET metadata='{"title":"Verified title","beatmaps":[{"id":101}]}',revision=0 WHERE native_set_id=100;
            SELECT schema27_assert((SELECT revision=2 FROM beatmap_catalogue_metadata WHERE native_set_id=100), 'metadata ABA advances revision without forged reset');`, 2);
          const rollbackEpoch = epoch();
          sql(db, `BEGIN;
            UPDATE beatmap_catalogue_metadata SET metadata='{"title":"Rolled back"}' WHERE native_set_id=100;
            SELECT schema27_assert((SELECT revision=3 FROM beatmap_catalogue_metadata WHERE native_set_id=100), 'metadata revision changes inside transaction');
            ROLLBACK;
            SELECT schema27_assert((SELECT revision=2 AND metadata->>'title'='Verified title' FROM beatmap_catalogue_metadata WHERE native_set_id=100), 'metadata update rolls back');`);
          assert.equal(epoch(), rollbackEpoch, 'catalogue generation rolls back with metadata');
          advances('per-difficulty override invalidates native continuation', `
            INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason) VALUES(101,100,'ranked',1,'Fixture authority');`);
          advances('map metadata refresh invalidates native continuation', `
            UPDATE beatmaps SET title='Catalogue title refresh' WHERE native_id=101;
            SELECT schema27_assert((SELECT status='ranked' AND status_revision=1 FROM active_beatmaps WHERE native_id=101), 'existing per-difficulty authority survives catalogue triggers');`);
          advances('availability withdrawal invalidates native continuation', `
            INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(100,TRUE,1,'Fixture withdrawal',1);
            SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM active_beatmaps WHERE native_set_id=100), 'withdrawal still excludes cached rows');`);
          advances('availability restoration invalidates native continuation', `
            UPDATE beatmapset_availability SET withdrawn=FALSE,revision=2,reason='Fixture restore' WHERE native_set_id=100;
            SELECT schema27_assert(EXISTS(SELECT 1 FROM active_beatmaps WHERE native_id=101 AND status='ranked'), 'restore retains private per-difficulty authority');`);
          advances('new map membership invalidates native continuation', `
            INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path)
              VALUES(102,101,'Temporary official','Fixture','Fixture','Temporary',0,'pending','official',1,'33333333333333333333333333333333','temporary.osu');`);
          advances('legacy source label refresh invalidates native continuation without replacing identity', `
            UPDATE beatmaps SET source='local' WHERE native_id=102;
            SELECT schema27_assert((SELECT official_id=102 AND official_set_id=101 AND native_id=102 AND native_set_id=101
              FROM beatmaps WHERE native_id=102), 'unarchived legacy source label does not replace immutable native identity');
            UPDATE beatmaps SET source='official' WHERE native_id=102;`, 2);
          advances('map removal invalidates native continuation', 'DELETE FROM beatmaps WHERE native_id=102;');
          advances('unused package publication invalidates native continuation', `
            INSERT INTO beatmap_packages(id,native_set_id,uploaded_by,kind,member_md5,legacy)
              VALUES('00000000-0000-0000-0000-000000000027',2000000027,1,'osu',ARRAY['44444444444444444444444444444444'],TRUE);`);
          advances('unused package removal invalidates native continuation', `DELETE FROM beatmap_packages WHERE id='00000000-0000-0000-0000-000000000027';`);
          const archiveEpoch = epoch();
          advances('verified archive and membership invalidate native continuation', `BEGIN;
            INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by)
              VALUES(100,'00000000-0000-0000-0000-000000000100','official-package-00000000-0000-0000-0000-000000000100/package.osz',repeat('a',64),1,ARRAY['11111111111111111111111111111111'],1);
            SELECT schema27_assert((SELECT epoch=${archiveEpoch + 1} FROM server_catalogue_revision WHERE singleton), 'archive publication advances catalogue generation');
            INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id)
              SELECT '00000000-0000-0000-0000-000000000100',id FROM beatmaps WHERE native_id=101;
            SELECT schema27_assert((SELECT epoch=${archiveEpoch + 2} FROM server_catalogue_revision WHERE singleton), 'archive membership publication advances catalogue generation');
            SET CONSTRAINTS ALL IMMEDIATE;
            COMMIT;`, 2);
          const neutralEpoch = epoch();
          sql(db, `
            INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp)
              SELECT 1,id,0,100,100,1,1 FROM beatmaps WHERE native_id=101;
            INSERT INTO chat_messages(id,channel_id,user_id,content) VALUES(22,100,2,'Activity does not change catalogue generation');
            INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata)
              VALUES(100,'official',1,'Private nomination does not publish metadata','{"private":"fixture"}');
            SELECT schema27_assert((SELECT metadata->>'title'='Verified title' AND NOT metadata ? 'private'
              FROM beatmap_catalogue_metadata WHERE native_set_id=100), 'private nomination metadata remains separate from public catalogue metadata');`);
          assert.equal(epoch(), neutralEpoch, 'score, PM activity and private nomination do not invalidate catalogue generation');
          const rejectionCases = [
            ["INSERT INTO beatmap_catalogue_metadata(native_set_id,metadata) VALUES(0,'{}')", '23514'],
            ["INSERT INTO beatmap_catalogue_metadata(native_set_id,metadata) VALUES(2000000000,'{}')", '23514'],
            ["INSERT INTO beatmap_catalogue_metadata(native_set_id,metadata) VALUES(999,'[]')", '23514'],
            ["INSERT INTO beatmap_catalogue_metadata(native_set_id,metadata) VALUES(999,jsonb_build_object('title',repeat('x',4194304)))", '23514'],
            ["UPDATE beatmap_catalogue_metadata SET native_set_id=101 WHERE native_set_id=100", '23514'],
            ["INSERT INTO server_catalogue_revision(singleton) VALUES(FALSE)", '23514'],
            ["INSERT INTO server_catalogue_revision(singleton) VALUES(TRUE)", '23505'],
            ["UPDATE server_catalogue_revision SET epoch=-1 WHERE singleton", '23514'],
            ["UPDATE beatmaps SET official_set_id=101 WHERE native_id=101", '23514'],
            ["UPDATE beatmaps SET md5='55555555555555555555555555555555' WHERE native_id=101", '23514'],
            ["UPDATE beatmaps SET source='local' WHERE native_id=101", '23514'],
            ["UPDATE official_beatmapset_archives SET sha256=repeat('b',64) WHERE official_set_id=100", '23514'],
            ["DELETE FROM official_beatmapset_archive_members WHERE archive_id='00000000-0000-0000-0000-000000000100'", '23514'],
            ["DELETE FROM beatmapset_availability WHERE native_set_id=100", '23514'],
            ["INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,source,stars,md5,osu_path) VALUES(103,102,'Invalid source','Fixture','Fixture','Invalid',0,'invented',1,'66666666666666666666666666666666','invalid.osu')", '23514'],
          ];
          const rejectedEpoch = epoch();
          for (const [statement, code] of rejectionCases) sql(db, `DO $$ BEGIN BEGIN ${statement};
            RAISE EXCEPTION 'Expected storage rejection'; EXCEPTION WHEN SQLSTATE '${code}' THEN NULL; END; END $$;`);
          assert.equal(epoch(), rejectedEpoch, 'rejected storage or identity changes do not change catalogue generation');
          advances('metadata removal invalidates native continuation', 'DELETE FROM beatmap_catalogue_metadata WHERE native_set_id=100;');
          sql(db, `
            SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_retained_pm h
              LEFT JOIN schema27_pm_history c USING(table_name,row_key)
              WHERE NOT(h.table_name='lazer_channel_members' AND h.row_key='1:100') AND h.data IS DISTINCT FROM c.data), 'catalogue writes preserve original PM/public/bridge history and unchanged memberships');
            SELECT schema27_assert(NOT EXISTS(SELECT 1 FROM schema27_receipts_after_pm h
              FULL JOIN pm_read_receipts r USING(reader_id,message_id) WHERE h.read_at IS DISTINCT FROM r.read_at), 'catalogue writes preserve durable PM receipt rows and timestamps');
            SELECT schema27_assert((SELECT last_read_id=0 FROM lazer_channel_members WHERE user_id=1 AND channel_id=100), 'catalogue writes preserve reset rejoin marker');`);
          const tableCounts = JSON.parse(sql(db, `SELECT json_build_object(
            'sqlx_migrations',(SELECT count(*) FROM _sqlx_migrations),
            'pm_read_receipts',(SELECT count(*) FROM pm_read_receipts),
            'chat_messages',(SELECT count(*) FROM chat_messages),
            'lazer_channel_members',(SELECT count(*) FROM lazer_channel_members),
            'beatmaps',(SELECT count(*) FROM beatmaps),
            'beatmap_packages',(SELECT count(*) FROM beatmap_packages),
            'beatmap_catalogue_metadata',(SELECT count(*) FROM beatmap_catalogue_metadata),
            'official_beatmapset_archives',(SELECT count(*) FROM official_beatmapset_archives),
            'official_beatmapset_archive_members',(SELECT count(*) FROM official_beatmapset_archive_members),
            'scores',(SELECT count(*) FROM scores),
            'beatmap_rank_requests',(SELECT count(*) FROM beatmap_rank_requests),
            'catalogue_epoch',(SELECT epoch FROM server_catalogue_revision WHERE singleton));`));
          unchangedSources();
          Object.assign(caseReceipt, { passed: true, sqlx_ledger: ledger, table_counts: tableCounts, negative_cases: rejectionCases.length,
            checks: ['contiguous original LF migration bytes and SQLx SHA384 ledger', 'retained PM/member/message/read timestamps',
              'PM26 equal marker and rejoin unread behavior after 27', 'retained map/package identities',
              'metadata revision/verification/identity and bounded official storage', 'catalogue epoch across seven publication tables',
              'metadata/generation rollback atomicity', 'status and availability authority composition',
              'score/chat/private nomination generation neutrality', 'source/immutable archive and storage rejection'] });
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
