//! Shared, transport-independent rules for Illyaverse.
pub mod beatmap_filename;
pub mod official_set;
pub mod score_math;
pub mod score_mods;
pub mod score_variant;
pub use score_variant::ScoreVariant;
use serde::{Deserialize, Serialize};

pub const PERMISSIONS: &[&str] = &[
    "chat.send",
    "chat.moderate",
    "social.manage",
    "beatmaps.upload",
    "beatmaps.import",
    "beatmaps.moderate",
    "users.moderate",
    "users.sessions.revoke",
    "roles.manage",
    "audit.read",
    "scores.moderate",
    "announcements.manage",
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
    pub pm_friends_only: bool,
    pub locale: String,
    pub mode: String,
    pub accent_color: String,
    pub background_url: Option<String>,
    pub background_blur: u8,
    pub background_dim: f32,
    pub panel_blur: u8,
    pub panel_opacity: f32,
    pub glass_level: u8,
    pub border_radius: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            pm_friends_only: false,
            locale: "zh-TW".into(),
            mode: "dark".into(),
            accent_color: "#f28eb6".into(),
            background_url: None,
            background_blur: 0,
            background_dim: 0.76,
            panel_blur: 8,
            panel_opacity: 0.55,
            glass_level: 100,
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
            || self.panel_blur > 48
            || self.glass_level > 100
            || self.border_radius > 40
            || !self.background_dim.is_finite()
            || !(0.0..=1.0).contains(&self.background_dim)
            || !self.panel_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.panel_opacity)
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
        assert!(PERMISSIONS.contains(&"chat.moderate"));
        assert!(has_permission(&["chat.moderate".into()], "chat.moderate"));
        assert!(!has_permission(&["users.moderate".into()], "chat.moderate"));
        assert!(!has_permission(&["chat.moderate".into()], "chat.send"));
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
    fn existing_preferences_keep_custom_values_and_gain_panel_defaults() {
        let p: Preferences = serde_json::from_str(
            r##"{"locale":"ja","mode":"light","accent_color":"#7dd8c5","background_url":"/media/backgrounds/player.webp","background_blur":12,"background_dim":0.4,"border_radius":20}"##,
        )
        .unwrap();
        assert!(p.validate().is_ok());
        assert_eq!(p.locale, "ja");
        assert_eq!(p.mode, "light");
        assert_eq!(p.accent_color, "#7dd8c5");
        assert_eq!(
            p.background_url.as_deref(),
            Some("/media/backgrounds/player.webp")
        );
        assert_eq!(p.background_blur, 12);
        assert_eq!(p.background_dim, 0.4);
        assert_eq!(p.border_radius, 20);
        assert_eq!(p.panel_blur, 8);
        assert_eq!(p.panel_opacity, 0.55);
        assert_eq!(p.glass_level, 100);
        assert!(!p.pm_friends_only);
    }
    #[test]
    fn friend_only_preferences_preserve_partial_theme_values() {
        let p: Preferences = serde_json::from_value(serde_json::json!({
            "pm_friends_only":true,"locale":"ja","mode":"light","accent_color":"#123456"
        }))
        .unwrap();
        assert!(p.pm_friends_only);
        assert_eq!(p.locale, "ja");
        assert_eq!(p.mode, "light");
        assert_eq!(p.accent_color, "#123456");
        assert_eq!(p.panel_blur, 8);
        assert!(p.validate().is_ok());
    }
    #[test]
    fn panel_preferences_reject_invalid_values_and_preserve_endpoints() {
        for opacity in [-0.01, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let p = Preferences {
                panel_opacity: opacity,
                ..Preferences::default()
            };
            assert!(p.validate().is_err());
        }
        let p = Preferences {
            panel_blur: 49,
            ..Preferences::default()
        };
        assert!(p.validate().is_err());
        for (panel_blur, panel_opacity) in [(0, 0.0), (48, 1.0)] {
            let p = Preferences {
                panel_blur,
                panel_opacity,
                ..Preferences::default()
            };
            assert!(p.validate().is_ok());
            let restored: Preferences =
                serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
            assert_eq!(restored.panel_blur, panel_blur);
            assert_eq!(restored.panel_opacity, panel_opacity);
        }
    }
    #[test]
    fn glass_coverage_is_bounded_and_round_trips_without_changing_other_controls() {
        for glass_level in [0, 25, 50, 75, 100] {
            let p = Preferences {
                glass_level,
                panel_blur: 17,
                panel_opacity: 0.42,
                ..Preferences::default()
            };
            assert!(p.validate().is_ok());
            let restored: Preferences =
                serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
            assert_eq!(restored.glass_level, glass_level);
            assert_eq!(restored.panel_blur, 17);
            assert_eq!(restored.panel_opacity, 0.42);
        }
        let invalid = Preferences {
            glass_level: 101,
            ..Preferences::default()
        };
        assert!(invalid.validate().is_err());
        for value in [
            serde_json::json!(-1),
            serde_json::json!(50.5),
            serde_json::json!(256),
        ] {
            assert!(
                serde_json::from_value::<Preferences>(serde_json::json!({
                    "glass_level": value
                }))
                .is_err()
            );
        }
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
