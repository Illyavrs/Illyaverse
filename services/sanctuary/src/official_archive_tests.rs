//! Full official archive fixtures own their PG17 schema and synthetic files.
use crate::{
    AppState, Router,
    maps::map_mutation_tests::{
        MULTIPART, MutationFixture, actor, file_count, osu_bytes, reject_audits_at_commit, request,
        wait_for_lock,
    },
    native_map_tests::{OfficialMap, OfficialServer},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor, path::Path, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

const IMPORT_PATH: &str = "/api/v1/beatmaps/import/archive";

#[derive(Clone)]
struct ArchiveMap {
    id: i64,
    set_id: i64,
    filename: String,
    bytes: Vec<u8>,
    metadata: Value,
}
impl ArchiveMap {
    fn new(id: i64, set_id: i64, version: &str, status: &str) -> Self {
        let bytes = String::from_utf8(osu_bytes(&format!("Official archive {set_id}")))
            .unwrap()
            .replace("Mode:0", "Mode:0\nAudioFilename:assets/audio.mp3")
            .replace(
                "Version:Easy",
                &format!("Version:{version}\nBeatmapID:{id}\nBeatmapSetID:{set_id}"),
            )
            .into_bytes();
        let checksum = format!("{:x}", md5::compute(&bytes));
        Self {
            id,
            set_id,
            filename: format!("Archive {set_id} [{version}].osu"),
            metadata: json!({"id":id,"beatmapset_id":set_id,"checksum":checksum,"mode_int":0,"status":status}),
            bytes,
        }
    }
    fn checksum(&self) -> String {
        format!("{:x}", md5::compute(&self.bytes))
    }
    fn upstream(&self) -> OfficialMap {
        OfficialMap::new(self.id, "ranked")
            .with_bytes(self.bytes.clone())
            .with_metadata(self.metadata.clone())
    }
}

struct ArchiveSet {
    id: i64,
    maps: Vec<ArchiveMap>,
    metadata: Value,
    payload: Vec<u8>,
}
impl ArchiveSet {
    fn new(id: i64) -> Self {
        let maps = vec![
            ArchiveMap::new(id * 10 + 1, id, "Easy", "ranked"),
            ArchiveMap::new(id * 10 + 2, id, "Hard", "loved"),
        ];
        let mut metadata = json!({
            "id":id,
            "beatmaps":maps.iter().map(|map| map.metadata.clone()).collect::<Vec<_>>(),
            // Conversion records are not original difficulties in the archive.
            "converts":[{"id":9_999_999_999_i64,"ignored":"conversion record"}],
        });
        metadata["beatmaps"][0]["convert"] = Value::Bool(false);
        let payload = archive(&maps, b"owned synthetic audio asset", true);
        Self {
            id,
            maps,
            metadata,
            payload,
        }
    }
}

fn archive(maps: &[ArchiveMap], audio: &[u8], include_cover: bool) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // Reverse entry order deliberately; map/filename associations must survive
    // the deterministic checksum and ID ordering used during publication.
    for map in maps.iter().rev() {
        writer
            .start_file(&map.filename, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, &map.bytes).unwrap();
    }
    writer
        .start_file("assets/audio.mp3", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut writer, audio).unwrap();
    if include_cover {
        writer
            .start_file("assets/cover.jpg", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, b"owned synthetic cover asset").unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn nested_map_archive(maps: &[ArchiveMap]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for map in maps {
        writer
            .start_file(
                format!("nested/{}", map.filename),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        std::io::Write::write_all(&mut writer, &map.bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn multipart_archive(set_id: i64, payload: &[u8]) -> Vec<u8> {
    let mut body = format!("--mutation-fixture\r\nContent-Disposition: form-data; name=\"official_set_id\"\r\n\r\n{set_id}\r\n--mutation-fixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"fixture.osz\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(payload);
    body.extend_from_slice(b"\r\n--mutation-fixture--\r\n");
    body
}

async fn import(app: &Router, cookie: &str, set_id: i64, payload: &[u8]) -> (StatusCode, Value) {
    request(
        app,
        IMPORT_PATH,
        Some(cookie),
        false,
        MULTIPART,
        multipart_archive(set_id, payload),
    )
    .await
}

async fn get_bytes(app: &Router, path: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, bytes)
}

fn ids(value: &Value) -> Vec<i64> {
    let mut ids = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|map| map["id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn id_for(value: &Value, official_id: i64) -> i64 {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|map| map["official_id"] == official_id)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

fn assert_package(value: &Value, set_id: i64, count: usize) -> Uuid {
    let package = &value["package"];
    let id = Uuid::parse_str(package["id"].as_str().unwrap()).unwrap();
    assert_eq!(package["source"], "official");
    assert_eq!(package["kind"], "osz");
    assert_eq!(package["native_set_id"], set_id);
    assert_eq!(package["difficulty_count"], count);
    assert_eq!(package["beatmap_ids"], json!(ids(value)));
    assert!(package["download_url"].as_str().is_some());
    for field in ["archive_path", "osu_path", "sha256", "member_md5"] {
        assert!(package.get(field).is_none());
    }
    for map in value["items"].as_array().unwrap() {
        assert_eq!(map["source"], "official");
        assert_eq!(map["official_set_id"], set_id);
        assert_eq!(map["native_set_id"], set_id);
        assert_eq!(map["native_id"], map["official_id"]);
        assert_eq!(map["package"], *package);
        assert!(map.get("archive_path").is_none());
        assert!(map.get("osu_path").is_none());
    }
    id
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    maps: Value,
    local_packages: Value,
    archives: Value,
    members: Value,
    aliases: Value,
    audits: Value,
    updates: Value,
}

async fn snapshot(state: &AppState) -> Snapshot {
    let maps = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(b) ORDER BY id),'[]'::jsonb) FROM beatmaps b",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let local_packages = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(p) ORDER BY id),'[]'::jsonb) FROM beatmap_packages p",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let archives = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY official_set_id),'[]'::jsonb) FROM official_beatmapset_archives a").fetch_one(&state.db).await.unwrap();
    let members = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(m) ORDER BY archive_id,beatmap_id),'[]'::jsonb) FROM official_beatmapset_archive_members m").fetch_one(&state.db).await.unwrap();
    let aliases = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY filename COLLATE \"C\",beatmap_id),'[]'::jsonb) FROM beatmap_filename_aliases a").fetch_one(&state.db).await.unwrap();
    let audits = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY id),'[]'::jsonb) FROM audit_events a",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let updates = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(u) ORDER BY id),'[]'::jsonb) FROM lazer_beatmap_updates u")
        .fetch_one(&state.db).await.unwrap();
    Snapshot {
        maps,
        local_packages,
        archives,
        members,
        aliases,
        audits,
        updates,
    }
}

async fn files_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let mut entries = tokio::fs::read_dir(directory).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            if entry.file_type().await.unwrap().is_dir() {
                directories.push(entry.path());
            } else {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                assert!(
                    files
                        .insert(relative, tokio::fs::read(path).await.unwrap())
                        .is_none()
                );
            }
        }
    }
    files
}

async fn seed_map(
    state: &AppState,
    owner: i64,
    bytes: &[u8],
    source: &str,
    official_id: Option<i64>,
    official_set_id: Option<i64>,
) -> i64 {
    let map = crate::maps::parse_map(bytes.to_vec()).unwrap();
    let path = format!("retained-{}.osu", Uuid::new_v4());
    tokio::fs::write(state.maps_root.join(&path), bytes)
        .await
        .unwrap();
    sqlx::query_scalar("INSERT INTO beatmaps(uploaded_by,title,artist,creator,version,mode,source,stars,bpm,length_seconds,max_combo,md5,osu_path,official_id,official_set_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING id")
        .bind(owner).bind(map.title).bind(map.artist).bind(map.creator).bind(map.version).bind(map.mode).bind(source).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(map.md5).bind(path).bind(official_id).bind(official_set_id).fetch_one(&state.db).await.unwrap()
}

async fn assert_rejected_unchanged(
    state: &AppState,
    app: &Router,
    cookie: &str,
    set_id: i64,
    payload: &[u8],
    status: StatusCode,
    code: &str,
) {
    let before = snapshot(state).await;
    let files = files_snapshot(&state.maps_root).await;
    let (actual_status, value) = import(app, cookie, set_id, payload).await;
    assert_eq!(actual_status, status, "set {set_id}: {value}");
    assert_eq!(value["error"]["code"], code, "{value}");
    assert_eq!(snapshot(state).await, before);
    assert_eq!(files_snapshot(&state.maps_root).await, files);
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn full_official_archive_publishes_one_complete_package_and_retains_original_assets() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_complete", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "ArchiveComplete", &["beatmaps.import"]).await;
        let set = ArchiveSet::new(6100);
        server.add_set(set.id, set.metadata.clone()).await;
        let (status, value) = import(&app, &cookie, set.id, &set.payload).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["idempotent"], false);
        let package = assert_package(&value, set.id, 2);
        assert_eq!(
            server.set_queries().await,
            [set.id],
            "validate the complete set in one metadata call"
        );
        assert_eq!(
            server.counters(),
            [1, 0, 0, 0],
            "archive validation does not fetch per-map metadata or files"
        );
        let stored = snapshot(&state).await;
        assert_eq!(stored.maps.as_array().unwrap().len(), 2);
        assert!(stored.local_packages.as_array().unwrap().is_empty());
        assert_eq!(stored.archives.as_array().unwrap().len(), 1);
        assert_eq!(stored.members.as_array().unwrap().len(), 2);
        assert_eq!(stored.aliases.as_array().unwrap().len(), 2);
        assert_eq!(stored.audits.as_array().unwrap().len(), 1);
        let row = &stored.archives[0];
        assert_eq!(row["id"], package.to_string());
        assert_eq!(row["official_set_id"], set.id);
        assert_eq!(row["uploaded_by"], owner);
        assert_eq!(row["byte_length"], set.payload.len());
        assert_eq!(row["sha256"], format!("{:x}", Sha256::digest(&set.payload)));
        let mut manifest = set
            .maps
            .iter()
            .map(ArchiveMap::checksum)
            .collect::<Vec<_>>();
        manifest.sort_unstable();
        assert_eq!(row["member_md5"], json!(manifest));
        for map in &set.maps {
            assert_eq!(map.set_id, set.id);
            let id = id_for(&value, map.id);
            let item = value["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["id"] == id)
                .unwrap();
            assert_eq!(item["md5"], map.checksum());
            assert_eq!(item["uploaded_by"], owner);
            assert_eq!(item["status"], map.metadata["status"]);
            let aliases: Vec<i64> = sqlx::query_scalar(
                "SELECT beatmap_id FROM beatmap_filename_aliases WHERE filename=$1",
            )
            .bind(&map.filename)
            .fetch_all(&state.db)
            .await
            .unwrap();
            assert_eq!(aliases, [id]);
            let (status, bytes) = get_bytes(&app, &format!("/api/v1/beatmaps/{id}/download")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(bytes, set.payload);
            let (status, bytes) = get_bytes(&app, &format!("/api/v1/beatmaps/{id}/osu")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(bytes, map.bytes);
        }
        let (status, bytes) =
            get_bytes(&app, value["package"]["download_url"].as_str().unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, set.payload);
        assert_eq!(file_count(&state.maps_root).await, 3);

        // Persisted official membership protects score/native identity even
        // against direct SQL changes, while moderation status stays mutable.
        let immutable = snapshot(&state).await;
        let map_id = id_for(&value, set.maps[0].id);
        for sql in [
            "UPDATE beatmaps SET source='local' WHERE id=$1",
            "UPDATE beatmaps SET mode=1 WHERE id=$1",
        ] {
            let error = sqlx::query(sql).bind(map_id).execute(&state.db).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        }
        let error = sqlx::query("UPDATE official_beatmapset_archives SET byte_length=byte_length+1 WHERE id=$1")
            .bind(package).execute(&state.db).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        let error = sqlx::query("UPDATE official_beatmapset_archive_members SET beatmap_id=$1 WHERE archive_id=$2 AND beatmap_id=$3")
            .bind(id_for(&value, set.maps[1].id)).bind(package).bind(map_id).execute(&state.db).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        let error = sqlx::query("DELETE FROM official_beatmapset_archive_members WHERE archive_id=$1 AND beatmap_id=$2")
            .bind(package).bind(map_id).execute(&state.db).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        assert_eq!(snapshot(&state).await, immutable);
        sqlx::query("UPDATE beatmaps SET status='unranked' WHERE id=$1").bind(map_id).execute(&state.db).await.unwrap();
        assert_eq!(sqlx::query_scalar::<_, String>("SELECT status FROM beatmaps WHERE id=$1").bind(map_id).fetch_one(&state.db).await.unwrap(), "unranked");
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_and_any_authorized_exact_retry_preserve_existing_owners_and_current_statuses()
 {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_retry", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (old_owner, _, _, old_cookie) =
            actor(&state, "ArchiveOldOwner", &["beatmaps.import"]).await;
        let (archive_owner, _, _, cookie) =
            actor(&state, "ArchiveNewOwner", &["beatmaps.import"]).await;
        let (_, _, _, retry_cookie) =
            actor(&state, "ArchiveRetryStaff", &["beatmaps.import"]).await;
        let set = ArchiveSet::new(6200);
        server.add(set.maps[0].upstream()).await;
        server.add(set.maps[1].upstream()).await;
        server.add_set(set.id, set.metadata.clone()).await;
        let (status, existing) = request(
            &app,
            "/api/v1/beatmaps/import",
            Some(&old_cookie),
            false,
            "application/json",
            json!({"official_id":set.maps[0].id})
                .to_string()
                .into_bytes(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{existing}");
        let old_id = existing["id"].as_i64().unwrap();
        let (status, second_existing) = request(
            &app,
            "/api/v1/beatmaps/import",
            Some(&cookie),
            false,
            "application/json",
            json!({"official_id":set.maps[1].id})
                .to_string()
                .into_bytes(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{second_existing}");
        let second_id = second_existing["id"].as_i64().unwrap();
        let second_row: Value =
            sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
                .bind(second_id)
                .fetch_one(&state.db)
                .await
                .unwrap();
        sqlx::query("UPDATE beatmaps SET status='unranked' WHERE id=$1")
            .bind(old_id)
            .execute(&state.db)
            .await
            .unwrap();
        let old_row: Value = sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
            .bind(old_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        let retained = state.maps_root.join(old_row["osu_path"].as_str().unwrap());
        assert_eq!(tokio::fs::read(&retained).await.unwrap(), set.maps[0].bytes);
        for missing in [true, false] {
            if missing {
                tokio::fs::remove_file(&retained).await.unwrap();
            } else {
                tokio::fs::write(&retained, b"owned fixture corrupt retained difficulty")
                    .await
                    .unwrap();
            }
            assert_rejected_unchanged(
                &state,
                &app,
                &cookie,
                set.id,
                &set.payload,
                StatusCode::CONFLICT,
                "official_archive_conflict",
            )
            .await;
            tokio::fs::write(&retained, &set.maps[0].bytes)
                .await
                .unwrap();
        }
        let updates_before: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM lazer_beatmap_updates WHERE beatmap_set_id=$1",
        )
        .bind(set.id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        let (status, created) = import(&app, &cookie, set.id, &set.payload).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let package = assert_package(&created, set.id, 2);
        assert_eq!(created["idempotent"], false);
        assert_eq!(id_for(&created, set.maps[0].id), old_id);
        assert_eq!(id_for(&created, set.maps[1].id), second_id);
        let after_second: Value =
            sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
                .bind(second_id)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(
            after_second, second_row,
            "fully cached difficulties keep their original rows and files"
        );
        let updates_after: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM lazer_beatmap_updates WHERE beatmap_set_id=$1",
        )
        .bind(set.id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(
            updates_after,
            updates_before + 1,
            "a fully cached set still publishes an archive availability event"
        );
        assert_eq!(
            file_count(&state.maps_root).await,
            3,
            "unused staged difficulty copies are removed"
        );
        let after_old: Value = sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
            .bind(old_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        for field in [
            "id",
            "md5",
            "osu_path",
            "archive_path",
            "uploaded_by",
            "status",
            "source",
            "native_id",
            "native_set_id",
            "created_at",
        ] {
            assert_eq!(after_old[field], old_row[field], "existing field {field}");
        }
        assert_eq!(after_old["uploaded_by"], old_owner);
        let new_id = id_for(&created, set.maps[1].id);
        sqlx::query("UPDATE beatmaps SET status='ranked' WHERE id=$1")
            .bind(new_id)
            .execute(&state.db)
            .await
            .unwrap();
        let before = snapshot(&state).await;
        assert_eq!(before.archives[0]["uploaded_by"], archive_owner);
        let retained_archive = state
            .maps_root
            .join(before.archives[0]["archive_path"].as_str().unwrap());
        for (path, original) in [
            (&retained, &set.maps[0].bytes),
            (&retained_archive, &set.payload),
        ] {
            assert_eq!(tokio::fs::read(path).await.unwrap(), *original);
            tokio::fs::write(path, b"owned fixture corrupt retained publication")
                .await
                .unwrap();
            assert_rejected_unchanged(
                &state,
                &app,
                &retry_cookie,
                set.id,
                &set.payload,
                StatusCode::CONFLICT,
                "official_archive_conflict",
            )
            .await;
            tokio::fs::write(path, original).await.unwrap();
        }
        let files = files_snapshot(&state.maps_root).await;
        for retry in [&cookie, &retry_cookie] {
            let (status, value) = import(&app, retry, set.id, &set.payload).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value["idempotent"], true);
            assert_eq!(assert_package(&value, set.id, 2), package);
            assert_eq!(ids(&value), ids(&created));
            for item in value["items"].as_array().unwrap() {
                let old = item["id"] == old_id;
                assert_eq!(
                    item["uploaded_by"],
                    if old { old_owner } else { archive_owner }
                );
                assert_eq!(item["status"], if old { "unranked" } else { "ranked" });
            }
            assert_eq!(snapshot(&state).await, before);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
        }
        let (status, bytes) = get_bytes(&app, &format!("/api/v1/beatmaps/{old_id}/download")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, set.payload);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn concurrent_exact_official_archives_commit_one_package_and_keep_the_first_importer() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_concurrent", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (first_owner, _, _, first_cookie) =
            actor(&state, "ArchiveConcurrentA", &["beatmaps.import"]).await;
        let (second_owner, _, _, second_cookie) =
            actor(&state, "ArchiveConcurrentB", &["beatmaps.import"]).await;
        let set = ArchiveSet::new(6300);
        server.add_set(set.id, set.metadata.clone()).await;
        let pause = server.pause_next_set().await;
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let first_app = app.clone();
        let first_payload = set.payload.clone();
        let set_id = set.id;
        let first =
            tokio::spawn(
                async move { import(&first_app, &first_cookie, set_id, &first_payload).await },
            );
        pause.wait_entered().await;
        let second =
            tokio::spawn(async move { import(&app, &second_cookie, set_id, &set.payload).await });
        tokio::time::timeout(Duration::from_secs(5), async {
            while file_count(&state.maps_root).await < 6 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("both requests must prepare their private archive and difficulty files");
        assert!(!first.is_finished() && !second.is_finished());
        assert_eq!(
            server.set_queries().await,
            [set_id],
            "the second request waits for the shared set flight"
        );
        pause.release();
        wait_for_lock(
            &state.db,
            &application,
            "LOCK TABLE roles,user_roles IN SHARE MODE",
        )
        .await;
        blocker.commit().await.unwrap();
        let (first, second) = tokio::join!(first, second);
        let (first_status, first) = first.unwrap();
        let (second_status, second) = second.unwrap();
        assert_eq!(first_status, StatusCode::OK, "{first}");
        assert_eq!(second_status, StatusCode::OK, "{second}");
        assert_eq!(first["package"], second["package"]);
        assert_eq!(ids(&first), ids(&second));
        assert_ne!(first["idempotent"], second["idempotent"]);
        let owner = if first["idempotent"] == false {
            first_owner
        } else {
            second_owner
        };
        let snapshot = snapshot(&state).await;
        assert_eq!(snapshot.archives.as_array().unwrap().len(), 1);
        assert_eq!(snapshot.members.as_array().unwrap().len(), 2);
        assert_eq!(snapshot.maps.as_array().unwrap().len(), 2);
        assert_eq!(snapshot.aliases.as_array().unwrap().len(), 2);
        assert_eq!(snapshot.audits.as_array().unwrap().len(), 1);
        assert_eq!(snapshot.archives[0]["uploaded_by"], owner);
        for map in snapshot.maps.as_array().unwrap() {
            assert_eq!(map["uploaded_by"], owner);
        }
        assert_eq!(file_count(&state.maps_root).await, 3);
        assert_eq!(server.set_queries().await, [set_id]);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_conflicts_and_incomplete_manifests_preserve_all_existing_rows_aliases_and_files()
 {
    assert_archive_conflict_cases(
        "archive_manifests",
        7000,
        &["missing", "extra", "file_id", "file_set", "mode"],
    )
    .await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_identity_asset_and_layout_conflicts_preserve_existing_data() {
    assert_archive_conflict_cases(
        "archive_conflicts",
        7500,
        &[
            "local_checksum",
            "checksum_identity",
            "official_version",
            "unknown_member",
            "repacked_assets",
            "nested_osu",
        ],
    )
    .await;
}

// Independent rejection groups each own a full application/PG/files fixture.
// Keep them below the unchanged production upload limiter's request budget;
// the repacked-assets case makes one successful upload before its rejection.
async fn assert_archive_conflict_cases(
    fixture_name: &str,
    first_set_id: i64,
    failures: &'static [&'static str],
) {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new(fixture_name, server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "ArchiveConflicts", &["beatmaps.import"]).await;
        for (index, &failure) in failures.iter().enumerate() {
            let set = ArchiveSet::new(first_set_id + index as i64 * 100);
            server.add_set(set.id, set.metadata.clone()).await;
            let mut uploaded = set.maps.clone();
            let mut status = StatusCode::CONFLICT;
            let mut code = "official_archive_manifest_mismatch";
            let payload = match failure {
                "missing" => archive(&uploaded[..1], b"original audio", true),
                "extra" => {
                    uploaded.push(ArchiveMap::new(set.id * 10 + 20, set.id, "Extra", "ranked"));
                    archive(&uploaded, b"original audio", true)
                }
                "file_id" | "file_set" | "mode" => {
                    let text = String::from_utf8(uploaded[0].bytes.clone()).unwrap();
                    uploaded[0].bytes = match failure {
                        "file_id" => text.replace(
                            &format!("BeatmapID:{}", uploaded[0].id),
                            &format!("BeatmapID:{}", uploaded[0].id + 99),
                        ),
                        "file_set" => text.replace(
                            &format!("BeatmapSetID:{}", set.id),
                            &format!("BeatmapSetID:{}", set.id + 1),
                        ),
                        _ => text.replace("Mode:0", "Mode:1"),
                    }
                    .into_bytes();
                    archive(&uploaded, b"original audio", true)
                }
                "local_checksum" => {
                    seed_map(&state, owner, &set.maps[0].bytes, "local", None, None).await;
                    code = "official_archive_conflict";
                    set.payload.clone()
                }
                "checksum_identity" => {
                    seed_map(
                        &state,
                        owner,
                        &set.maps[0].bytes,
                        "official",
                        Some(set.maps[0].id + 99),
                        Some(set.id + 9999),
                    )
                    .await;
                    code = "official_archive_conflict";
                    set.payload.clone()
                }
                "official_version" => {
                    let old = String::from_utf8(set.maps[0].bytes.clone())
                        .unwrap()
                        .replace("Version:Easy", "Version:Old version")
                        .into_bytes();
                    seed_map(
                        &state,
                        owner,
                        &old,
                        "official",
                        Some(set.maps[0].id),
                        Some(set.id),
                    )
                    .await;
                    code = "official_version_conflict";
                    set.payload.clone()
                }
                "unknown_member" => {
                    let old = ArchiveMap::new(
                        set.id * 10 + 99,
                        set.id,
                        "Retained old difficulty",
                        "ranked",
                    );
                    seed_map(
                        &state,
                        owner,
                        &old.bytes,
                        "official",
                        Some(old.id),
                        Some(set.id),
                    )
                    .await;
                    code = "official_set_version_conflict";
                    set.payload.clone()
                }
                "repacked_assets" => {
                    let (status, value) = import(&app, &cookie, set.id, &set.payload).await;
                    assert_eq!(status, StatusCode::OK, "{value}");
                    code = "official_set_version_conflict";
                    archive(&uploaded, b"different asset bytes", true)
                }
                "nested_osu" => {
                    status = StatusCode::BAD_REQUEST;
                    code = "official_archive_invalid";
                    nested_map_archive(&uploaded)
                }
                _ => unreachable!(),
            };
            assert_rejected_unchanged(&state, &app, &cookie, set.id, &payload, status, code).await;
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn new_and_exact_retry_official_archives_recheck_queued_session_revocation() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_authority", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        for (index, retry) in [false, true].into_iter().enumerate() {
            let (_, _, sid, cookie) = actor(
                &state,
                &format!("ArchiveAuthority{retry}"),
                &["beatmaps.import"],
            )
            .await;
            let set = ArchiveSet::new(8200 + index as i64 * 100);
            server.add_set(set.id, set.metadata.clone()).await;
            if retry {
                let (status, value) = import(&app, &cookie, set.id, &set.payload).await;
                assert_eq!(status, StatusCode::OK, "{value}");
            }
            let before = snapshot(&state).await;
            let files = files_snapshot(&state.maps_root).await;
            let pause = if retry {
                None
            } else {
                Some(server.pause_next_set().await)
            };
            let mut blocker = state.db.begin().await.unwrap();
            sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
                .execute(&mut *blocker)
                .await
                .unwrap();
            let queued_app = app.clone();
            let set_id = set.id;
            let pending =
                tokio::spawn(
                    async move { import(&queued_app, &cookie, set_id, &set.payload).await },
                );
            if let Some(pause) = pause {
                pause.wait_entered().await;
                pause.release();
            }
            wait_for_lock(
                &state.db,
                &application,
                "LOCK TABLE roles,user_roles IN SHARE MODE",
            )
            .await;
            if !retry {
                assert!(
                    file_count(&state.maps_root).await > files.len(),
                    "fresh files are staged before the commit authority fence"
                );
            }
            sqlx::query("DELETE FROM sessions WHERE id=$1")
                .bind(sid)
                .execute(&mut *blocker)
                .await
                .unwrap();
            blocker.commit().await.unwrap();
            let (status, value) = pending.await.unwrap();
            assert_eq!(status, StatusCode::UNAUTHORIZED, "retry={retry}: {value}");
            assert_eq!(snapshot(&state).await, before);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_upstream_limits_cache_backoff_without_staged_or_published_data() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_upstream", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "ArchiveUpstream", &["beatmaps.import"]).await;
        let limited = ArchiveSet::new(9000);
        server.add_set(limited.id, limited.metadata.clone()).await;
        server.set_rate_limits(1, "1").await;
        for _ in 0..2 {
            assert_rejected_unchanged(
                &state,
                &app,
                &cookie,
                limited.id,
                &limited.payload,
                StatusCode::TOO_MANY_REQUESTS,
                "official_rate_limited",
            )
            .await;
        }
        assert_eq!(
            server.set_queries().await,
            [limited.id],
            "the retry respects the upstream backoff"
        );
        assert_eq!(file_count(&state.maps_root).await, 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_invalid_original_metadata_leaves_no_staged_or_published_data() {
    assert_invalid_metadata_cases(
        "archive_metadata_members",
        9100,
        &[
            "missing",
            "empty",
            "duplicate",
            "convert_true",
            "convert_null",
        ],
    )
    .await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_invalid_metadata_identity_leaves_no_staged_or_published_data() {
    assert_invalid_metadata_cases(
        "archive_metadata_identity",
        9600,
        &["convert_string", "deleted", "checksum", "mode", "wrong_set"],
    )
    .await;
}

async fn assert_invalid_metadata_cases(
    fixture_name: &str,
    first_set_id: i64,
    failures: &'static [&'static str],
) {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new(fixture_name, server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "ArchiveMetadata", &["beatmaps.import"]).await;
        for (index, &failure) in failures.iter().enumerate() {
            let set = ArchiveSet::new(first_set_id + index as i64 * 100);
            let mut metadata = set.metadata.clone();
            match failure {
                "missing" => {
                    metadata.as_object_mut().unwrap().remove("beatmaps");
                }
                "empty" => metadata["beatmaps"] = json!([]),
                "duplicate" => {
                    metadata["beatmaps"] = json!([
                        metadata["beatmaps"][0].clone(),
                        metadata["beatmaps"][0].clone()
                    ])
                }
                "convert_true" => metadata["beatmaps"][0]["convert"] = Value::Bool(true),
                "convert_null" => metadata["beatmaps"][0]["convert"] = Value::Null,
                "convert_string" => metadata["beatmaps"][0]["convert"] = json!("false"),
                "deleted" => metadata["beatmaps"][0]["deleted_at"] = json!("2026-10-08T00:00:00Z"),
                "checksum" => metadata["beatmaps"][0]["checksum"] = json!("g".repeat(32)),
                "mode" => metadata["beatmaps"][0]["mode_int"] = json!(99),
                "wrong_set" => metadata["id"] = json!(set.id + 1),
                _ => unreachable!(),
            }
            server.add_set(set.id, metadata).await;
            assert_rejected_unchanged(
                &state,
                &app,
                &cookie,
                set.id,
                &set.payload,
                StatusCode::BAD_GATEWAY,
                "official_identity_mismatch",
            )
            .await;
        }
        assert_eq!(file_count(&state.maps_root).await, 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_deferred_commit_rejection_rolls_back_members_aliases_audit_and_owned_files()
 {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_commit", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "ArchiveCommit", &["beatmaps.import"]).await;
        let set = ArchiveSet::new(10300);
        server.add_set(set.id, set.metadata.clone()).await;
        reject_audits_at_commit(&state.db).await;
        assert_rejected_unchanged(
            &state,
            &app,
            &cookie,
            set.id,
            &set.payload,
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
        )
        .await;
        assert_eq!(file_count(&state.maps_root).await, 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_archive_permission_and_owned_storage_failures_cannot_publish_rows_or_remove_unowned_bytes()
 {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("archive_storage", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, denied_cookie) = actor(&state, "ArchiveDenied", &[]).await;
        let (_, _, _, cookie) = actor(&state, "ArchiveStorage", &["beatmaps.import"]).await;
        let set = ArchiveSet::new(10400);
        server.add_set(set.id, set.metadata.clone()).await;
        let before_auth = snapshot(&state).await;
        let (status, value) = request(
            &app,
            IMPORT_PATH,
            None,
            false,
            MULTIPART,
            multipart_archive(set.id, &set.payload),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(value["error"]["code"], "unauthorized");
        assert_eq!(snapshot(&state).await, before_auth);
        assert_eq!(file_count(&state.maps_root).await, 0);
        assert!(
            server.set_queries().await.is_empty(),
            "missing session precedes upstream work"
        );
        assert_rejected_unchanged(
            &state,
            &app,
            &denied_cookie,
            set.id,
            &set.payload,
            StatusCode::FORBIDDEN,
            "forbidden",
        )
        .await;
        assert!(
            server.set_queries().await.is_empty(),
            "permission denial precedes upstream work"
        );
        let before = snapshot(&state).await;
        // This path belongs exclusively to the disposable fixture. Making it a
        // regular file forces deterministic staging failure without permissions
        // changes, external paths, or an unsafe database transport disruption.
        tokio::fs::remove_dir(&state.maps_root).await.unwrap();
        let marker = b"owned fixture storage blocker; not owned by this import";
        tokio::fs::write(&state.maps_root, marker).await.unwrap();
        let (status, value) = import(&app, &cookie, set.id, &set.payload).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{value}");
        assert_eq!(value["error"]["code"], "internal_error");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(tokio::fs::read(&state.maps_root).await.unwrap(), marker);
    })
    .await;
    fixture.finish(result).await;
}
