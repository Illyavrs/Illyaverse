//! Shared, transport-independent rules for Illyaverse.
pub mod score_math;
pub mod score_mods;
use serde::{Deserialize, Serialize};

pub const PERMISSIONS: &[&str] = &[
    "chat.send",
    "social.manage",
    "beatmaps.upload",
    "beatmaps.import",
    "beatmaps.moderate",
    "users.moderate",
    "roles.manage",
    "audit.read",
    "scores.moderate",
    "admin.view",
];

pub fn has_permission(permissions: &[String], required: &str) -> bool {
    permissions
        .iter()
        .any(|permission| permission == "*" || permission == required)
}

pub fn validate_username(value: &str) -> Result<(), &'static str> {
    if !(3..=24).contains(&value.len())
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
        || value.trim() != value
    {
        return Err(
            "Username must be 3–24 ASCII letters, numbers, spaces, underscores or hyphens.",
        );
    }
    Ok(())
}

pub fn validate_password(value: &str) -> Result<(), &'static str> {
    if !(12..=128).contains(&value.len()) {
        return Err("Password must be 12–128 bytes.");
    }
    Ok(())
}

pub fn validate_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub locale: String,
    pub mode: String,
    pub accent_color: String,
    pub background_url: Option<String>,
    pub background_blur: u8,
    pub background_dim: f32,
    pub border_radius: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            locale: "zh-TW".into(),
            mode: "dark".into(),
            accent_color: "#f28eb6".into(),
            background_url: None,
            background_blur: 0,
            background_dim: 0.76,
            border_radius: 14,
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.locale.as_str(), "zh-TW" | "en" | "ja") {
            return Err("Unsupported locale.");
        }
        if !matches!(self.mode.as_str(), "dark" | "light" | "system") {
            return Err("Unknown theme mode.");
        }
        if !validate_color(&self.accent_color) {
            return Err("Accent color must be a six-digit hex color.");
        }
        if self.background_blur > 48
            || self.border_radius > 40
            || !self.background_dim.is_finite()
            || !(0.0..=1.0).contains(&self.background_dim)
        {
            return Err("Theme values are outside their permitted range.");
        }
        if self
            .background_url
            .as_ref()
            .is_some_and(|v| !v.starts_with("/media/") || v.contains(".."))
        {
            return Err("Background must be an uploaded image.");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Pagination {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}
impl Pagination {
    pub fn bounds(&self) -> (i64, i64, i64) {
        let page = i64::from(self.page.unwrap_or(1).clamp(1, 1_000_000));
        let size = i64::from(self.page_size.unwrap_or(24).clamp(1, 100));
        (page, size, (page - 1) * size)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HitCounts {
    pub n300: u32,
    pub n100: u32,
    pub n50: u32,
    pub misses: u32,
    pub geki: u32,
    pub katu: u32,
    pub large_tick_hits: u32,
    pub small_tick_hits: u32,
    pub slider_end_hits: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_wildcard_is_explicit() {
        assert!(has_permission(&["*".into()], "roles.manage"));
        assert!(!has_permission(&["admin.view".into()], "roles.manage"));
    }
    #[test]
    fn rejects_unsafe_theme_urls_and_nan() {
        let mut p = Preferences {
            background_url: Some("https://remote.example/a.jpg".into()),
            ..Preferences::default()
        };
        assert!(p.validate().is_err());
        p.background_url = None;
        p.background_dim = f32::NAN;
        assert!(p.validate().is_err());
    }
    #[test]
    fn usernames_cannot_inject_control_characters() {
        assert!(validate_username("Illya_01").is_ok());
        assert!(validate_username("a\nb").is_err());
        assert!(validate_username(" leading").is_err());
    }
    #[test]
    fn pagination_is_bounded() {
        assert_eq!(
            Pagination {
                page: Some(0),
                page_size: Some(1000)
            }
            .bounds(),
            (1, 100, 0)
        );
    }
}
