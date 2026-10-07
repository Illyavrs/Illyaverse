//! Profile score totals use a display scale; best-per-map selection uses the
//! standardized scale. PP selection is independent of both score choices.
use sqlx::{PgPool, postgres::PgRow};

pub(crate) async fn lifetime(pool: &PgPool, user: i64, mode: i16) -> Result<PgRow, sqlx::Error> {
    sqlx::query(
        "SELECT count(*)::bigint plays,
         LEAST(COALESCE(sum(CASE WHEN client='stable' AND (mods & 536870912)=0 THEN score WHEN client IN ('stable','lazer') THEN classic_score END),0),9223372036854775807)::bigint total,
         count(*) FILTER(WHERE (client='stable' AND (mods & 536870912)=0) OR (client IN ('stable','lazer') AND classic_score IS NOT NULL))::bigint scaled_plays,
         COALESCE(avg(accuracy),0)::float8 accuracy,COALESCE(max(max_combo),0)::integer combo,
         COALESCE(sum(COALESCE((hit_counts->>'n300')::bigint,0)+COALESCE((hit_counts->>'n100')::bigint,0)+COALESCE((hit_counts->>'misses')::bigint,0)+CASE WHEN mode<>1 THEN COALESCE((hit_counts->>'n50')::bigint,0) ELSE 0 END+CASE WHEN mode=3 THEN COALESCE((hit_counts->>'geki')::bigint,0) ELSE 0 END+CASE WHEN mode IN(2,3) THEN COALESCE((hit_counts->>'katu')::bigint,0) ELSE 0 END),0)::bigint hits
         FROM scores WHERE user_id=$1 AND mode=$2 AND verification='verified'",
    ).bind(user).bind(mode).fetch_one(pool).await
}

pub(crate) async fn ranking(pool: &PgPool, user: i64, mode: i16) -> Result<PgRow, sqlx::Error> {
    sqlx::query(
        "WITH eligible AS (
           SELECT s.* FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id
           WHERE s.verification='verified' AND s.passed AND b.status='ranked' AND s.pp_eligible AND s.mode=$2
         ), best_pp AS (
           SELECT user_id,beatmap_id,max(pp) pp FROM eligible GROUP BY user_id,beatmap_id
         ), weighted AS (
           SELECT user_id,pp,row_number() OVER(PARTITION BY user_id ORDER BY pp DESC,beatmap_id) n FROM best_pp
         ), pp_totals AS (
           SELECT user_id,sum(pp*power(0.95,n-1)) pp FROM weighted GROUP BY user_id
         ), best_score AS (
           SELECT DISTINCT ON(user_id,beatmap_id) user_id,beatmap_id,classic_score
           FROM eligible WHERE standardised_score IS NOT NULL AND classic_score IS NOT NULL
           ORDER BY user_id,beatmap_id,standardised_score DESC,id
         ), score_totals AS (
           SELECT user_id,LEAST(sum(classic_score),9223372036854775807)::bigint ranked_score FROM best_score GROUP BY user_id
         ), placed AS (
           SELECT u.id,COALESCE(t.pp,0)::float8 pp,COALESCE(s.ranked_score,0)::bigint ranked_score,
           row_number() OVER(ORDER BY COALESCE(t.pp,0) DESC,u.id) global_rank,
           row_number() OVER(PARTITION BY u.country ORDER BY COALESCE(t.pp,0) DESC,u.id) country_rank
           FROM users u LEFT JOIN pp_totals t ON t.user_id=u.id LEFT JOIN score_totals s ON s.user_id=u.id
           WHERE NOT u.is_restricted
         ) SELECT * FROM placed WHERE id=$1",
    ).bind(user).bind(mode).fetch_one(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Row, postgres::PgPoolOptions};

    #[tokio::test]
    #[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
    async fn mixed_score_aggregates_use_display_values_after_standardised_selection() {
        let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("score_scales_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let search_path = format!("SET search_path TO {schema},public");
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .after_connect(move |conn, _| {
                let query = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&query).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES('Scale fixture','scales@invalid.test','fixture','fixture') RETURNING id").fetch_one(&pool).await.unwrap();
        let map: i64 = sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES('Scale fixture','Illyaverse','Fixture','Two circles',0,'ranked',1,'scale-fixture','fixture.osu') RETURNING id").fetch_one(&pool).await.unwrap();
        // A longer raw legacy number loses to the native standardized result.
        // PP's winning play is a third result; it must not choose ranked score.
        for (raw, standardised, display, pp, client, review) in [
            (
                5_000_000,
                Some(200_000),
                Some(20_026),
                100.0,
                "stable",
                "verified",
            ),
            (
                800_000,
                Some(800_000),
                Some(80_104),
                50.0,
                "lazer",
                "verified",
            ),
            (
                700_000,
                Some(700_000),
                Some(70_091),
                300.0,
                "lazer",
                "verified",
            ),
            (
                1_000_000,
                Some(1_000_000),
                Some(100_130),
                900.0,
                "lazer",
                "pending",
            ),
            (9_000_000, None, None, 0.0, "api", "verified"),
        ] {
            sqlx::query("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp,client,verification,standardised_score,classic_score,score_scale_status) VALUES($1,$2,0,$3,100,2,$4,$5,$6,$7,$8,CASE WHEN $5='stable' THEN 'legacy_estimated' WHEN $5='lazer' THEN 'native_reported' ELSE 'unavailable' END)")
                .bind(user).bind(map).bind(raw).bind(pp).bind(client).bind(review).bind(standardised).bind(display).execute(&pool).await.unwrap();
        }
        let row = lifetime(&pool, user, 0).await.unwrap();
        assert_eq!(row.get::<i64, _>("total"), 5_150_195);
        assert_eq!(row.get::<i64, _>("plays"), 4);
        assert_eq!(row.get::<i64, _>("scaled_plays"), 3);
        let row = ranking(&pool, user, 0).await.unwrap();
        assert_eq!(row.get::<i64, _>("ranked_score"), 80_104);
        assert_eq!(row.get::<f64, _>("pp"), 300.0);
        let v2: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,client,verification,standardised_score,classic_score,score_scale_status) VALUES($1,$2,0,1000000,100,2,536870912,10,'stable','verified',1000000,100130,'legacy_scorev2') RETURNING id")
            .bind(user).bind(map).fetch_one(&pool).await.unwrap();
        assert_eq!(
            lifetime(&pool, user, 0)
                .await
                .unwrap()
                .get::<i64, _>("total"),
            5_250_325
        );
        assert_eq!(
            ranking(&pool, user, 0)
                .await
                .unwrap()
                .get::<i64, _>("ranked_score"),
            100_130
        );
        sqlx::query("UPDATE scores SET standardised_score=NULL,classic_score=NULL,score_scale_status='unavailable' WHERE id=$1").bind(v2).execute(&pool).await.unwrap();
        let row = lifetime(&pool, user, 0).await.unwrap();
        assert_eq!(row.get::<i64, _>("total"), 5_150_195);
        assert_eq!(row.get::<i64, _>("scaled_plays"), 3);
        // Missing conversion context must not leak a raw score into this total.
        sqlx::query("UPDATE scores SET standardised_score=NULL,classic_score=NULL,score_scale_status='unavailable' WHERE client='lazer'").execute(&pool).await.unwrap();
        assert_eq!(
            ranking(&pool, user, 0)
                .await
                .unwrap()
                .get::<i64, _>("ranked_score"),
            20_026
        );
        assert_eq!(
            lifetime(&pool, user, 0)
                .await
                .unwrap()
                .get::<i64, _>("total"),
            5_000_000
        );
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&control)
            .await
            .unwrap();
        control.close().await;
    }
}
