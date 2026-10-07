//! Score/replay commit guards run against an owned isolated PostgreSQL 17 schema.
use super::*;
use crate::maps::map_mutation_tests::{
    MULTIPART, MutationFixture, actor, file_count, multipart, osu_bytes, reject_audits_at_commit,
    request, wait_for_lock, wait_until_expired,
};
use axum::{Router, http::StatusCode};

async fn map(state: &AppState) -> i64 {
    let bytes = osu_bytes("Score mutation");
    tokio::fs::write(state.maps_root.join("fixture.osu"), &bytes)
        .await
        .unwrap();
    sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,stars,max_combo,md5,osu_path) VALUES('Fixture','Illyaverse','Tests','Easy',0,1,2,$1,'fixture.osu') RETURNING id")
        .bind(format!("{:x}", md5::compute(bytes))).fetch_one(&state.db).await.unwrap()
}

fn submission(map: i64) -> Value {
    json!({"beatmap_id":map,"mode":0,"score":1234,"accuracy":100.0,"max_combo":2,"mods":0,"passed":true,"hit_counts":{"n300":2}})
}

async fn native_ingest(app: &Router, body: &Value) -> (StatusCode, Value) {
    request(
        app,
        "/api/v1/internal/scores",
        None,
        true,
        "application/json",
        body.to_string().into_bytes(),
    )
    .await
}

fn immutable_retry_changes(input: &Value) -> Vec<(&'static str, Value)> {
    let mut changes = Vec::new();
    for (field, value, native_field) in [
        ("score", json!(1235), "total_score"),
        ("max_combo", json!(1), "max_combo"),
        ("passed", json!(false), "passed"),
        ("mode", json!(1), "ruleset_id"),
    ] {
        let mut changed = input.clone();
        changed[field] = value.clone();
        if changed.get("native_submission").is_some() {
            changed["native_submission"][native_field] = value;
        }
        changes.push((field, changed));
    }
    for (name, mods, acronym) in [("mods", 8, "HD"), ("variant", 128, "RX")] {
        let mut changed = input.clone();
        changed["mods"] = json!(mods);
        if changed.get("native_submission").is_some() {
            changed["api_mods"] = json!([{"acronym":acronym}]);
            changed["native_submission"]["mods"] = changed["api_mods"].clone();
        }
        changes.push((name, changed));
    }
    let mut counts = input.clone();
    counts["hit_counts"] = json!({"n300":1,"n100":1});
    if counts.get("native_submission").is_some() {
        counts["accuracy"] = json!(100.0 * (2.0 / 3.0));
        counts["native_submission"]["accuracy"] = json!(2.0 / 3.0);
        counts["native_submission"]["statistics"] = json!({"great":1,"ok":1});
    }
    changes.push(("individual_judgements", counts));
    let mut client = input.clone();
    if client["client"] == "stable" {
        client["client"] = json!("lazer");
    } else {
        client["client"] = json!("stable");
        client.as_object_mut().unwrap().remove("native_submission");
        client.as_object_mut().unwrap().remove("api_mods");
    }
    changes.push(("client", client));
    if input.get("native_submission").is_some() {
        for (field, value) in [
            ("rank", json!("S")),
            ("pauses", json!([100])),
            ("total_score_without_mods", json!(1235)),
            ("maximum_statistics", json!({"great":3})),
        ] {
            let mut changed = input.clone();
            changed["native_submission"][field] = value;
            changes.push((field, changed));
        }
        let mut changed = input.clone();
        changed["accuracy"] = json!(99.0);
        changed["native_submission"]["accuracy"] = json!(0.99);
        changes.push(("reported_accuracy", changed));
        let mut settings = input.clone();
        settings["api_mods"][1]["settings"]["speed_change"] = json!(1.6);
        settings["native_submission"]["mods"] = settings["api_mods"].clone();
        changes.push(("mod_settings", settings));
    } else {
        for field in [
            "n300",
            "n100",
            "n50",
            "misses",
            "geki",
            "katu",
            "large_tick_hits",
            "small_tick_hits",
            "slider_end_hits",
        ] {
            let mut changed = input.clone();
            changed["hit_counts"][field] = json!(1);
            changes.push((field, changed));
        }
    }
    changes
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn native_saved_retries_bind_immutable_input_after_review_and_withdrawal() {
    let fixture = MutationFixture::new("score_retry_binding", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let map = map(&state).await;
        let (user, _, _, _) = actor(&state, "RetryBinding", &[]).await;
        let set: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1")
            .bind(map).fetch_one(&state.db).await.unwrap();
        let mut stable = submission(map);
        stable["user_id"] = json!(user);
        stable["client"] = json!("stable");
        stable["ingestion_key"] = json!("retry-binding-stable");
        let mut lazer = stable.clone();
        lazer["client"] = json!("lazer");
        lazer["ingestion_key"] = json!("retry-binding-lazer");
        lazer["mods"] = json!(72);
        lazer["api_mods"] = json!([{"acronym":"HD"},{"acronym":"DT","settings":{"speed_change":1.5}}]);
        lazer["native_submission"] = json!({"ruleset_id":0,"passed":true,"total_score":1234,"total_score_without_mods":1234,"accuracy":1.0,"max_combo":2,"rank":"X","mods":lazer["api_mods"],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]});
        let mut saved = Vec::new();
        for input in [stable, lazer] {
            let (status, value) = native_ingest(&app, &input).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            let id = value["id"].as_i64().unwrap();
            // Historical sparse counters, moderation and recalculation caches
            // must not alter the meaning of an exact transport retry.
            sqlx::query("UPDATE scores SET hit_counts='{\"n300\":2}',verification='verified',pp=321,pp_calculation_version='fixture-reviewed.v2',score_scale_version='fixture-cache.v2',replay_path='retained-fixture.osr' WHERE id=$1")
                .bind(id).execute(&state.db).await.unwrap();
            sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.verify','score',$2,'{\"verification\":\"verified\"}')")
                .bind(user).bind(id.to_string()).execute(&state.db).await.unwrap();
            let snapshot: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM scores s WHERE id=$1")
                .bind(id).fetch_one(&state.db).await.unwrap();
            saved.push((input, id, snapshot));
        }
        for withdrawn in [false, true] {
            if withdrawn {
                let mut tx = state.db.begin().await.unwrap();
                crate::beatmap_availability::lock_exclusive_set(&mut tx, set).await.unwrap();
                sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'Synthetic retry boundary',$2)")
                    .bind(set).bind(user).execute(&mut *tx).await.unwrap();
                tx.commit().await.unwrap();
            }
            let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
                .fetch_one(&state.db).await.unwrap();
            for (input, id, snapshot) in &saved {
                let (status, value) = native_ingest(&app, input).await;
                assert_eq!(status, StatusCode::OK, "withdrawn={withdrawn}: {value}");
                assert_eq!(value["id"], *id);
                assert_eq!(value["verification"], "verified");
                assert_eq!(value["pp"], 321.0);
                for (field, changed) in immutable_retry_changes(input) {
                    let (status, value) = native_ingest(&app, &changed).await;
                    assert_eq!(status, StatusCode::CONFLICT, "{field}, withdrawn={withdrawn}: {value}");
                    assert_eq!(value["error"]["code"], "ingestion_conflict");
                }
                if input["client"] == "stable" {
                    let mut ignored_accuracy = input.clone();
                    ignored_accuracy["accuracy"] = json!(0.0);
                    assert_eq!(native_ingest(&app, &ignored_accuracy).await.0, StatusCode::OK);
                } else {
                    let mut semantic_retry = input.clone();
                    semantic_retry["api_mods"].as_array_mut().unwrap().reverse();
                    semantic_retry["native_submission"]["mods"] = semantic_retry["api_mods"].clone();
                    semantic_retry["native_submission"]["statistics"]["miss"] = json!(0);
                    semantic_retry["native_submission"]["maximum_statistics"]["miss"] = json!(0);
                    assert_eq!(native_ingest(&app, &semantic_retry).await.0, StatusCode::OK);
                }
                let retained: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM scores s WHERE id=$1")
                    .bind(id).fetch_one(&state.db).await.unwrap();
                assert_eq!(retained, *snapshot, "retries must leave the saved row intact");
            }
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM scores").fetch_one(&state.db).await.unwrap(), 2);
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_events").fetch_one(&state.db).await.unwrap(), audit_count);
        }
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn native_changed_retry_rechecks_a_winner_published_during_set_wait() {
    let fixture = MutationFixture::new("score_retry_wait", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let map = map(&state).await;
        let (user, _, _, _) = actor(&state, "RetryWaitBinding", &[]).await;
        let set: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1")
            .bind(map).fetch_one(&state.db).await.unwrap();
        let mut original = submission(map);
        original["user_id"] = json!(user);
        original["client"] = json!("stable");
        original["ingestion_key"] = json!("retry-wait-source");
        let (status, value) = native_ingest(&app, &original).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let source = value["id"].as_i64().unwrap();
        for withdrawn in [false, true] {
            let key = format!("retry-wait-winner-{withdrawn}");
            let mut input = original.clone();
            input["ingestion_key"] = json!(key);
            input["hit_counts"] = json!({"n300":1,"n100":1});
            let mut blocker = state.db.begin().await.unwrap();
            crate::beatmap_availability::lock_exclusive_set(&mut blocker, set).await.unwrap();
            let queued_app = app.clone();
            let pending = tokio::spawn(async move { native_ingest(&queued_app, &input).await });
            wait_for_lock(&state.db, &application, "illya_lock_beatmapset").await;
            let winner: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,pp_eligible,standardised_score,standardised_without_mods,classic_score,native_submission,native_basic_maximum,score_scale_status,score_scale_version,score_scale_reason,legacy_attributes,score_variant,pp_calculation_version) SELECT user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,$2,api_mods,pp_eligible,standardised_score,standardised_without_mods,classic_score,native_submission,native_basic_maximum,score_scale_status,score_scale_version,score_scale_reason,legacy_attributes,score_variant,pp_calculation_version FROM scores WHERE id=$1 RETURNING id")
                .bind(source).bind(&key).fetch_one(&mut *blocker).await.unwrap();
            if withdrawn {
                sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'Synthetic queued retry',$2)")
                    .bind(set).bind(user).execute(&mut *blocker).await.unwrap();
            }
            let before: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM scores s WHERE id=$1")
                .bind(winner).fetch_one(&mut *blocker).await.unwrap();
            blocker.commit().await.unwrap();
            let (status, value) = pending.await.unwrap();
            assert_eq!(status, StatusCode::CONFLICT, "withdrawn={withdrawn}: {value}");
            assert_eq!(value["error"]["code"], "ingestion_conflict");
            assert_eq!(sqlx::query_scalar::<_, Value>("SELECT to_jsonb(s) FROM scores s WHERE id=$1").bind(winner).fetch_one(&state.db).await.unwrap(), before);
            let mut exact = original.clone();
            exact["ingestion_key"] = json!(key);
            let (status, value) = native_ingest(&app, &exact).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value["id"], winner);
        }
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM scores").fetch_one(&state.db).await.unwrap(), 3);
    }).await;
    fixture.finish(result).await;
}

async fn replay_score(state: &AppState, user: i64, map: i64, name: &str) -> (i64, Vec<u8>) {
    let input: resonance_protocol::lazer::SoloScoreSubmission = serde_json::from_value(json!({"ruleset_id":0,"passed":true,"total_score":1234,"total_score_without_mods":1234,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]})).unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,api_mods,native_submission) VALUES($1,$2,0,1234,100,2,0,0,true,'{\"n300\":2}','lazer','[]',$3) RETURNING id")
        .bind(user).bind(map).bind(json!(input)).fetch_one(&state.db).await.unwrap();
    let checksum: String = sqlx::query_scalar("SELECT md5 FROM beatmaps WHERE id=$1")
        .bind(map)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let binding = resonance_protocol::lazer_replay::ReplayScore {
        online_id: id,
        user_id: user,
        username: name,
        beatmap_md5: &checksum,
        client_version: "fixture",
        mods_bits: 0,
        timestamp_ticks: 639269732961234567,
        submission: &input,
    };
    (
        id,
        resonance_protocol::lazer_replay::encode(
            &binding,
            b"0|256|192|0,100|256|192|1,-12345|0|0|0",
        )
        .unwrap(),
    )
}

async fn relax_score(state: &AppState, user: i64, map: i64) -> i64 {
    sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,score_variant,pp_eligible) VALUES($1,$2,0,1234,100,2,128,0,true,'{\"n300\":2}','stable','relax',true) RETURNING id")
        .bind(user).bind(map).fetch_one(&state.db).await.unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn score_and_replay_writes_recheck_queued_authority() {
    let fixture = MutationFixture::new("scores", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let map = map(&state).await;
        for operation in ["score", "replay", "replay_cached", "native", "native_cached", "recalculate"] {
            let changes: &[&str] = if operation.starts_with("native") { &["restrict"] } else { &["revoke", "restrict", "expire"] };
            for change in changes {
                let name = format!("Score{operation}{change}");
                let permissions: &[&str] = if operation == "recalculate" { &["scores.moderate"] } else { &[] };
                let (user, _, sid, cookie) = actor(&state, &name, permissions).await;
                let mut body = submission(map);
                let mut replay_id = None;
                let (path, content_type, raw, service) = if operation.starts_with("replay") {
                    let (id, bytes) = replay_score(&state, user, map, &name).await;
                    replay_id = Some(id);
                    let path = format!("/api/v1/scores/{id}/replay");
                    if operation == "replay_cached" {
                        let (status, value) = request(&app, &path, Some(&cookie), false, MULTIPART, multipart("fixture.osr", &bytes)).await;
                        assert_eq!(status, StatusCode::OK, "{value}");
                    }
                    (path, MULTIPART, multipart("fixture.osr", &bytes), false)
                } else if operation == "recalculate" {
                    let id = relax_score(&state, user, map).await;
                    ("/api/v1/admin/scores/recalculate-relax".to_string(), "application/json", json!({"score_ids":[id]}).to_string().into_bytes(), false)
                } else if operation.starts_with("native") {
                    body["user_id"] = json!(user); body["client"] = json!("stable"); body["ingestion_key"] = json!(format!("native-{user}"));
                    if operation == "native_cached" {
                        let (status, value) = request(&app, "/api/v1/internal/scores", None, true, "application/json", body.to_string().into_bytes()).await;
                        assert_eq!(status, StatusCode::OK, "{value}");
                    }
                    ("/api/v1/internal/scores".to_string(), "application/json", body.to_string().into_bytes(), true)
                } else {
                    ("/api/v1/scores".to_string(), "application/json", body.to_string().into_bytes(), false)
                };
                let scores_before: i64 = sqlx::query_scalar("SELECT count(*) FROM scores").fetch_one(&state.db).await.unwrap();
                let audits_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events").fetch_one(&state.db).await.unwrap();
                let files_before = file_count(&state.replay_root).await;
                let previous: Option<String> = if let Some(id) = replay_id {
                    sqlx::query_scalar("SELECT replay_path FROM scores WHERE id=$1").bind(id).fetch_one(&state.db).await.unwrap()
                } else { None };
                let mut blocker = state.db.begin().await.unwrap();
                sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(user).fetch_one(&mut *blocker).await.unwrap();
                let queued_app = app.clone();
                let pending = tokio::spawn(async move { request(&queued_app, &path, Some(&cookie), service, content_type, raw).await });
                let fragment = if service { "SELECT NOT is_restricted FROM users WHERE id=$1 FOR SHARE" } else { "SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR UPDATE" };
                wait_for_lock(&state.db, &application, fragment).await;
                match *change {
                    "revoke" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); }
                    "restrict" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(user).execute(&mut *blocker).await.unwrap(); }
                    "expire" => { sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); }
                    _ => unreachable!(),
                }
                blocker.commit().await.unwrap();
                let (status, value) = pending.await.unwrap();
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{operation}/{change}: {value}");
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM scores").fetch_one(&state.db).await.unwrap(), scores_before);
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events").fetch_one(&state.db).await.unwrap(), audits_before);
                assert_eq!(file_count(&state.replay_root).await, files_before, "rejected replay staging must be removed");
                if let Some(id) = replay_id {
                    assert_eq!(sqlx::query_scalar::<_,Option<String>>("SELECT replay_path FROM scores WHERE id=$1").bind(id).fetch_one(&state.db).await.unwrap(), previous);
                }
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM bancho_packet_queue").fetch_one(&state.db).await.unwrap(), 0);
            }
        }
        // No submission permission is invented: an ordinary active session works.
        let (_, _, _, cookie) = actor(&state, "ScoreAllowed", &[]).await;
        let (status, value) = request(&app, "/api/v1/scores", Some(&cookie), false, "application/json", submission(map).to_string().into_bytes()).await;
        assert_eq!(status, StatusCode::OK, "{value}");
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn score_and_replay_deadlines_are_checked_after_resource_waits() {
    let fixture = MutationFixture::new("score_deadline", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let map = map(&state).await;
        for operation in ["score", "replay", "replay_cached", "recalculate"] {
            let name = format!("Deadline{operation}");
            let permissions: &[&str] = if operation == "recalculate" { &["scores.moderate"] } else { &[] };
            let (user, _, sid, cookie) = actor(&state, &name, permissions).await;
            let (path, content_type, raw, replay_id) = if operation.starts_with("replay") {
                let (id, bytes) = replay_score(&state, user, map, &name).await;
                let path = format!("/api/v1/scores/{id}/replay");
                if operation == "replay_cached" {
                    assert_eq!(
                        request(
                            &app,
                            &path,
                            Some(&cookie),
                            false,
                            MULTIPART,
                            multipart("fixture.osr", &bytes)
                        )
                        .await
                        .0,
                        StatusCode::OK
                    );
                }
                (path, MULTIPART, multipart("fixture.osr", &bytes), Some(id))
            } else if operation == "recalculate" {
                let id = relax_score(&state, user, map).await;
                ("/api/v1/admin/scores/recalculate-relax".into(), "application/json", json!({"score_ids":[id]}).to_string().into_bytes(), Some(id))
            } else {
                (
                    "/api/v1/scores".into(),
                    "application/json",
                    submission(map).to_string().into_bytes(),
                    None,
                )
            };
            let scores_before: i64 = sqlx::query_scalar("SELECT count(*) FROM scores")
                .fetch_one(&state.db)
                .await
                .unwrap();
            let audits_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
                .fetch_one(&state.db)
                .await
                .unwrap();
            let files_before = file_count(&state.replay_root).await;
            let expires_at = sqlx::query_scalar(
                "UPDATE sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE id=$1 RETURNING expires_at",
            )
            .bind(sid)
            .fetch_one(&state.db)
            .await
            .unwrap();
            let mut blocker = state.db.begin().await.unwrap();
            if let Some(id) = replay_id {
                sqlx::query("SELECT id FROM scores WHERE id=$1 FOR UPDATE")
                    .bind(id)
                    .fetch_one(&mut *blocker)
                    .await
                    .unwrap();
            } else {
                sqlx::query("SELECT id FROM beatmaps WHERE id=$1 FOR UPDATE")
                    .bind(map)
                    .fetch_one(&mut *blocker)
                    .await
                    .unwrap();
            }
            let queued_app = app.clone();
            let pending = tokio::spawn(async move {
                request(&queued_app, &path, Some(&cookie), false, content_type, raw).await
            });
            wait_for_lock(
                &state.db,
                &application,
                if replay_id.is_some() {
                    "FOR UPDATE OF s"
                } else {
                    "INSERT INTO scores("
                },
            )
            .await;
            wait_until_expired(&state.db, expires_at).await;
            blocker.commit().await.unwrap();
            let (status, value) = pending.await.unwrap();
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{operation}: {value}");
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM scores")
                    .fetch_one(&state.db)
                    .await
                    .unwrap(),
                scores_before
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_events")
                    .fetch_one(&state.db)
                    .await
                    .unwrap(),
                audits_before
            );
            assert_eq!(file_count(&state.replay_root).await, files_before);
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn replay_files_are_cleaned_after_definite_commit_rejection() {
    let fixture = MutationFixture::new("replay_commit", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let map = map(&state).await;
        let (user, _, _, cookie) = actor(&state, "ReplayCommit", &[]).await;
        let (id, bytes) = replay_score(&state, user, map, "ReplayCommit").await;
        reject_audits_at_commit(&state.db).await;
        let (status, value) = request(
            &app,
            &format!("/api/v1/scores/{id}/replay"),
            Some(&cookie),
            false,
            MULTIPART,
            multipart("fixture.osr", &bytes),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{value}");
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>("SELECT replay_path FROM scores WHERE id=$1")
                .bind(id)
                .fetch_one(&state.db)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_events")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(file_count(&state.replay_root).await, 0);
    })
    .await;
    fixture.finish(result).await;
}
