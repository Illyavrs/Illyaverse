//! Recovery fixtures own a disposable PG17 schema and synthetic retained files.
use super::*;
use crate::{
    Router,
    maps::map_mutation_tests::{
        MutationFixture, actor, osu_bytes, reject_audits_at_commit, request, wait_for_lock,
        wait_until_expired,
    },
    native_map_tests::OfficialServer,
};
use axum::http::StatusCode;
use serde_json::json;
use std::io::Cursor;

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

async fn backfill(app: &Router, cookie: &str, id: Uuid, body: Value) -> (StatusCode, Value) {
    request(
        app,
        &format!("/api/v1/admin/beatmap-packages/{id}/filename-aliases/backfill"),
        Some(cookie),
        false,
        "application/json",
        body.to_string().into_bytes(),
    )
    .await
}

async fn seed(
    state: &AppState,
    owner: i64,
    payload: &[u8],
    maps: &[Vec<u8>],
    key: Option<&str>,
    fingerprint: bool,
) -> (Uuid, Vec<i64>, String) {
    let id = Uuid::new_v4();
    let directory = id.to_string();
    tokio::fs::create_dir(state.maps_root.join(&directory))
        .await
        .unwrap();
    let archive_path = key
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{directory}/package.osz"));
    if key.is_none() {
        tokio::fs::write(state.maps_root.join(&archive_path), payload)
            .await
            .unwrap();
    }
    let mut manifest = maps
        .iter()
        .map(|bytes| format!("{:x}", md5::compute(bytes)))
        .collect::<Vec<_>>();
    manifest.sort_unstable();
    let digest = fingerprint.then(|| format!("{:x}", Sha256::digest(payload)));
    sqlx::query("INSERT INTO beatmap_packages(id,uploaded_by,kind,sha256,byte_length,member_md5,archive_path,legacy) VALUES($1,$2,'osz',$3,$4,$5,$6,$7)")
        .bind(id).bind(owner).bind(digest).bind(fingerprint.then_some(payload.len() as i64)).bind(manifest).bind(&archive_path).bind(!fingerprint).execute(&state.db).await.unwrap();
    let mut ids = Vec::new();
    for bytes in maps {
        let checksum = format!("{:x}", md5::compute(bytes));
        let path = format!("{directory}/{checksum}.osu");
        tokio::fs::write(state.maps_root.join(&path), bytes)
            .await
            .unwrap();
        let map_id = sqlx::query_scalar("INSERT INTO beatmaps(local_package_id,uploaded_by,title,artist,creator,version,mode,status,stars,md5,osu_path,archive_path) VALUES($1,$2,'Recovery fixture','Illyaverse','Fixture','Easy',0,'loved',1,$3,$4,$5) RETURNING id")
            .bind(id).bind(owner).bind(checksum).bind(path).bind(&archive_path).fetch_one(&state.db).await.unwrap();
        ids.push(map_id);
    }
    (id, ids, archive_path)
}

async fn snapshot(state: &AppState) -> Value {
    let maps: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(b) ORDER BY id),'[]'::jsonb) FROM beatmaps b",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let packages: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(p) ORDER BY id),'[]'::jsonb) FROM beatmap_packages p",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    let aliases: Value = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY filename COLLATE \"C\",beatmap_id),'[]'::jsonb) FROM beatmap_filename_aliases a").fetch_one(&state.db).await.unwrap();
    let audits: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY id),'[]'::jsonb) FROM audit_events a",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    json!({"maps":maps,"packages":packages,"aliases":aliases,"audits":audits})
}

async fn assert_rejected(state: &AppState, app: &Router, cookie: &str, id: Uuid, code: &str) {
    let before = snapshot(state).await;
    for apply in [false, true] {
        let (status, value) = backfill(app, cookie, id, json!({"apply":apply})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["error"]["code"], code, "{value}");
        assert_eq!(snapshot(state).await, before);
    }
}

#[test]
fn exact_archive_pairing_preserves_unicode_bytes_and_rejects_duplicate_checksums() {
    let first = osu_bytes("Archive first");
    let second = osu_bytes("Archive second");
    let members = archive_members(&archive(&[
        ("nested/夏 [Hard].osu", &second),
        ("cafe\u{301} [Easy].OSU", &first),
    ]))
    .unwrap();
    assert_eq!(
        members[&format!("{:x}", md5::compute(&first))],
        ("cafe\u{301} [Easy].OSU".into(), first.clone())
    );
    assert_eq!(
        members[&format!("{:x}", md5::compute(&second))],
        ("夏 [Hard].osu".into(), second)
    );
    assert_eq!(
        archive_members(&archive(&[("a.osu", &first), ("b.osu", &first)]))
            .unwrap_err()
            .1,
        "filename_backfill_manifest_mismatch"
    );
    assert_eq!(
        archive_members(b"not a zip").unwrap_err().1,
        "filename_backfill_malformed_archive"
    );
    assert_eq!(
        archive_members(&archive(&[("../unsafe.osu", &first)]))
            .unwrap_err()
            .1,
        "filename_backfill_malformed_archive"
    );
}

#[tokio::test]
async fn retained_reader_rejects_portable_unsafe_paths_and_bounds_files() {
    let root = std::env::temp_dir().join(format!("backfill_reader_{}", Uuid::new_v4().simple()));
    tokio::fs::create_dir(&root).await.unwrap();
    tokio::fs::write(root.join("safe.osu"), b"safe")
        .await
        .unwrap();
    for path in [
        "/absolute.osu",
        "../escape.osu",
        "a/../safe.osu",
        "C:/safe.osu",
        "a\\safe.osu",
        "//server/share.osu",
        "./safe.osu",
        "a//safe.osu",
        "",
    ] {
        assert_eq!(
            retained_bytes(&root, path, MAX_MEMBER_BYTES)
                .await
                .unwrap_err()
                .1,
            "filename_backfill_unsafe_file",
            "{path}"
        );
    }
    assert_eq!(
        retained_bytes(&root, "safe.osu", 3).await.unwrap_err().1,
        "filename_backfill_unsafe_file"
    );
    assert_eq!(
        retained_bytes(&root, "missing.osu", MAX_MEMBER_BYTES)
            .await
            .unwrap_err()
            .1,
        "filename_backfill_missing_file"
    );
    tokio::fs::create_dir(root.join("directory.osu"))
        .await
        .unwrap();
    assert_eq!(
        retained_bytes(&root, "directory.osu", MAX_MEMBER_BYTES)
            .await
            .unwrap_err()
            .1,
        "filename_backfill_unsafe_file"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("safe.osu"), root.join("link.osu")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("ancestor")).unwrap();
        for path in ["link.osu", "ancestor/safe.osu"] {
            assert_eq!(
                retained_bytes(&root, path, MAX_MEMBER_BYTES)
                    .await
                    .unwrap_err()
                    .1,
                "filename_backfill_unsafe_file"
            );
        }
    }
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn preview_apply_and_retry_preserve_content_identity_and_unknown_fingerprint() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("backfill_publish", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "BackfillPublish", &["beatmaps.moderate"]).await;
        let easy = osu_bytes("Recovery easy"); let hard = osu_bytes("Recovery hard");
        let payload = archive(&[("nested/夏 [Hard].osu", &hard), ("cafe\u{301} [Easy].OSU", &easy), ("audio.mp3", b"original audio")]);
        let (id, ids, path) = seed(&state, owner, &payload, &[easy.clone(), hard.clone()], None, false).await;
        let before = snapshot(&state).await;
        let (status, report) = backfill(&app, &cookie, id, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{report}");
        assert_eq!(report, json!({"package_id":id,"apply":false,"difficulty_count":2,"verified_aliases":2,"existing_aliases":0,"missing_aliases":2,"inserted_aliases":0,"ambiguous_aliases":0}));
        assert_eq!(snapshot(&state).await, before);
        let (status, report) = backfill(&app, &cookie, id, json!({"apply":true})).await;
        assert_eq!(status, StatusCode::OK, "{report}"); assert_eq!(report["inserted_aliases"], 2);
        let after = snapshot(&state).await;
        assert_eq!(after["maps"], before["maps"]); assert_eq!(after["packages"], before["packages"]);
        assert_eq!(after["audits"].as_array().unwrap().len(), 1);
        assert_eq!(tokio::fs::read(state.maps_root.join(path)).await.unwrap(), payload);
        for (map_id, bytes, filename) in [(ids[0], easy, "cafe\u{301} [Easy].OSU"), (ids[1], hard, "夏 [Hard].osu")] {
            let checksum = format!("{:x}", md5::compute(&bytes));
            assert_eq!(tokio::fs::read(state.maps_root.join(format!("{id}/{checksum}.osu"))).await.unwrap(), bytes);
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT beatmap_id FROM beatmap_filename_aliases WHERE filename=$1").bind(filename).fetch_one(&state.db).await.unwrap(), map_id);
            let (status, value) = request(&app, "/api/v1/internal/beatmaps/resolve", None, true, "application/json", json!({"user_id":owner,"filename":filename}).to_string().into_bytes()).await;
            assert_eq!(status, StatusCode::OK, "{value}"); assert_eq!(value["id"], map_id); assert_eq!(value["status"], "loved");
        }
        let (status, report) = backfill(&app, &cookie, id, json!({"apply":true})).await;
        assert_eq!(status, StatusCode::OK, "{report}"); assert_eq!(report["existing_aliases"], 2); assert_eq!(report["inserted_aliases"], 0);
        assert_eq!(snapshot(&state).await, after);
        assert_eq!(server.counters(), [0;4]);
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn missing_unsafe_malformed_and_changed_retained_files_never_publish() {
    let fixture = MutationFixture::new("backfill_invalid", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "BackfillInvalid", &["beatmaps.moderate"]).await;
        for failure in [
            "missing_archive",
            "missing_member",
            "directory",
            "malformed",
            "manifest",
            "incomplete",
            "member",
            "fingerprint",
            "unsafe_key",
            "duplicate",
        ] {
            let bytes = osu_bytes(&format!("Recovery {failure}"));
            let payload = archive(&[("original.osu", &bytes)]);
            let key = (failure == "unsafe_key").then_some("../outside.osz");
            let (id, ids, path) = seed(
                &state,
                owner,
                &payload,
                std::slice::from_ref(&bytes),
                key,
                failure == "fingerprint",
            )
            .await;
            let archive_file = state.maps_root.join(&path);
            let member_file = state
                .maps_root
                .join(format!("{id}/{:x}.osu", md5::compute(&bytes)));
            let code = match failure {
                "missing_archive" => {
                    tokio::fs::remove_file(archive_file).await.unwrap();
                    "filename_backfill_missing_file"
                }
                "missing_member" => {
                    tokio::fs::remove_file(member_file).await.unwrap();
                    "filename_backfill_missing_file"
                }
                "directory" => {
                    tokio::fs::remove_file(&archive_file).await.unwrap();
                    tokio::fs::create_dir(archive_file).await.unwrap();
                    "filename_backfill_unsafe_file"
                }
                "malformed" => {
                    tokio::fs::write(archive_file, b"not zip").await.unwrap();
                    "filename_backfill_malformed_archive"
                }
                "manifest" => {
                    tokio::fs::write(
                        archive_file,
                        archive(&[("changed.osu", &osu_bytes("different content"))]),
                    )
                    .await
                    .unwrap();
                    "filename_backfill_manifest_mismatch"
                }
                "incomplete" => {
                    sqlx::query("DELETE FROM beatmaps WHERE id=$1")
                        .bind(ids[0])
                        .execute(&state.db)
                        .await
                        .unwrap();
                    "filename_backfill_manifest_mismatch"
                }
                "member" => {
                    tokio::fs::write(member_file, b"wrong exact bytes")
                        .await
                        .unwrap();
                    "filename_backfill_member_mismatch"
                }
                "fingerprint" => {
                    tokio::fs::write(
                        archive_file,
                        archive(&[("original.osu", &bytes), ("audio.mp3", b"different asset")]),
                    )
                    .await
                    .unwrap();
                    "filename_backfill_fingerprint_mismatch"
                }
                "unsafe_key" => "filename_backfill_unsafe_file",
                "duplicate" => {
                    tokio::fs::write(
                        archive_file,
                        archive(&[("a.osu", &bytes), ("b.osu", &bytes)]),
                    )
                    .await
                    .unwrap();
                    "filename_backfill_manifest_mismatch"
                }
                _ => unreachable!(),
            };
            assert_rejected(&state, &app, &cookie, id, code).await;
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn verified_same_basename_aliases_report_ambiguity_and_never_contact_upstream() {
    let server = OfficialServer::start().await;
    let fixture = MutationFixture::new("backfill_ambiguity", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) =
            actor(&state, "BackfillAmbiguity", &["beatmaps.moderate"]).await;
        let first = osu_bytes("First ambiguous");
        let second = osu_bytes("Second ambiguous");
        let payload = archive(&[("folderA/same.osu", &first), ("folderB/same.osu", &second)]);
        let (id, _, _) = seed(&state, owner, &payload, &[first, second], None, false).await;
        for apply in [false, true] {
            let (status, value) = backfill(&app, &cookie, id, json!({"apply":apply})).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value["ambiguous_aliases"], 2);
        }
        let before = snapshot(&state).await;
        let (status, value) = request(
            &app,
            "/api/v1/internal/beatmaps/resolve",
            None,
            true,
            "application/json",
            json!({"user_id":owner,"filename":"same.osu"})
                .to_string()
                .into_bytes(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["error"]["code"], "beatmap_filename_ambiguous");
        assert_eq!(snapshot(&state).await, before);
        assert_eq!(server.counters(), [0; 4]);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn denied_unknown_and_singleton_requests_preserve_database() {
    let fixture = MutationFixture::new("backfill_denied", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "BackfillDenied", &["beatmaps.moderate"]).await;
        let (_, _, _, denied) = actor(&state, "BackfillNoPermission", &["beatmaps.upload"]).await;
        let bytes = osu_bytes("Denied fixture"); let payload = archive(&[("denied.osu", &bytes)]);
        let (id, _, _) = seed(&state, owner, &payload, std::slice::from_ref(&bytes), None, false).await;
        let before = snapshot(&state).await;
        assert_eq!(request(&app, &format!("/api/v1/admin/beatmap-packages/{id}/filename-aliases/backfill"), None, false, "application/json", b"{}".to_vec()).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(request(&app, &format!("/api/v1/admin/beatmap-packages/{id}/filename-aliases/backfill"), Some(&cookie), false, "application/json", vec![b' '; 1025]).await.0, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(backfill(&app, &denied, id, json!({"apply":true})).await.0, StatusCode::FORBIDDEN);
        assert_eq!(backfill(&app, &cookie, id, json!({"apply":true,"unknown":true})).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, value) = backfill(&app, &cookie, Uuid::new_v4(), json!({"apply":true})).await;
        assert_eq!(status, StatusCode::NOT_FOUND); assert_eq!(value["error"]["code"], "filename_backfill_package_not_found");
        assert_eq!(snapshot(&state).await, before);
        let singleton: Uuid = sqlx::query_scalar("INSERT INTO beatmap_packages(uploaded_by,kind,member_md5,legacy) VALUES($1,'osu',$2,true) RETURNING id")
            .bind(owner).bind(vec![format!("{:x}", md5::compute(&bytes))]).fetch_one(&state.db).await.unwrap();
        assert_rejected(&state, &app, &cookie, singleton, "filename_backfill_unsupported_package").await;
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn established_exact_fingerprint_can_recover_aliases_without_changing_fingerprint() {
    let fixture =
        MutationFixture::new("backfill_known_fingerprint", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) =
            actor(&state, "BackfillKnownFingerprint", &["beatmaps.moderate"]).await;
        let bytes = osu_bytes("Known original fingerprint");
        let payload = archive(&[("known.osu", &bytes), ("audio.mp3", b"unchanged asset")]);
        let (id, _, path) = seed(&state, owner, &payload, &[bytes], None, true).await;
        let before = snapshot(&state).await;
        for apply in [false, true] {
            let (status, report) = backfill(&app, &cookie, id, json!({"apply":apply})).await;
            assert_eq!(status, StatusCode::OK, "{report}");
            assert_eq!(report["inserted_aliases"], usize::from(apply));
            let after = snapshot(&state).await;
            assert_eq!(after["packages"], before["packages"]);
            assert_eq!(after["maps"], before["maps"]);
            assert_eq!(
                tokio::fs::read(state.maps_root.join(&path)).await.unwrap(),
                payload
            );
            if !apply {
                assert_eq!(after, before);
            }
        }
        assert_eq!(
            snapshot(&state).await["audits"].as_array().unwrap().len(),
            1
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn queued_revocation_restriction_permission_and_expiry_recheck_current_actor() {
    let fixture = MutationFixture::new("backfill_authority", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        for change in ["revoke", "restrict", "permission", "expire"] {
            let (owner, role, sid, cookie) = actor(&state, &format!("Backfill{change}"), &["beatmaps.moderate"]).await;
            let bytes = osu_bytes(&format!("Queued {change}")); let payload = archive(&[("queued.osu", &bytes)]);
            let (id, _, _) = seed(&state, owner, &payload, &[bytes], None, false).await;
            let before = snapshot(&state).await;
            let mut blocker = state.db.begin().await.unwrap();
            sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();
            let queued_app = app.clone();
            let pending = tokio::spawn(async move { backfill(&queued_app, &cookie, id, json!({"apply":true})).await });
            wait_for_lock(&state.db, &application, "LOCK TABLE roles,user_roles IN SHARE MODE").await;
            match change {
                "revoke" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); },
                "restrict" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(owner).execute(&mut *blocker).await.unwrap(); },
                "permission" => { sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1").bind(role).execute(&mut *blocker).await.unwrap(); },
                "expire" => { sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); },
                _ => unreachable!(),
            }
            blocker.commit().await.unwrap();
            let (status, value) = pending.await.unwrap();
            assert_eq!(status, if change == "permission" { StatusCode::FORBIDDEN } else { StatusCode::UNAUTHORIZED }, "{change}: {value}");
            assert_eq!(snapshot(&state).await, before);
        }
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn natural_expiry_after_resource_wait_prevents_publication() {
    let fixture = MutationFixture::new("backfill_deadline", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (owner, _, sid, cookie) = actor(&state, "BackfillDeadline", &["beatmaps.moderate"]).await;
        let bytes = osu_bytes("Natural expiry"); let payload = archive(&[("deadline.osu", &bytes)]);
        let (id, _, _) = seed(&state, owner, &payload, &[bytes], None, false).await;
        let expires_at = sqlx::query_scalar("UPDATE sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE id=$1 RETURNING expires_at").bind(sid).fetch_one(&state.db).await.unwrap();
        let set_id: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmap_packages WHERE id=$1").bind(id).fetch_one(&state.db).await.unwrap();
        let before = snapshot(&state).await;
        let mut blocker = state.db.begin().await.unwrap();
        availability::lock_exclusive_set(&mut blocker, set_id).await.unwrap();
        let pending = tokio::spawn(async move { backfill(&app, &cookie, id, json!({"apply":true})).await });
        wait_for_lock(&state.db, &application, "SELECT pg_advisory_xact_lock($1,$2)").await;
        wait_until_expired(&state.db, expires_at).await;
        blocker.commit().await.unwrap();
        let (status, value) = pending.await.unwrap(); assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(snapshot(&state).await, before);
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn concurrent_apply_publishes_once_with_one_audit_and_commit_failure_rolls_back() {
    let fixture = MutationFixture::new("backfill_concurrent", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "BackfillConcurrent", &["beatmaps.moderate"]).await;
        let (_, _, _, other_cookie) = actor(&state, "BackfillConcurrentOther", &["beatmaps.moderate"]).await;
        let bytes = osu_bytes("Concurrent recovery"); let payload = archive(&[("concurrent.osu", &bytes)]);
        let (id, _, _) = seed(&state, owner, &payload, &[bytes], None, false).await;
        let (first, second) = tokio::join!(backfill(&app, &cookie, id, json!({"apply":true})), backfill(&app, &other_cookie, id, json!({"apply":true})));
        assert_eq!(first.0, StatusCode::OK, "{}", first.1); assert_eq!(second.0, StatusCode::OK, "{}", second.1);
        assert_eq!(first.1["inserted_aliases"].as_u64().unwrap()+second.1["inserted_aliases"].as_u64().unwrap(), 1);
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_events WHERE action='beatmaps.filename_aliases.backfill'").fetch_one(&state.db).await.unwrap(), 1);
        let bytes = osu_bytes("Commit failure recovery"); let payload = archive(&[("rollback.osu", &bytes)]);
        let (id, _, _) = seed(&state, owner, &payload, &[bytes], None, false).await;
        let before = snapshot(&state).await;
        reject_audits_at_commit(&state.db).await;
        assert_eq!(backfill(&app, &cookie, id, json!({"apply":true})).await.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(snapshot(&state).await, before);
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn queued_withdrawal_serializes_and_rejects_alias_publication() {
    let fixture = MutationFixture::new("backfill_withdrawn", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (owner, _, _, cookie) = actor(&state, "BackfillWithdrawal", &["beatmaps.moderate"]).await;
        let (withdrawal_user, _, withdrawal_session, _) = actor(&state, "BackfillWithdrawalStaff", &["beatmaps.moderate"]).await;
        let bytes = osu_bytes("Withdrawn recovery"); let payload = archive(&[("withdrawn.osu", &bytes)]);
        let (id, _, _) = seed(&state, owner, &payload, &[bytes], None, false).await;
        let set_id: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmap_packages WHERE id=$1").bind(id).fetch_one(&state.db).await.unwrap();
        let before = snapshot(&state).await;
        let mut blocker = state.db.begin().await.unwrap();
        // Model the real withdrawal order with its own staff actor. Using the
        // backfill actor here would invert its user lock and the set fence:
        // the availability FK would wait on the user already held by backfill.
        let withdrawal_snapshot = auth::User { id: withdrawal_user, session: withdrawal_session, permissions: vec!["beatmaps.moderate".into()] };
        let withdrawal_actor = auth::mutation_actor(&mut blocker, &withdrawal_snapshot, &[], auth::MutationScope::Permissions).await.unwrap();
        withdrawal_actor.require("beatmaps.moderate").unwrap();
        availability::lock_exclusive_set(&mut blocker, set_id).await.unwrap();
        let pending = tokio::spawn(async move { backfill(&app, &cookie, id, json!({"apply":true})).await });
        wait_for_lock(&state.db, &application, "SELECT pg_advisory_xact_lock($1,$2)").await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'owned fixture',$2)").bind(set_id).bind(withdrawal_actor.id).execute(&mut *blocker).await.unwrap();
        auth::ensure_mutation_actor(&mut blocker, &withdrawal_actor).await.unwrap();
        blocker.commit().await.unwrap();
        let (status, value) = pending.await.unwrap(); assert_eq!(status, StatusCode::GONE, "{value}"); assert_eq!(value["error"]["code"], "beatmap_withdrawn");
        assert_eq!(snapshot(&state).await, before);
    }).await;
    fixture.finish(result).await;
}
