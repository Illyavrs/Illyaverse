//! Public profile best plays use the same PP qualification and weights as statistics.
use crate::maps::map_mutation_tests::{MutationFixture, actor, wait_for_lock};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn request(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder().method(Method::GET).uri(path);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
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

async fn map(db: &PgPool, title: &str, status: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path,archive_path)
         VALUES($1,'Best fixture','Illyaverse','Test difficulty',0,$2,1,$3,'private-map.osu','private-archive.osz') RETURNING id",
    )
    .bind(title)
    .bind(status)
    .bind(format!("{:x}", md5::compute(title)))
    .fetch_one(db)
    .await
    .unwrap()
}

struct Play {
    mode: i16,
    relax: bool,
    pp: f64,
    accuracy: f64,
    combo: i32,
    raw: i64,
    scaled: Option<i64>,
    passed: bool,
    verification: &'static str,
    eligible: bool,
    client: &'static str,
    replay: Option<&'static str>,
}

impl Play {
    fn pp(pp: f64) -> Self {
        Self {
            mode: 0,
            relax: false,
            pp,
            accuracy: 95.0,
            combo: 100,
            raw: 12345,
            scaled: Some(500000),
            passed: true,
            verification: "verified",
            eligible: true,
            client: "stable",
            replay: None,
        }
    }
}

async fn score(db: &PgPool, user: i64, map: i64, play: Play) -> i64 {
    let scale_status = match (play.scaled, play.client) {
        (None, _) => "unavailable",
        (Some(_), "lazer") => "native_reported",
        _ => "legacy_estimated",
    };
    sqlx::query_scalar(
        "INSERT INTO scores(user_id,beatmap_id,mode,score_variant,mods,pp,accuracy,max_combo,score,
            standardised_score,classic_score,score_scale_status,passed,verification,pp_eligible,
            client,replay_path,ingestion_key,hit_counts)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10,$11,$12,$13,$14,$15,$16,$17,'{\"n300\":100}') RETURNING id",
    )
    .bind(user)
    .bind(map)
    .bind(play.mode)
    .bind(if play.relax { "relax" } else { "vanilla" })
    .bind(if play.relax { 128_i64 } else { 0 })
    .bind(play.pp)
    .bind(play.accuracy)
    .bind(play.combo)
    .bind(play.raw)
    .bind(play.scaled)
    .bind(scale_status)
    .bind(play.passed)
    .bind(play.verification)
    .bind(play.eligible)
    .bind(play.client)
    .bind(play.replay)
    .bind(format!("private-best-key-{}", uuid::Uuid::new_v4()))
    .fetch_one(db)
    .await
    .unwrap()
}

fn rows(value: &Value) -> &[Value] {
    value["items"].as_array().unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn public_best_selects_pp_winners_and_weights_before_paging() {
    let fixture = MutationFixture::new("profile_best", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, owner_cookie) = actor(&state, "BestOwner", &[]).await;
        let (moderator, _, _, moderator_cookie) =
            actor(&state, "BestModerator", &["scores.moderate"]).await;
        let mut maps = Vec::new();
        for title in ["PP first", "Tie breakers", "Equal PP", "Fourth", "Zero PP"] {
            maps.push(map(&state.db, title, "ranked").await);
        }
        let first = score(
            &state.db,
            user,
            maps[0],
            Play {
                pp: 500.0,
                raw: 123,
                scaled: Some(100000),
                client: "lazer",
                replay: Some("private-replay.osr"),
                ..Play::pp(500.0)
            },
        )
        .await;
        let score_winner = score(
            &state.db,
            user,
            maps[0],
            Play {
                raw: 9999999,
                scaled: Some(900000),
                ..Play::pp(100.0)
            },
        )
        .await;
        score(
            &state.db,
            user,
            maps[1],
            Play {
                accuracy: 90.0,
                combo: 1000,
                ..Play::pp(400.0)
            },
        )
        .await;
        score(
            &state.db,
            user,
            maps[1],
            Play {
                accuracy: 98.0,
                combo: 10,
                ..Play::pp(400.0)
            },
        )
        .await;
        let tie_winner = score(
            &state.db,
            user,
            maps[1],
            Play {
                accuracy: 98.0,
                combo: 20,
                ..Play::pp(400.0)
            },
        )
        .await;
        score(
            &state.db,
            user,
            maps[1],
            Play {
                accuracy: 98.0,
                combo: 20,
                ..Play::pp(400.0)
            },
        )
        .await;
        let equal_pp = score(&state.db, user, maps[2], Play::pp(500.0)).await;
        let fourth = score(&state.db, user, maps[3], Play::pp(300.0)).await;
        let zero = score(&state.db, user, maps[4], Play::pp(0.0)).await;
        let pending = score(
            &state.db,
            user,
            maps[0],
            Play {
                verification: "pending",
                ..Play::pp(9000.0)
            },
        )
        .await;
        let rejected = score(
            &state.db,
            user,
            maps[0],
            Play {
                verification: "rejected",
                ..Play::pp(8000.0)
            },
        )
        .await;
        score(
            &state.db,
            user,
            maps[0],
            Play {
                passed: false,
                ..Play::pp(7000.0)
            },
        )
        .await;
        score(
            &state.db,
            user,
            maps[0],
            Play {
                eligible: false,
                ..Play::pp(6000.0)
            },
        )
        .await;
        for (title, status) in [
            ("Pending map", "pending"),
            ("Loved map", "loved"),
            ("Unranked map", "unranked"),
        ] {
            let map = map(&state.db, title, status).await;
            score(&state.db, user, map, Play::pp(10000.0)).await;
        }
        score(&state.db, moderator, maps[0], Play::pp(100000.0)).await;

        let path = format!("/api/v1/users/{user}/scores/best");
        let (status, all) = request(&app, &path, None).await;
        assert_eq!(status, StatusCode::OK, "{all}");
        assert_eq!(all["collection"], "best");
        assert_eq!(all["ordered_by"], "pp");
        assert_eq!(all["mode"], 0);
        assert_eq!(all["score_variant"], "vanilla");
        assert_eq!(all["page"], 1);
        assert_eq!(all["page_size"], 24);
        assert_eq!(all["total"], 5);
        assert_eq!(
            rows(&all)
                .iter()
                .map(|row| row["id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [first, equal_pp, tie_winner, fourth, zero]
        );
        let mut weighted_total = 0.0;
        for (index, row) in rows(&all).iter().enumerate() {
            assert_eq!(row["position"], index + 1);
            let weight = 0.95_f64.powi(index as i32);
            near(row["pp_weight"].as_f64().unwrap(), weight);
            near(
                row["weighted_pp"].as_f64().unwrap(),
                row["pp"].as_f64().unwrap() * weight,
            );
            weighted_total += row["weighted_pp"].as_f64().unwrap();
            assert_eq!(row["username"], "BestOwner");
            assert_eq!(row["version"], "Test difficulty");
            assert_eq!(row["verification"], "verified");
            assert!(row["created_at"].is_string());
            for private in ["replay_path", "ingestion_key", "osu_path", "archive_path"] {
                assert!(row.get(private).is_none(), "unexpected {private}: {row}");
            }
        }
        assert_eq!(all["items"][0]["title"], "PP first");
        assert_eq!(all["items"][0]["score"], 123);
        assert_eq!(all["items"][0]["standardised_score"], 100000);
        assert_eq!(all["items"][0]["has_replay"], true);
        assert_eq!(all["items"][0]["accuracy_source"], "client_reported");
        assert_eq!(all["items"][1]["has_replay"], false);
        assert_eq!(all["items"][1]["accuracy_source"], "judgements");
        let (status, profile) = request(&app, &format!("/api/v1/users/{user}"), None).await;
        assert_eq!(status, StatusCode::OK, "{profile}");
        near(
            profile["statistics"]["pp"].as_f64().unwrap(),
            weighted_total,
        );
        let (_, board) = request(&app, &format!("/api/v1/beatmaps/{}/scores", maps[0]), None).await;
        assert_eq!(board["items"][0]["id"], score_winner);
        assert_eq!(board["ordered_by"], "standardised_score");

        for cookie in [
            None,
            Some(owner_cookie.as_str()),
            Some(moderator_cookie.as_str()),
        ] {
            for page in 1..=3 {
                let (status, paged) =
                    request(&app, &format!("{path}?page_size=2&page={page}"), cookie).await;
                assert_eq!(status, StatusCode::OK, "{paged}");
                assert_eq!(paged["total"], 5);
                let start = (page - 1) * 2;
                assert_eq!(rows(&paged), &rows(&all)[start..(start + 2).min(5)]);
            }
        }
        // The separate recent endpoint retains the owner's and moderator's review visibility.
        for cookie in [&owner_cookie, &moderator_cookie] {
            let (status, recent) = request(
                &app,
                &format!("/api/v1/scores?user_id={user}"),
                Some(cookie),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{recent}");
            for id in [pending, rejected] {
                assert!(rows(&recent).iter().any(|row| row["id"] == id));
            }
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn public_best_partitions_visibility_and_pagination_are_bounded() {
    let fixture = MutationFixture::new("best_bounds", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, owner_cookie) = actor(&state, "PartitionOwner", &[]).await;
        let (_, _, _, moderator_cookie) =
            actor(&state, "PartitionModerator", &["scores.moderate"]).await;
        let (empty, _, _, _) = actor(&state, "BestEmpty", &[]).await;
        let map = map(&state.db, "Partition map", "ranked").await;
        let mut expected = Vec::new();
        for mode in 0..=3 {
            expected.push((
                mode,
                "vanilla",
                score(
                    &state.db,
                    user,
                    map,
                    Play {
                        mode,
                        ..Play::pp(100.0 + f64::from(mode))
                    },
                )
                .await,
            ));
            if mode != 3 {
                expected.push((
                    mode,
                    "relax",
                    score(
                        &state.db,
                        user,
                        map,
                        Play {
                            mode,
                            relax: true,
                            raw: 0,
                            scaled: None,
                            ..Play::pp(500.0 + f64::from(mode))
                        },
                    )
                    .await,
                ));
            }
        }
        let path = format!("/api/v1/users/{user}/scores/best");
        for (mode, variant, id) in expected {
            let (status, value) =
                request(&app, &format!("{path}?mode={mode}&variant={variant}"), None).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value["mode"], mode);
            assert_eq!(value["score_variant"], variant);
            assert_eq!(value["total"], 1);
            assert_eq!(value["items"][0]["id"], id);
            near(value["items"][0]["pp_weight"].as_f64().unwrap(), 1.0);
            let (_, profile) = request(
                &app,
                &format!("/api/v1/users/{user}?mode={mode}&variant={variant}"),
                None,
            )
            .await;
            near(
                value["items"][0]["weighted_pp"].as_f64().unwrap(),
                profile["statistics"]["pp"].as_f64().unwrap(),
            );
            if variant == "relax" {
                assert_eq!(value["items"][0]["score"], 0);
                assert!(value["items"][0]["standardised_score"].is_null());
            }
        }
        for suffix in [
            "mode=-1",
            "mode=4",
            "mode=32767",
            "mode=3&variant=relax",
            "variant=autopilot",
            "mode=oops",
            "page=-1",
            "page=4294967296",
        ] {
            assert_eq!(
                request(&app, &format!("{path}?{suffix}"), None).await.0,
                StatusCode::BAD_REQUEST,
                "{suffix}"
            );
        }
        let (_, clamped) = request(&app, &format!("{path}?page=0&page_size=0"), None).await;
        assert_eq!(clamped["page"], 1);
        assert_eq!(clamped["page_size"], 1);
        assert_eq!(rows(&clamped).len(), 1);
        let (_, beyond) = request(
            &app,
            &format!("{path}?page=4294967295&page_size=4294967295"),
            None,
        )
        .await;
        assert_eq!(beyond["page"], 1000000);
        assert_eq!(beyond["page_size"], 100);
        assert_eq!(beyond["total"], 1);
        assert!(rows(&beyond).is_empty());
        let (status, value) =
            request(&app, &format!("/api/v1/users/{empty}/scores/best"), None).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["total"], 0);
        assert!(rows(&value).is_empty());
        assert_eq!(
            request(&app, "/api/v1/users/9223372036854775807/scores/best", None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(user)
            .execute(&state.db)
            .await
            .unwrap();
        for cookie in [
            None,
            Some(owner_cookie.as_str()),
            Some(moderator_cookie.as_str()),
        ] {
            assert_eq!(request(&app, &path, cookie).await.0, StatusCode::NOT_FOUND);
        }
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn public_best_visibility_count_and_items_share_a_snapshot() {
    let fixture = MutationFixture::new("best_snapshot", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "SnapshotPlayer", &[]).await;
        let first_map = map(&state.db, "Snapshot first", "ranked").await;
        let second_map = map(&state.db, "Snapshot second", "ranked").await;
        let first = score(&state.db, user, first_map, Play::pp(500.0)).await;
        let second = score(&state.db, user, second_map, Play::pp(400.0)).await;
        // A fixture-only view pauses score reads after the statement snapshot
        // exists. Production tables and authorization require no test hooks.
        sqlx::query("ALTER TABLE scores RENAME TO fixture_best_scores").execute(&state.db).await.unwrap();
        sqlx::query("CREATE FUNCTION fixture_best_gate() RETURNS boolean LANGUAGE plpgsql VOLATILE AS $$ BEGIN PERFORM pg_advisory_xact_lock(841927); RETURN true; END $$").execute(&state.db).await.unwrap();
        sqlx::query("CREATE VIEW scores AS SELECT * FROM fixture_best_scores WHERE fixture_best_gate()").execute(&state.db).await.unwrap();
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("SELECT pg_advisory_xact_lock(841927)").execute(&mut *blocker).await.unwrap();
        let path = format!("/api/v1/users/{user}/scores/best");
        let queued_app = app.clone();
        let queued_path = path.clone();
        let pending = tokio::spawn(async move { request(&queued_app, &queued_path, None).await });
        wait_for_lock(&state.db, &application, "WITH public_player").await;
        let mut change = state.db.begin().await.unwrap();
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(user).execute(&mut *change).await.unwrap();
        sqlx::query("UPDATE fixture_best_scores SET verification='rejected' WHERE id=$1").bind(second).execute(&mut *change).await.unwrap();
        change.commit().await.unwrap();
        blocker.commit().await.unwrap();
        let (status, before) = pending.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{before}");
        assert_eq!(before["total"], 2);
        assert_eq!(rows(&before).iter().map(|row| row["id"].as_i64().unwrap()).collect::<Vec<_>>(), [first, second]);
        assert_eq!(before["items"][1]["verification"], "verified");
        near(before["items"][1]["weighted_pp"].as_f64().unwrap(), 380.0);
        assert_eq!(request(&app, &path, None).await.0, StatusCode::NOT_FOUND);
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(user).execute(&state.db).await.unwrap();
        let (status, after) = request(&app, &path, None).await;
        assert_eq!(status, StatusCode::OK, "{after}");
        assert_eq!(after["total"], 1);
        assert_eq!(rows(&after).len(), 1);
        assert_eq!(after["items"][0]["id"], first);
    }).await;
    fixture.finish(result).await;
}
