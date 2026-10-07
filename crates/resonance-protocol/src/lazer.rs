//! Models for the supported OAuth and solo-score surface of osu!lazer.
//! These models do not provide the API or SignalR multiplayer server by themselves.

use crate::{ProtocolError, Result, stable::is_md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Deserialize)]
#[serde(tag = "grant_type")]
pub enum OAuthGrant {
    #[serde(rename = "password")]
    Password { username: String, password: String },
    #[serde(rename = "refresh_token")]
    Refresh { refresh_token: String },
}

impl fmt::Debug for OAuthGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { username, .. } => f
                .debug_struct("PasswordGrant")
                .field("username", username)
                .field("password", &"[redacted]")
                .finish(),
            Self::Refresh { .. } => f.write_str("RefreshGrant { refresh_token: [redacted] }"),
        }
    }
}

impl OAuthGrant {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Password { username, password } => {
                if username.is_empty()
                    || username.len() > 64
                    || username.chars().any(char::is_control)
                {
                    return Err(ProtocolError::Invalid("OAuth username"));
                }
                if password.is_empty() || password.len() > 1024 {
                    return Err(ProtocolError::Invalid("OAuth password"));
                }
            }
            Self::Refresh { refresh_token } => {
                if refresh_token.is_empty() || refresh_token.len() > 4096 {
                    return Err(ProtocolError::Invalid("refresh token"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub token_type: String,
    pub expires_in: u64,
    pub access_token: String,
    pub refresh_token: String,
}

impl fmt::Debug for OAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthToken")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("tokens", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreTokenRequest {
    pub beatmap_hash: String,
    pub version_hash: String,
    pub ruleset_id: u8,
}

impl ScoreTokenRequest {
    pub fn validate(&self) -> Result<()> {
        if !is_md5(&self.beatmap_hash) {
            return Err(ProtocolError::Invalid("beatmap digest"));
        }
        if self.version_hash.is_empty()
            || self.version_hash.len() > 128
            || self.version_hash.chars().any(char::is_control)
        {
            return Err(ProtocolError::Invalid("version hash"));
        }
        if self.ruleset_id > 3 {
            return Err(ProtocolError::Invalid("unsupported ruleset"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMod {
    pub acronym: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoloScoreSubmission {
    pub ruleset_id: u8,
    pub passed: bool,
    pub total_score: u64,
    #[serde(default)]
    pub total_score_without_mods: u64,
    pub accuracy: f64,
    pub max_combo: u32,
    pub rank: String,
    #[serde(default)]
    pub mods: Vec<ApiMod>,
    #[serde(default)]
    pub statistics: BTreeMap<String, u32>,
    #[serde(default)]
    pub maximum_statistics: BTreeMap<String, u32>,
    #[serde(default)]
    pub pauses: Vec<u32>,
    // The gateway binds user/beatmap/token from the authenticated request and URL.
    // Supplied unknown fields must never override those authoritative values.
}

impl SoloScoreSubmission {
    pub fn validate(&self) -> Result<()> {
        if self.ruleset_id > 3 {
            return Err(ProtocolError::Invalid("unsupported ruleset"));
        }
        if !self.accuracy.is_finite() || !(0.0..=1.0).contains(&self.accuracy) {
            return Err(ProtocolError::Invalid("score accuracy"));
        }
        if self.total_score > i64::MAX as u64
            || self.total_score_without_mods > i64::MAX as u64
            || self.max_combo > 10_000_000
        {
            return Err(ProtocolError::Limit("score values"));
        }
        if !matches!(
            self.rank.as_str(),
            "XH" | "X" | "SH" | "S" | "A" | "B" | "C" | "D" | "F"
        ) {
            return Err(ProtocolError::Invalid("score rank"));
        }
        if self.mods.len() > 64
            || self.statistics.len() > 64
            || self.maximum_statistics.len() > 64
            || self.pauses.len() > 10_000
        {
            return Err(ProtocolError::Limit("score collections"));
        }
        for modifier in &self.mods {
            if modifier.acronym.is_empty()
                || modifier.acronym.len() > 16
                || !modifier
                    .acronym
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                || modifier.settings.len() > 64
            {
                return Err(ProtocolError::Invalid("score modifier"));
            }
            if serde_json::to_vec(&modifier.settings)
                .map_err(|_| ProtocolError::Invalid("modifier settings"))?
                .len()
                > 4096
            {
                return Err(ProtocolError::Limit("modifier settings"));
            }
        }
        for (key, value) in self.statistics.iter().chain(self.maximum_statistics.iter()) {
            if key.is_empty()
                || key.len() > 64
                || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                || *value > 10_000_000
            {
                return Err(ProtocolError::Invalid("hit statistics"));
            }
        }
        if self.pauses.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(ProtocolError::Invalid("pause ordering"));
        }
        Ok(())
    }
}

/// SignalR JSON transport framing only. Implementing these boundaries does not
/// implement hub negotiation, invocation dispatch, reconnection, or osu! rooms.
pub fn split_signalr_json_frames(input: &[u8]) -> Result<Vec<Value>> {
    if input.len() > 1_048_576 {
        return Err(ProtocolError::Limit("SignalR frame size"));
    }
    if input.last() != Some(&0x1e) {
        return Err(ProtocolError::Truncated);
    }
    let mut frames = Vec::new();
    for segment in input[..input.len() - 1].split(|byte| *byte == 0x1e) {
        if frames.len() == 256 {
            return Err(ProtocolError::Limit("SignalR frame count"));
        }
        let value: Value =
            serde_json::from_slice(segment).map_err(|_| ProtocolError::Invalid("SignalR JSON"))?;
        if !value.is_object() {
            return Err(ProtocolError::Invalid("SignalR envelope"));
        }
        frames.push(value);
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score() -> SoloScoreSubmission {
        serde_json::from_value(serde_json::json!({ "ruleset_id": 0, "passed": true, "total_score": 1000000, "accuracy": 0.99, "max_combo": 1000, "rank": "S", "mods": [{"acronym": "DT", "settings": {"speed_change": 1.5}}], "statistics": {"great": 300, "miss": 1}, "maximum_statistics": {"great": 301}, "user_id": 999999 })).unwrap()
    }
    #[test]
    fn standard_submission_accepts_mod_settings_and_ignores_identity_claims() {
        let value = score();
        value.validate().unwrap();
        assert!(
            serde_json::to_value(value)
                .unwrap()
                .get("user_id")
                .is_none()
        );
    }
    #[test]
    fn score_validation_rejects_unbounded_or_non_finite_input() {
        let mut value = score();
        value.accuracy = f64::NAN;
        assert!(value.validate().is_err());
        value.accuracy = 1.1;
        assert!(value.validate().is_err());
        value.accuracy = 1.0;
        value.ruleset_id = 255;
        assert!(value.validate().is_err());
        value.ruleset_id = 0;
        value.pauses = vec![100, 50];
        assert!(value.validate().is_err());
    }
    #[test]
    fn signalr_frames_require_record_separator_and_objects() {
        let frames =
            split_signalr_json_frames(b"{\"protocol\":\"json\",\"version\":1}\x1e{\"type\":6}\x1e")
                .unwrap();
        assert_eq!(frames.len(), 2);
        assert!(split_signalr_json_frames(b"{\"type\":6}").is_err());
        assert!(split_signalr_json_frames(b"[]\x1e").is_err());
    }
    #[test]
    fn grants_and_responses_never_print_credentials() {
        let grant = OAuthGrant::Password {
            username: "Illya".into(),
            password: "very-secret-password".into(),
        };
        grant.validate().unwrap();
        assert!(!format!("{grant:?}").contains("very-secret"));
        let token = OAuthToken {
            token_type: "Bearer".into(),
            expires_in: 3600,
            access_token: "access-secret".into(),
            refresh_token: "refresh-secret".into(),
        };
        assert!(!format!("{token:?}").contains("secret"));
    }
}
