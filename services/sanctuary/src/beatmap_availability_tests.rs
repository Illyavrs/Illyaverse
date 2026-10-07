//! Withdrawal fixtures use owned PG17 schemas and loopback upstreams only.
use crate::{
    AppState, Router,
    maps::map_mutation_tests::{
        MULTIPART, MutationFixture, actor, file_count, multipart, osu_bytes, request as post,
        wait_for_lock,
    },
    native_map_tests::{OfficialMap, OfficialServer},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::Row;
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    value: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(value.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
pub(crate) async fn change(
    app: &Router,
    cookie: &str,
    set: i64,
    withdrawn: bool,
    revision: i64,
    reason: &str,
) -> (StatusCode, Value) {
    request(
        app,
        "PATCH",
        &format!("/api/v1/admin/beatmapsets/{set}/availability"),
        Some(cookie),
        json!({"withdrawn":withdrawn,"expected_revision":revision,"reason":reason}),
    )
    .await
}
async fn upload(app: &Router, cookie: &str, title: &str) -> Value {
    let (status, value) = post(
        app,
        "/api/v1/beatmaps/upload",
        Some(cookie),
        false,
        MULTIPART,
        multipart("Availability.osu", &osu_bytes(title)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
async fn metadata(app: &Router, user: i64, set: i64) -> (StatusCode, Value) {
    post(
        app,
        "/api/v1/internal/beatmapsets/metadata",
        None,
        true,
        "application/json",
        json!({"user_id":user,"official_set_id":set})
            .to_string()
            .into_bytes(),
    )
    .await
}
async fn resolve(app: &Router, body: Value) -> (StatusCode, Value) {
    post(
        app,
        "/api/v1/internal/beatmaps/resolve",
        None,
        true,
        "application/json",
        body.to_string().into_bytes(),
    )
    .await
}
fn set_metadata(set: i64) -> Value {
    json!({"id":set,"artist":"Fixture","artist_unicode":"","title":"Availability","title_unicode":"","creator":"Fixture","status":"ranked","bpm":120,"source":"","tags":"","nsfw":false,"submitted_date":"2020-01-01T00:00:00Z","last_updated":"2020-01-01T00:00:00Z","ranked_date":null,"genre":{"id":1,"name":"Unspecified"},"language":{"id":1,"name":"Unspecified"},"beatmaps":[{"id":set+100,"beatmapset_id":set,"checksum":format!("{set:032x}"),"version":"Easy","mode_int":0,"mode":"osu","status":"ranked","last_updated":"2020-01-01T00:00:00Z","difficulty_rating":1.0,"bpm":120,"total_length":2,"hit_length":2,"max_combo":2,"ar":5.0,"cs":4.0,"accuracy":5.0,"drain":5.0,"count_circles":2,"count_sliders":0,"count_spinners":0}]})
}
async fn state_counts(state: &AppState) -> (i64, i64) {
    (sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action IN('beatmaps.withdraw','beatmaps.restore')").fetch_one(&state.db).await.unwrap(),sqlx::query_scalar("SELECT count(*) FROM lazer_beatmap_updates").fetch_one(&state.db).await.unwrap())
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn withdrawal_permissions_revision_idempotency_and_atomic_events() {
    let fixture = MutationFixture::new("availability_api", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, viewer) = actor(&state, "AvailabilityViewer", &["admin.view"]).await;
        let (_, _, _, staff) = actor(
            &state,
            "AvailabilityStaff",
            &["beatmaps.moderate", "admin.view"],
        )
        .await;
        assert_eq!(
            request(
                &app,
                "PATCH",
                "/api/v1/admin/beatmapsets/42/availability",
                None,
                json!({"withdrawn":true,"expected_revision":0,"reason":"Review"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            change(&app, &viewer, 42, true, 0, "Review").await.0,
            StatusCode::FORBIDDEN
        );
        for reason in [" ".into(), "x".repeat(501)] {
            assert_eq!(
                change(&app, &staff, 42, true, 0, &reason).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            change(&app, &staff, 2_000_000_000, true, 0, "Unknown local set")
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            change(&app, &staff, 0, true, 0, "Invalid").await.0,
            StatusCode::BAD_REQUEST
        );
        let initial = state_counts(&state).await;
        let (status, noop) = change(&app, &staff, 42, false, 0, "No transition").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(noop["availability"]["revision"], 0);
        assert_eq!(state_counts(&state).await, initial);
        let (status, first) = change(&app, &staff, 42, true, 0, "  Staff review  ").await;
        assert_eq!(status, StatusCode::OK, "{first}");
        assert_eq!(first["availability"]["revision"], 1);
        assert_eq!(first["availability"]["reason"], "Staff review");
        let after = state_counts(&state).await;
        assert_eq!(after, (initial.0 + 1, initial.1 + 1));
        let (status, same) = change(&app, &staff, 42, true, 1, "Replacement ignored").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(same, first);
        assert_eq!(state_counts(&state).await, after);
        let (status, stale) = change(&app, &staff, 42, false, 0, "Stale editor").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(stale["error"]["code"], "availability_conflict");
        let (status, restored) = change(&app, &staff, 42, false, 1, "Resolved").await;
        assert_eq!(status, StatusCode::OK, "{restored}");
        assert_eq!(restored["availability"]["revision"], 2);
        assert_eq!(state_counts(&state).await, (initial.0 + 2, initial.1 + 2));
        let audits: Vec<Value> = sqlx::query_scalar(
            "SELECT details FROM audit_events WHERE target_type='beatmapset' ORDER BY id",
        )
        .fetch_all(&state.db)
        .await
        .unwrap();
        assert_eq!(audits[0]["before"]["revision"], 0);
        assert_eq!(audits[0]["after"]["withdrawn"], true);
        assert_eq!(audits[1]["after"]["withdrawn"], false);
        crate::maps::map_mutation_tests::reject_audits_at_commit(&state.db).await;
        assert_eq!(
            change(&app, &staff, 47, true, 0, "Atomic audit rejection")
                .await
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(state_counts(&state).await, (initial.0 + 2, initial.1 + 2));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM beatmapset_availability WHERE native_set_id=47"
            )
            .fetch_one(&state.db)
            .await
            .unwrap(),
            0
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn withdrawal_preserves_history_bytes_identity_and_restores_active_pp() {
    let fixture = MutationFixture::new("availability_lifecycle", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result=tokio::spawn(async move {
        let (owner,_,_,cookie)=actor(&state,"AvailabilityOwner",&["beatmaps.upload","beatmaps.moderate","admin.view"]).await;
        let uploaded=upload(&app,&cookie,"Availability lifecycle").await;let id=uploaded["items"][0]["id"].as_i64().unwrap();let set=uploaded["items"][0]["native_set_id"].as_i64().unwrap();
        let original=sqlx::query("SELECT md5,native_id,native_set_id,osu_path,local_package_id FROM beatmaps WHERE id=$1").bind(id).fetch_one(&state.db).await.unwrap();
        let path:String=original.get("osu_path");let bytes=tokio::fs::read(state.maps_root.join(&path)).await.unwrap();let files=file_count(&state.maps_root).await;
        assert_eq!(request(&app,"PATCH",&format!("/api/v1/admin/beatmaps/{id}"),Some(&cookie),json!({"status":"ranked"})).await.0,StatusCode::OK);
        let score:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,verification,pp_eligible,standardised_score,classic_score,score_scale_status,replay_path,ingestion_key) VALUES($1,$2,0,123,100,2,0,200,true,'{\"n300\":2}','stable','verified',true,500000,500000,'legacy_estimated','kept.osr','availability-saved') RETURNING id").bind(owner).bind(id).fetch_one(&state.db).await.unwrap();
        tokio::fs::write(state.replay_root.join("kept.osr"),b"retained immutable replay fixture").await.unwrap();
        let profile=format!("/api/v1/users/{owner}");let best=format!("/api/v1/users/{owner}/scores/best");
        assert_eq!(request(&app,"GET",&profile,None,Value::Null).await.1["statistics"]["pp"],200.0);
        assert_eq!(request(&app,"GET",&best,None,Value::Null).await.1["total"],1);
        assert_eq!(change(&app,&cookie,set,true,0,"Package review").await.0,StatusCode::OK);
        for route in [format!("/api/v1/beatmaps/{id}"),format!("/api/v1/beatmaps/{id}/download"),format!("/api/v1/beatmaps/{id}/osu"),format!("/api/v1/beatmaps/{id}/package/difficulties"),format!("/api/v1/beatmaps/{id}/scores")] {
            let (status,value)=request(&app,"GET",&route,Some(&cookie),Value::Null).await;assert_eq!(status,StatusCode::GONE,"{route}: {value}");assert_eq!(value["error"]["code"],"beatmap_withdrawn");
        }
        assert_eq!(request(&app,"GET","/api/v1/beatmaps?availability=all&status=ranked",None,Value::Null).await.1["total"],0);
        assert_eq!(request(&app,"GET","/api/v1/admin/beatmaps",Some(&cookie),Value::Null).await.1["total"],0);
        let retained=request(&app,"GET","/api/v1/admin/beatmaps?availability=withdrawn",Some(&cookie),Value::Null).await.1;assert_eq!(retained["total"],1);assert_eq!(retained["items"][0]["availability"]["reason"],"Package review");
        let retained=request(&app,"GET",&format!("/api/v1/admin/beatmaps/{id}"),Some(&cookie),Value::Null).await;assert_eq!(retained.0,StatusCode::OK);assert_eq!(retained.1["availability"]["withdrawn"],true);
        let changed=request(&app,"PATCH",&format!("/api/v1/admin/beatmaps/{id}"),Some(&cookie),json!({"status":"loved"})).await;assert_eq!(changed.0,StatusCode::OK);assert_eq!(changed.1["availability"]["withdrawn"],true);
        assert_eq!(request(&app,"GET",&best,None,Value::Null).await.1["total"],0);
        let stats=request(&app,"GET",&profile,None,Value::Null).await.1;assert_eq!(stats["statistics"]["pp"],0.0);assert_eq!(stats["statistics"]["play_count"],1);
        let history=request(&app,"GET",&format!("/api/v1/scores/{score}"),None,Value::Null).await;assert_eq!(history.0,StatusCode::OK);assert_eq!(history.1["has_replay"],true);
        assert_eq!(request(&app,"GET",&format!("/api/v1/scores?user_id={owner}"),None,Value::Null).await.1["total"],1);
        assert_eq!(file_count(&state.maps_root).await,files);assert_eq!(tokio::fs::read(state.maps_root.join(&path)).await.unwrap(),bytes);assert!(state.replay_root.join("kept.osr").exists());
        let current=sqlx::query("SELECT md5,native_id,native_set_id,osu_path,local_package_id FROM beatmaps WHERE id=$1").bind(id).fetch_one(&state.db).await.unwrap();
        for field in ["md5","osu_path"] {assert_eq!(current.get::<String,_>(field),original.get::<String,_>(field));}
        for field in ["native_id","native_set_id"] {assert_eq!(current.get::<i64,_>(field),original.get::<i64,_>(field));}
        assert_eq!(current.get::<uuid::Uuid,_>("local_package_id"),original.get::<uuid::Uuid,_>("local_package_id"));
        assert_eq!(request(&app,"PATCH",&format!("/api/v1/admin/beatmaps/{id}"),Some(&cookie),json!({"status":"ranked"})).await.0,StatusCode::OK);
        assert_eq!(change(&app,&cookie,set,false,1,"Review complete").await.0,StatusCode::OK);
        let public=request(&app,"GET",&format!("/api/v1/beatmaps/{id}"),None,Value::Null).await;assert_eq!(public.0,StatusCode::OK);assert!(public.1["availability"].get("reason").is_none());assert!(public.1["availability"].get("actor_id").is_none());
        assert_eq!(request(&app,"GET",&best,None,Value::Null).await.1["total"],1);assert_eq!(request(&app,"GET",&profile,None,Value::Null).await.1["statistics"]["pp"],200.0);
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn withdrawal_overrides_metadata_caches_and_uncached_official_resolvers() {
    let server = OfficialServer::start().await;
    server.add_set(42, set_metadata(42)).await;
    let map = OfficialMap::new(700, "ranked");
    let checksum = map.checksum();
    server.add(map.clone()).await;
    server.add_filename("Cold.osu", map).await;
    let fixture = MutationFixture::new("availability_cache", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) = actor(&state, "AvailabilityCache", &["beatmaps.moderate"]).await;
        assert_eq!(metadata(&app, user, 42).await.0, StatusCode::OK);
        assert_eq!(metadata(&app, user, 43).await.0, StatusCode::NOT_FOUND);
        let calls = server.set_queries().await;
        assert_eq!(calls, vec![42, 43]);
        for set in [42, 43, 1700] {
            assert_eq!(
                change(&app, &cookie, set, true, 0, "Withheld").await.0,
                StatusCode::OK
            );
        }
        for set in [42, 43] {
            let result = metadata(&app, user, set).await;
            assert_eq!(result.0, StatusCode::GONE, "{}", result.1);
        }
        assert_eq!(server.set_queries().await, calls);
        for selector in [
            json!({"official_id":700}),
            json!({"checksum":checksum}),
            json!({"filename":"Cold.osu"}),
        ] {
            let mut input = selector;
            input["user_id"] = json!(user);
            let result = resolve(&app, input).await;
            assert_eq!(result.0, StatusCode::GONE, "{}", result.1);
        }
        assert_eq!(
            server.counters()[3],
            0,
            "Withdrawal must prevent the .osu request after set identification"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM beatmaps")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(file_count(&state.maps_root).await, 0);
        assert_eq!(
            change(&app, &cookie, 42, false, 1, "Approved").await.0,
            StatusCode::OK
        );
        assert_eq!(metadata(&app, user, 42).await.0, StatusCode::OK);
        assert_eq!(server.set_queries().await, calls);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn withdrawal_during_upstream_wait_prevents_cache_response_and_publication() {
    let server = OfficialServer::start().await;
    server.add_set(44, set_metadata(44)).await;
    server.add(OfficialMap::new(701, "ranked")).await;
    let fixture = MutationFixture::new("availability_flight", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, cookie) =
            actor(&state, "AvailabilityFlight", &["beatmaps.moderate"]).await;
        let pause = server.pause_next_set().await;
        let request_app = app.clone();
        let job = tokio::spawn(async move { metadata(&request_app, user, 44).await });
        pause.wait_entered().await;
        assert_eq!(
            change(&app, &cookie, 44, true, 0, "Withdraw while fetching")
                .await
                .0,
            StatusCode::OK
        );
        pause.release();
        assert_eq!(job.await.unwrap().0, StatusCode::GONE);
        assert_eq!(state.official.lock().await.metadata_cache_shape().0, 0);
        let pause = server.pause_next_lookup().await;
        let request_app = app.clone();
        let job = tokio::spawn(async move {
            resolve(
                &request_app,
                json!({"user_id":user,"checksum":OfficialMap::new(701,"ranked").checksum()}),
            )
            .await
        });
        pause.wait_entered().await;
        assert_eq!(
            change(
                &app,
                &cookie,
                1701,
                true,
                0,
                "Withdraw difficulty during discovery"
            )
            .await
            .0,
            StatusCode::OK
        );
        pause.release();
        assert_eq!(job.await.unwrap().0, StatusCode::GONE);
        assert_eq!(server.counters()[3], 0);
        assert_eq!(file_count(&state.maps_root).await, 0);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM beatmaps")
                .fetch_one(&state.db)
                .await
                .unwrap(),
            0
        );
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn withdrawal_queued_editors_use_one_revision_and_recheck_permissions() {
    let fixture = MutationFixture::new("availability_wait", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, one) = actor(&state, "AvailabilityEditorOne", &["beatmaps.moderate"]).await;
        let (_, _, _, two) = actor(&state, "AvailabilityEditorTwo", &["beatmaps.moderate"]).await;
        let mut blocker = state.db.begin().await.unwrap();
        crate::beatmap_availability::lock_exclusive_set(&mut blocker, 45)
            .await
            .unwrap();
        let a = app.clone();
        let b = app.clone();
        let first =
            tokio::spawn(async move { change(&a, &one, 45, true, 0, "First editor").await });
        let second =
            tokio::spawn(async move { change(&b, &two, 45, true, 0, "Second editor").await });
        wait_for_lock(&state.db, &application, "pg_advisory_xact_lock").await;
        blocker.commit().await.unwrap();
        let mut results = [first.await.unwrap().0, second.await.unwrap().0];
        results.sort();
        assert_eq!(results, [StatusCode::OK, StatusCode::CONFLICT]);
        assert_eq!(state_counts(&state).await.0, 1);
        let (user, role, _, cookie) =
            actor(&state, "AvailabilityRevoked", &["beatmaps.moderate"]).await;
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let a = app.clone();
        let job =
            tokio::spawn(
                async move { change(&a, &cookie, 46, true, 0, "Queued permission").await },
            );
        wait_for_lock(&state.db, &application, "ORDER BY id FOR UPDATE").await;
        // The request's permission table SHARE fence blocks external role edits;
        // a session revocation by this preexisting authority holder remains safe.
        sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(user)
            .execute(&mut *blocker)
            .await
            .unwrap();
        blocker.commit().await.unwrap();
        assert_eq!(job.await.unwrap().0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM beatmapset_availability WHERE native_set_id=46"
            )
            .fetch_one(&state.db)
            .await
            .unwrap(),
            0
        );
        assert!(role > 0);
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn withdrawal_saved_score_retry_survives_but_queued_new_score_and_upload_do_not() {
    let fixture = MutationFixture::new("availability_retries", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result=tokio::spawn(async move {
        let (user,_,_,cookie)=actor(&state,"AvailabilityRetry",&["beatmaps.upload","beatmaps.moderate"]).await;let uploaded=upload(&app,&cookie,"Availability retry").await;let id=uploaded["items"][0]["id"].as_i64().unwrap();let set=uploaded["items"][0]["native_set_id"].as_i64().unwrap();
        let input=json!({"user_id":user,"beatmap_id":id,"mode":0,"score":123,"accuracy":100.0,"max_combo":2,"mods":0,"passed":true,"hit_counts":{"n300":2},"client":"stable","ingestion_key":"availability-score-retry"});
        let (status,saved)=post(&app,"/api/v1/internal/scores",None,true,"application/json",input.to_string().into_bytes()).await;assert_eq!(status,StatusCode::OK,"{saved}");
        let mut blocker=state.db.begin().await.unwrap();crate::beatmap_availability::lock_exclusive_set(&mut blocker,set).await.unwrap();
        let mut fresh=input.clone();fresh["ingestion_key"]=json!("availability-new-score");let a=app.clone();let job=tokio::spawn(async move {post(&a,"/api/v1/internal/scores",None,true,"application/json",fresh.to_string().into_bytes()).await});wait_for_lock(&state.db,&application,"illya_lock_beatmapset").await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'Withdrawal during score admission',$2)").bind(set).bind(user).execute(&mut *blocker).await.unwrap();blocker.commit().await.unwrap();assert_eq!(job.await.unwrap().0,StatusCode::GONE);
        let retried=post(&app,"/api/v1/internal/scores",None,true,"application/json",input.to_string().into_bytes()).await;assert_eq!(retried.0,StatusCode::OK);assert_eq!(retried.1["id"],saved["id"]);
        let files=file_count(&state.maps_root).await;let retry=post(&app,"/api/v1/beatmaps/upload",Some(&cookie),false,MULTIPART,multipart("Availability.osu",&osu_bytes("Availability retry"))).await;assert_eq!(retry.0,StatusCode::GONE,"{}",retry.1);assert_eq!(file_count(&state.maps_root).await,files);assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM scores").fetch_one(&state.db).await.unwrap(),1);
        let mismatched=resolve(&app,json!({"user_id":user,"filename":"Availability.osu"})).await;assert_eq!(mismatched.0,StatusCode::GONE);assert_eq!(resolve(&app,json!({"user_id":user,"checksum":uploaded["items"][0]["md5"]})).await.0,StatusCode::GONE);
        assert_eq!(change(&app,&cookie,set,false,1,"Restored retry").await.0,StatusCode::OK);let retry=post(&app,"/api/v1/beatmaps/upload",Some(&cookie),false,MULTIPART,multipart("Availability.osu",&osu_bytes("Availability retry"))).await;assert_eq!(retry.0,StatusCode::OK);assert_eq!(retry.1["idempotent"],true);assert_eq!(retry.1["items"][0]["id"],id);
    }).await;
    fixture.finish(result).await;
}
