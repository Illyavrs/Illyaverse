//! Original upgrade fixture: public identity changes must preserve content and
//! every internal score, replay, token, and frozen-round binding.
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgPoolOptions};

const FIRST: &str = "11111111111111111111111111111111";
const SECOND: &str = "22222222222222222222222222222222";

async fn upgrade_fixture(pool: sqlx::PgPool) {
    let migrator = sqlx::migrate!("../../migrations");
    for migration in migrator.iter().filter(|migration| migration.version < 16) {
        sqlx::raw_sql(&migration.sql).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO users(id,username,email,password_hash,password_md5_hash) VALUES
            (31,'PackageOwner','owner@fixture.invalid','unused','unused'),
            (32,'OtherOwner','other@fixture.invalid','unused','unused');
        INSERT INTO beatmaps(id,uploaded_by,title,artist,creator,version,mode,status,stars,md5,osu_path,archive_path) VALUES
            (51,31,'Stored set','Fixture','Fixture','First',0,'ranked',1,'11111111111111111111111111111111','11111111-aaaa-bbbb-cccc-111111111111/11111111111111111111111111111111.osu','11111111-aaaa-bbbb-cccc-111111111111/package.osz'),
            (52,31,'Different metadata','Other artist','Other creator','Second',0,'loved',2,'22222222222222222222222222222222','11111111-aaaa-bbbb-cccc-111111111111/22222222222222222222222222222222.osu','11111111-aaaa-bbbb-cccc-111111111111/package.osz'),
            (53,31,'Mixed owner','Fixture','Fixture','First',0,'pending',1,'33333333333333333333333333333333','22222222-aaaa-bbbb-cccc-222222222222/33333333333333333333333333333333.osu','22222222-aaaa-bbbb-cccc-222222222222/package.osz'),
            (54,32,'Mixed owner','Fixture','Fixture','Second',0,'pending',1,'44444444444444444444444444444444','22222222-aaaa-bbbb-cccc-222222222222/44444444444444444444444444444444.osu','22222222-aaaa-bbbb-cccc-222222222222/package.osz'),
            (55,31,'Untrusted path','Fixture','Fixture','First',0,'pending',1,'55555555555555555555555555555555','first.osu','shared.osz'),
            (56,31,'Untrusted path','Fixture','Fixture','Second',0,'pending',1,'66666666666666666666666666666666','second.osu','shared.osz'),
            (57,31,'Standalone','Fixture','Fixture','Only',0,'unranked',1,'77777777777777777777777777777777','standalone.osu',NULL);
        INSERT INTO beatmaps(id,official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path)
            VALUES(501,51,75,'Official collision','Fixture','Fixture','Official',0,'ranked','official',1,'88888888888888888888888888888888','official.osu');
        INSERT INTO scores(id,user_id,beatmap_id,mode,score,accuracy,max_combo,pp,replay_path)
            VALUES(9,31,51,0,1234,100,2,1,'retained-replay.osr');
        INSERT INTO lazer_score_tokens(id,user_id,beatmap_id,ruleset_id,version_hash,expires_at)
            VALUES(19,31,51,0,'retained-client-version',now()+interval '1 hour');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let snapshot = json!({"id":1,"beatmapID":51,"beatmapChecksum":FIRST,"rulesetID":0,
        "freestyle":false,"requiredMods":[],"allowedMods":[],"originalLabel":"retained snapshot"});
    sqlx::query("INSERT INTO lazer_rooms(id,host_id,settings,playlist) VALUES(7,31,'{}',$1)")
        .bind(json!([snapshot.clone()]))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO lazer_room_playlist_versions(id,room_id,playlist_item_id,snapshot) VALUES(11,7,1,$1)")
        .bind(&snapshot).execute(&pool).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO lazer_room_rounds(id,room_id,playlist_item_id,playlist_version_id,status) VALUES(13,7,1,11,'finished');
        INSERT INTO lazer_room_round_participants(round_id,user_id,beatmap_id,beatmap_checksum,ruleset_id,mods)
            VALUES(13,31,51,'11111111111111111111111111111111',0,'[]');
        INSERT INTO lazer_score_tokens(id,user_id,beatmap_id,ruleset_id,version_hash,expires_at,scope,room_id,playlist_item_id,round_id)
            VALUES(20,31,51,0,'retained-room-version',now()+interval '1 hour','room',7,1,13);
        INSERT INTO lazer_room_members(room_id,user_id,beatmap_id) VALUES(7,31,51),(7,32,52);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let initial_capture =
        json!({"beatmapID":51,"rulesetID":0,"scoreID":19,"mods":[],"recorder":"retained"});
    sqlx::query("INSERT INTO lazer_replay_captures(token_id,user_id,connection_key,initial_state,frame_bytes,frame_count,last_time,last_bundle_hash) VALUES(19,31,'owned-fixture',$1,13,1,20,'retained-chunk-digest')")
        .bind(&initial_capture).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO lazer_replay_chunks(token_id,sequence,frame_text) VALUES(19,0,$1)")
        .bind(b"retained chunk".as_slice())
        .execute(&pool)
        .await
        .unwrap();
    let upgrade = migrator
        .iter()
        .find(|migration| migration.version == 16)
        .unwrap();
    let receipt = json!({"beatmap_id":51,"original":"received intent"});
    let receipt_digest = "a".repeat(64);
    sqlx::query("UPDATE lazer_score_tokens SET processing_status='received',submission=$1,submission_sha256=$2,received_at=now() WHERE id=20")
        .bind(&receipt).bind(&receipt_digest).execute(&pool).await.unwrap();
    let refused = sqlx::raw_sql(&upgrade.sql)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55000")
    );
    assert_eq!(
        sqlx::query_scalar::<_, Value>("SELECT submission FROM lazer_score_tokens WHERE id=20")
            .fetch_one(&pool)
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT submission_sha256 FROM lazer_score_tokens WHERE id=20"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        receipt_digest
    );
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='beatmaps' AND column_name='native_id'").fetch_one(&pool).await.unwrap(), 0);
    sqlx::query("UPDATE lazer_score_tokens SET processing_status='issued',submission=NULL,submission_sha256=NULL,received_at=NULL WHERE id=20").execute(&pool).await.unwrap();
    sqlx::raw_sql(&upgrade.sql).execute(&pool).await.unwrap();

    let maps = sqlx::query("SELECT * FROM beatmaps ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(maps.len(), 8);
    assert_eq!(maps[0].get::<i64, _>("id"), 51);
    assert_eq!(maps[0].get::<String, _>("md5"), FIRST);
    assert_eq!(maps[1].get::<String, _>("md5"), SECOND);
    assert_eq!(maps[0].get::<String, _>("status"), "ranked");
    assert_eq!(maps[1].get::<String, _>("status"), "loved");
    assert_eq!(maps[0].get::<Option<i64>, _>("uploaded_by"), Some(31));
    assert_eq!(
        maps[0].get::<String, _>("osu_path"),
        format!("11111111-aaaa-bbbb-cccc-111111111111/{FIRST}.osu")
    );
    let first_native: i64 = maps[0].get("native_id");
    let second_native: i64 = maps[1].get("native_id");
    let first_set: i64 = maps[0].get("native_set_id");
    for map in &maps[..7] {
        assert!((2_000_000_000..=i64::from(i32::MAX)).contains(&map.get::<i64, _>("native_id")));
        assert!(
            (2_000_000_000..=i64::from(i32::MAX)).contains(&map.get::<i64, _>("native_set_id"))
        );
    }
    assert_ne!(first_native, second_native);
    assert_eq!(first_set, maps[1].get::<i64, _>("native_set_id"));
    for (left, right) in [(2, 3), (4, 5)] {
        assert_ne!(
            maps[left].get::<i64, _>("native_set_id"),
            maps[right].get::<i64, _>("native_set_id")
        );
    }
    assert_eq!(maps[7].get::<i64, _>("native_id"), 51);
    assert_eq!(maps[7].get::<i64, _>("native_set_id"), 75);
    let package = sqlx::query("SELECT * FROM beatmap_packages WHERE native_set_id=$1")
        .bind(first_set)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        package.get::<Vec<String>, _>("member_md5"),
        vec![FIRST, SECOND]
    );
    assert!(package.get::<bool, _>("legacy"));
    assert!(package.get::<Option<String>, _>("sha256").is_none());
    assert!(package.get::<Option<i64>, _>("byte_length").is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM beatmap_packages")
            .fetch_one(&pool)
            .await
            .unwrap(),
        6
    );
    let score = sqlx::query("SELECT * FROM scores WHERE id=9")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(score.get::<i64, _>("beatmap_id"), 51);
    assert_eq!(
        score.get::<Option<String>, _>("replay_path").as_deref(),
        Some("retained-replay.osr")
    );
    assert_eq!(
        sqlx::query_scalar::<_, Vec<i64>>(
            "SELECT array_agg(beatmap_id ORDER BY id) FROM lazer_score_tokens"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        vec![51, 51]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT beatmap_id FROM lazer_room_round_participants")
            .fetch_one(&pool)
            .await
            .unwrap(),
        51
    );
    let mut rebound_capture = initial_capture;
    rebound_capture["beatmapID"] = json!(first_native);
    let capture = sqlx::query("SELECT * FROM lazer_replay_captures WHERE token_id=19")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(capture.get::<Value, _>("initial_state"), rebound_capture);
    assert_eq!(
        capture
            .get::<Option<String>, _>("last_bundle_hash")
            .as_deref(),
        Some("retained-chunk-digest")
    );
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT frame_text FROM lazer_replay_chunks WHERE token_id=19"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        b"retained chunk"
    );
    let mut rebound = snapshot.clone();
    rebound["beatmapID"] = json!(first_native);
    assert_eq!(
        sqlx::query_scalar::<_, Value>("SELECT playlist FROM lazer_rooms WHERE id=7")
            .fetch_one(&pool)
            .await
            .unwrap(),
        json!([rebound.clone()])
    );
    assert_eq!(
        sqlx::query_scalar::<_, Value>(
            "SELECT snapshot FROM lazer_room_playlist_versions WHERE id=11"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        rebound
    );
    // A member ID without a checksum retains the old official-first meaning.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT beatmap_id FROM lazer_room_members WHERE user_id=31")
            .fetch_one(&pool)
            .await
            .unwrap(),
        51
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT beatmap_id FROM lazer_room_members WHERE user_id=32")
            .fetch_one(&pool)
            .await
            .unwrap(),
        second_native
    );

    for statement in [
        "UPDATE beatmaps SET md5='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' WHERE id=51",
        "UPDATE beatmaps SET official_id=55 WHERE id=51",
        "UPDATE beatmaps SET native_set_id=1 WHERE id=51",
        "UPDATE beatmaps SET archive_path='replacement.osz' WHERE id=51",
        "UPDATE beatmap_packages SET member_md5=ARRAY['aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'] WHERE native_set_id=(SELECT native_set_id FROM beatmaps WHERE id=51)",
        "INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,stars,md5,osu_path) VALUES(2000000000,1,'Forbidden','Fixture','Fixture','Fixture',0,1,'99999999999999999999999999999999','unused.osu')",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }
    sqlx::query("UPDATE beatmaps SET status='unranked' WHERE id=51")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE beatmaps SET archive_path='first-official-archive.osz' WHERE id=501")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE beatmaps SET archive_path='replacement.osz' WHERE id=501")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "requires owned PostgreSQL via ILLYA_TEST_DATABASE_URL"]
async fn package_upgrade_preserves_immutable_content_and_score_bindings() {
    let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("owned PostgreSQL fixture URL");
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("illya_package_migration_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |connection, _| {
            let search_path = search_path.clone();
            Box::pin(async move {
                sqlx::query(&search_path).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let body_pool = pool.clone();
    // A child task lets us clean the exact owned schema even if an assertion
    // fails. The UUID identifier is generated locally and contains no input.
    let result = tokio::spawn(async move { upgrade_fixture(body_pool).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}
