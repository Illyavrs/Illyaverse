//! Local package fixtures own a disposable PG17 schema and synthetic map files.
use super::map_mutation_tests::{
    MULTIPART, MutationFixture, actor, multipart, osu_bytes, request, wait_for_lock,
};
use crate::{AppState, Router};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Cursor, path::Path, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

fn difficulty(title: &str, version: &str) -> Vec<u8> {
    String::from_utf8(osu_bytes(title))
        .unwrap()
        .replace(
            "Version:Easy",
            &format!("Version:{version}\nBeatmapID:-1\nBeatmapSetID:-1"),
        )
        .into_bytes()
}

fn with_official_identity(bytes: Vec<u8>, id: i64, set_id: i64) -> Vec<u8> {
    String::from_utf8(bytes)
        .unwrap()
        .replace("BeatmapID:-1", &format!("BeatmapID:{id}"))
        .replace("BeatmapSetID:-1", &format!("BeatmapSetID:{set_id}"))
        .into_bytes()
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

async fn upload(app: &Router, cookie: &str, filename: &str, bytes: &[u8]) -> (StatusCode, Value) {
    request(
        app,
        "/api/v1/beatmaps/upload",
        Some(cookie),
        false,
        MULTIPART,
        multipart(filename, bytes),
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

async fn get_json(app: &Router, path: &str) -> Value {
    let (status, bytes) = get_bytes(app, path).await;
    assert_eq!(status, StatusCode::OK, "{path}");
    serde_json::from_slice(&bytes).unwrap()
}

fn ids(value: &Value) -> Vec<i64> {
    let mut ids = value["items"]
        .as_array()
        .expect("upload preserves the items array")
        .iter()
        .map(|item| item["id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn package_id(value: &Value) -> Uuid {
    Uuid::parse_str(value["package"]["id"].as_str().unwrap()).unwrap()
}

fn assert_public_package(value: &Value, kind: &str, count: usize, legacy: bool) {
    let package = &value["package"];
    let _ = package_id(value);
    assert!((2_000_000_000..=2_147_483_647).contains(&package["native_set_id"].as_i64().unwrap()));
    assert_eq!(package["kind"], kind);
    assert_eq!(package["legacy"], legacy);
    assert_eq!(package["difficulty_count"], count);
    assert_eq!(package["beatmap_ids"], serde_json::json!(ids(value)));
    assert!(package["download_url"].as_str().is_some());
    for internal in ["sha256", "member_md5", "archive_path", "osu_path"] {
        assert!(
            package.get(internal).is_none(),
            "private package field {internal}"
        );
    }
    for item in value["items"].as_array().unwrap() {
        assert_eq!(item["package"], *package);
        assert!((2_000_000_000..=2_147_483_647).contains(&item["native_id"].as_i64().unwrap()));
        assert_eq!(item["native_set_id"], package["native_set_id"]);
        assert!(item.get("osu_path").is_none());
        assert!(item.get("archive_path").is_none());
    }
}

#[derive(Debug, PartialEq)]
struct DatabaseSnapshot {
    maps: Value,
    packages: Value,
    audits: Value,
}

async fn database_snapshot(state: &AppState) -> DatabaseSnapshot {
    let maps = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(b) ORDER BY id),'[]'::jsonb) FROM beatmaps b",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let packages = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(p) ORDER BY id),'[]'::jsonb) FROM beatmap_packages p",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let audits = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY id),'[]'::jsonb) FROM audit_events a",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    DatabaseSnapshot {
        maps,
        packages,
        audits,
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

async fn package_row(state: &AppState, id: Uuid) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(p) FROM beatmap_packages p WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

async fn seed_legacy(
    state: &AppState,
    owner: i64,
    kind: &str,
    payload: &[u8],
    maps: &[Vec<u8>],
) -> (Uuid, Vec<i64>) {
    // Seed the post-migration shape directly. A verified modern package cannot
    // be downgraded to legacy because its fingerprint is immutable.
    let package = Uuid::new_v4();
    let directory = state.maps_root.join(package.to_string());
    tokio::fs::create_dir(&directory).await.unwrap();
    let archive_path = (kind == "osz").then(|| format!("{package}/package.osz"));
    if let Some(path) = &archive_path {
        tokio::fs::write(state.maps_root.join(path), payload)
            .await
            .unwrap();
    }
    let mut manifest = maps
        .iter()
        .map(|bytes| format!("{:x}", md5::compute(bytes)))
        .collect::<Vec<_>>();
    manifest.sort_unstable();
    let mut tx = state.db.begin().await.unwrap();
    sqlx::query("INSERT INTO beatmap_packages(id,uploaded_by,kind,member_md5,archive_path,legacy) VALUES($1,$2,$3,$4,$5,true)")
        .bind(package).bind(owner).bind(kind).bind(&manifest).bind(&archive_path).execute(&mut *tx).await.unwrap();
    let mut map_ids = Vec::new();
    for bytes in maps {
        let map = super::parse_map(bytes.clone()).unwrap();
        let path = format!("{package}/{}.osu", map.md5);
        tokio::fs::write(state.maps_root.join(&path), &map.bytes)
            .await
            .unwrap();
        let id = sqlx::query_scalar("INSERT INTO beatmaps(local_package_id,uploaded_by,title,artist,creator,version,mode,status,stars,bpm,length_seconds,max_combo,md5,osu_path,archive_path) VALUES($1,$2,$3,$4,$5,$6,$7,'loved',$8,$9,$10,$11,$12,$13,$14) RETURNING id")
            .bind(package).bind(owner).bind(&map.title).bind(&map.artist).bind(&map.creator).bind(&map.version).bind(map.mode).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(&map.md5).bind(&path).bind(&archive_path).fetch_one(&mut *tx).await.unwrap();
        map_ids.push(id);
    }
    map_ids.sort_unstable();
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.upload','beatmap_package',$2,$3)")
        .bind(owner).bind(package.to_string()).bind(serde_json::json!({"beatmap_ids":map_ids})).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (package, map_ids)
}

async fn wait_for_two_guards(state: &AppState, application: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE '%LOCK TABLE roles,user_roles IN SHARE MODE%'")
                .bind(application).fetch_one(&state.db).await.unwrap();
            if waiting >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both package requests must reach the authorization fence");
}

async fn assert_conflict_unchanged(
    state: &AppState,
    app: &Router,
    cookie: &str,
    filename: &str,
    payload: &[u8],
) {
    let before = database_snapshot(state).await;
    let files = files_snapshot(&state.maps_root).await;
    let (status, value) = upload(app, cookie, filename, payload).await;
    assert_eq!(status, StatusCode::CONFLICT, "{filename}: {value}");
    assert_eq!(value["error"]["code"], "map_package_conflict", "{value}");
    assert_eq!(database_snapshot(state).await, before);
    assert_eq!(files_snapshot(&state.maps_root).await, files);
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn local_osz_package_and_exact_retry_preserve_durable_identity() {
    let fixture = MutationFixture::new("package_exact", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "PackageExact", &["beatmaps.upload"]).await;
        let easy = difficulty("Durable set", "Easy");
        let hard = difficulty("Durable set", "Hard");
        let payload = archive(&[
            ("set/hard.osu", &hard),
            ("set/easy.osu", &easy),
            ("set/audio.mp3", b"owned synthetic audio asset"),
        ]);
        let (status, created) = upload(&app, &cookie, "durable.osz", &payload).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        assert_eq!(created["idempotent"], false);
        assert_public_package(&created, "osz", 2, false);
        let package = package_id(&created);
        let row = package_row(&state, package).await;
        assert_eq!(row["uploaded_by"], owner);
        assert_eq!(row["kind"], "osz");
        assert_eq!(row["byte_length"], payload.len());
        assert_eq!(row["sha256"], format!("{:x}", Sha256::digest(&payload)));
        let mut manifest = vec![format!("{:x}", md5::compute(&easy)), format!("{:x}", md5::compute(&hard))];
        manifest.sort_unstable();
        assert_eq!(row["member_md5"], serde_json::json!(manifest));
        assert_eq!(row["legacy"], false);
        assert!(row["archive_path"].as_str().is_some());
        let snapshot = database_snapshot(&state).await;
        assert_eq!(snapshot.maps.as_array().unwrap().len(), 2);
        assert_eq!(snapshot.packages.as_array().unwrap().len(), 1);
        assert_eq!(snapshot.audits.as_array().unwrap().len(), 1);
        let mut native_ids = Vec::new();
        for map in snapshot.maps.as_array().unwrap() {
            assert_eq!(map["local_package_id"], package.to_string());
            assert_eq!(map["native_set_id"], row["native_set_id"]);
            assert_eq!(map["uploaded_by"], owner);
            assert_eq!(map["source"], "local");
            assert!(map["official_id"].is_null());
            assert!(map["official_set_id"].is_null());
            let native_id = map["native_id"].as_i64().unwrap();
            assert!((2_000_000_000..=2_147_483_647).contains(&native_id));
            native_ids.push(native_id);
        }
        native_ids.sort_unstable();
        native_ids.dedup();
        assert_eq!(native_ids.len(), 2);
        for item in created["items"].as_array().unwrap() {
            let id = item["id"].as_i64().unwrap();
            let detail = get_json(&app, &format!("/api/v1/beatmaps/{id}")).await;
            assert_eq!(detail["package"], created["package"]);
            let (status, downloaded) = get_bytes(&app, item["download_url"].as_str().unwrap()).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(downloaded, payload);
            let (status, osu) = get_bytes(&app, &format!("/api/v1/beatmaps/{id}/osu")).await;
            assert_eq!(status, StatusCode::OK);
            assert!(osu == easy || osu == hard);
        }
        let list = get_json(&app, "/api/v1/beatmaps?page_size=50").await;
        for item in list["items"].as_array().unwrap() {
            assert_eq!(item["package"], created["package"]);
        }
        let (status, downloaded) = get_bytes(&app, created["package"]["download_url"].as_str().unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(downloaded, payload);

        // A retry returns the moderator's current states, without recreating rows.
        let map_ids = ids(&created);
        sqlx::query("UPDATE beatmaps SET status=CASE WHEN id=$1 THEN 'ranked' ELSE 'unranked' END WHERE local_package_id=$2")
            .bind(map_ids[0]).bind(package).execute(&state.db).await.unwrap();
        let before = database_snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let (status, retried) = upload(&app, &cookie, "renamed.osz", &payload).await;
        assert_eq!(status, StatusCode::OK, "{retried}");
        assert_eq!(retried["idempotent"], true);
        assert_public_package(&retried, "osz", 2, false);
        assert_eq!(retried["package"], created["package"]);
        assert_eq!(ids(&retried), map_ids);
        for item in retried["items"].as_array().unwrap() {
            assert_eq!(item["status"], if item["id"] == map_ids[0] {"ranked"} else {"unranked"});
            assert_eq!(item["uploaded_by"], owner);
        }
        assert_eq!(database_snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(files.len(), 3, "one archive and two difficulty files are retained");
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn local_osu_exact_retry_is_a_single_difficulty_package() {
    let fixture = MutationFixture::new("package_osu", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "PackageOsu", &["beatmaps.upload"]).await;
        let payload = difficulty("Standalone package", "Normal");
        let (status, created) = upload(&app, &cookie, "standalone.osu", &payload).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        assert_public_package(&created, "osu", 1, false);
        assert_eq!(created["idempotent"], false);
        let row = package_row(&state, package_id(&created)).await;
        assert!(row["archive_path"].is_null());
        assert_eq!(row["sha256"], format!("{:x}", Sha256::digest(&payload)));
        let (status, downloaded) =
            get_bytes(&app, created["package"]["download_url"].as_str().unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(downloaded, payload);
        let before = database_snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let (status, retried) = upload(&app, &cookie, "same.osu", &payload).await;
        assert_eq!(status, StatusCode::OK, "{retried}");
        assert_eq!(retried["idempotent"], true);
        assert_eq!(retried["package"], created["package"]);
        assert_eq!(ids(&retried), ids(&created));
        assert_eq!(database_snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(files.len(), 1);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn package_overlap_owner_and_asset_conflicts_leave_every_row_and_file_unchanged() {
    let fixture = MutationFixture::new("package_conflict", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, owner) = actor(&state, "PackageOwner", &["beatmaps.upload"]).await;
        let (_, _, _, other) = actor(&state, "PackageOther", &["beatmaps.upload"]).await;
        let easy = difficulty("Conflict package", "Easy");
        let hard = difficulty("Conflict package", "Hard");
        let extra = difficulty("Conflict package", "Extra");
        let original = archive(&[
            ("easy.osu", &easy),
            ("hard.osu", &hard),
            ("audio.mp3", b"original asset"),
        ]);
        let (status, created) = upload(&app, &owner, "original.osz", &original).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        assert_conflict_unchanged(&state, &app, &other, "different-owner.osz", &original).await;
        let partial = archive(&[("easy.osu", &easy), ("extra.osu", &extra)]);
        assert_conflict_unchanged(&state, &app, &owner, "partial-overlap.osz", &partial).await;
        let repacked = archive(&[
            ("easy.osu", &easy),
            ("hard.osu", &hard),
            ("audio.mp3", b"different asset"),
        ]);
        assert_conflict_unchanged(&state, &app, &owner, "changed-assets.osz", &repacked).await;
        // A difficulty member is not an exact retry of its enclosing archive.
        assert_conflict_unchanged(&state, &app, &owner, "member.osu", &easy).await;
        assert_eq!(
            database_snapshot(&state)
                .await
                .maps
                .as_array()
                .unwrap()
                .len(),
            2
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn duplicate_archive_checksums_are_rejected_before_any_package_is_published() {
    let fixture = MutationFixture::new("package_duplicate", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "PackageDuplicate", &["beatmaps.upload"]).await;
        let map = difficulty("Duplicated checksum", "Easy");
        let payload = archive(&[("first.osu", &map), ("nested/second.osu", &map)]);
        let before = database_snapshot(&state).await;
        let (status, value) = upload(&app, &cookie, "duplicate.osz", &payload).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
        assert_eq!(database_snapshot(&state).await, before);
        assert!(files_snapshot(&state.maps_root).await.is_empty());
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn new_local_packages_with_positive_official_metadata_require_import_before_publication() {
    let fixture =
        MutationFixture::new("package_official_metadata", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) =
            actor(&state, "PackageOfficialMetadata", &["beatmaps.upload"]).await;
        let private = difficulty("Existing private package", "Easy");
        let (status, created) = upload(&app, &cookie, "private.osu", &private).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let before = database_snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        for (field, id, set_id) in [("map", 12345, -1), ("set", -1, 67890)] {
            for kind in ["osu", "osz"] {
                let official = with_official_identity(
                    difficulty(&format!("Positive official {field} {kind}"), "Easy"),
                    id,
                    set_id,
                );
                let sibling = difficulty(&format!("Private sibling {field} {kind}"), "Hard");
                let payload = if kind == "osu" {
                    official
                } else {
                    // The otherwise valid sibling cannot survive rejection of
                    // the official member, regardless of archive entry order.
                    archive(&[("private.osu", &sibling), ("official.osu", &official)])
                };
                let (status, value) =
                    upload(&app, &cookie, &format!("official.{kind}"), &payload).await;
                assert_eq!(
                    status,
                    StatusCode::BAD_REQUEST,
                    "positive {field}/{kind}: {value}"
                );
                assert_eq!(value["error"]["code"], "official_beatmap_requires_import");
                assert_eq!(database_snapshot(&state).await, before);
                assert_eq!(files_snapshot(&state.maps_root).await, files);
            }
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn concurrent_exact_uploads_commit_one_package_and_return_the_same_ids() {
    let fixture = MutationFixture::new("package_concurrent", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "PackageConcurrent", &["beatmaps.upload"]).await;
        let easy = difficulty("Concurrent exact", "Easy");
        let hard = difficulty("Concurrent exact", "Hard");
        let payload = archive(&[("easy.osu", &easy), ("hard.osu", &hard)]);
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let first_app = app.clone();
        let first_cookie = cookie.clone();
        let first_payload = payload.clone();
        let first = tokio::spawn(async move {
            upload(&first_app, &first_cookie, "first.osz", &first_payload).await
        });
        let second =
            tokio::spawn(async move { upload(&app, &cookie, "second.osz", &payload).await });
        wait_for_two_guards(&state, &application).await;
        blocker.commit().await.unwrap();
        let (first_status, first) = first.await.unwrap();
        let (second_status, second) = second.await.unwrap();
        assert_eq!(first_status, StatusCode::OK, "{first}");
        assert_eq!(second_status, StatusCode::OK, "{second}");
        assert_eq!(ids(&first), ids(&second));
        assert_eq!(first["package"], second["package"]);
        let retry_flags = [
            first["idempotent"].as_bool().unwrap(),
            second["idempotent"].as_bool().unwrap(),
        ];
        assert_eq!(retry_flags.iter().filter(|&&flag| flag).count(), 1);
        let snapshot = database_snapshot(&state).await;
        assert_eq!(snapshot.maps.as_array().unwrap().len(), 2);
        assert_eq!(snapshot.packages.as_array().unwrap().len(), 1);
        assert_eq!(snapshot.audits.as_array().unwrap().len(), 1);
        assert_eq!(files_snapshot(&state.maps_root).await.len(), 3);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn opposite_archive_orders_from_different_owners_return_a_conflict_without_deadlock() {
    let fixture = MutationFixture::new("package_reverse", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (first_owner, _, _, first_cookie) =
            actor(&state, "PackageReverseA", &["beatmaps.upload"]).await;
        let (second_owner, _, _, second_cookie) =
            actor(&state, "PackageReverseB", &["beatmaps.upload"]).await;
        let easy = difficulty("Reverse overlap", "Easy");
        let hard = difficulty("Reverse overlap", "Hard");
        let first_payload = archive(&[("easy.osu", &easy), ("hard.osu", &hard)]);
        let second_payload = archive(&[("hard.osu", &hard), ("easy.osu", &easy)]);
        assert_ne!(first_payload, second_payload);
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            upload(&first_app, &first_cookie, "forward.osz", &first_payload).await
        });
        let second = tokio::spawn(async move {
            upload(&app, &second_cookie, "reverse.osz", &second_payload).await
        });
        wait_for_two_guards(&state, &application).await;
        blocker.commit().await.unwrap();
        let (first, second) = tokio::time::timeout(Duration::from_secs(10), async {
            (first.await.unwrap(), second.await.unwrap())
        })
        .await
        .expect("overlapping uploads must finish without a lock cycle");
        let winner = match (first.0, second.0) {
            (StatusCode::OK, StatusCode::CONFLICT) => {
                assert_eq!(second.1["error"]["code"], "map_package_conflict");
                first_owner
            }
            (StatusCode::CONFLICT, StatusCode::OK) => {
                assert_eq!(first.1["error"]["code"], "map_package_conflict");
                second_owner
            }
            _ => panic!("expected one complete package and one conflict: {first:?}, {second:?}"),
        };
        let snapshot = database_snapshot(&state).await;
        assert_eq!(snapshot.maps.as_array().unwrap().len(), 2);
        assert_eq!(snapshot.packages.as_array().unwrap().len(), 1);
        assert_eq!(snapshot.audits.as_array().unwrap().len(), 1);
        assert_eq!(snapshot.packages[0]["uploaded_by"], winner);
        for map in snapshot.maps.as_array().unwrap() {
            assert_eq!(map["uploaded_by"], winner);
        }
        assert_eq!(files_snapshot(&state.maps_root).await.len(), 3);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn legacy_exact_retries_hydrate_only_from_retained_payload_without_recreating_content() {
    let fixture = MutationFixture::new("package_legacy", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "PackageLegacy", &["beatmaps.upload"]).await;
        for kind in ["osu", "osz"] {
            let easy = difficulty(&format!("Legacy {kind}"), "Easy");
            // Historical positive metadata is retained byte-for-byte. A full,
            // same-owner legacy retry must remain valid while new uploads of
            // official content are directed to the official import route.
            let easy = if kind == "osu" {
                with_official_identity(easy, 12345, 67890)
            } else {
                easy
            };
            let hard = difficulty(&format!("Legacy {kind}"), "Hard");
            let payload = if kind == "osu" {
                easy.clone()
            } else {
                archive(&[
                    ("easy.osu", &easy),
                    ("hard.osu", &hard),
                    ("audio.mp3", b"legacy retained asset"),
                ])
            };
            let filename = format!("legacy.{kind}");
            let maps = if kind == "osu" {
                vec![easy]
            } else {
                vec![easy, hard]
            };
            let (package, map_ids) = seed_legacy(&state, owner, kind, &payload, &maps).await;
            let before = database_snapshot(&state).await;
            let before_package = package_row(&state, package).await;
            let files = files_snapshot(&state.maps_root).await;
            let (status, retried) = upload(&app, &cookie, &filename, &payload).await;
            assert_eq!(status, StatusCode::OK, "{retried}");
            assert_eq!(retried["idempotent"], true);
            assert_public_package(&retried, kind, if kind == "osu" { 1 } else { 2 }, false);
            assert_eq!(package_id(&retried), package);
            assert_eq!(ids(&retried), map_ids);
            for map in retried["items"].as_array().unwrap() {
                assert_eq!(map["status"], "loved");
                assert!(map["official_id"].is_null());
                assert!(map["official_set_id"].is_null());
            }
            let after = database_snapshot(&state).await;
            assert_eq!(after.maps, before.maps);
            assert_eq!(after.audits, before.audits);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
            let mut hydrated = package_row(&state, package).await;
            assert_eq!(
                hydrated["sha256"],
                format!("{:x}", Sha256::digest(&payload))
            );
            assert_eq!(hydrated["byte_length"], payload.len());
            assert_eq!(hydrated["legacy"], false);
            hydrated["sha256"] = Value::Null;
            hydrated["byte_length"] = Value::Null;
            hydrated["legacy"] = Value::Bool(true);
            assert_eq!(
                hydrated, before_package,
                "hydration changes only verified fingerprint, byte length, and legacy flag"
            );
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn legacy_missing_mismatched_or_incomplete_content_rejects_retry_and_cleans_staging() {
    let fixture = MutationFixture::new("package_legacy_bad", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "PackageLegacyBad", &["beatmaps.upload"]).await;
        for failure in ["missing", "mismatch", "incomplete"] {
            let easy = difficulty(&format!("Legacy failure {failure}"), "Easy");
            let hard = difficulty(&format!("Legacy failure {failure}"), "Hard");
            let payload = archive(&[
                ("easy.osu", &easy),
                ("hard.osu", &hard),
                ("audio.mp3", b"original asset"),
            ]);
            let (package, map_ids) = seed_legacy(
                &state,
                owner,
                "osz",
                &payload,
                &[easy.clone(), hard.clone()],
            )
            .await;
            let row = package_row(&state, package).await;
            let retained = state.maps_root.join(row["archive_path"].as_str().unwrap());
            match failure {
                "missing" => tokio::fs::remove_file(retained).await.unwrap(),
                "mismatch" => {
                    let changed = archive(&[
                        ("easy.osu", &easy),
                        ("hard.osu", &hard),
                        ("audio.mp3", b"changed retained asset"),
                    ]);
                    tokio::fs::write(retained, changed).await.unwrap();
                }
                "incomplete" => {
                    sqlx::query("DELETE FROM beatmaps WHERE id=$1")
                        .bind(map_ids[0])
                        .execute(&state.db)
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert_conflict_unchanged(&state, &app, &cookie, "legacy.osz", &payload).await;
            assert!(package_row(&state, package).await["sha256"].is_null());
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn queued_exact_and_legacy_retries_recheck_the_actor_before_returning_or_hydrating() {
    let fixture = MutationFixture::new("package_authority", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        for legacy in [false, true] {
            let (owner, _, sid, cookie) = actor(
                &state,
                &format!("PackageAuthority{legacy}"),
                &["beatmaps.upload"],
            )
            .await;
            let payload = difficulty(&format!("Queued package {legacy}"), "Easy");
            if legacy {
                seed_legacy(
                    &state,
                    owner,
                    "osu",
                    &payload,
                    std::slice::from_ref(&payload),
                )
                .await;
            } else {
                let (status, created) = upload(&app, &cookie, "queued.osu", &payload).await;
                assert_eq!(status, StatusCode::OK, "{created}");
            }
            let before = database_snapshot(&state).await;
            let files = files_snapshot(&state.maps_root).await;
            let mut blocker = state.db.begin().await.unwrap();
            sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
                .execute(&mut *blocker)
                .await
                .unwrap();
            let queued_app = app.clone();
            let pending =
                tokio::spawn(
                    async move { upload(&queued_app, &cookie, "queued.osu", &payload).await },
                );
            wait_for_lock(
                &state.db,
                &application,
                "LOCK TABLE roles,user_roles IN SHARE MODE",
            )
            .await;
            sqlx::query("DELETE FROM sessions WHERE id=$1")
                .bind(sid)
                .execute(&mut *blocker)
                .await
                .unwrap();
            blocker.commit().await.unwrap();
            let (status, value) = pending.await.unwrap();
            assert_eq!(status, StatusCode::UNAUTHORIZED, "legacy={legacy}: {value}");
            assert_eq!(database_snapshot(&state).await, before);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn reserved_official_selectors_cannot_claim_local_identity_or_contact_upstream() {
    // A network request to this closed loopback port cannot produce the expected
    // namespace response. Both entry points must reject the selector locally.
    let fixture = MutationFixture::new("package_namespace", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(
            &state,
            "PackageNamespace",
            &["beatmaps.upload", "beatmaps.import"],
        )
        .await;
        let local = difficulty("Reserved identity belongs to local content", "Easy");
        let (status, created) = upload(&app, &cookie, "local.osu", &local).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        assert_eq!(created["items"][0]["native_id"], 2_000_000_000_i64);
        let before = database_snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        for path in [
            "/api/v1/beatmaps/import",
            "/api/v1/internal/beatmaps/resolve",
        ] {
            let service = path.ends_with("/resolve");
            let body = if service {
                serde_json::json!({"user_id":user,"official_id":2_000_000_000_i64})
            } else {
                serde_json::json!({"official_id":2_000_000_000_i64})
            };
            let (status, value) = request(
                &app,
                path,
                Some(&cookie),
                service,
                "application/json",
                body.to_string().into_bytes(),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "{path}: {value}");
            assert_eq!(value["error"]["code"], "official_namespace_conflict");
            assert_eq!(database_snapshot(&state).await, before);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn checksum_discovery_rejects_reserved_official_metadata_before_fetching_or_storing_files() {
    let server = crate::native_map_tests::OfficialServer::start().await;
    let fixture = MutationFixture::new("package_upstream_namespace", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "PackageUpstreamNamespace", &["beatmaps.import"]).await;
        let before = database_snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        // The first map has an allowed ID but a reserved set ID (id + 1000).
        // The second also has a reserved difficulty ID. Reuse the existing
        // synthetic upstream server instead of creating another protocol stub.
        for id in [1_999_999_999_i64, 2_000_000_000_i64] {
            let map = crate::native_map_tests::OfficialMap::new(id, "ranked");
            let checksum = map.checksum();
            server.add(map).await;
            let (status, value) = request(
                &app,
                "/api/v1/internal/beatmaps/resolve",
                None,
                true,
                "application/json",
                serde_json::json!({"user_id":user,"checksum":checksum})
                    .to_string()
                    .into_bytes(),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT, "official ID {id}: {value}");
            assert_eq!(value["error"]["code"], "official_namespace_conflict");
            assert_eq!(database_snapshot(&state).await, before);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
        }
        assert_eq!(
            server.counters(),
            [1, 2, 0, 0],
            "only OAuth and checksum metadata lookups are allowed"
        );
    })
    .await;
    fixture.finish(result).await;
}
