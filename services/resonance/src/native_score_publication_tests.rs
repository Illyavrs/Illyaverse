use super::{NativeClient, publish};
use futures_util::FutureExt;
use serde_json::{Value, json};
use sqlx::{
    PgPool, Postgres, Transaction,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{str::FromStr, time::Duration};

struct Fixture {
    control: PgPool,
    pool: PgPool,
    schema: String,
    owner: i64,
    moderator: i64,
    map: i64,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(
            matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "loopback disposable database required"
        );
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("native_publication_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .application_name(&schema)
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let owner = user(&pool, "NativePublicationOwner").await;
        let moderator = user(&pool, "NativePublicationModerator").await;
        let map: i64 = sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES('Publication','Fixture','Fixture','Fixture',0,'ranked','local',1,'0123456789abcdef0123456789abcdef','fixture.osu') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        Self {
            control,
            pool,
            schema,
            owner,
            moderator,
            map,
        }
    }

    async fn score(
        &self,
        client: &str,
        passed: bool,
        path: Option<&str>,
        complete: Option<bool>,
    ) -> (i64, String) {
        let key = format!("{client}:{}:{}", self.owner, uuid::Uuid::new_v4().simple());
        let id: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp,passed,client,ingestion_key,replay_path,replay_complete) VALUES($1,$2,0,600,100,2,1,$3,$4,$5,$6,$7) RETURNING id")
            .bind(self.owner).bind(self.map).bind(passed).bind(client).bind(&key)
            .bind(path).bind(complete).fetch_one(&self.pool).await.unwrap();
        (id, key)
    }

    async fn outcome(&self, score: i64) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('verification',s.verification,'audits',(SELECT count(*) FROM audit_events WHERE action='scores.native_publish' AND target_type='score' AND target_id=s.id::text),'details',(SELECT details FROM audit_events WHERE action='scores.native_publish' AND target_type='score' AND target_id=s.id::text ORDER BY id LIMIT 1)) FROM scores s WHERE s.id=$1")
            .bind(score).fetch_one(&self.pool).await.unwrap()
    }

    async fn moderate(&self, tx: &mut Transaction<'_, Postgres>, score: i64, verification: &str) {
        sqlx::query("UPDATE scores SET verification=$2 WHERE id=$1")
            .bind(score)
            .bind(verification)
            .execute(&mut **tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.verify','score',$2,$3)")
            .bind(self.moderator).bind(score.to_string()).bind(json!({"verification":verification}))
            .execute(&mut **tx).await.unwrap();
    }

    async fn wait_for_lock(&self, query: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                    .bind(&self.schema).bind(query).fetch_one(&self.control).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("expected PostgreSQL row lock barrier");
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
    }
}

async fn user(pool: &PgPool, name: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'unused','unused') RETURNING id")
        .bind(name).bind(format!("{name}@invalid.test")).fetch_one(pool).await.unwrap()
}

async fn attempt(pool: &PgPool, owner: i64, score: i64, client: NativeClient, key: &str) -> bool {
    let mut tx = pool.begin().await.unwrap();
    let owner = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM users WHERE id=$1 AND NOT is_restricted FOR SHARE",
    )
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await
    .unwrap();
    let result = if let Some(owner) = owner {
        publish(&mut tx, score, owner, client, key).await.unwrap()
    } else {
        false
    };
    tx.commit().await.unwrap();
    result
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_publication_requires_matching_source_owner_key_and_saved_replay() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        for (client_name, client) in [("stable", NativeClient::Stable), ("lazer", NativeClient::Lazer)] {
            let (score, key) = f.score(client_name, true, None, Some(true)).await;
            assert!(!attempt(&f.pool, f.owner, score, client, &key).await);
            assert_eq!(f.outcome(score).await, json!({"verification":"pending","audits":0,"details":null}));
            sqlx::query("UPDATE scores SET replay_path='validated-fixture.osr' WHERE id=$1").bind(score).execute(&f.pool).await.unwrap();
            assert!(!attempt(&f.pool, f.moderator, score, client, &key).await);
            assert!(!attempt(&f.pool, f.owner, score, client, "wrong-immutable-ingestion-key").await);
            let other = if client_name == "stable" { NativeClient::Lazer } else { NativeClient::Stable };
            assert!(!attempt(&f.pool, f.owner, score, other, &key).await);
            assert!(attempt(&f.pool, f.owner, score, client, &key).await);
            let outcome = f.outcome(score).await;
            assert_eq!(outcome["verification"], "verified");
            assert_eq!(outcome["audits"], 1);
            assert_eq!(outcome["details"], json!({"client":client_name,"admission":"native_protocol","gameplay_verified":false}));
            assert!(!outcome.to_string().contains(&key));
        }

        let (api, key) = f.score("api", true, Some("api.osr"), Some(true)).await;
        assert!(!attempt(&f.pool, f.owner, api, NativeClient::Stable, &key).await);
        assert!(!attempt(&f.pool, f.owner, api, NativeClient::Lazer, &key).await);
        for complete in [None, Some(false)] {
            let (truncated, key) = f.score("lazer", true, Some("interrupted.osr"), complete).await;
            assert!(!attempt(&f.pool, f.owner, truncated, NativeClient::Lazer, &key).await);
            assert_eq!(f.outcome(truncated).await["verification"], "pending");
        }
        let (empty, key) = f.score("stable", true, Some(""), None).await;
        assert!(!attempt(&f.pool, f.owner, empty, NativeClient::Stable, &key).await);
        let (failed, key) = f.score("stable", false, None, None).await;
        assert!(attempt(&f.pool, f.owner, failed, NativeClient::Stable, &key).await);
        assert_eq!(f.outcome(failed).await["verification"], "verified");
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_publication_retries_are_atomic_and_single_audit() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let (score, key) = f.score("stable", true, Some("validated.osr"), None).await;
        let mut locked = f.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 AND NOT is_restricted FOR SHARE")
            .bind(f.owner)
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        assert!(
            publish(&mut locked, score, f.owner, NativeClient::Stable, &key)
                .await
                .unwrap()
        );
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let pool = f.pool.clone();
            let key = key.clone();
            let owner = f.owner;
            tasks.push(tokio::spawn(async move {
                attempt(&pool, owner, score, NativeClient::Stable, &key).await
            }));
        }
        f.wait_for_lock("%FROM scores s%FOR UPDATE OF s%").await;
        locked.commit().await.unwrap();
        for task in tasks {
            assert!(
                !tokio::time::timeout(Duration::from_secs(5), task)
                    .await
                    .unwrap()
                    .unwrap()
            );
        }
        assert_eq!(f.outcome(score).await["audits"], 1);
        assert_eq!(f.outcome(score).await["verification"], "verified");

        let (rolled_back, key) = f.score("stable", false, None, None).await;
        let mut tx = f.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(f.owner)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            publish(&mut tx, rolled_back, f.owner, NativeClient::Stable, &key)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
        assert_eq!(
            f.outcome(rolled_back).await,
            json!({"verification":"pending","audits":0,"details":null})
        );
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_publication_preserves_admin_decisions_after_score_lock_wait() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        for verification in ["pending", "rejected", "verified"] {
            let (score, key) = f.score("stable", true, Some("validated.osr"), None).await;
            let mut admin = f.pool.begin().await.unwrap();
            f.moderate(&mut admin, score, verification).await;
            let pool = f.pool.clone();
            let owner = f.owner;
            let task = tokio::spawn(async move {
                attempt(&pool, owner, score, NativeClient::Stable, &key).await
            });
            f.wait_for_lock("%FROM scores s%FOR UPDATE OF s%").await;
            admin.commit().await.unwrap();
            assert!(
                !tokio::time::timeout(Duration::from_secs(5), task)
                    .await
                    .unwrap()
                    .unwrap()
            );
            assert_eq!(
                f.outcome(score).await,
                json!({"verification":verification,"audits":0,"details":null})
            );
        }
        let (rejected, key) = f.score("stable", false, None, None).await;
        sqlx::query("UPDATE scores SET verification='rejected' WHERE id=$1")
            .bind(rejected)
            .execute(&f.pool)
            .await
            .unwrap();
        assert!(!attempt(&f.pool, f.owner, rejected, NativeClient::Stable, &key).await);

        let (published, key) = f.score("stable", true, Some("validated.osr"), None).await;
        assert!(attempt(&f.pool, f.owner, published, NativeClient::Stable, &key).await);
        let mut admin = f.pool.begin().await.unwrap();
        f.moderate(&mut admin, published, "pending").await;
        admin.commit().await.unwrap();
        assert!(!attempt(&f.pool, f.owner, published, NativeClient::Stable, &key).await);
        assert_eq!(f.outcome(published).await["verification"], "pending");
        assert_eq!(f.outcome(published).await["audits"], 1);
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_publication_cannot_win_after_concurrent_owner_restriction() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let (score, key) = f.score("stable", false, None, None).await;
        let mut restriction = f.pool.begin().await.unwrap();
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(f.owner)
            .execute(&mut *restriction)
            .await
            .unwrap();
        let pool = f.pool.clone();
        let owner = f.owner;
        let retry_key = key.clone();
        let task = tokio::spawn(async move {
            attempt(&pool, owner, score, NativeClient::Stable, &retry_key).await
        });
        f.wait_for_lock("%FROM users%FOR SHARE%").await;
        restriction.commit().await.unwrap();
        assert!(
            !tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap()
        );
        assert_eq!(
            f.outcome(score).await,
            json!({"verification":"pending","audits":0,"details":null})
        );

        // The helper also rejects a restricted owner even if an integration
        // mistakenly passes the owner snapshot from before the restriction.
        let mut tx = f.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(f.owner)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            !publish(&mut tx, score, f.owner, NativeClient::Stable, &key)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
