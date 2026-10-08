//! Filename fixtures use only an owned disposable PG17 schema and synthetic maps.
use crate::{
    AppState, Router,
    maps::map_mutation_tests::{
        MULTIPART, MutationFixture, actor, file_count, multipart, osu_bytes,
        reject_audits_at_commit, request, wait_for_lock,
    },
    native_map_tests::{OfficialMap, OfficialServer},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Cursor, path::Path, time::Duration};
use tower::ServiceExt;

const RESOLVE_PATH: &str = "/api/v1/internal/beatmaps/resolve";

fn local_map(title: &str, version: &str) -> Vec<u8> {
    String::from_utf8(osu_bytes(title))
        .unwrap()
        .replace("Version:Easy", &format!("Version:{version}"))
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

async fn resolve_filename(app: &Router, user: i64, filename: &str) -> (StatusCode, Value) {
    request(
        app,
        RESOLVE_PATH,
        None,
        true,
        "application/json",
        json!({"user_id":user,"filename":filename})
            .to_string()
            .into_bytes(),
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

fn map_id(upload: &Value, checksum: &str) -> i64 {
    upload["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|map| map["md5"] == checksum)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

fn assert_public_resolution(value: &Value, id: i64, checksum: &str, source: &str) {
    let mut keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "id",
            "md5",
            "mode",
            "native_id",
            "native_set_id",
            "official_id",
            "official_set_id",
            "source",
            "status",
        ],
        "filename lookup returns only the public nine-field resolution"
    );
    assert_eq!(value["id"], id);
    assert_eq!(value["md5"], checksum);
    assert_eq!(value["source"], source);
    if source == "local" {
        assert!(value["official_id"].is_null());
        assert!(value["official_set_id"].is_null());
        for field in ["native_id", "native_set_id"] {
            assert!((2_000_000_000..=2_147_483_647).contains(&value[field].as_i64().unwrap()));
        }
    } else {
        assert_eq!(value["native_id"], value["official_id"]);
        assert_eq!(value["native_set_id"], value["official_set_id"]);
        for field in ["native_id", "native_set_id"] {
            assert!((1..=1_999_999_999).contains(&value[field].as_i64().unwrap()));
        }
    }
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    maps: Value,
    packages: Value,
    aliases: Value,
    audits: Value,
}

async fn snapshot(state: &AppState) -> Snapshot {
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
    let aliases = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY filename COLLATE \"C\",beatmap_id),'[]'::jsonb) FROM beatmap_filename_aliases a")
        .fetch_one(&state.db).await.unwrap();
    let audits = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY id),'[]'::jsonb) FROM audit_events a",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    Snapshot {
        maps,
        packages,
        aliases,
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

async fn alias_ids(state: &AppState, filename: &str) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT beatmap_id FROM beatmap_filename_aliases WHERE filename=$1 ORDER BY beatmap_id",
    )
    .bind(filename)
    .fetch_all(&state.db)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn local_standalone_filenames_preserve_case_unicode_and_current_map_authority() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_exact", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(&state, "FilenameExact", &["beatmaps.upload"]).await;
        let mut expected = Vec::new();
        for (index, filename) in [
            "Exact [Easy].OSU",
            "exact [Easy].osu",
            "café [Hard].osu",
            "cafe\u{301} [Hard].osu",
        ]
        .into_iter()
        .enumerate()
        {
            let bytes = local_map(&format!("Exact filename {index}"), "Easy");
            let checksum = format!("{:x}", md5::compute(&bytes));
            let (status, value) = upload(&app, &cookie, filename, &bytes).await;
            assert_eq!(status, StatusCode::OK, "{filename}: {value}");
            let id = map_id(&value, &checksum);
            assert_eq!(alias_ids(&state, filename).await, [id]);
            expected.push((filename, checksum, id, bytes));
        }
        // A stored alias returns the moderator's current state without fetching
        // or auditing official metadata, even when the filename is unusual.
        sqlx::query("UPDATE beatmaps SET status='loved' WHERE id=$1")
            .bind(expected[0].2)
            .execute(&state.db)
            .await
            .unwrap();
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        for (filename, checksum, id, bytes) in expected {
            let (status, value) = resolve_filename(&app, user, filename).await;
            assert_eq!(status, StatusCode::OK, "{filename}: {value}");
            assert_public_resolution(&value, id, &checksum, "local");
            if filename == "Exact [Easy].OSU" {
                assert_eq!(value["status"], "loved");
            }
            let (status, downloaded) =
                get_bytes(&app, &format!("/api/v1/beatmaps/{id}/download")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(downloaded, bytes);
        }
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(
            server.counters(),
            [0; 4],
            "local aliases never contact upstream"
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn archive_basenames_bind_their_actual_difficulty_after_checksum_sorting() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_archive", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(&state, "FilenameArchive", &["beatmaps.upload"]).await;
        let easy = local_map("Filename archive", "Easy");
        let hard = local_map("Filename archive", "Hard");
        let easy_name = "雪 [Easy].OSU";
        let hard_name = "Hard & +% [Hard].osu";
        let bytes = archive(&[
            ("z-folder/Hard & +% [Hard].osu", &hard),
            ("a-folder/雪 [Easy].OSU", &easy),
            ("audio.mp3", b"owned synthetic archive asset"),
        ]);
        let (status, value) = upload(&app, &cookie, "package.osz", &bytes).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        assert_eq!(before.aliases.as_array().unwrap().len(), 2);
        for (filename, raw) in [(easy_name, &easy), (hard_name, &hard)] {
            let checksum = format!("{:x}", md5::compute(raw));
            let id = map_id(&value, &checksum);
            assert_eq!(alias_ids(&state, filename).await, [id]);
            let (status, resolved) = resolve_filename(&app, user, filename).await;
            assert_eq!(status, StatusCode::OK, "{resolved}");
            assert_public_resolution(&resolved, id, &checksum, "local");
            let (status, downloaded) =
                get_bytes(&app, &format!("/api/v1/beatmaps/{id}/download")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(downloaded, bytes);
            let (status, downloaded) = get_bytes(&app, &format!("/api/v1/beatmaps/{id}/osu")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(downloaded, *raw);
        }
        for path in [
            "z-folder/Hard & +% [Hard].osu",
            "a-folder/雪 [Easy].OSU",
            "package.osz",
        ] {
            assert!(alias_ids(&state, path).await.is_empty());
        }
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(server.counters(), [0; 4]);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn cp932_archive_aliases_preserve_unicode_original_bytes_and_exact_retry() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_cp932_archive", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) =
            actor(&state, "FilenameCp932Archive", &["beatmaps.upload"]).await;
        let easy = local_map("CP932 archive", "Easy");
        let hard = local_map("CP932 archive", "Hard");
        let bytes = crate::maps::archive_names::tests::stored_archive(&[
            (b"\x83\x5c.osu", 0, &easy),
            (b"\x90\xaf.osu", 0, &hard),
            (b"audio.mp3", 0, b"owned archive asset"),
        ]);
        let (status, created) = upload(&app, &cookie, "cp932.osz", &bytes).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        assert_eq!(before.aliases.as_array().unwrap().len(), 2);
        for (filename, raw) in [("ソ.osu", &easy), ("星.osu", &hard)] {
            let checksum = format!("{:x}", md5::compute(raw));
            let id = map_id(&created, &checksum);
            assert_eq!(alias_ids(&state, filename).await, [id]);
            let (status, resolved) = resolve_filename(&app, user, filename).await;
            assert_eq!(status, StatusCode::OK, "{resolved}");
            assert_public_resolution(&resolved, id, &checksum, "local");
            assert_eq!(
                get_bytes(&app, &format!("/api/v1/beatmaps/{id}/download")).await,
                (StatusCode::OK, bytes.clone())
            );
            assert_eq!(
                get_bytes(&app, &format!("/api/v1/beatmaps/{id}/osu")).await,
                (StatusCode::OK, raw.clone())
            );
        }
        let (status, retried) = upload(&app, &cookie, "renamed.osz", &bytes).await;
        assert_eq!(status, StatusCode::OK, "{retried}");
        assert_eq!(retried["idempotent"], true);
        assert_eq!(retried["package"], created["package"]);
        for raw in [&easy, &hard] {
            let checksum = format!("{:x}", md5::compute(raw));
            assert_eq!(map_id(&retried, &checksum), map_id(&created, &checksum));
        }
        assert!(alias_ids(&state, "É».osu").await.is_empty());
        assert!(alias_ids(&state, "cp932.osz").await.is_empty());
        assert!(alias_ids(&state, "renamed.osz").await.is_empty());
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(server.counters(), [0; 4]);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn distinct_local_maps_with_the_same_exact_filename_are_ambiguous_without_upstream_fallback()
{
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_ambiguous", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(&state, "FilenameAmbiguous", &["beatmaps.upload"]).await;
        let filename = "Shared [Easy].osu";
        for title in [
            "First private map",
            "Second private map",
            "Third private map",
        ] {
            let (status, value) = upload(&app, &cookie, filename, &local_map(title, "Easy")).await;
            assert_eq!(status, StatusCode::OK, "{value}");
        }
        assert_eq!(alias_ids(&state, filename).await.len(), 3);
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let (status, value) = resolve_filename(&app, user, filename).await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["error"]["code"], "beatmap_filename_ambiguous");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(
            server.counters(),
            [0; 4],
            "ambiguity never falls through to official lookup"
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn exact_package_retries_preserve_aliases_without_registering_the_new_upload_name() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_retry", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(&state, "FilenameRetry", &["beatmaps.upload"]).await;
        let raw = local_map("Retry alias", "Easy");
        let original = "Original [Easy].osu";
        let renamed = "新名字 [Easy].OSU";
        let (status, created) = upload(&app, &cookie, original, &raw).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let checksum = format!("{:x}", md5::compute(&raw));
        let id = map_id(&created, &checksum);
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let (status, retried) = upload(&app, &cookie, renamed, &raw).await;
        assert_eq!(status, StatusCode::OK, "{retried}");
        assert_eq!(retried["idempotent"], true);
        assert_eq!(map_id(&retried, &checksum), id);
        assert!(alias_ids(&state, renamed).await.is_empty());
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        let (status, value) = resolve_filename(&app, user, original).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_public_resolution(&value, id, &checksum, "local");
        assert_eq!(server.counters(), [0; 4]);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn filename_aliases_rollback_with_package_rows_and_files_after_definite_commit_failure() {
    let fixture = MutationFixture::new("filename_commit", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) = actor(&state, "FilenameCommit", &["beatmaps.upload"]).await;
        reject_audits_at_commit(&state.db).await;
        let before = snapshot(&state).await;
        let easy = local_map("Rolled back alias", "Easy");
        let hard = local_map("Rolled back alias", "Hard");
        let payload = archive(&[("nested/Easy.osu", &easy), ("Hard.osu", &hard)]);
        let (status, value) = upload(&app, &cookie, "rollback.osz", &payload).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{value}");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(file_count(&state.maps_root).await, 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn queued_upload_rechecks_revoked_actor_before_publishing_any_filename_alias() {
    let fixture = MutationFixture::new("filename_revocation", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (_, _, sid, cookie) = actor(&state, "FilenameRevocation", &["beatmaps.upload"]).await;
        let before = snapshot(&state).await;
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *blocker)
            .await
            .unwrap();
        let pending = tokio::spawn(async move {
            upload(
                &app,
                &cookie,
                "Revoked.osu",
                &local_map("Queued alias", "Easy"),
            )
            .await
        });
        wait_for_lock(
            &state.db,
            &application,
            "LOCK TABLE roles,user_roles IN SHARE MODE",
        )
        .await;
        assert!(
            file_count(&state.maps_root).await > 0,
            "private files are prepared before the guard"
        );
        sqlx::query("DELETE FROM sessions WHERE id=$1")
            .bind(sid)
            .execute(&mut *blocker)
            .await
            .unwrap();
        blocker.commit().await.unwrap();
        let (status, value) = pending.await.unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(file_count(&state.maps_root).await, 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_filename_lookup_encodes_names_caches_misses_and_coalesces_guarded_alias_additions()
 {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_official", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(&state, "FilenameOfficial", &["beatmaps.upload"]).await;
        let map = OfficialMap::new(301, "ranked");
        let checksum = map.checksum();
        let encoded = "Artist & +?#% 雪 [Hard].osu";
        let second_name = "Other exact name [Hard].OSU";
        let concurrent_name = "Concurrent alias [Hard].osu";
        let missing_name = "Missing then locally supplied [Easy].osu";
        let restricted_name = "Restricted new alias [Hard].osu";
        for filename in [encoded, second_name, concurrent_name, restricted_name] {
            server.add_filename(filename, map.clone()).await;
        }
        let (status, created) = resolve_filename(&app, user, encoded).await;
        assert_eq!(status, StatusCode::OK, "{created}");
        let id = created["id"].as_i64().unwrap();
        assert_public_resolution(&created, id, &checksum, "official");
        assert_eq!(alias_ids(&state, encoded).await, [id]);
        assert_eq!(server.lookup_filenames().await, [encoded]);
        assert_eq!(server.counters(), [1, 1, 0, 1]);
        sqlx::query("UPDATE beatmaps SET status='loved' WHERE id=$1")
            .bind(id)
            .execute(&state.db)
            .await
            .unwrap();
        let original = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        assert_eq!(original.audits.as_array().unwrap().len(), 1);

        // Fetching another exact filename for an existing official checksum can
        // add an alias, without replacing content or adding a creation audit.
        let (status, reused) = resolve_filename(&app, user, second_name).await;
        assert_eq!(status, StatusCode::OK, "{reused}");
        assert_public_resolution(&reused, id, &checksum, "official");
        assert_eq!(reused["status"], "loved");
        assert_eq!(alias_ids(&state, second_name).await, [id]);
        let after = snapshot(&state).await;
        assert_eq!(after.maps, original.maps);
        assert_eq!(after.packages, original.packages);
        assert_eq!(after.audits, original.audits);
        assert_eq!(files_snapshot(&state.maps_root).await, files);

        // Keep the first lookup in flight while another request asks for that
        // same missing alias. Only one upstream lookup and one alias survive.
        let pause = server.pause_next_lookup().await;
        let first_app = app.clone();
        let first =
            tokio::spawn(async move { resolve_filename(&first_app, user, concurrent_name).await });
        pause.wait_entered().await;
        let second_app = app.clone();
        let second =
            tokio::spawn(async move { resolve_filename(&second_app, user, concurrent_name).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        pause.release();
        let (first, second) = tokio::join!(first, second);
        let (first_status, first) = first.unwrap();
        let (second_status, second) = second.unwrap();
        assert_eq!(first_status, StatusCode::OK, "{first}");
        assert_eq!(second_status, StatusCode::OK, "{second}");
        assert_eq!(first, second);
        assert_public_resolution(&first, id, &checksum, "official");
        assert_eq!(alias_ids(&state, concurrent_name).await, [id]);
        assert_eq!(
            server.lookup_filenames().await,
            [encoded, second_name, concurrent_name]
        );
        let after = snapshot(&state).await;
        assert_eq!(after.maps, original.maps);
        assert_eq!(after.audits, original.audits);
        assert_eq!(after.aliases.as_array().unwrap().len(), 3);
        assert_eq!(files_snapshot(&state.maps_root).await, files);

        let before_miss = snapshot(&state).await;
        let counters = server.counters();
        for _ in 0..2 {
            let (status, value) = resolve_filename(&app, user, missing_name).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{value}");
            assert_eq!(value["error"]["code"], "beatmap_not_found");
        }
        assert_eq!(
            server.counters(),
            [counters[0], counters[1] + 1, counters[2], counters[3]],
            "the second miss uses the filename failure cache"
        );
        assert_eq!(snapshot(&state).await, before_miss);
        assert_eq!(files_snapshot(&state.maps_root).await, files);

        // Local aliases remain authoritative even while a prior official miss
        // is cached for this exact filename.
        let local = local_map("Locally supplied after a miss", "Easy");
        let local_checksum = format!("{:x}", md5::compute(&local));
        let (status, uploaded) = upload(&app, &cookie, missing_name, &local).await;
        assert_eq!(status, StatusCode::OK, "{uploaded}");
        let local_id = map_id(&uploaded, &local_checksum);
        let counters = server.counters();
        let (status, value) = resolve_filename(&app, user, missing_name).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_public_resolution(&value, local_id, &local_checksum, "local");
        assert_eq!(server.counters(), counters);

        // An initially active native actor may be restricted while network work
        // runs. Existing-official alias publication must still recheck authority.
        let before_restriction = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let pause = server.pause_next_lookup().await;
        let restricted_app = app.clone();
        let pending =
            tokio::spawn(
                async move { resolve_filename(&restricted_app, user, restricted_name).await },
            );
        pause.wait_entered().await;
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(user)
            .execute(&state.db)
            .await
            .unwrap();
        pause.release();
        let (status, value) = pending.await.unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(snapshot(&state).await, before_restriction);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert!(alias_ids(&state, restricted_name).await.is_empty());
        assert_eq!(
            server.lookup_filenames().await,
            [
                encoded,
                second_name,
                concurrent_name,
                missing_name,
                restricted_name
            ]
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_filename_integrity_and_existing_local_checksum_conflicts_never_publish_aliases() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("filename_integrity", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "FilenameIntegrity", &["beatmaps.upload"]).await;
        for (id, defect, status, code) in [
            (401_i64, "checksum", StatusCode::BAD_GATEWAY, "checksum_mismatch"),
            (402, "mode", StatusCode::BAD_GATEWAY, "official_identity_mismatch"),
            (403, "file_id", StatusCode::BAD_GATEWAY, "official_identity_mismatch"),
            (404, "file_set", StatusCode::BAD_GATEWAY, "official_identity_mismatch"),
            (1_999_999_999, "reserved_set", StatusCode::CONFLICT, "official_namespace_conflict"),
            (2_000_000_000, "reserved_id", StatusCode::CONFLICT, "official_namespace_conflict"),
        ] {
            let mut map = OfficialMap::new(id, "ranked");
            let mut metadata = json!({"id":id,"beatmapset_id":id+1000,"checksum":map.checksum(),"mode_int":0,"status":"ranked"});
            match defect {
                "checksum" => metadata["checksum"] = json!("4".repeat(32)),
                "mode" => metadata["mode_int"] = json!(1),
                "file_id" | "file_set" => {
                    let text = String::from_utf8(map.bytes().to_vec()).unwrap();
                    let bytes = if defect == "file_id" {
                        text.replace(&format!("BeatmapID:{id}"), &format!("BeatmapID:{}", id+99))
                    } else {
                        text.replace(&format!("BeatmapSetID:{}", id+1000), &format!("BeatmapSetID:{}", id+1099))
                    }.into_bytes();
                    metadata["checksum"] = json!(format!("{:x}", md5::compute(&bytes)));
                    map = map.with_bytes(bytes);
                }
                "reserved_set" | "reserved_id" => {}
                _ => unreachable!(),
            }
            map = map.with_metadata(metadata);
            let filename = format!("Integrity {defect}.osu");
            server.add_filename(&filename, map).await;
            let before = snapshot(&state).await;
            let files = files_snapshot(&state.maps_root).await;
            let (actual_status, value) = resolve_filename(&app, user, &filename).await;
            assert_eq!(actual_status, status, "{defect}: {value}");
            assert_eq!(value["error"]["code"], code, "{defect}: {value}");
            assert_eq!(snapshot(&state).await, before);
            assert_eq!(files_snapshot(&state.maps_root).await, files);
        }

        // A legacy local row may have the same retained bytes as official
        // content. Filename discovery cannot attach an official alias to it.
        let map = OfficialMap::new(405, "ranked");
        let bytes = map.bytes().to_vec();
        let parsed = crate::maps::parse_map(bytes.clone()).unwrap();
        let path = "retained-local-checksum.osu";
        tokio::fs::write(state.maps_root.join(path), &bytes).await.unwrap();
        let local_id: i64 = sqlx::query_scalar("INSERT INTO beatmaps(uploaded_by,title,artist,creator,version,mode,stars,bpm,length_seconds,max_combo,md5,osu_path) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id")
            .bind(user).bind(parsed.title).bind(parsed.artist).bind(parsed.creator).bind(parsed.version).bind(parsed.mode).bind(parsed.stars).bind(parsed.bpm).bind(parsed.length).bind(parsed.max_combo).bind(parsed.md5).bind(path).fetch_one(&state.db).await.unwrap();
        let filename = "Official lookup overlaps local checksum.osu";
        server.add_filename(filename, map).await;
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let (status, value) = resolve_filename(&app, user, filename).await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["error"]["code"], "beatmap_filename_conflict");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert!(alias_ids(&state, filename).await.is_empty());
        assert_eq!(before.maps[0]["id"], local_id);
        assert_eq!(before.maps[0]["source"], "local");
        assert!(before.maps[0]["official_id"].is_null());
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn official_filename_publication_rechecks_a_local_alias_created_while_lookup_is_in_flight() {
    let server = OfficialServer::start().await;
    let filename = "Concurrent local and official [Easy].osu";
    server
        .add_filename(filename, OfficialMap::new(501, "ranked"))
        .await;
    let fixture = MutationFixture::new("filename_race", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (reader, _, _, _) = actor(&state, "FilenameRaceReader", &[]).await;
        let (publisher, _, _, cookie) =
            actor(&state, "FilenameRaceOwner", &["beatmaps.upload"]).await;
        let pause = server.pause_next_lookup().await;
        let official_app = app.clone();
        let pending =
            tokio::spawn(async move { resolve_filename(&official_app, reader, filename).await });
        pause.wait_entered().await;
        let local = local_map("Private publication wins the filename", "Easy");
        let checksum = format!("{:x}", md5::compute(&local));
        let (status, uploaded) = upload(&app, &cookie, filename, &local).await;
        assert_eq!(status, StatusCode::OK, "{uploaded}");
        let id = map_id(&uploaded, &checksum);
        assert_eq!(alias_ids(&state, filename).await, [id]);
        let before = snapshot(&state).await;
        let files = files_snapshot(&state.maps_root).await;
        let (status, resolved) = resolve_filename(&app, reader, filename).await;
        assert_eq!(status, StatusCode::OK, "{resolved}");
        assert_public_resolution(&resolved, id, &checksum, "local");
        pause.release();
        let (status, value) = pending.await.unwrap();
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["error"]["code"], "beatmap_filename_conflict");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(files_snapshot(&state.maps_root).await, files);
        assert_eq!(server.lookup_filenames().await, [filename]);
        assert_eq!(before.maps.as_array().unwrap().len(), 1);
        assert_eq!(before.maps[0]["uploaded_by"], publisher);
        assert_eq!(before.audits.as_array().unwrap().len(), 1);
    })
    .await;
    fixture.finish(result).await;
}
