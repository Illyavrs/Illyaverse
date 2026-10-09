use super::*;
use crate::{
    maps::map_mutation_tests::{MutationFixture, actor, file_count},
    native_map_tests::OfficialServer,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

fn metadata(set_id: i64, count: usize) -> Value {
    json!({"id":set_id,"artist":"Artist","artist_unicode":"歌手","title":"Title","title_unicode":"標題","creator":"Creator","status":"ranked","bpm":180.0,"source":"","tags":"fixture","nsfw":false,"submitted_date":"2020-01-02T03:04:05Z","last_updated":"2020-02-02T03:04:05Z","ranked_date":null,"genre":{"id":5,"name":"Video Game"},"language":{"id":3,"name":"Japanese"},"beatmaps":(0..count).map(|index| { let id=set_id*10+index as i64; json!({"id":id,"beatmapset_id":set_id,"checksum":format!("{id:032x}"),"version":format!("Hard {index}"),"mode_int":0,"mode":"osu","status":"ranked","last_updated":"2020-02-02T03:04:05Z","difficulty_rating":index as f64 / 10.0+1.0,"bpm":180.0,"total_length":150,"hit_length":135,"max_combo":432,"ar":9.5,"cs":4.0,"accuracy":8.5,"drain":6.0,"count_circles":100,"count_sliders":140,"count_spinners":2}) }).collect::<Vec<_>>()})
}
#[test]
fn map_card_preview_preserves_all_official_identities_without_retained_content() {
    let mut upstream = metadata(42, 25);
    upstream["beatmaps"][0]["version"] = json!("Hard\n Name");
    let validated = OfficialSetMetadata::from_upstream(&upstream, 42).unwrap();
    let result = official_preview(validated, &HashMap::from([(420, 7)])).unwrap();
    assert_eq!(result.total, 25);
    assert_eq!(result.stored_count, 1);
    assert_eq!(result.items[0].id, Some(7));
    assert_eq!(result.items[0].official_id, Some(420));
    assert!(result.items.iter().skip(1).all(|item| item.id.is_none()));
    assert_eq!(result.items[0].version, "Hard  Name");
    let json = serde_json::to_value(result).unwrap();
    assert!(json["items"][0].get("checksum").is_none());
    assert!(json.get("title").is_none());
}
#[test]
fn map_card_preview_revalidates_bounded_metadata() {
    let mut valid = OfficialSetMetadata::from_upstream(&metadata(42, 1), 42).unwrap();
    valid.beatmaps[0].version = "unsafe\nname".into();
    assert!(official_preview(valid, &HashMap::new()).is_err());
    let mut valid = OfficialSetMetadata::from_upstream(&metadata(42, 1), 42).unwrap();
    valid.beatmaps = vec![valid.beatmaps[0].clone(); 513];
    assert!(official_preview(valid, &HashMap::new()).is_err());
}
async fn get_preview(app: &Router, id: i64) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/beatmaps/{id}/set/preview"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn seed_official(state: &AppState, set_id: i64) -> i64 {
    let official_id = set_id * 10;
    sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES($1,$2,'Retained private title','A','C','Retained old version',0,'ranked','official',99,$3,'private.osu') RETURNING id")
        .bind(official_id).bind(set_id).bind(format!("{official_id:032x}")).fetch_one(&state.db).await.unwrap()
}
async fn seed_local(state: &AppState) -> i64 {
    sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES('Local','A','C','Local Normal',0,'ranked','local',1,$1,'private-local.osu') RETURNING id")
        .bind("a".repeat(32)).fetch_one(&state.db).await.unwrap()
}
async fn seed_local_package(state: &AppState, count: usize) -> i64 {
    let package = Uuid::new_v4();
    let manifest = (1..=count)
        .map(|index| format!("{index:032x}"))
        .collect::<Vec<_>>();
    let mut tx = state.db.begin().await.unwrap();
    sqlx::query("INSERT INTO beatmap_packages(id,kind,member_md5,archive_path,legacy) VALUES($1,'osz',$2,'private-local.osz',true)")
        .bind(package).bind(&manifest).execute(&mut *tx).await.unwrap();
    let mut first = None;
    for (index, md5) in manifest.iter().enumerate() {
        let id: i64 = sqlx::query_scalar("INSERT INTO beatmaps(local_package_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path,archive_path) VALUES($1,'Local','A','C',$2,0,'ranked','local',$3,$4,'private-local.osu','private-local.osz') RETURNING id")
            .bind(package).bind(format!("Local {index}")).bind(index as f64 / 10.0).bind(md5).fetch_one(&mut *tx).await.unwrap();
        first.get_or_insert(id);
    }
    tx.commit().await.unwrap();
    first.unwrap()
}
async fn unchanged(state: &AppState, maps: i64) {
    for (table, expected) in [
        ("beatmaps", maps),
        ("beatmap_filename_aliases", 0),
        ("official_beatmapset_archives", 0),
        ("official_beatmapset_archive_members", 0),
        ("audit_events", 0),
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, expected, "{table}");
    }
    assert_eq!(file_count(&state.maps_root).await, 0);
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn map_card_preview_known_local_does_not_need_official_configuration() {
    let fixture = MutationFixture::new("card_preview_local", "http://127.0.0.1:1".into()).await;
    let mut state = fixture.state.clone();
    state.official = Arc::new(Mutex::new(OfficialState::fixture(
        "http://127.0.0.1:1".into(),
        false,
    )));
    let app = crate::router(state.clone());
    let result = tokio::spawn(async move {
        let local = seed_local(&state).await;
        let (status, value) = get_preview(&app, local).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["source"], "local");
        assert_eq!(value["total"], 1);
        assert_eq!(value["stored_count"], 1);
        assert_eq!(value["items"][0]["id"], local);
        assert_eq!(value["items"][0]["official_id"], Value::Null);
        for id in [0, -1, 99999] {
            assert_eq!(get_preview(&app, id).await.0, StatusCode::NOT_FOUND);
        }
        let package = seed_local_package(&state, 25).await;
        let (status, value) = get_preview(&app, package).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["total"], 25);
        assert_eq!(value["stored_count"], 25);
        assert_eq!(value["items"][24]["version"], "Local 24");
        let official = seed_official(&state, 42).await;
        let (status, value) = get_preview(&app, official).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "official_api_not_configured");
        unchanged(&state, 27).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn map_card_preview_official_complete_cache_and_fresh_withdrawal() {
    let upstream = OfficialServer::start().await;
    upstream.add_set(42, metadata(42, 25)).await;
    let fixture = MutationFixture::new("card_preview_complete", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result=tokio::spawn(async move {
        let known=seed_official(&state,42).await;
        for _ in 0..2 { let (status,value)=get_preview(&app,known).await; assert_eq!(status,StatusCode::OK,"{value}"); assert_eq!(value["total"],25); assert_eq!(value["stored_count"],1); assert_eq!(value["items"][0]["id"],known); assert_eq!(value["items"][1]["id"],Value::Null); assert_eq!(value["items"][0]["stars"],1.0); assert_eq!(value["items"][0]["version"],"Hard 0"); }
        assert_eq!(upstream.set_queries().await,vec![42]); assert_eq!(upstream.counters(),[1,0,0,0]);
        let (staff,_,_,_)=actor(&state,"PreviewWithdrawal",&[]).await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(42,true,1,'Owned fixture',$1)").bind(staff).execute(&state.db).await.unwrap();
        let (status,value)=get_preview(&app,known).await; assert_eq!(status,StatusCode::NOT_FOUND); assert_eq!(value["error"]["code"],"not_found"); assert!(value.get("items").is_none());
        assert_eq!(upstream.set_queries().await,vec![42]); unchanged(&state,1).await;
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn map_card_preview_coalesces_with_native_and_preserves_actor_restriction() {
    let upstream = OfficialServer::start().await;
    upstream.add_set(42, metadata(42, 25)).await;
    let pause = upstream.pause_next_set().await;
    let fixture = MutationFixture::new("card_preview_native", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let known = seed_official(&state, 42).await;
        let (user, _, _, _) = actor(&state, "PreviewNativeActor", &[]).await;
        let native_app = app.clone();
        let key = state.internal_key.to_string();
        let native = tokio::spawn(async move {
            let response = native_app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/internal/beatmapsets/metadata")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header("x-illya-service-key", key)
                        .body(Body::from(
                            json!({"user_id":user,"official_set_id":42}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            response.status()
        });
        pause.wait_entered().await;
        let preview_app = app.clone();
        let public = tokio::spawn(async move { get_preview(&preview_app, known).await });
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(user)
            .execute(&state.db)
            .await
            .unwrap();
        pause.release();
        assert_eq!(native.await.unwrap(), StatusCode::UNAUTHORIZED);
        let (status, value) = public.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["total"], 25);
        assert_eq!(upstream.set_queries().await, vec![42]);
        unchanged(&state, 1).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn map_card_preview_withdrawal_during_wait_precedes_upstream_success() {
    let upstream = OfficialServer::start().await;
    upstream.add_set(42, metadata(42, 25)).await;
    let pause = upstream.pause_next_set().await;
    let fixture = MutationFixture::new("card_preview_race", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result=tokio::spawn(async move {
        let known=seed_official(&state,42).await; let (staff,_,_,_)=actor(&state,"PreviewRaceActor",&[]).await;
        let request_app=app.clone(); let request=tokio::spawn(async move { get_preview(&request_app,known).await }); pause.wait_entered().await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(42,true,1,'Owned fixture',$1)").bind(staff).execute(&state.db).await.unwrap(); pause.release();
        let (status,value)=request.await.unwrap(); assert_eq!(status,StatusCode::NOT_FOUND); assert_eq!(value["error"]["code"],"not_found"); unchanged(&state,1).await;
    }).await;
    fixture.finish(result).await;
}
