import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const image = 'sha256:1bea307dfb3ee30541a7acf7de14b58bcd6948da98e5d31a04c627c4d35ec64b';
const label = 'org.illyaverse.schema25.owner';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const sources = readdirSync(path.join(workspace, 'migrations')).filter(name => /^\d{4}_.*\.sql$/.test(name))
  .sort().map(name => ({ name, version: Number(name.slice(0, 4)), bytes: readFileSync(path.join(workspace, 'migrations', name)) }));

test('schema 25 fresh and retained-version upgrades preserve per-difficulty authority and CAS revisions', {
  skip: process.env.ILLYA_SCHEMA25_ALLOW_DISPOSABLE !== '1',
}, async t => {
  assert.equal(sources.at(-1).version, 25, 'this gate binds exactly migration 25');
  assert.equal(sources.length, 25);
  const owner = randomUUID();
  const name = `illya-schema25-${owner.replaceAll('-', '')}`;
  const artifact = path.join(workspace, '.artifacts', 'tests', `schema25-${new Date().toISOString().replaceAll(/[-:.]/g, '')}-${owner}`);
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
  let containerId;
  const sql = (db, text) => docker(['exec', '-i', name, 'psql', '-X', '-v', 'ON_ERROR_STOP=1', '-U', 'postgres', '-d', db, '-Atq'], text).stdout.trim();
  function unchangedSources() {
    for (const source of sources) assert.equal(digest(readFileSync(path.join(workspace, 'migrations', source.name))), digest(source.bytes));
  }
  try {
    assert.equal(docker(['image', 'inspect', image, '--format', '{{.Id}}']).stdout.trim(), image);
    containerId = docker(['run', '-d', '--pull=never', '--name', name, '--label', `${label}=${owner}`, '--network', 'none',
      '--mount', 'type=tmpfs,destination=/var/lib/postgresql/data,tmpfs-size=268435456',
      '-e', `POSTGRES_PASSWORD=${randomBytes(32).toString('hex')}`, image]).stdout.trim();
    assert.match(containerId, /^[0-9a-f]{64}$/);
    created = true;
    const inspected = JSON.parse(docker(['inspect', name]).stdout)[0];
    assert.equal(inspected.Id, containerId);
    assert.equal(inspected.Image, image);
    assert.equal(inspected.HostConfig.NetworkMode, 'none');
    assert.equal(Object.keys(inspected.HostConfig.PortBindings ?? {}).length, 0);
    assert.equal(inspected.Mounts.length, 1);
    assert.equal(inspected.Mounts[0].Type, 'tmpfs');
    for (let attempt = 0; ; attempt++) {
      const ready = docker(['exec', name, 'pg_isready', '-U', 'postgres'], undefined, false);
      if (ready.status === 0) break;
      assert.ok(attempt < 45, 'owned PostgreSQL becomes ready');
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    for (const previous of [0, 22, 24]) {
      await t.test(previous === 0 ? 'fresh installation' : `upgrade from ${previous}`, () => {
        unchangedSources();
        const db = `schema25_${previous}`;
        sql('postgres', `CREATE DATABASE ${db};`);
        const apply = selected => sql(db, selected.map(source => source.bytes.toString('utf8')).join('\n'));
        if (previous) apply(sources.filter(source => source.version <= previous));
        if (!previous) apply(sources);
        sql(db, `CREATE FUNCTION schema25_assert(condition BOOLEAN, detail TEXT) RETURNS VOID LANGUAGE plpgsql AS $$
          BEGIN IF condition IS NOT TRUE THEN RAISE EXCEPTION 'schema25 assertion: %', detail; END IF; END $$;
          INSERT INTO users(id,username,email,password_hash,password_md5_hash) VALUES(1,'Schema25Fixture','schema25@example.invalid','fixture','fixture');
          INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path)
            SELECT 'Historical','Fixture','Fixture',state,0,state,1,lpad(to_hex(ordinal),32,'0'),'fixture-'||ordinal||'.osu'
            FROM unnest(ARRAY['pending','ranked','loved','unranked']) WITH ORDINALITY states(state,ordinal);
          CREATE TABLE schema25_historical AS SELECT id,native_id,native_set_id,status,md5,created_at FROM beatmaps;`);
        if (previous === 22) sql(db, `INSERT INTO roles(name,description,permissions)
          VALUES('beatmap_nominator','Existing custom role',ARRAY['social.manage']);`);
        if (previous) apply(sources.filter(source => source.version > previous));
        const checks = `
          SELECT schema25_assert(NOT EXISTS(SELECT 1 FROM schema25_historical h JOIN beatmaps b USING(id)
            WHERE (h.native_id,h.native_set_id,h.status,h.md5,h.created_at) IS DISTINCT FROM
              (b.native_id,b.native_set_id,b.status,b.md5,b.created_at)), 'historical rows unchanged');
          SELECT schema25_assert((SELECT bool_and(status_revision=0) FROM beatmaps), 'initial revisions zero');
          SELECT schema25_assert((SELECT permissions @> ARRAY['chat.send','beatmaps.upload','beatmaps.rank.request']
            AND NOT permissions && ARRAY['beatmaps.rank.review','*'] FROM roles WHERE name='player'), 'player request grants');
          SELECT schema25_assert((SELECT permissions @> ARRAY['beatmaps.rank.request','beatmaps.rank.review']
            AND NOT '*'=ANY(permissions) FROM roles WHERE name='moderator'), 'moderator explicit grants');
          SELECT schema25_assert((SELECT permissions=ARRAY['*'] FROM roles WHERE name='administrator'), 'admin unchanged');
          SELECT schema25_assert((SELECT permissions=${previous === 22 ? "ARRAY['social.manage']" : "ARRAY['chat.send','beatmaps.upload','beatmaps.import','beatmaps.moderate','beatmaps.rank.request','beatmaps.rank.review']"}
            FROM roles WHERE name='beatmap_nominator'), 'BN collision preserves custom grants');
          INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path,status_revision)
            VALUES(101,100,'Official','Fixture','Fixture','Selected',0,'pending','official',1,'11111111111111111111111111111111','selected.osu',90),
              (102,100,'Official','Fixture','Fixture','Sibling',0,'wip','official',1,'22222222222222222222222222222222','sibling.osu',90);
          INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,revision,actor_id,reason)
            VALUES(101,100,'ranked',70,1,'Selected difficulty only'),(103,100,'loved',70,1,'Future official difficulty');
          SELECT schema25_assert((SELECT bool_and(revision=1) FROM beatmap_status_overrides), 'override insert forgery neutralized');
          UPDATE beatmaps SET status='pending',status_revision=50 WHERE native_id IN(101,102);
          SELECT schema25_assert((SELECT status='ranked' AND status_revision=1 FROM beatmaps WHERE native_id=101), 'selected override only');
          SELECT schema25_assert((SELECT status='pending' AND status_revision=1 FROM beatmaps WHERE native_id=102), 'sibling keeps own transition');
          UPDATE beatmaps SET status='graveyard',status_revision=0 WHERE native_id=101;
          SELECT schema25_assert((SELECT status='ranked' AND status_revision=1 FROM beatmaps WHERE native_id=101), 'upstream update cannot undo private rank');
          INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path,status_revision)
            VALUES(103,100,'Official','Fixture','Fixture','Future',0,'graveyard','official',1,'33333333333333333333333333333333','future.osu',90);
          SELECT schema25_assert((SELECT status='loved' AND status_revision=0 FROM beatmaps WHERE native_id=103), 'future insert override after identity binding');
          UPDATE beatmap_status_overrides SET status='graveyard',revision=0 WHERE native_id=101;
          UPDATE beatmaps SET status='ranked',status_revision=0 WHERE native_id=101;
          UPDATE beatmap_status_overrides SET status='ranked',revision=0 WHERE native_id=101;
          UPDATE beatmaps SET status='graveyard',status_revision=0 WHERE native_id=101;
          SELECT schema25_assert((SELECT status='ranked' AND status_revision=3 FROM beatmaps WHERE native_id=101), 'map ABA increments');
          SELECT schema25_assert((SELECT status='ranked' AND revision=3 FROM beatmap_status_overrides WHERE native_id=101), 'override ABA increments');
          UPDATE beatmap_status_overrides SET revision=0,changed_at='2000-01-01' WHERE native_id=101;
          SELECT schema25_assert((SELECT revision=3 AND changed_at>'2000-01-01' FROM beatmap_status_overrides WHERE native_id=101), 'override reset neutralized');
          SELECT schema25_assert((SELECT status_revision=3 FROM active_beatmaps WHERE native_id=101), 'active view has revision');
          INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path)
            VALUES('Local','Fixture','Fixture','Graveyard',0,'graveyard',1,'44444444444444444444444444444444','local.osu');
          INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason)
            SELECT native_id,native_set_id,'wip',1,'Local identity' FROM beatmaps WHERE md5='44444444444444444444444444444444';
          UPDATE beatmaps SET title='Local refreshed' WHERE md5='44444444444444444444444444444444';
          SELECT schema25_assert((SELECT status='wip' AND status_revision=1 FROM beatmaps WHERE md5='44444444444444444444444444444444'), 'local native ID override');
          INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata,revision)
            VALUES(100,'official',1,'Please review selected maps','{"title":"Fixture","difficulties":[101,102]}',99);
          SELECT schema25_assert((SELECT revision=0 FROM beatmap_rank_requests WHERE id=1), 'request insert forgery neutralized');
          UPDATE beatmap_rank_requests SET revision=80,updated_at='2000-01-01' WHERE id=1;
          SELECT schema25_assert((SELECT revision=0 AND updated_at=created_at FROM beatmap_rank_requests WHERE id=1), 'request reset neutralized');
          UPDATE beatmap_rank_requests SET status='approved',approved_difficulty_ids=ARRAY[101]::BIGINT[],review_reason='Selected difficulty approved',reviewed_by=1,reviewed_at=clock_timestamp(),revision=0 WHERE id=1;
          SELECT schema25_assert((SELECT revision=1 AND approved_difficulty_ids=ARRAY[101]::BIGINT[] FROM beatmap_rank_requests WHERE id=1), 'approval subset stored');
          UPDATE beatmap_rank_requests SET status='pending',approved_difficulty_ids=NULL,revision=0 WHERE id=1;
          UPDATE beatmap_rank_requests SET status='approved',approved_difficulty_ids=ARRAY[101]::BIGINT[],revision=0 WHERE id=1;
          SELECT schema25_assert((SELECT revision=3 FROM beatmap_rank_requests WHERE id=1), 'request ABA increments');
          UPDATE beatmap_rank_requests SET reason='Updated request reason',revision=0 WHERE id=1;
          SELECT schema25_assert((SELECT revision=4 FROM beatmap_rank_requests WHERE id=1), 'reason change increments');
        `;
        sql(db, checks);
        const rejectionCases = [
          ["INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason) VALUES(104,100,'invented',1,'Invalid')", '23514'],
          ["INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason) VALUES(2000000900,2000000900,'ranked',1,'Unknown local')", '23514'],
          ["UPDATE beatmap_status_overrides SET native_set_id=99 WHERE native_id=101", '23514'],
          ["INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason) VALUES(102,999,'ranked',1,'Wrong known set')", '23514'],
          ["DELETE FROM beatmap_status_overrides WHERE native_id=101", '23514'],
          ["INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES(104,999,'Wrong','Fixture','Fixture','Wrong set',0,'pending','official',1,'55555555555555555555555555555555','wrong.osu')", '23514', "INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason) VALUES(104,100,'ranked',1,'Verified upstream set')"],
          ["UPDATE beatmap_rank_requests SET metadata='{}' WHERE id=1", '23514'],
          ["UPDATE beatmap_rank_requests SET native_set_id=101 WHERE id=1", '23514'],
          ["UPDATE beatmap_rank_requests SET approved_difficulty_ids=ARRAY[]::BIGINT[] WHERE id=1", '23514'],
          ["UPDATE beatmap_rank_requests SET approved_difficulty_ids=ARRAY[101,101]::BIGINT[] WHERE id=1", '23514'],
          ["UPDATE beatmap_rank_requests SET approved_difficulty_ids=ARRAY[NULL]::BIGINT[] WHERE id=1", '23514'],
          ["UPDATE beatmap_rank_requests SET approved_difficulty_ids=NULL WHERE id=1", '23514'],
          ["INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata) VALUES(999,'official',999,'Missing actor','{}')", '23503'],
        ];
        for (const [statement, code, setup] of rejectionCases) {
          if (setup) sql(db, setup);
          sql(db, `DO $$ BEGIN BEGIN ${statement}; RAISE EXCEPTION 'Expected rejection';
            EXCEPTION WHEN SQLSTATE '${code}' THEN NULL; END; END $$;`);
        }
        sql(db, `INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata) VALUES(100,'official',1,'A new review','{}');
          DO $$ BEGIN BEGIN INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata)
            VALUES(100,'official',1,'Duplicate pending','{}'); RAISE EXCEPTION 'Expected duplicate rejection';
            EXCEPTION WHEN unique_violation THEN NULL; END; END $$;
          INSERT INTO beatmaps(title,artist,creator,version,mode,stars,md5,osu_path)
            VALUES('Legacy default','Fixture','Fixture','Default',0,1,'66666666666666666666666666666666','default.osu');
          SELECT schema25_assert((SELECT status='pending' FROM beatmaps WHERE md5='66666666666666666666666666666666'), 'legacy INSERT default retained');`);
        unchangedSources();
        receipt.cases.push({ previous_version: previous, passed: true, negative_cases: rejectionCases.length + 1,
          checks: ['historical identities/statuses', 'least-privilege roles/custom BN collision', 'per-difficulty sibling independence',
            'future official insert and update', 'local native identity', 'status/override/request monotonic ABA revisions',
            'revision/timestamp forgery neutralization', 'approved selected subset', 'pending uniqueness', 'FK/identity/invalid array rejection', 'active view revision', 'legacy default'] });
        save();
      });
    }
    receipt.outcome = receipt.cases.length === 3 ? 'passed' : 'failed';
  } catch (error) {
    receipt.outcome = 'failed';
    receipt.failure = { name: error.name, message: error.message };
    throw error;
  } finally {
    if (created) {
      const current = JSON.parse(docker(['inspect', name]).stdout)[0];
      assert.equal(current.Id, containerId);
      assert.equal(current.Image, image);
      assert.equal(current.Config.Labels[label], owner);
      assert.equal(current.Name, `/${name}`);
      docker(['rm', '-f', containerId]);
      assert.equal(docker(['ps', '-aq', '--filter', `label=${label}=${owner}`]).stdout.trim(), '');
      receipt.cleanup = { owned_container_absent: true, volumes_created: 0, networks_created: 0, ports_published: 0 };
    }
    receipt.finished_at = new Date().toISOString();
    save();
    t.diagnostic(`Receipt: ${path.relative(workspace, path.join(artifact, 'verification.json'))}`);
  }
});
