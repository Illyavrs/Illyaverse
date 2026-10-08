//! Real collection and HTTP results in owned disposable PostgreSQL 17 schemas.
use crate::{
    Router,
    maps::map_mutation_tests::{MutationFixture, actor},
    rank_history::{CAPTURE_LOCK, CaptureOutcome, capture_current_day},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::{DateTime, Days, NaiveDate, Utc};
use futures_util::FutureExt;
use illya_domain::ScoreVariant;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{panic::AssertUnwindSafe, time::Duration};
use tower::ServiceExt;

async fn request(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder().uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
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

async fn get(app: &Router, path: &str) -> Value {
    let (status, value) = request(app, path, None).await;
    assert_eq!(status, StatusCode::OK, "{path}: {value}");
    value
}

async fn history(app: &Router, user: i64, mode: i16, variant: ScoreVariant) -> Value {
    get(
        app,
        &format!(
            "/api/v1/users/{user}/rank-history?mode={mode}&variant={}&days=90",
            variant.as_str()
        ),
    )
    .await
}

async fn map(db: &PgPool, label: &str, mode: i16, status: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES($1,'Rank history fixture','Illyaverse','Test',$2,$3,1,$4,'fixture.osu') RETURNING id")
        .bind(label).bind(mode).bind(status).bind(format!("{:x}",md5::compute(label))).fetch_one(db).await.unwrap()
}

struct Play {
    mode: i16,
    variant: ScoreVariant,
    pp: f64,
    passed: bool,
    verification: &'static str,
    eligible: bool,
}

impl Play {
    fn pp(mode: i16, variant: ScoreVariant, pp: f64) -> Self {
        Self {
            mode,
            variant,
            pp,
            passed: true,
            verification: "verified",
            eligible: true,
        }
    }
}

async fn score(db: &PgPool, user: i64, map: i64, play: Play) -> i64 {
    sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score_variant,mods,pp,score,accuracy,max_combo,passed,verification,pp_eligible,client) VALUES($1,$2,$3,$4,$5,$6,12345,95,100,$7,$8,$9,'stable') RETURNING id")
        .bind(user).bind(map).bind(play.mode).bind(play.variant.as_str()).bind(if play.variant == ScoreVariant::Relax {128_i64}else{0})
        .bind(play.pp).bind(play.passed).bind(play.verification).bind(play.eligible).fetch_one(db).await.unwrap()
}

async fn observed(
    db: &PgPool,
    user: i64,
    date: NaiveDate,
    mode: i16,
    variant: ScoreVariant,
) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('pp',pp,'rank',rank,'country_rank',country_rank,'country',country,'date',date,'captured_at',captured_at) FROM rank_history_daily WHERE user_id=$1 AND date=$2 AND mode=$3 AND variant=$4")
        .bind(user).bind(date).bind(mode).bind(variant.as_str()).fetch_one(db).await.unwrap()
}

async fn stored(db: &PgPool) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('batches',COALESCE((SELECT jsonb_agg(to_jsonb(b) ORDER BY date) FROM rank_history_batches b),'[]'::jsonb),'points',COALESCE((SELECT jsonb_agg(to_jsonb(h) ORDER BY user_id,mode,variant,date) FROM rank_history_daily h),'[]'::jsonb))")
        .fetch_one(db).await.unwrap()
}

async fn wait_for_capture_barrier(db: &PgPool, application: &str, key: i64) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // pg_stat_activity truncates long SQL before the INSERT appended to
            // RANKING_CTE. Check its visible prefix and the exact fixture lock,
            // rather than relying on a suffix that may never be observable.
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid
                 WHERE a.application_name=$1 AND a.wait_event_type='Lock'
                 AND a.query LIKE 'WITH eligible AS (%' AND l.locktype='advisory'
                 AND NOT l.granted AND l.classid::bigint=1229738578
                 AND l.objid::bigint=$2 AND l.objsubid=2)",
            )
            .bind(application)
            .bind(key)
            .fetch_one(db)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("collector must wait on its exact owned advisory barrier");
}

fn captured(outcome: CaptureOutcome) -> (NaiveDate, DateTime<Utc>, u64) {
    match outcome {
        CaptureOutcome::Captured {
            date,
            captured_at,
            rows,
        } => (date, captured_at, rows),
        other => panic!("owned fixture must record a new observation: {other:?}"),
    }
}

async fn force_due_in_fixture(db: &PgPool) {
    // Only this owned test schema relaxes the batch UTC-date check. Aging the
    // metadata by 1801 seconds remains deterministic even during UTC00:00–00:30;
    // every newly collected point still comes from the real database clock.
    sqlx::query(
        "ALTER TABLE rank_history_batches DROP CONSTRAINT IF EXISTS rank_history_batches_utc_date",
    )
    .execute(db)
    .await
    .unwrap();
    sqlx::query("UPDATE rank_history_batches SET captured_at=clock_timestamp()-interval '1801 seconds' WHERE date=(clock_timestamp() AT TIME ZONE 'UTC')::date")
        .execute(db).await.unwrap();
}

async fn seed_history(
    db: &PgPool,
    user: i64,
    date: NaiveDate,
    country: &str,
    pp: f64,
    rank: Option<i64>,
) -> DateTime<Utc> {
    let time = date.and_hms_opt(12, 0, 0).unwrap().and_utc();
    sqlx::query("INSERT INTO rank_history_batches(date,captured_at,row_count) VALUES($1,$2,1) ON CONFLICT(date) DO NOTHING")
        .bind(date).bind(time).execute(db).await.unwrap();
    sqlx::query("INSERT INTO rank_history_daily(user_id,date,mode,variant,captured_at,country,pp,rank,country_rank) VALUES($1,$2,0,'vanilla',$3,$4,$5,$6,$6)")
        .bind(user).bind(date).bind(time).bind(country).bind(pp).bind(rank).execute(db).await.unwrap();
    time
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn rank_history_capture_matches_public_rankings_in_all_seven_partitions() {
    let fixture = MutationFixture::new("rank_history_rankings", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let outcome = tokio::spawn(async move {
        let (alice, _, _, _) = actor(&state, "HistoryAlice", &[]).await;
        let (bob, _, _, _) = actor(&state, "HistoryBob", &[]).await;
        let (carol, _, _, _) = actor(&state, "HistoryCarol", &[]).await;
        let (hidden, _, _, _) = actor(&state, "HistoryRestricted", &[]).await;
        sqlx::query("UPDATE users SET country='JP' WHERE id=$1").bind(carol).execute(&state.db).await.unwrap();
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(hidden).execute(&state.db).await.unwrap();
        let first = map(&state.db, "History first", 0, "ranked").await;
        let second = map(&state.db, "History second", 0, "ranked").await;
        let unranked = map(&state.db, "History excluded map", 0, "unranked").await;
        let v = ScoreVariant::Vanilla;
        let r = ScoreVariant::Relax;
        score(&state.db, alice, first, Play::pp(0,v,100.0)).await;
        let duplicate = score(&state.db, alice, first, Play::pp(0,v,50.0)).await;
        sqlx::query("UPDATE scores SET score=9999999 WHERE id=$1").bind(duplicate).execute(&state.db).await.unwrap();
        score(&state.db, alice, second, Play::pp(0,v,50.0)).await;
        score(&state.db, bob, first, Play::pp(0,v,147.5)).await;
        score(&state.db, carol, first, Play::pp(0,v,100.0)).await;
        score(&state.db, hidden, first, Play::pp(0,v,999999.0)).await;
        for play in [
            Play { verification:"pending", ..Play::pp(0,v,9000.0) },
            Play { verification:"rejected", ..Play::pp(0,v,8000.0) },
            Play { passed:false, ..Play::pp(0,v,7000.0) },
            Play { eligible:false, ..Play::pp(0,v,6000.0) },
        ] { score(&state.db, alice, first, play).await; }
        score(&state.db, alice, unranked, Play::pp(0,v,5000.0)).await;
        score(&state.db, alice, first, Play::pp(0,r,600.0)).await;
        score(&state.db, alice, second, Play::pp(0,r,100.0)).await;
        for mode in 1..=3 {
            let map = map(&state.db, &format!("History mode {mode}"), mode, "ranked").await;
            score(&state.db, alice, map, Play::pp(mode,v,100.0*f64::from(mode+1))).await;
            if mode < 3 { score(&state.db, alice, map, Play::pp(mode,r,100.0*f64::from(mode+6))).await; }
        }
        let (date, time, rows) = captured(capture_current_day(&state.db).await.unwrap());
        assert_eq!(rows,21);
        assert_eq!(date,time.date_naive());
        let count: (i64,i64,i64) = sqlx::query_as("SELECT count(*),count(DISTINCT captured_at),count(DISTINCT (mode,variant)) FROM rank_history_daily")
            .fetch_one(&state.db).await.unwrap();
        assert_eq!(count,(21,1,7));
        let restricted_count: i64 = sqlx::query_scalar("SELECT count(*) FROM rank_history_daily WHERE user_id=$1").bind(hidden).fetch_one(&state.db).await.unwrap();
        assert_eq!(restricted_count,0);
        let alice_v = observed(&state.db,alice,date,0,v).await;
        assert_eq!(alice_v["pp"],147.5);
        assert_eq!((alice_v["rank"].as_i64(),alice_v["country_rank"].as_i64()),(Some(1),Some(1)));
        assert_eq!(observed(&state.db,bob,date,0,v).await["rank"],2,"equal PP uses original user ID ordering");
        assert_eq!(observed(&state.db,carol,date,0,v).await["country_rank"],1);
        assert_eq!(observed(&state.db,alice,date,0,r).await["pp"],695.0);
        for variant in [v,r] {
            for mode in 0..=3 {
                if mode==3 && variant==r { continue; }
                for user in [alice,bob,carol] {
                    let point = observed(&state.db,user,date,mode,variant).await;
                    let profile = get(&app,&format!("/api/v1/users/{user}?mode={mode}&variant={}",variant.as_str())).await;
                    let observed_pp = point["pp"].as_f64().unwrap();
                    let public_pp = profile["statistics"]["pp"].as_f64().unwrap();
                    assert!((observed_pp-public_pp).abs()<=1e-9,"observed PP {observed_pp} must match public PP {public_pp}");
                    assert_eq!(point["rank"],profile["statistics"]["rank"]);
                    assert_eq!(point["country_rank"],profile["statistics"]["country_rank"]);
                    let response = history(&app,user,mode,variant).await;
                    assert_eq!(response["points"][0],point);
                }
            }
        }
        let zero = observed(&state.db,bob,date,1,r).await;
        assert_eq!(zero["pp"],0.0);
        assert!(zero["rank"].is_null() && zero["country_rank"].is_null());
        assert_eq!(stored(&state.db).await["batches"][0]["row_count"],21);
    }).await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn rank_history_current_day_replacement_preserves_history_and_country() {
    let fixture = MutationFixture::new("rank_history_updates", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let outcome = tokio::spawn(async move {
        let (owner, _, _, _) = actor(&state,"HistoryMovementOwner",&[]).await;
        let (other, _, _, _) = actor(&state,"HistoryMovementOther",&[]).await;
        let (admin, _, _, admin_cookie) = actor(&state,"HistoryMovementAdmin",&["*"]).await;
        let map = map(&state.db,"History movement map",0,"ranked").await;
        let v = ScoreVariant::Vanilla;
        score(&state.db,owner,map,Play::pp(0,v,100.0)).await;
        let rival_score = score(&state.db,other,map,Play::pp(0,v,50.0)).await;
        let (date, _, rows) = captured(capture_current_day(&state.db).await.unwrap());
        assert_eq!(rows,21);
        let past = date.checked_sub_days(Days::new(3)).unwrap();
        seed_history(&state.db,owner,past,"TW",90.0,Some(5)).await;
        let old = observed(&state.db,owner,past,0,v).await;
        assert_eq!(observed(&state.db,owner,date,0,v).await["rank"],1);
        // Other players can move the owner's rank while the owner's PP is fixed.
        sqlx::query("UPDATE scores SET pp=200 WHERE id=$1").bind(rival_score).execute(&state.db).await.unwrap();
        sqlx::query("UPDATE users SET country='JP' WHERE id=$1").bind(owner).execute(&state.db).await.unwrap();
        force_due_in_fixture(&state.db).await;
        captured(capture_current_day(&state.db).await.unwrap());
        let moved = observed(&state.db,owner,date,0,v).await;
        assert_eq!(moved["pp"],100.0);
        assert_eq!(moved["rank"],2);
        assert_eq!(moved["country"],"JP");
        assert_eq!(observed(&state.db,owner,past,0,v).await,old,"past observations remain immutable");
        let response = history(&app,owner,0,v).await;
        assert_eq!(response["points"].as_array().unwrap().len(),2,"missed days are not filled");
        assert_eq!(response["points"][0]["country"],"TW");
        assert_eq!(response["points"][1]["country"],"JP");
        let native_set: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1").bind(map).fetch_one(&state.db).await.unwrap();
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'owned history fixture',$2)")
            .bind(native_set).bind(admin).execute(&state.db).await.unwrap();
        force_due_in_fixture(&state.db).await;
        captured(capture_current_day(&state.db).await.unwrap());
        let zero = observed(&state.db,owner,date,0,v).await;
        assert_eq!(zero["pp"],0.0);
        assert!(zero["rank"].is_null() && zero["country_rank"].is_null());
        assert_eq!(observed(&state.db,owner,past,0,v).await,old);
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(owner).execute(&state.db).await.unwrap();
        let path = format!("/api/v1/users/{owner}/rank-history");
        for cookie in [None,Some(admin_cookie.as_str())] {
            assert_eq!(request(&app,&path,cookie).await.0,StatusCode::NOT_FOUND,"existing history must not leak while currently restricted");
        }
        force_due_in_fixture(&state.db).await;
        let (_,_,rows) = captured(capture_current_day(&state.db).await.unwrap());
        assert_eq!(rows,14);
        let dates: Vec<NaiveDate> = sqlx::query_scalar("SELECT DISTINCT date FROM rank_history_daily WHERE user_id=$1 ORDER BY date").bind(owner).fetch_all(&state.db).await.unwrap();
        assert_eq!(dates,vec![past],"only the generated current-day observation is removed");
        assert_eq!(observed(&state.db,owner,past,0,v).await,old);
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(owner).execute(&state.db).await.unwrap();
        assert_eq!(history(&app,owner,0,v).await["points"],json!([old]));
    }).await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn rank_history_public_window_validation_and_missing_observations() {
    let fixture = MutationFixture::new("rank_history_window", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let outcome = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "HistoryWindowOwner", &[]).await;
        let (empty, _, _, _) = actor(&state, "HistoryWindowEmpty", &[]).await;
        let today: NaiveDate =
            sqlx::query_scalar("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date")
                .fetch_one(&state.db)
                .await
                .unwrap();
        let path = format!("/api/v1/users/{user}/rank-history");
        let blank = get(&app, &path).await;
        assert_eq!(blank["points"], json!([]));
        assert!(blank["last_captured_at"].is_null());
        assert_eq!(blank["mode"], 0);
        assert_eq!(blank["variant"], "vanilla");
        assert_eq!(blank["days"], 90);
        assert_eq!(blank["end_date"], today.to_string());
        assert_eq!(
            blank["start_date"],
            today.checked_sub_days(Days::new(89)).unwrap().to_string()
        );
        assert_eq!(blank["time_zone"], "UTC");
        assert_eq!(blank["collection_interval_seconds"], 1800);
        assert_eq!(blank.as_object().unwrap().len(), 9);
        for age in [90, 89, 3] {
            seed_history(
                &state.db,
                user,
                today.checked_sub_days(Days::new(age)).unwrap(),
                "TW",
                100.0,
                Some(2),
            )
            .await;
        }
        // An unrelated captured batch controls freshness, even for a user with
        // no observations. Future fixture rows never enter the requested window.
        let latest = seed_history(
            &state.db,
            empty,
            today.checked_sub_days(Days::new(2)).unwrap(),
            "JP",
            0.0,
            None,
        )
        .await;
        seed_history(
            &state.db,
            user,
            today.checked_add_days(Days::new(1)).unwrap(),
            "TW",
            200.0,
            Some(1),
        )
        .await;
        sqlx::query("UPDATE users SET country='JP' WHERE id=$1")
            .bind(user)
            .execute(&state.db)
            .await
            .unwrap();
        let response = get(&app, &path).await;
        assert_eq!(response["points"].as_array().unwrap().len(), 2);
        assert_eq!(
            response["points"][0]["date"],
            today.checked_sub_days(Days::new(89)).unwrap().to_string()
        );
        assert_eq!(
            response["points"][1]["date"],
            today.checked_sub_days(Days::new(3)).unwrap().to_string()
        );
        assert!(
            response["points"]
                .as_array()
                .unwrap()
                .iter()
                .all(|point| point["country"] == "TW" && point.as_object().unwrap().len() == 6)
        );
        let last: DateTime<Utc> = response["last_captured_at"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(last, latest);
        let one = get(&app, &format!("{path}?days=1")).await;
        assert_eq!(one["points"], json!([]));
        assert!(one["last_captured_at"].is_null());
        assert_eq!(
            get(&app, &format!("{path}?days=180")).await["points"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        for query in [
            "mode=-1",
            "mode=4",
            "variant=rx",
            "variant=autopilot",
            "mode=3&variant=relax",
            "days=0",
            "days=181",
            "days=-1",
            "days=100000",
            "days=nope",
        ] {
            assert_eq!(
                request(&app, &format!("{path}?{query}"), None).await.0,
                StatusCode::BAD_REQUEST,
                "{query}"
            );
        }
        assert_eq!(
            request(&app, "/api/v1/users/999999/rank-history", None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    })
    .await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn rank_history_replica_fence_cooldown_and_atomic_failure() {
    let fixture = MutationFixture::new("rank_history_atomic", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let application = fixture.schema.clone();
    let outcome = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state,"HistoryAtomicOwner",&[]).await;
        let mut lock = state.db.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock($1,$2)").bind(CAPTURE_LOCK.0).bind(CAPTURE_LOCK.1).execute(&mut *lock).await.unwrap();
        let before = stored(&state.db).await;
        assert_eq!(capture_current_day(&state.db).await.unwrap(),CaptureOutcome::SkippedBusy);
        assert_eq!(stored(&state.db).await,before);
        lock.commit().await.unwrap();
        let first = captured(capture_current_day(&state.db).await.unwrap());
        assert_eq!(first.2,7);
        let baseline = stored(&state.db).await;
        assert_eq!(capture_current_day(&state.db).await.unwrap(),CaptureOutcome::SkippedCooldown);
        assert_eq!(stored(&state.db).await,baseline);
        force_due_in_fixture(&state.db).await;
        // Hold a different fixture barrier after the collector obtains its own
        // lock. A competing replica must skip while collection is in progress.
        sqlx::query("CREATE FUNCTION fixture_history_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.mode=0 AND NEW.variant='vanilla' THEN PERFORM pg_advisory_xact_lock(1229738578,91); END IF; RETURN NEW; END $$")
            .execute(&state.db).await.unwrap();
        sqlx::query("CREATE TRIGGER fixture_history_barrier BEFORE INSERT ON rank_history_daily FOR EACH ROW EXECUTE FUNCTION fixture_history_barrier()")
            .execute(&state.db).await.unwrap();
        let mut barrier = state.db.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(1229738578,91)").execute(&mut *barrier).await.unwrap();
        let db = state.db.clone();
        let queued = tokio::spawn(async move { capture_current_day(&db).await });
        wait_for_capture_barrier(&state.db,&application,91).await;
        assert_eq!(capture_current_day(&state.db).await.unwrap(),CaptureOutcome::SkippedBusy);
        barrier.commit().await.unwrap();
        captured(tokio::time::timeout(Duration::from_secs(5),queued).await.unwrap().unwrap().unwrap());
        sqlx::query("DROP TRIGGER fixture_history_barrier ON rank_history_daily").execute(&state.db).await.unwrap();
        force_due_in_fixture(&state.db).await;
        let before_failure = stored(&state.db).await;
        sqlx::query("CREATE FUNCTION fixture_history_reject() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.mode=1 AND NEW.variant='relax' THEN RAISE EXCEPTION 'owned fixture rejects a later partition'; END IF; RETURN NEW; END $$")
            .execute(&state.db).await.unwrap();
        sqlx::query("CREATE TRIGGER fixture_history_reject BEFORE INSERT ON rank_history_daily FOR EACH ROW EXECUTE FUNCTION fixture_history_reject()")
            .execute(&state.db).await.unwrap();
        assert!(capture_current_day(&state.db).await.is_err());
        assert_eq!(stored(&state.db).await,before_failure,"later partition failure must roll back earlier rows, deletion and metadata");
        sqlx::query("DROP TRIGGER fixture_history_reject ON rank_history_daily").execute(&state.db).await.unwrap();
        sqlx::query("CREATE FUNCTION fixture_history_reject_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned fixture rejects batch commit'; END $$")
            .execute(&state.db).await.unwrap();
        sqlx::query("CREATE CONSTRAINT TRIGGER fixture_history_reject_commit AFTER UPDATE ON rank_history_batches DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fixture_history_reject_commit()")
            .execute(&state.db).await.unwrap();
        assert!(capture_current_day(&state.db).await.is_err());
        assert_eq!(stored(&state.db).await,before_failure,"a COMMIT rejection must roll back all partitions and metadata");
        let bad_rank = sqlx::query("UPDATE rank_history_daily SET rank=1 WHERE user_id=$1 AND pp=0").bind(user).execute(&state.db).await.unwrap_err();
        assert_eq!(bad_rank.as_database_error().unwrap().code().as_deref(),Some("23514"));
        let bad_partition = sqlx::query("UPDATE rank_history_daily SET mode=3 WHERE user_id=$1 AND variant='relax'").bind(user).execute(&state.db).await.unwrap_err();
        assert_eq!(bad_partition.as_database_error().unwrap().code().as_deref(),Some("23514"));
    }).await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn rank_history_repeatable_snapshot_keeps_partitions_consistent() {
    let fixture = MutationFixture::new("rank_history_snapshot", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let application = fixture.schema.clone();
    let outcome = tokio::spawn(async move {
        let (owner, _, _, _) = actor(&state,"HistorySnapshotOwner",&[]).await;
        let (peer, _, _, _) = actor(&state,"HistorySnapshotPeer",&[]).await;
        let map = map(&state.db,"History snapshot map",0,"ranked").await;
        let vanilla = score(&state.db,owner,map,Play::pp(0,ScoreVariant::Vanilla,100.0)).await;
        let relax = score(&state.db,owner,map,Play::pp(0,ScoreVariant::Relax,200.0)).await;
        sqlx::query("CREATE FUNCTION fixture_snapshot_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.mode=0 AND NEW.variant='vanilla' THEN PERFORM pg_advisory_xact_lock(1229738578,92); END IF; RETURN NEW; END $$")
            .execute(&state.db).await.unwrap();
        sqlx::query("CREATE TRIGGER fixture_snapshot_barrier BEFORE INSERT ON rank_history_daily FOR EACH ROW EXECUTE FUNCTION fixture_snapshot_barrier()")
            .execute(&state.db).await.unwrap();
        let mut barrier = state.db.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(1229738578,92)").execute(&mut *barrier).await.unwrap();
        let db = state.db.clone();
        let queued = tokio::spawn(async move { capture_current_day(&db).await });
        wait_for_capture_barrier(&state.db,&application,92).await;
        sqlx::query("UPDATE scores SET pp=pp+1000 WHERE id=ANY($1)").bind(vec![vanilla,relax]).execute(&state.db).await.unwrap();
        let native_set: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1").bind(map).fetch_one(&state.db).await.unwrap();
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'owned snapshot fixture',$2)")
            .bind(native_set).bind(owner).execute(&state.db).await.unwrap();
        barrier.commit().await.unwrap();
        let (date,_,rows) = captured(tokio::time::timeout(Duration::from_secs(5),queued).await.unwrap().unwrap().unwrap());
        assert_eq!(rows,14,"the entire batch uses the original unrestricted population");
        assert_eq!(observed(&state.db,owner,date,0,ScoreVariant::Vanilla).await["pp"],100.0);
        assert_eq!(observed(&state.db,owner,date,0,ScoreVariant::Relax).await["pp"],200.0);
        let countries: Vec<String> = sqlx::query_scalar("SELECT DISTINCT country FROM rank_history_daily WHERE user_id=$1").bind(owner).fetch_all(&state.db).await.unwrap();
        assert_eq!(countries,vec!["TW".to_string()]);
        let peer_count: i64 = sqlx::query_scalar("SELECT count(*) FROM rank_history_daily WHERE user_id=$1").bind(peer).fetch_one(&state.db).await.unwrap();
        assert_eq!(peer_count,7);
    }).await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn rank_history_migration_21_to_22_preserves_populated_sources() {
    let fixture = MutationFixture::new("rank_history_upgrade", "http://127.0.0.1:1".into()).await;
    let db = fixture.db.clone();
    let schema = format!("{}_upgrade", fixture.schema);
    // SQLx raw migration execution borrows this transaction through a local
    // future. Catch panic locally so cleanup retains the original fixture
    // guarantee without requiring that borrowed migration future to be Send.
    let outcome = AssertUnwindSafe(async move {
        // The temporary upgrade schema is created inside this transaction and
        // removed by rollback; the outer MutationFixture owns all resources.
        let mut tx = db.begin().await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&mut *tx).await.unwrap();
        sqlx::query(&format!("SET LOCAL search_path TO {schema}")).execute(&mut *tx).await.unwrap();
        let migrator = sqlx::migrate!("../../migrations");
        for migration in migrator.iter().filter(|migration| migration.version<=21) {
            sqlx::raw_sql(&migration.sql).execute(&mut *tx).await.unwrap();
        }
        sqlx::raw_sql("INSERT INTO users(id,username,email,password_hash,password_md5_hash,country,preferences) VALUES(31,'HistoryUpgradeOwner','history-upgrade@fixture.invalid','retained synthetic hash','retained synthetic hash','TW','{\"locale\":\"ja\"}');
            INSERT INTO user_roles(user_id,role_id) SELECT 31,id FROM roles WHERE name='player';
            INSERT INTO beatmaps(id,uploaded_by,title,artist,creator,version,mode,status,stars,md5,osu_path,archive_path) VALUES(51,31,'Retained map','Fixture','Illyaverse','Test',0,'ranked',1,'11111111111111111111111111111111','retained.osu','retained.osz');
            INSERT INTO scores(id,user_id,beatmap_id,mode,score,accuracy,max_combo,pp,verification,pp_eligible,replay_path) VALUES(71,31,51,0,12345,95,100,250,'verified',true,'retained.osr');
            INSERT INTO announcements(id,title,body,status,revision,created_by,updated_by,published_at) VALUES(91,'Retained announcement','Retained public text','published',4,31,31,clock_timestamp());")
            .execute(&mut *tx).await.unwrap();
        let fingerprints = "SELECT jsonb_build_object('users',(SELECT md5(jsonb_agg(to_jsonb(u) ORDER BY id)::text) FROM users u),'maps',(SELECT md5(jsonb_agg(to_jsonb(b) ORDER BY id)::text) FROM beatmaps b),'scores',(SELECT md5(jsonb_agg(to_jsonb(s) ORDER BY id)::text) FROM scores s),'announcements',(SELECT md5(jsonb_agg(to_jsonb(a) ORDER BY id)::text) FROM announcements a),'roles',(SELECT md5(jsonb_agg(to_jsonb(r) ORDER BY id)::text) FROM roles r))";
        let before: Value = sqlx::query_scalar(fingerprints).fetch_one(&mut *tx).await.unwrap();
        let migration = migrator.iter().find(|migration| migration.version==22).unwrap();
        sqlx::raw_sql(&migration.sql).execute(&mut *tx).await.unwrap();
        let after: Value = sqlx::query_scalar(fingerprints).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(after,before,"the additive migration preserves existing records");
        let counts: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM rank_history_daily),(SELECT count(*) FROM rank_history_batches)").fetch_one(&mut *tx).await.unwrap();
        assert_eq!(counts,(0,0),"migration does not invent earlier observations");
        tx.rollback().await.unwrap();
    })
    .catch_unwind()
    .await;
    fixture.finish(Ok(())).await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
