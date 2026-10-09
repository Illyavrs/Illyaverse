import { createHash } from 'node:crypto';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { requireDisposable } from './protocol-smoke.mjs';
import {
  artifactNames, checkedDocker, cleanupTemp, exactMatch, nonce, ownedProject, ownedResource,
  ownedTempDirectory, ownerLabel, readSourceMigrations, validImage, verifyBackup,
} from './recovery-lib.mjs';
import { backupDeployment } from './recovery-backup.mjs';

export function drillArguments(args) {
  if (args.length === 0) return { dryRun: false };
  if (args.length === 1 && args[0] === '--dry-run') return { dryRun: true };
  throw new Error('Recovery drill accepts only --dry-run; projects, images, paths and Docker flags cannot be supplied.');
}

export function sanctuaryImageReference(value) {
  if (value === undefined) return 'illyaverse/sanctuary:local';
  if (!validImage(value)) throw new Error('An explicit recovery Sanctuary image must be an existing immutable sha256 image ID.');
  return value;
}

export function recoveryConfig({ project, owner, images, password, sessionSecret, internalKey, restore = false }) {
  ownedProject(project, owner);
  if (!validImage(images.postgres) || !validImage(images.sanctuary) ||
      [password, sessionSecret, internalKey].some(value => !exactMatch(/^[a-f0-9]{64}$/, value)) ||
      typeof restore !== 'boolean' || !project.endsWith(restore ? '-restore' : '-source'))
    throw new Error('Recovery config requires pinned local images and generated hex secrets.');
  const labels = { [ownerLabel]: owner };
  return {
    name: project,
    services: {
      postgres: {
        image: images.postgres, pull_policy: 'never', labels,
        environment: { POSTGRES_USER: 'illya', POSTGRES_DB: 'illyaverse', POSTGRES_PASSWORD: password, POSTGRES_INITDB_ARGS: '--auth-host=scram-sha-256 --data-checksums' },
        volumes: [{ type: 'volume', source: 'postgres_data', target: '/var/lib/postgresql/data' }],
        healthcheck: { test: ['CMD', 'pg_isready', '-U', 'illya', '-d', 'illyaverse'], interval: '2s', timeout: '3s', retries: 45 },
        networks: { default: null },
      },
      sanctuary: {
        image: images.sanctuary, pull_policy: 'never', labels, init: true,
        environment: {
          DATABASE_URL: `postgres://illya:${password}@postgres:5432/illyaverse`, SESSION_SECRET: sessionSecret, INTERNAL_SERVICE_KEY: internalKey,
          PUBLIC_ORIGIN: 'http://localhost:39999', COOKIE_SECURE: 'false', BIND_ADDRESS: '0.0.0.0:8080', TRUSTED_PROXY_CIDRS: '',
          MEDIA_ROOT: '/data/media', MAPS_ROOT: '/data/maps', REPLAY_ROOT: '/data/replays',
        },
        volumes: [{ type: 'volume', source: 'shared_data', target: '/data' }],
        depends_on: { postgres: { condition: 'service_healthy' } },
        healthcheck: { test: ['CMD', 'curl', '--fail', '--silent', '--max-time', '3', 'http://127.0.0.1:8080/health/ready'], interval: '2s', timeout: '5s', retries: 45 },
        security_opt: ['no-new-privileges:true'], cap_drop: ['ALL'], networks: { default: null },
      },
      archive: {
        image: images.sanctuary, pull_policy: 'never', labels, user: '0:0', entrypoint: ['sleep'], command: ['infinity'],
        volumes: [{ type: 'volume', source: 'shared_data', target: '/data', read_only: !restore }],
        network_mode: 'none', security_opt: ['no-new-privileges:true'],
      },
    },
    networks: { default: { name: `${project}_default`, internal: true, labels } },
    volumes: Object.fromEntries(['postgres_data', 'shared_data'].map(name => [name, { name: `${project}_${name}`, labels }])),
  };
}

const keys = value => Object.keys(value ?? {}).sort().join();
// Check the Docker-rendered config, not merely the file we intended to render.
export function validateRecoveryConfig(config, expected) {
  const project = expected.name;
  const owner = expected.networks.default.labels[ownerLabel];
  ownedProject(project, owner);
  if (config.name !== project || keys(config.services) !== 'archive,postgres,sanctuary' || keys(config.volumes) !== 'postgres_data,shared_data' || keys(config.networks) !== 'default')
    throw new Error('Unexpected recovery Compose project/resources.');
  for (const [name, volume] of Object.entries(config.volumes)) {
    if (volume.external || volume.driver || volume.driver_opts || volume.name !== `${project}_${name}` || volume.labels?.[ownerLabel] !== owner)
      throw new Error('Recovery data volumes must be fresh resources exclusively owned by this nonce.');
  }
  const network = config.networks.default;
  if (network.external || network.driver || network.driver_opts || keys(network.ipam) !== '' || network.name !== `${project}_default` || network.internal !== true || network.labels?.[ownerLabel] !== owner)
    throw new Error('Recovery network must be its own internal network.');
  for (const [name, service] of Object.entries(config.services)) {
    const reference = expected.services[name];
    const allowed = new Set([...Object.keys(reference), 'restart', 'working_dir', 'command', 'entrypoint']);
    if (Object.keys(service).some(key => !allowed.has(key)) || service.restart || service.working_dir ||
        service.image !== reference.image || !validImage(service.image) || service.pull_policy !== 'never' ||
        service.labels?.[ownerLabel] !== owner || service.network_mode !== reference.network_mode ||
        keys(service.networks) !== keys(reference.networks))
      throw new Error('Recovery services must use only pinned local images and their expected resources.');
    const actualMounts = service.volumes ?? [];
    if (actualMounts.length !== 1 || actualMounts[0].type !== 'volume' || actualMounts[0].source !== reference.volumes[0].source ||
        actualMounts[0].target !== reference.volumes[0].target || Boolean(actualMounts[0].read_only) !== Boolean(reference.volumes[0].read_only) ||
        actualMounts[0].volume?.subpath || actualMounts[0].volume?.nocopy)
      throw new Error('Recovery mounts must be only the expected project data volumes.');
    if (JSON.stringify(service.environment ?? {}) !== JSON.stringify(reference.environment ?? {})) {
      // Compose sorts environment keys; compare them independently of insertion order.
      if (keys(service.environment) !== keys(reference.environment) || Object.entries(reference.environment ?? {}).some(([key, value]) => service.environment[key] !== value))
        throw new Error('Recovery environment must use only generated secrets and this fresh database.');
    }
    for (const key of ['entrypoint', 'command', 'user', 'healthcheck', 'cap_drop', 'security_opt']) {
      if (key === 'healthcheck') {
        if (JSON.stringify(service.healthcheck?.test) !== JSON.stringify(reference.healthcheck?.test)) throw new Error('Unexpected recovery health command.');
      } else if (JSON.stringify(service[key] ?? undefined) !== JSON.stringify(reference[key] ?? undefined)) throw new Error(`Unexpected recovery service ${key}.`);
    }
  }
}

export async function requireFreshProject(docker, project, owner, signal) {
  ownedProject(project, owner);
  for (const args of [['ps', '-a', '-q'], ['volume', 'ls', '-q'], ['network', 'ls', '-q']]) {
    if ((await docker([...args, '--filter', `label=com.docker.compose.project=${project}`], signal)).trim())
      throw new Error('Refusing an existing recovery project. No populated project may be restored.');
  }
  const volumeNames = (await docker(['volume', 'ls', '--format', '{{.Name}}'], signal)).trim().split(/\r?\n/);
  if (['postgres_data', 'shared_data'].some(name => volumeNames.includes(`${project}_${name}`)))
    throw new Error('Refusing an existing recovery volume, even without Compose labels.');
  if ((await docker(['network', 'ls', '--format', '{{.Name}}'], signal)).trim().split(/\r?\n/).includes(`${project}_default`))
    throw new Error('Refusing an existing recovery network, even without Compose labels.');
}

export async function cleanupProject(docker, base, project, owner) {
  ownedProject(project, owner);
  for (const [kind, list] of [['container', ['ps', '-a', '-q', '--no-trunc']], ['volume', ['volume', 'ls', '-q']], ['network', ['network', 'ls', '-q']]]) {
    const ids = (await docker([...list, '--filter', `label=com.docker.compose.project=${project}`], null)).trim().split(/\s+/).filter(Boolean);
    for (const id of ids) {
      if (kind === 'container' && !/^[a-f0-9]{64}$/.test(id)) throw new Error('Unexpected recovery cleanup container ID.');
      const resource = JSON.parse(await docker([...(kind === 'container' ? ['inspect'] : [kind, 'inspect']), id], null))[0];
      ownedResource(resource, { project, owner, kind, name: resource.Labels?.['com.docker.compose.volume'] });
    }
  }
  // Compose also selects resources by their configured exact names. Audit those
  // names independently so a missing/replaced project label cannot bypass checks.
  const namedResources = {};
  for (const kind of ['volume', 'network']) namedResources[kind] = (await docker([kind, 'ls', '--format', '{{.Name}}'], null)).trim().split(/\r?\n/);
  for (const [kind, name, logical] of [['volume', `${project}_postgres_data`, 'postgres_data'], ['volume', `${project}_shared_data`, 'shared_data'], ['network', `${project}_default`, 'default']]) {
    if (namedResources[kind].includes(name)) {
      const resource = JSON.parse(await docker([kind, 'inspect', name], null))[0];
      ownedResource(resource, { project, owner, kind, name: logical });
    }
  }
  // Removal is permitted only after every resource selected by this project was audited.
  await docker([...base, 'down', '--volumes', '--timeout', '10'], null);
  await requireFreshProject(docker, project, owner, null);
}

const fixtureBytes = {
  'media/recovery.bin': Buffer.from([0, 255, 13, 10, 128, 1, 2, 3]),
  'maps/recovery.osu': Buffer.from('osu file format v14\n[Metadata]\nTitle:Recovery fixture\n'),
  'replays/recovery.osr': Buffer.from([0, 1, 2, 255, 254, 0, 128, 13, 10]),
};
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const fixtureSql = `BEGIN;
INSERT INTO users(username,email,password_hash,password_md5_hash,avatar_url,joined_at) VALUES ('recovery_fixture','recovery@example.test','synthetic-not-a-login','synthetic-not-a-login','/media/recovery.bin','2026-01-01T00:00:00Z');
INSERT INTO users(username,email,password_hash,password_md5_hash,joined_at) VALUES ('recovery_moderator','moderator@example.test','synthetic-not-a-login','synthetic-not-a-login','2026-01-01T00:00:00Z');
INSERT INTO user_roles SELECT u.id,r.id FROM users u CROSS JOIN roles r WHERE u.username='recovery_fixture' AND r.name='player';
INSERT INTO user_roles SELECT u.id,r.id FROM users u CROSS JOIN roles r WHERE u.username='recovery_moderator' AND r.name='moderator';
INSERT INTO sessions(id,user_id,expires_at,created_at) SELECT '11111111-1111-4111-8111-111111111111',id,'2099-01-01T00:00:00Z','2026-01-01T00:00:00Z' FROM users WHERE username='recovery_fixture';
UPDATE users SET chat_silenced_until=clock_timestamp()+interval '1 hour' WHERE username='recovery_fixture';
UPDATE users SET chat_silenced_until=chat_silenced_until+interval '1 hour' WHERE username='recovery_fixture';
INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at,last_chat_silence_until,created_at) SELECT '22222222-2222-4222-8222-222222222222','${sha256('synthetic recovery native token')}',id,'2099-01-01T00:00:00Z',chat_silenced_until,'2026-01-01T00:00:00Z' FROM users WHERE username='recovery_fixture';
INSERT INTO beatmaps(uploaded_by,title,artist,creator,version,mode,status,source,stars,md5,osu_path,created_at) SELECT id,'Recovery fixture','Synthetic','Operator','Fixture',0,'ranked','local',1,'0123456789abcdef0123456789abcdef','recovery.osu','2026-01-01T00:00:00Z' FROM users WHERE username='recovery_fixture';
INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp,verification,replay_path,replay_sha256,replay_format,replay_complete,created_at) SELECT u.id,b.id,0,123456,98.5,42,12.3,'pending','recovery.osr','${sha256(fixtureBytes['replays/recovery.osr'])}','legacy',true,'2026-01-01T00:00:00Z' FROM users u CROSS JOIN beatmaps b WHERE u.username='recovery_fixture' AND b.md5='0123456789abcdef0123456789abcdef';
UPDATE scores SET verification='rejected' WHERE user_id=(SELECT id FROM users WHERE username='recovery_fixture');
UPDATE scores SET verification='verified' WHERE user_id=(SELECT id FROM users WHERE username='recovery_fixture');
INSERT INTO audit_events(actor_id,action,target_type,target_id,details,created_at) SELECT a.id,'chat.silence','user',u.id::text,jsonb_build_object('previous',jsonb_build_object('chat_silenced_until',u.chat_silenced_until-interval '1 hour','chat_silence_revision',1),'new',jsonb_build_object('chat_silenced_until',u.chat_silenced_until,'chat_silence_revision',u.chat_silence_revision),'reason','Private synthetic recovery reason'),'2026-01-01T00:00:00Z' FROM users a CROSS JOIN users u WHERE a.username='recovery_moderator' AND u.username='recovery_fixture';
INSERT INTO audit_events(actor_id,action,target_type,target_id,details,created_at) SELECT a.id,'scores.verify','score',s.id::text,'{"previous":{"verification":"rejected","moderation_revision":1},"new":{"verification":"verified","moderation_revision":2}}'::jsonb,'2026-01-01T00:00:00Z' FROM users a CROSS JOIN scores s JOIN users u ON s.user_id=u.id WHERE a.username='recovery_moderator' AND u.username='recovery_fixture';
INSERT INTO chat_messages(channel_id,user_id,content,created_at) SELECT c.id,u.id,'Synthetic recovery fixture','2026-01-01T00:00:00Z' FROM users u CROSS JOIN channels c WHERE u.username='recovery_fixture' AND c.name='general';
-- Original synthetic official identities exercise per-difficulty decisions without
-- fetching an upstream map or touching a retained deployment. The original
-- nomination includes both difficulties; approval selects only the first.
INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,md5,osu_path,created_at)
    SELECT f.native_id,1900000010,u.id,'Recovery ranking fixture','Synthetic','Operator',f.version,0,'graveyard','official',1,f.md5,'recovery.osu','2026-01-01T00:00:00Z'
    FROM users u CROSS JOIN (VALUES (1900000001::bigint,'Selected','11111111111111111111111111111111'),(1900000002::bigint,'Sibling','22222222222222222222222222222222')) f(native_id,version,md5)
    WHERE u.username='recovery_fixture';
INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata)
    SELECT 1900000010,'official',u.id,'Private synthetic approved nomination',
        jsonb_build_object('title','Recovery ranking fixture','artist','Synthetic','creator','Operator','difficulty_count',2,'official_url',null,'upstream_status','graveyard',
            'manifest',(SELECT jsonb_agg(jsonb_build_object('native_id',b.native_id,'md5',b.md5,'version',b.version,'mode',b.mode,'stars',b.stars,'status',b.status) ORDER BY b.native_id) FROM beatmaps b WHERE b.native_set_id=1900000010))
    FROM users u WHERE u.username='recovery_fixture';
INSERT INTO audit_events(actor_id,action,target_type,target_id,details)
    SELECT r.requester_id,'beatmaps.rank_request.create','beatmap_rank_request',r.id::text,jsonb_build_object('native_set_id',r.native_set_id,'source',r.source,'reason',r.reason)
    FROM beatmap_rank_requests r WHERE r.reason='Private synthetic approved nomination';
INSERT INTO beatmap_status_overrides(native_id,native_set_id,status,actor_id,reason)
    SELECT 1900000001,1900000010,'pending',u.id,'Private synthetic preliminary difficulty decision' FROM users u WHERE u.username='recovery_moderator';
UPDATE beatmap_status_overrides SET status='ranked',reason='Private synthetic ranked difficulty decision' WHERE native_id=1900000001;
UPDATE beatmaps SET status='ranked' WHERE native_id=1900000001;
UPDATE beatmap_rank_requests SET status='approved',review_reason='Private synthetic selected approval',reviewed_by=(SELECT id FROM users WHERE username='recovery_moderator'),
    approved_difficulty_ids=ARRAY[1900000001::bigint],reviewed_at=clock_timestamp() WHERE reason='Private synthetic approved nomination';
INSERT INTO audit_events(actor_id,action,target_type,target_id,details)
    SELECT o.actor_id,'beatmaps.status','beatmap',b.id::text,jsonb_build_object('native_id',o.native_id,'native_set_id',o.native_set_id,'status',o.status,'reason',o.reason,'previous',jsonb_build_object('status','graveyard','status_revision',0))
    FROM beatmap_status_overrides o JOIN beatmaps b ON b.native_id=o.native_id WHERE o.native_id=1900000001;
INSERT INTO audit_events(actor_id,action,target_type,target_id,details)
    SELECT r.reviewed_by,'beatmaps.rank_request.review','beatmap_rank_request',r.id::text,jsonb_build_object('native_set_id',r.native_set_id,'decision',r.status,'difficulty_ids',r.approved_difficulty_ids,'reason',r.review_reason,'previous_revision',0)
    FROM beatmap_rank_requests r WHERE r.reason='Private synthetic approved nomination';
-- A later nomination stores only the still-unpublished sibling in its immutable
-- submission snapshot. It does not inherit the earlier approval's selection.
INSERT INTO beatmap_rank_requests(native_set_id,source,requester_id,reason,metadata)
    SELECT 1900000010,'official',u.id,'Private synthetic pending nomination',
        jsonb_build_object('title','Recovery ranking fixture','artist','Synthetic','creator','Operator','difficulty_count',1,'official_url',null,'upstream_status','graveyard',
            'manifest',(SELECT jsonb_agg(jsonb_build_object('native_id',b.native_id,'md5',b.md5,'version',b.version,'mode',b.mode,'stars',b.stars,'status',b.status) ORDER BY b.native_id) FROM beatmaps b WHERE b.native_id=1900000002))
    FROM users u WHERE u.username='recovery_fixture';
INSERT INTO audit_events(actor_id,action,target_type,target_id,details)
    SELECT r.requester_id,'beatmaps.rank_request.create','beatmap_rank_request',r.id::text,jsonb_build_object('native_set_id',r.native_set_id,'source',r.source,'reason',r.reason)
    FROM beatmap_rank_requests r WHERE r.reason='Private synthetic pending nomination';
COMMIT;`;

// Exercise the restored trigger behavior in one connection and always roll the
// successful probe back. A failed DO block closes its psql connection with an
// aborted transaction, which PostgreSQL also rolls back. No fixture changes or
// sequence allocations may escape this probe.
const moderationProbeSql = `BEGIN;
DO $$
DECLARE
    player_id bigint;
    score_id bigint;
    original_until timestamptz;
    revision bigint;
    current_until timestamptz;
    current_verification text;
BEGIN
    SELECT id,chat_silenced_until INTO STRICT player_id,original_until FROM users WHERE username='recovery_fixture';
    SELECT s.id INTO STRICT score_id FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.user_id=player_id AND b.md5='0123456789abcdef0123456789abcdef';
    IF original_until IS NULL OR original_until<=clock_timestamp()
        OR (SELECT chat_silence_revision FROM users WHERE id=player_id) IS DISTINCT FROM 2::bigint
        OR NOT EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=player_id AND last_chat_silence_until=original_until)
        OR NOT EXISTS(SELECT 1 FROM scores WHERE id=score_id AND verification='verified' AND moderation_revision=2)
        OR NOT EXISTS(SELECT 1 FROM roles WHERE name='moderator' AND 'chat.moderate'=ANY(permissions)) THEN
        RAISE EXCEPTION 'Restored moderation fixture or native silence cursor is incomplete';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM audit_events e JOIN users a ON a.id=e.actor_id WHERE a.username='recovery_moderator' AND e.action='chat.silence' AND e.target_type='user' AND e.target_id=player_id::text AND e.details->>'reason'='Private synthetic recovery reason' AND e.details->'new'->>'chat_silence_revision'='2')
        OR NOT EXISTS(SELECT 1 FROM audit_events e JOIN users a ON a.id=e.actor_id WHERE a.username='recovery_moderator' AND e.action='scores.verify' AND e.target_type='score' AND e.target_id=score_id::text AND e.details->'new'->>'moderation_revision'='2') THEN
        RAISE EXCEPTION 'Restored private moderation audit is incomplete';
    END IF;
    UPDATE users SET chat_silence_revision=0 WHERE id=player_id RETURNING chat_silence_revision,chat_silenced_until INTO revision,current_until;
    IF revision<>2 OR current_until IS DISTINCT FROM original_until THEN RAISE EXCEPTION 'Unchanged silence accepted a direct revision reset'; END IF;
    UPDATE users SET chat_silenced_until=original_until+interval '10 minutes',chat_silence_revision=99 WHERE id=player_id RETURNING chat_silence_revision INTO revision;
    IF revision<>3 THEN RAISE EXCEPTION 'Changed silence did not advance its durable revision'; END IF;
    UPDATE users SET chat_silenced_until=original_until,chat_silence_revision=0 WHERE id=player_id RETURNING chat_silence_revision INTO revision;
    IF revision<>4 THEN RAISE EXCEPTION 'Silence ABA did not advance its durable revision'; END IF;
    UPDATE users SET chat_silence_revision=99 WHERE id=player_id RETURNING chat_silence_revision INTO revision;
    IF revision<>4 THEN RAISE EXCEPTION 'Unchanged silence accepted a forged revision'; END IF;
    UPDATE scores SET moderation_revision=0 WHERE id=score_id RETURNING moderation_revision,verification INTO revision,current_verification;
    IF revision<>2 OR current_verification IS DISTINCT FROM 'verified' THEN RAISE EXCEPTION 'Unchanged score accepted a direct revision reset'; END IF;
    UPDATE scores SET verification='rejected',moderation_revision=99 WHERE id=score_id RETURNING moderation_revision INTO revision;
    IF revision<>3 THEN RAISE EXCEPTION 'Changed score did not advance its durable revision'; END IF;
    UPDATE scores SET verification='verified',moderation_revision=0 WHERE id=score_id RETURNING moderation_revision INTO revision;
    IF revision<>4 THEN RAISE EXCEPTION 'Score ABA did not advance its durable revision'; END IF;
    UPDATE scores SET moderation_revision=99 WHERE id=score_id RETURNING moderation_revision INTO revision;
    IF revision<>4 THEN RAISE EXCEPTION 'Unchanged score accepted a forged revision'; END IF;
END;
$$;
ROLLBACK;
SELECT 'recovery-moderation-probe-passed';`;

export async function verifyRestoredModeration(docker, base, { owner } = {}) {
  const project = ownedProject(base[base.indexOf('--project-name') + 1], owner);
  if (!project.endsWith('-restore')) throw new Error('Moderation behavior may be probed only in this owned restore project.');
  const result = await docker([...base, 'exec', '-T', 'postgres', 'psql', '-X', '-q', '-At', '--set', 'ON_ERROR_STOP=1', '-U', 'illya', '-d', 'illyaverse', '-c', moderationProbeSql]);
  if (result.trim() !== 'recovery-moderation-probe-passed') throw new Error('Restored moderation behavior probe did not complete its rollback.');
  return { activeSilence: true, nativeSilenceCursor: true, privateAudit: true, durableUserRevision: 2, durableScoreRevision: 2,
    unchangedRevisionWritesIgnored: true, changesAdvanceRevisions: true, abaAdvancesRevisions: true };
}

// Updates to requests and overrides do not allocate sequences. To exercise a
// stored map's override/status revision, use a metadata refresh: the restored
// BEFORE triggers apply its override, while the notification trigger (UPDATE OF
// status) is not selected. Every probe write remains inside the rolled-back
// transaction, including failed immutable-identity attempts in subtransactions.
const rankingProbeSql = `BEGIN;
DO $$
DECLARE
    approved_id bigint;
    pending_id bigint;
    approved_metadata jsonb;
    pending_metadata jsonb;
    original_created timestamptz;
    original_updated timestamptz;
    actual_revision bigint;
    map_revision bigint;
    changed_rows integer;
    rejected boolean;
BEGIN
    SELECT id,metadata,created_at,updated_at INTO STRICT approved_id,approved_metadata,original_created,original_updated
        FROM beatmap_rank_requests WHERE reason='Private synthetic approved nomination';
    SELECT id,metadata INTO STRICT pending_id,pending_metadata FROM beatmap_rank_requests WHERE reason='Private synthetic pending nomination';
    IF NOT EXISTS(SELECT 1 FROM beatmap_status_overrides WHERE native_id=1900000001 AND native_set_id=1900000010 AND status='ranked' AND revision=2 AND reason='Private synthetic ranked difficulty decision')
        OR NOT EXISTS(SELECT 1 FROM beatmaps WHERE native_id=1900000001 AND native_set_id=1900000010 AND status='ranked' AND status_revision=1)
        OR NOT EXISTS(SELECT 1 FROM beatmaps WHERE native_id=1900000002 AND native_set_id=1900000010 AND status='graveyard' AND status_revision=0)
        OR EXISTS(SELECT 1 FROM beatmap_status_overrides WHERE native_id=1900000002)
        OR NOT EXISTS(SELECT 1 FROM beatmap_rank_requests WHERE id=approved_id AND status='approved' AND revision=1 AND approved_difficulty_ids=ARRAY[1900000001::bigint] AND review_reason='Private synthetic selected approval' AND reviewed_by=(SELECT id FROM users WHERE username='recovery_moderator') AND reviewed_at IS NOT NULL)
        OR NOT EXISTS(SELECT 1 FROM beatmap_rank_requests WHERE id=pending_id AND status='pending' AND revision=0 AND approved_difficulty_ids IS NULL AND reviewed_by IS NULL)
        OR approved_metadata->>'difficulty_count' IS DISTINCT FROM '2'
        OR approved_metadata->'manifest' IS DISTINCT FROM '[{"native_id":1900000001,"md5":"11111111111111111111111111111111","version":"Selected","mode":0,"stars":1,"status":"graveyard"},{"native_id":1900000002,"md5":"22222222222222222222222222222222","version":"Sibling","mode":0,"stars":1,"status":"graveyard"}]'::jsonb
        OR pending_metadata->>'difficulty_count' IS DISTINCT FROM '1'
        OR pending_metadata->'manifest' IS DISTINCT FROM '[{"native_id":1900000002,"md5":"22222222222222222222222222222222","version":"Sibling","mode":0,"stars":1,"status":"graveyard"}]'::jsonb THEN
        RAISE EXCEPTION 'Restored per-difficulty ranking state or immutable selected manifest is incomplete';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM audit_events e JOIN users a ON a.id=e.actor_id WHERE a.username='recovery_fixture' AND e.action='beatmaps.rank_request.create' AND e.target_type='beatmap_rank_request' AND e.target_id=approved_id::text AND e.details->>'reason'='Private synthetic approved nomination')
        OR NOT EXISTS(SELECT 1 FROM audit_events e JOIN users a ON a.id=e.actor_id WHERE a.username='recovery_fixture' AND e.action='beatmaps.rank_request.create' AND e.target_type='beatmap_rank_request' AND e.target_id=pending_id::text AND e.details->>'reason'='Private synthetic pending nomination')
        OR NOT EXISTS(SELECT 1 FROM audit_events e JOIN users a ON a.id=e.actor_id WHERE a.username='recovery_moderator' AND e.action='beatmaps.rank_request.review' AND e.target_type='beatmap_rank_request' AND e.target_id=approved_id::text AND e.details->>'reason'='Private synthetic selected approval' AND e.details->>'decision'='approved' AND e.details->'difficulty_ids'='[1900000001]'::jsonb AND e.details->>'previous_revision'='0')
        OR NOT EXISTS(SELECT 1 FROM audit_events e JOIN users a ON a.id=e.actor_id JOIN beatmaps b ON b.id::text=e.target_id WHERE a.username='recovery_moderator' AND e.action='beatmaps.status' AND e.target_type='beatmap' AND b.native_id=1900000001 AND e.details->>'reason'='Private synthetic ranked difficulty decision' AND e.details->>'native_set_id'='1900000010' AND e.details->'previous'->>'status_revision'='0') THEN
        RAISE EXCEPTION 'Restored private ranking audit is incomplete';
    END IF;
    rejected := false;
    BEGIN
        UPDATE beatmap_status_overrides SET native_set_id=1900000011 WHERE native_id=1900000001;
    EXCEPTION WHEN check_violation THEN rejected := true;
    END;
    IF NOT rejected THEN RAISE EXCEPTION 'Difficulty override set identity accepted a mutation'; END IF;
    rejected := false;
    BEGIN
        DELETE FROM beatmap_status_overrides WHERE native_id=1900000001;
    EXCEPTION WHEN check_violation THEN rejected := true;
    END;
    IF NOT rejected THEN RAISE EXCEPTION 'Difficulty override accepted silent deletion'; END IF;
    rejected := false;
    BEGIN
        UPDATE beatmap_rank_requests SET metadata=jsonb_set(metadata,'{manifest}','[]'::jsonb) WHERE id=pending_id;
    EXCEPTION WHEN check_violation THEN rejected := true;
    END;
    IF NOT rejected THEN RAISE EXCEPTION 'Pending request accepted replacement of its selected manifest'; END IF;
    rejected := false;
    BEGIN
        UPDATE beatmap_rank_requests SET metadata=jsonb_set(metadata,'{manifest}','[]'::jsonb) WHERE id=approved_id;
    EXCEPTION WHEN check_violation THEN rejected := true;
    END;
    IF NOT rejected THEN RAISE EXCEPTION 'Approved request accepted replacement of its nominated manifest'; END IF;
    rejected := false;
    BEGIN
        UPDATE beatmap_rank_requests SET native_set_id=1900000011 WHERE id=pending_id;
    EXCEPTION WHEN check_violation THEN rejected := true;
    END;
    IF NOT rejected THEN RAISE EXCEPTION 'Request accepted a changed set identity'; END IF;
    UPDATE beatmap_status_overrides SET revision=0 WHERE native_id=1900000001 RETURNING revision INTO actual_revision;
    IF actual_revision<>2 THEN RAISE EXCEPTION 'Unchanged difficulty override accepted a revision reset'; END IF;
    UPDATE beatmaps SET stars=stars,status_revision=99 WHERE native_id=1900000001 RETURNING status_revision INTO map_revision;
    IF map_revision<>1 THEN RAISE EXCEPTION 'Unchanged stored map accepted a forged status revision'; END IF;
    UPDATE beatmap_status_overrides SET status='loved',revision=99 WHERE native_id=1900000001 RETURNING revision INTO actual_revision;
    IF actual_revision<>3 THEN RAISE EXCEPTION 'Changed difficulty override did not advance its revision'; END IF;
    UPDATE beatmaps SET stars=stars,status_revision=0 WHERE native_id=1900000001 RETURNING status_revision INTO map_revision;
    IF map_revision<>2 OR (SELECT status FROM beatmaps WHERE native_id=1900000001)<>'loved' THEN RAISE EXCEPTION 'Metadata refresh did not apply the restored override and advance the stored status revision'; END IF;
    UPDATE beatmap_status_overrides SET status='ranked',revision=0 WHERE native_id=1900000001 RETURNING revision INTO actual_revision;
    IF actual_revision<>4 THEN RAISE EXCEPTION 'Difficulty override ABA did not advance its revision'; END IF;
    UPDATE beatmaps SET stars=stars,status_revision=99 WHERE native_id=1900000001 RETURNING status_revision INTO map_revision;
    IF map_revision<>3 OR (SELECT status FROM beatmaps WHERE native_id=1900000001)<>'ranked' THEN RAISE EXCEPTION 'Stored status ABA did not advance its revision'; END IF;
    UPDATE beatmap_status_overrides SET revision=99 WHERE native_id=1900000001 RETURNING revision INTO actual_revision;
    IF actual_revision<>4 THEN RAISE EXCEPTION 'Unchanged difficulty override accepted a forged revision'; END IF;
    UPDATE beatmap_status_overrides SET status='loved' WHERE native_id=1900000001 AND revision=2 AND EXISTS(SELECT 1 FROM beatmaps WHERE native_id=1900000001 AND status_revision=3);
    GET DIAGNOSTICS changed_rows=ROW_COUNT;
    IF changed_rows<>0 THEN RAISE EXCEPTION 'Stale override revision passed the restored CAS predicate after ABA'; END IF;
    UPDATE beatmap_status_overrides SET status='loved' WHERE native_id=1900000001 AND revision=4 AND EXISTS(SELECT 1 FROM beatmaps WHERE native_id=1900000001 AND status_revision=1);
    GET DIAGNOSTICS changed_rows=ROW_COUNT;
    IF changed_rows<>0 THEN RAISE EXCEPTION 'Stale stored-map revision passed the restored CAS predicate after ABA'; END IF;
    UPDATE beatmap_status_overrides SET status='loved' WHERE native_id=1900000001 AND revision=4 AND EXISTS(SELECT 1 FROM beatmaps WHERE native_id=1900000001 AND status_revision=3) RETURNING revision INTO actual_revision;
    GET DIAGNOSTICS changed_rows=ROW_COUNT;
    IF changed_rows<>1 OR actual_revision<>5 THEN RAISE EXCEPTION 'Fresh dual-revision difficulty CAS did not advance the override'; END IF;
    UPDATE beatmap_rank_requests SET revision=0,created_at=original_created,updated_at=clock_timestamp() WHERE id=approved_id RETURNING revision INTO actual_revision;
    IF actual_revision<>1 OR (SELECT updated_at FROM beatmap_rank_requests WHERE id=approved_id) IS DISTINCT FROM original_updated THEN RAISE EXCEPTION 'Unchanged approved request accepted a revision or timestamp reset'; END IF;
    UPDATE beatmap_rank_requests SET approved_difficulty_ids=ARRAY[1900000002::bigint],revision=99 WHERE id=approved_id RETURNING revision INTO actual_revision;
    IF actual_revision<>2 THEN RAISE EXCEPTION 'Changed approval selection did not advance its revision'; END IF;
    UPDATE beatmap_rank_requests SET approved_difficulty_ids=ARRAY[1900000001::bigint],revision=0 WHERE id=approved_id RETURNING revision INTO actual_revision;
    IF actual_revision<>3 THEN RAISE EXCEPTION 'Approval selection ABA did not advance its revision'; END IF;
    UPDATE beatmap_rank_requests SET review_reason='Stale probe decision' WHERE id=approved_id AND revision=1;
    GET DIAGNOSTICS changed_rows=ROW_COUNT;
    IF changed_rows<>0 THEN RAISE EXCEPTION 'Stale approval selection passed the restored CAS predicate after ABA'; END IF;
    UPDATE beatmap_rank_requests SET status='rejected',revision=99 WHERE id=pending_id RETURNING revision INTO actual_revision;
    IF actual_revision<>1 THEN RAISE EXCEPTION 'Changed pending request did not advance its revision'; END IF;
    UPDATE beatmap_rank_requests SET status='pending',revision=0 WHERE id=pending_id RETURNING revision INTO actual_revision;
    IF actual_revision<>2 THEN RAISE EXCEPTION 'Pending request ABA did not advance its revision'; END IF;
    UPDATE beatmap_rank_requests SET status='withdrawn' WHERE id=pending_id AND status='pending' AND revision=0;
    GET DIAGNOSTICS changed_rows=ROW_COUNT;
    IF changed_rows<>0 THEN RAISE EXCEPTION 'Stale pending request passed the restored CAS predicate after ABA'; END IF;
    UPDATE beatmap_rank_requests SET status='withdrawn' WHERE id=pending_id AND status='pending' AND revision=2 RETURNING revision INTO actual_revision;
    GET DIAGNOSTICS changed_rows=ROW_COUNT;
    IF changed_rows<>1 OR actual_revision<>3 THEN RAISE EXCEPTION 'Fresh pending request CAS did not advance its revision'; END IF;
    IF (SELECT metadata FROM beatmap_rank_requests WHERE id=approved_id) IS DISTINCT FROM approved_metadata
        OR (SELECT metadata FROM beatmap_rank_requests WHERE id=pending_id) IS DISTINCT FROM pending_metadata
        OR NOT EXISTS(SELECT 1 FROM beatmaps WHERE native_id=1900000002 AND status='graveyard' AND status_revision=0)
        OR EXISTS(SELECT 1 FROM beatmap_status_overrides WHERE native_id=1900000002) THEN
        RAISE EXCEPTION 'Selected difficulty probe changed a submission snapshot or its sibling';
    END IF;
END;
$$;
ROLLBACK;
SELECT 'recovery-ranking-probe-passed';`;

export async function verifyRestoredRanking(docker, base, { owner } = {}) {
  const project = ownedProject(base[base.indexOf('--project-name') + 1], owner);
  if (!project.endsWith('-restore')) throw new Error('Ranking behavior may be probed only in this owned restore project.');
  const result = await docker([...base, 'exec', '-T', 'postgres', 'psql', '-X', '-q', '-At', '--set', 'ON_ERROR_STOP=1', '-U', 'illya', '-d', 'illyaverse', '-c', rankingProbeSql]);
  if (result.trim() !== 'recovery-ranking-probe-passed') throw new Error('Restored ranking behavior probe did not complete its rollback.');
  return { selectedApproval: true, pendingSelectedManifest: true, privateAudit: true, immutableSubmissionSnapshots: true,
    durableOverrideRevision: 2, durableStoredStatusRevision: 1, durableApprovedRequestRevision: 1, durablePendingRequestRevision: 0,
    unchangedRevisionWritesIgnored: true, abaAdvancesRevisions: true, staleCasRejected: true, freshCasAccepted: true, siblingUnchanged: true,
    scope: 'Restored database state, triggers and CAS predicates; HTTP authorization and review enforcement are covered by separate integration tests.' };
}

export async function databaseSnapshot(docker, base, { canonicalizeSchema = false, owner } = {}) {
  if (canonicalizeSchema) {
    const project = ownedProject(base[base.indexOf('--project-name') + 1], owner);
    if (!project.endsWith('-source')) throw new Error('Schema reference DB may be created only in this owned source project.');
  }
  const exec = args => docker([...base, 'exec', '-T', 'postgres', ...args]);
  const sql = query => exec(['psql', '-X', '-At', '--set', 'ON_ERROR_STOP=1', '-U', 'illya', '-d', 'illyaverse', '-c', query]);
  const tables = (await sql("SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY tablename;")).trim().split('\n');
  if (!tables.length || tables.some(table => !/^[a-z_][a-z0-9_]*$/.test(table))) throw new Error('Unexpected recovery database table names.');
  const rowQuery = tables.map(table => `SELECT '${table}:' || COALESCE(jsonb_agg(rowdata ORDER BY rowdata::text)::text,'[]') FROM (SELECT to_jsonb(t) rowdata FROM public.${table} t) records;`).join('\n');
  const rows = await sql(rowQuery);
  const sequences = await sql("SELECT schemaname,sequencename,start_value,min_value,max_value,increment_by,cycle,cache_size,last_value FROM pg_sequences WHERE schemaname='public' ORDER BY sequencename;");
  const migrations = (await sql('SELECT version,success,encode(checksum,\'hex\') FROM _sqlx_migrations ORDER BY version;')).trim();
  const dumpSchema = async database => {
    await exec(['pg_dump', '-U', 'illya', '-d', database, '--schema-only', '--no-owner', '--no-acl', '--file', '/tmp/recovery-schema.sql']);
    return (await exec(['cat', '/tmp/recovery-schema.sql'])).split('\n').filter(line => !/^\\(?:un)?restrict\b/.test(line)).join('\n');
  };
  const originalSchema = await dumpSchema('illyaverse');
  let schema = originalSchema;
  if (canonicalizeSchema) {
    // PostgreSQL can flatten associative CHECK Boolean expressions when parsing
    // dumped DDL. Normalize with that same parser in a brand-new reference DB,
    // rather than strip parentheses or weaken the actual schema comparison.
    const reference = `recovery_schema_${nonce()}`;
    await exec(['pg_dump', '-U', 'illya', '-d', 'illyaverse', '--schema-only', '-Fc', '--file', '/tmp/recovery-schema-reference.dump']);
    await exec(['createdb', '-U', 'illya', reference]); // Existing names fail; never clean/reuse a DB.
    await exec(['pg_restore', '--exit-on-error', '--no-owner', '--no-acl', '-U', 'illya', '-d', reference, '/tmp/recovery-schema-reference.dump']);
    schema = await dumpSchema(reference);
    // Reference DB contains schema only and is removed with this owned project's volume.
  }
  const rowCounts = Object.fromEntries(rows.trim().split('\n').map(line => {
    const boundary = line.indexOf(':');
    return [line.slice(0, boundary), JSON.parse(line.slice(boundary + 1)).length];
  }));
  return { rows: sha256(rows), sequences: sha256(sequences), schema: sha256(schema), originalSchema: sha256(originalSchema), schemaDefinition: schema, migrations, rowCounts,
    tableCount: tables.length, sequenceCount: sequences.trim().split('\n').filter(Boolean).length };
}

export function sameDatabase(source, restored) {
  for (const value of [source, restored]) {
    if (!value || ['rows', 'sequences', 'schema'].some(key => !exactMatch(/^[a-f0-9]{64}$/, value[key])) ||
        typeof value.migrations !== 'string' || !value.migrations || !Number.isInteger(value.tableCount) || value.tableCount < 1 ||
        Object.keys(value.rowCounts ?? {}).length !== value.tableCount || !Number.isInteger(value.sequenceCount) || value.sequenceCount < 0)
      return false;
  }
  return ['rows', 'sequences', 'schema', 'migrations', 'rowCounts', 'tableCount', 'sequenceCount']
    .every(key => JSON.stringify(source[key]) === JSON.stringify(restored[key]));
}

export function checkMigrations(snapshot, sourceMigrations) {
  if (!Array.isArray(sourceMigrations) || !sourceMigrations.length || sourceMigrations.some((migration, index) =>
      !Number.isInteger(migration.version) || migration.version < 1 || !exactMatch(/^[a-f0-9]{96}$/, migration.checksum) ||
      (index > 0 && sourceMigrations[index - 1].version >= migration.version)))
    throw new Error('Recovery requires the ordered source migration versions and exact SHA-384 checksums.');
  const expected = sourceMigrations.map(({ version, checksum }) => `${version}|t|${checksum}`).join('\n');
  if (snapshot.migrations !== expected) throw new Error(`Pinned Sanctuary image must apply all ${sourceMigrations.length} exact current-source migration versions and checksums.`);
  return sourceMigrations.length;
}

async function fileSnapshot(docker, base) {
  const result = await docker([...base, 'exec', '-T', 'archive', 'sh', '-c', 'cd /data && find . -type f -print | LC_ALL=C sort | while IFS= read -r file; do sha256sum "$file"; done']);
  const ownership = (await docker([...base, 'exec', '-T', 'archive', 'sh', '-c', "find /data -printf '%P|%U|%G|%m|%y\\n' | LC_ALL=C sort"])).trim();
  if (ownership.split('\n').some(line => !/^[^|]*\|10001\|10001\|[0-7]{3,4}\|[df]$/.test(line)))
    throw new Error('Recovery files/directories must retain UID:GID10001:10001 and contain no special files.');
  for (const [file, bytes] of Object.entries(fixtureBytes)) {
    if (!result.includes(`${sha256(bytes)}  ./${file}\n`)) throw new Error(`Recovery binary fixture hash failed: ${file}.`);
  }
  return { hashes: result, ownership };
}

export async function recoveryDrill({ dryRun = false, environment = process.env, runner, sanctuaryImage } = {}) {
  if (dryRun) {
    console.log('Recovery drill dry run: no Docker calls or files created. Live mode generates two nonce-owned fresh Compose projects/volumes, pins existing postgres:17-alpine and illyaverse/sanctuary:local image IDs, derives every expected migration version and exact SHA-384 checksum from current SQL files, creates synthetic DB/media/maps/replays, durable moderation state and selected per-difficulty ranking requests, invokes the guarded deployment backup helper with all owned writers stopped, checks binary backup + SHA manifest, restores into fresh volumes, compares schema/all rows/sequences/file hashes/UID:GID, probes restored moderation and ranking triggers/CAS predicates in rollback-only transactions and verifies unchanged snapshots, verifies readiness, then audits ownership and removes only its two projects and exact temporary files.');
    return;
  }
  requireDisposable(environment);
  const sanctuaryReference = sanctuaryImageReference(sanctuaryImage);
  const sourceMigrations = await readSourceMigrations();
  const owner = nonce();
  const projects = ['source', 'restore'].map(role => `illya-recovery-${owner}-${role}`);
  const report = { format: 'illyaverse-disposable-recovery-v1', owner, projects, images: {}, backupHelper: 'backupDeployment', sourceMigrations,
    checks: {}, cleanup: { projects: [], temporaryFiles: false } };
  const controller = new AbortController();
  const docker = checkedDocker({ environment, runner, signal: controller.signal });
  const interrupt = () => controller.abort();
  process.on('SIGINT', interrupt); process.on('SIGTERM', interrupt);
  let temporary;
  const started = [];
  const files = ['source.json', 'restore.json', 'empty.env', ...artifactNames, ...artifactNames.map(name => `${name}.partial`), 'manifest.json', 'manifest.json.partial'];
  try {
    const images = {};
    for (const [service, tag] of [['postgres', 'postgres:17-alpine'], ['sanctuary', sanctuaryReference]]) {
      images[service] = (await docker(['image', 'inspect', tag, '--format', '{{.Id}}'])).trim();
      if (!validImage(images[service])) throw new Error('Recovery drill requires both existing local images; it never builds or pulls.');
      if (service === 'sanctuary' && sanctuaryImage !== undefined && images[service] !== sanctuaryImage)
        throw new Error('The inspected Sanctuary image differs from the explicitly approved immutable image ID.');
    }
    report.images = images;
    for (const project of projects) await requireFreshProject(docker, project, owner);
    temporary = await ownedTempDirectory(owner, runner);
    const directory = temporary.directory;
    const secrets = { password: nonce() + nonce() + nonce() + nonce(), sessionSecret: nonce() + nonce() + nonce() + nonce(), internalKey: nonce() + nonce() + nonce() + nonce() };
    await writeFile(path.join(directory, 'empty.env'), '', { flag: 'wx', mode: 0o600 });
    const bases = [];
    for (const [index, project] of projects.entries()) {
      const expected = recoveryConfig({ project, owner, images, ...secrets, restore: index === 1 });
      const configFile = path.join(directory, index ? 'restore.json' : 'source.json');
      await writeFile(configFile, `${JSON.stringify(expected)}\n`, { flag: 'wx', mode: 0o600 });
      const base = ['compose', '--project-name', project, '--env-file', path.join(directory, 'empty.env'), '--file', configFile];
      validateRecoveryConfig(JSON.parse(await docker([...base, 'config', '--format', 'json'])), expected);
      bases.push(base);
    }
    const [source, restore] = bases;
    console.log(`Recovery drill owns only ${projects.join(' and ')}; no published ports.`);
    console.log(`Pinned PostgreSQL: ${images.postgres}; Sanctuary: ${images.sanctuary}.`);
    await requireFreshProject(docker, projects[0], owner);
    started.push({ base: source, project: projects[0] });
    await docker([...source, 'up', '-d', '--no-build', '--pull', 'never', '--wait', '--wait-timeout', '120', 'postgres', 'sanctuary']);
    await docker([...source, 'stop', '--timeout', '10', 'sanctuary']);
    await docker([...source, 'exec', '-T', 'postgres', 'psql', '-X', '--set', 'ON_ERROR_STOP=1', '-U', 'illya', '-d', 'illyaverse', '-c', fixtureSql]);
    const seed = Object.entries(fixtureBytes).map(([name, bytes]) => `printf '%s' '${bytes.toString('base64')}' | base64 -d > /data/${name}`).join('\n');
    await docker([...source, 'run', '--rm', '-T', '--no-deps', '--pull', 'never', '--entrypoint', 'sh', 'sanctuary', '-c', `set -eu\numask 022\nmkdir -p /data/media /data/maps /data/replays\n${seed}`]);
    await docker([...source, 'up', '-d', '--no-build', '--pull', 'never', 'archive']);
    const sourceDb = await databaseSnapshot(docker, source, { canonicalizeSchema: true, owner });
    checkMigrations(sourceDb, sourceMigrations);
    const sourceFiles = await fileSnapshot(docker, source);
    const backupDirectory = path.join(directory, 'backup');
    // Exercise the same source guards, read-only helper, binary copies and completion
    // rules used by an operator. Never substitute a retained deployment project here.
    await docker([...source, 'stop', '--timeout', '10', 'archive']);
    ownedProject(projects[0], owner);
    const backupManifest = await backupDeployment({ project: projects[0], output: backupDirectory, writersStopped: true },
      { environment, runner, signal: controller.signal });
    await verifyBackup(backupDirectory);
    report.backup = { artifacts: backupManifest.artifacts, postgresImage: backupManifest.postgresImage, sanctuaryImage: backupManifest.sanctuaryImage };
    report.source = { database: sourceDb, files: sourceFiles };
    report.schemaComparison = 'Source schema normalized by a schema-only PostgreSQL 17 round trip into a fresh reference DB in the owned source project; original source dump hash retained.';
    await requireFreshProject(docker, projects[1], owner);
    started.push({ base: restore, project: projects[1] });
    await docker([...restore, 'up', '-d', '--no-build', '--pull', 'never', '--wait', '--wait-timeout', '120', 'postgres', 'archive']);
    const sql = query => docker([...restore, 'exec', '-T', 'postgres', 'psql', '-X', '-At', '--set', 'ON_ERROR_STOP=1', '-U', 'illya', '-d', 'illyaverse', '-c', query]);
    if ((await sql("SELECT count(*) FROM pg_tables WHERE schemaname='public';")).trim() !== '0') throw new Error('Refusing restore into a database with existing application tables.');
    if ((await docker([...restore, 'exec', '-T', 'archive', 'find', '/data', '-mindepth', '1', '-type', 'f', '-print'])).trim()) throw new Error('Refusing restore into populated shared data.');
    for (const [name, service, remote] of [['illyaverse.dump', 'postgres', '/tmp/recovery.dump'], ['shared-data.tar.gz', 'archive', '/tmp/recovery.tar.gz']]) {
      const id = (await docker([...restore, 'ps', '-q', service])).trim();
      if (!/^[a-f0-9]{12,64}$/.test(id)) throw new Error('Unexpected recovery restore container ID.');
      await docker(['cp', path.join(backupDirectory, name), `${id}:${remote}`]);
    }
    await docker([...restore, 'exec', '-T', 'postgres', 'pg_restore', '--list', '/tmp/recovery.dump']);
    await docker([...restore, 'exec', '-T', 'postgres', 'pg_restore', '--exit-on-error', '--no-owner', '--no-acl', '-U', 'illya', '-d', 'illyaverse', '/tmp/recovery.dump']);
    await docker([...restore, 'exec', '-T', 'archive', 'tar', '--numeric-owner', '-xzf', '/tmp/recovery.tar.gz', '-C', '/data']);
    const restoredDb = await databaseSnapshot(docker, restore);
    const restoredFiles = await fileSnapshot(docker, restore);
    report.restore = { database: restoredDb, files: restoredFiles };
    if (!sameDatabase(sourceDb, restoredDb) || JSON.stringify(restoredFiles) !== JSON.stringify(sourceFiles))
      throw new Error('Restored schema/all table rows/sequences/migrations/file hashes/ownership differ from source.');
    report.checks = { migrationCount: sourceMigrations.length, schema: true, allTableRows: true, sequences: true, fileHashes: true, ownership: true };
    report.moderation = await verifyRestoredModeration(docker, restore, { owner });
    const afterModerationProbe = await databaseSnapshot(docker, restore);
    if (!sameDatabase(restoredDb, afterModerationProbe)) throw new Error('Rollback-only moderation probe changed restored schema/rows/sequences/migrations.');
    report.moderation.rollbackUnchanged = true;
    report.moderation.afterProbe = afterModerationProbe;
    report.checks.moderationState = true;
    report.checks.moderationTriggers = true;
    report.checks.moderationRollback = true;
    report.ranking = await verifyRestoredRanking(docker, restore, { owner });
    const afterRankingProbe = await databaseSnapshot(docker, restore);
    if (!sameDatabase(restoredDb, afterRankingProbe)) throw new Error('Rollback-only ranking probe changed restored schema/rows/sequences/migrations.');
    report.ranking.rollbackUnchanged = true;
    report.ranking.afterProbe = afterRankingProbe;
    report.checks.rankingState = true;
    report.checks.rankingTriggers = true;
    report.checks.rankingRollback = true;
    await docker([...restore, 'up', '-d', '--no-build', '--pull', 'never', '--wait', '--wait-timeout', '120', 'sanctuary']);
    await docker([...restore, 'exec', '-T', 'sanctuary', 'curl', '--fail', '--silent', 'http://127.0.0.1:8080/health/ready']);
    await docker([...restore, 'stop', '--timeout', '10', 'sanctuary']);
    checkMigrations(await databaseSnapshot(docker, restore), sourceMigrations);
    report.checks.readiness = true;
    console.log('Recovery comparison and restored Sanctuary readiness passed.');
  } catch (error) {
    report.error = error.message;
    error.recoveryReport = report;
    throw error;
  } finally {
    process.removeListener('SIGINT', interrupt); process.removeListener('SIGTERM', interrupt);
    const failures = [];
    for (const { base, project } of started.reverse()) {
      try { await cleanupProject(docker, base, project, owner); report.cleanup.projects.push(project); } catch (error) { failures.push(`${project}: ${error.message}`); }
    }
    if (temporary) {
      try {
        const backup = path.join(temporary.directory, 'backup');
        // This child is created by this run. Remove only exact artifacts, then an empty directory.
        const { unlink, rmdir, lstat, realpath } = await import('node:fs/promises');
        if ((await lstat(backup).catch(error => error.code === 'ENOENT' ? null : Promise.reject(error)))) {
          if (await realpath(backup) !== backup || (await lstat(backup)).isSymbolicLink()) throw new Error('Refusing a redirected recovery backup directory.');
          for (const name of files.filter(name => !['source.json', 'restore.json', 'empty.env'].includes(name))) await unlink(path.join(backup, name)).catch(error => { if (error.code !== 'ENOENT') throw error; });
          await rmdir(backup);
        }
        await cleanupTemp({ ...temporary, owner, files: ['source.json', 'restore.json', 'empty.env'] });
        report.cleanup.temporaryFiles = true;
      } catch (error) { failures.push(`temporary files: ${error.message}`); }
    }
    if (failures.length) {
      const error = new Error(`Recovery cleanup needs attention: ${failures.join('; ')}.`);
      report.cleanup.errors = failures;
      error.recoveryReport = report;
      throw error;
    }
    console.log('Verified removal of both owned projects, volumes, networks and private temporary files.');
  }
  return report;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await recoveryDrill({ ...drillArguments(process.argv.slice(2)) }); }
  catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
