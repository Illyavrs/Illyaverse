//! Publication after the native gateway has accepted the immutable submission
//! and its replay. This is protocol admission, not gameplay replay verification.

use serde_json::json;
use sqlx::{Postgres, Row, Transaction};

#[derive(Clone, Copy)]
pub(crate) enum NativeClient {
    Stable,
    Lazer,
}

impl NativeClient {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Lazer => "lazer",
        }
    }
}

/// Publish an accepted native score once and atomically record the admission.
///
/// The caller must first lock and recheck the unrestricted owner in this same
/// transaction, before taking any score lock. It must also have checked the
/// immutable score against the validated submission and, for passed scores,
/// validated the replay before setting its saved path/completeness. This helper
/// cannot validate file bytes. It rechecks the owner, source and ingestion key
/// under the score lock and never overrides a moderator's explicit decision.
/// Returns true only for the first pending -> verified transition.
pub(crate) async fn publish(
    tx: &mut Transaction<'_, Postgres>,
    score_id: i64,
    user_id: i64,
    client: NativeClient,
    ingestion_key: &str,
) -> Result<bool, sqlx::Error> {
    let Some(score) = sqlx::query(
        "SELECT s.verification,s.passed,s.replay_path,s.replay_complete \
         FROM scores s JOIN users u ON u.id=s.user_id \
         WHERE s.id=$1 AND s.user_id=$2 AND s.client=$3 AND s.ingestion_key=$4 \
         AND NOT u.is_restricted FOR UPDATE OF s",
    )
    .bind(score_id)
    .bind(user_id)
    .bind(client.as_str())
    .bind(ingestion_key)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(false);
    };

    if score.get::<String, _>("verification") != "pending" {
        return Ok(false);
    }
    if score.get::<bool, _>("passed") {
        if score
            .get::<Option<String>, _>("replay_path")
            .is_none_or(|path| path.is_empty())
        {
            return Ok(false);
        }
        if matches!(client, NativeClient::Lazer)
            && score.get::<Option<bool>, _>("replay_complete") != Some(true)
        {
            return Ok(false);
        }
    }

    // A pending decision is as deliberate as a rejection. Read this after the
    // score lock so an administrator's earlier update and audit are both visible.
    let moderated: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM audit_events \
         WHERE action='scores.verify' AND target_type='score' AND target_id=$1)",
    )
    .bind(score_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    if moderated {
        return Ok(false);
    }

    sqlx::query("UPDATE scores SET verification='verified' WHERE id=$1")
        .bind(score_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO audit_events(actor_id,action,target_type,target_id,details) \
         VALUES($1,'scores.native_publish','score',$2,$3)",
    )
    .bind(user_id)
    .bind(score_id.to_string())
    .bind(json!({"client":client.as_str(),"admission":"native_protocol","gameplay_verified":false}))
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

#[cfg(test)]
#[path = "native_score_publication_tests.rs"]
mod tests;
