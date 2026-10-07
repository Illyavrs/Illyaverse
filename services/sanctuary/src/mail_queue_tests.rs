//! A fresh PostgreSQL schema and an original loopback SMTP fixture. No external mail.
use crate::mail_delivery::{MailConfig, MailKind, process_one};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::Notify,
    task::JoinHandle,
};
use uuid::Uuid;

struct SmtpFixture {
    address: SocketAddr,
    reached: Arc<Notify>,
    release: Arc<Notify>,
    task: JoinHandle<String>,
}
async fn smtp(reply: &'static str, hold: bool) -> SmtpFixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let reached = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let ready = reached.clone();
    let resume = release.clone();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        write
            .write_all(b"220 local-fixture ESMTP\r\n")
            .await
            .unwrap();
        let mut payload = String::new();
        let mut in_data = false;
        loop {
            let mut line = String::new();
            if read.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            if in_data && line != ".\r\n" {
                payload.push_str(&line);
                continue;
            }
            let response = if in_data {
                in_data = false;
                ready.notify_one();
                if hold {
                    resume.notified().await;
                }
                reply
            } else if line.starts_with("EHLO ") {
                "250-local-fixture\r\n250 8BITMIME\r\n"
            } else if line.starts_with("DATA") {
                in_data = true;
                "354 send message\r\n"
            } else if line.starts_with("QUIT") {
                write.write_all(b"221 goodbye\r\n").await.unwrap();
                break;
            } else {
                "250 accepted\r\n"
            };
            write.write_all(response.as_bytes()).await.unwrap();
        }
        payload
    });
    SmtpFixture {
        address,
        reached,
        release,
        task,
    }
}

async fn queue(db: &PgPool, config: &MailConfig, user: i64) -> (Uuid, Uuid) {
    let token = Uuid::new_v4();
    let job = Uuid::new_v4();
    let raw = Uuid::new_v4().simple().to_string().repeat(2);
    let ciphertext = config
        .encrypt_job(
            job,
            user,
            MailKind::ResetPassword,
            "queue@example.test",
            "ja",
            &raw,
        )
        .unwrap();
    assert!(
        !ciphertext
            .windows(raw.len())
            .any(|part| part == raw.as_bytes())
    );
    assert!(
        !ciphertext
            .windows(18)
            .any(|part| part == b"queue@example.test")
    );
    let mut tx = db.begin().await.unwrap();
    sqlx::query("INSERT INTO account_recovery_tokens(id,user_id,purpose,email_snapshot,token_hash,expires_at) VALUES($1,$2,'reset','queue@example.test',$3,now()+interval '15 minutes')")
        .bind(token).bind(user).bind(crate::account_recovery::canonical_token_hash(&raw).unwrap()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO mail_jobs(id,token_id,user_id,kind,encrypted_payload) VALUES($1,$2,$3,'reset',$4)")
        .bind(job).bind(token).bind(user).bind(ciphertext).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    (token, job)
}
async fn status(db: &PgPool, id: Uuid) -> (String, i32, bool) {
    let row = sqlx::query(
        "SELECT status,attempts,encrypted_payload IS NULL AS erased FROM mail_jobs WHERE id=$1",
    )
    .bind(id)
    .fetch_one(db)
    .await
    .unwrap();
    (row.get("status"), row.get("attempts"), row.get("erased"))
}
async fn capture(task: JoinHandle<String>) -> String {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; fresh PostgreSQL schema and loopback SMTP"]
async fn durable_mail_queue_leases_retries_and_cleanup() {
    let url = std::env::var("ILLYA_TEST_DATABASE_URL").unwrap();
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("mail_queue_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let db = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let pool = db.clone();
    let outcome = tokio::spawn(async move {
        let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES('MailQueue','queue@example.test','fixture','fixture') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let server = smtp("250 queued\r\n", true).await;
        let config = Arc::new(MailConfig::local_for_test("https://queue.illya.test", server.address, [17; 32]));
        let (_, job) = queue(&pool, &config, user).await;
        let sender_pool = pool.clone();
        let sender_config = config.clone();
        let sender = tokio::spawn(async move { process_one(&sender_pool, &sender_config).await.unwrap() });
        tokio::time::timeout(Duration::from_secs(5), server.reached.notified()).await.unwrap();
        assert_eq!(status(&pool, job).await, ("sending".into(), 1, false));
        assert!(!process_one(&pool, &config).await.unwrap(), "a leased job cannot be claimed twice");
        server.release.notify_one();
        assert!(sender.await.unwrap());
        let message = capture(server.task).await;
        assert!(message.contains(&format!("account-{job}@queue.illya.test")));
        let parsed = mailparse::parse_mail(message.as_bytes()).unwrap();
        assert!(parsed.subparts.iter().any(|part| part.get_body().unwrap().contains("/reset-password#token=")));
        assert_eq!(status(&pool, job).await, ("sent".into(), 1, true));

        let first = smtp("450 temporarily unavailable\r\n", false).await;
        let config = MailConfig::local_for_test("https://queue.illya.test", first.address, [17; 32]);
        let (_, retry) = queue(&pool, &config, user).await;
        assert!(process_one(&pool, &config).await.unwrap());
        let first_message = capture(first.task).await;
        assert_eq!(status(&pool, retry).await, ("pending".into(), 1, false));
        assert!(!process_one(&pool, &config).await.unwrap(), "retry backoff must be respected");
        sqlx::query("UPDATE mail_jobs SET available_at=now() WHERE id=$1").bind(retry).execute(&pool).await.unwrap();
        let second = smtp("250 queued\r\n", false).await;
        let config = MailConfig::local_for_test("https://queue.illya.test", second.address, [17; 32]);
        assert!(process_one(&pool, &config).await.unwrap());
        let second_message = capture(second.task).await;
        let expected_id = format!("account-{retry}@queue.illya.test");
        assert!(first_message.contains(&expected_id) && second_message.contains(&expected_id));
        assert_eq!(status(&pool, retry).await, ("sent".into(), 2, true));

        let rejected = smtp("550 permanent rejection\r\n", false).await;
        let config = MailConfig::local_for_test("https://queue.illya.test", rejected.address, [17; 32]);
        let (_, failure) = queue(&pool, &config, user).await;
        assert!(process_one(&pool, &config).await.unwrap());
        capture(rejected.task).await;
        assert_eq!(status(&pool, failure).await, ("failed".into(), 1, true));

        let (_, exhausted) = queue(&pool, &config, user).await;
        sqlx::query("UPDATE mail_jobs SET status='sending',attempts=8,lease_id=$2,lease_until=now()-interval '1 minute' WHERE id=$1")
            .bind(exhausted).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
        assert!(!process_one(&pool, &MailConfig::disabled()).await.unwrap());
        assert_eq!(status(&pool, exhausted).await, ("failed".into(), 8, true));

        let (inactive_token, canceled) = queue(&pool, &config, user).await;
        sqlx::query("UPDATE account_recovery_tokens SET consumed_at=now() WHERE id=$1").bind(inactive_token).execute(&pool).await.unwrap();
        process_one(&pool, &MailConfig::disabled()).await.unwrap();
        assert_eq!(status(&pool, canceled).await, ("canceled".into(), 0, true));

        let (_, substituted) = queue(&pool, &config, user).await;
        let (different_token, inactive_job) = queue(&pool, &config, user).await;
        sqlx::query("UPDATE mail_jobs SET status='canceled',encrypted_payload=NULL WHERE id=$1")
            .bind(inactive_job).execute(&pool).await.unwrap();
        sqlx::query("UPDATE mail_jobs SET token_id=$2 WHERE id=$1")
            .bind(substituted).bind(different_token).execute(&pool).await.unwrap();
        assert!(process_one(&pool, &config).await.unwrap());
        assert_eq!(status(&pool, substituted).await, ("canceled".into(), 1, true));

        let (old_token, old_job) = queue(&pool, &config, user).await;
        sqlx::query("UPDATE account_recovery_tokens SET created_at=now()-interval '9 days',expires_at=now()-interval '8 days' WHERE id=$1")
            .bind(old_token).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO account_recovery_limits(recipient_hash,window_started_at) VALUES($1,now()-interval '2 days')")
            .bind(vec![42_u8; 32]).execute(&pool).await.unwrap();
        process_one(&pool, &MailConfig::disabled()).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mail_jobs WHERE id=$1").bind(old_job).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0, "old token deletion must cascade its mail job");
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM account_recovery_limits").fetch_one(&pool).await.unwrap(), 0);
    }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
