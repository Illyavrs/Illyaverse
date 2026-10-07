//! Server-owned score partitions. The native ruleset remains in the 0–3 range.
use serde::{Deserialize, Serialize};

pub const RELAX_BIT: u32 = 128;
const AUTOPLAY_BIT: u32 = 2048;
const AUTOPILOT_BIT: u32 = 8192;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreVariant {
    #[default]
    Vanilla,
    Relax,
}

impl ScoreVariant {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Relax => "relax",
        }
    }

    /// Derive the partition from the validated legacy projection. This does not
    /// validate every other mod combination; transport-specific validation must
    /// still run before calculation or persistence.
    pub fn from_legacy(mode: u8, mods: u32) -> Result<Self, &'static str> {
        if mode > 3 {
            return Err("Unsupported ruleset.");
        }
        if mods & RELAX_BIT == 0 {
            return Ok(Self::Vanilla);
        }
        if mode == 3 {
            return Err("Relax is unavailable for osu!mania.");
        }
        Ok(Self::Relax)
    }
}

/// PP eligibility after legacy mod validation. Relax belongs to its own ranked
/// partition; autoplay and autopilot retain their existing PP exclusion.
pub fn legacy_pp_eligible(mode: u8, mods: u32) -> Result<bool, &'static str> {
    ScoreVariant::from_legacy(mode, mods)?;
    Ok(mods & (AUTOPLAY_BIT | AUTOPILOT_BIT) == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_have_a_stable_public_serialization() {
        assert_eq!(ScoreVariant::default(), ScoreVariant::Vanilla);
        for variant in [ScoreVariant::Vanilla, ScoreVariant::Relax] {
            let encoded = serde_json::to_string(&variant).unwrap();
            assert_eq!(encoded, format!("\"{}\"", variant.as_str()));
            assert_eq!(
                serde_json::from_str::<ScoreVariant>(&encoded).unwrap(),
                variant
            );
        }
        assert!(serde_json::from_str::<ScoreVariant>("\"rx\"").is_err());
        assert!(serde_json::from_str::<ScoreVariant>("\"autopilot\"").is_err());
    }

    #[test]
    fn relax_changes_the_partition_without_changing_the_ruleset() {
        for mode in 0..=2 {
            assert_eq!(
                ScoreVariant::from_legacy(mode, 0),
                Ok(ScoreVariant::Vanilla)
            );
            assert_eq!(
                ScoreVariant::from_legacy(mode, RELAX_BIT | 64 | 8),
                Ok(ScoreVariant::Relax)
            );
            assert_eq!(legacy_pp_eligible(mode, RELAX_BIT), Ok(true));
        }
        assert_eq!(ScoreVariant::from_legacy(3, 0), Ok(ScoreVariant::Vanilla));
        assert!(ScoreVariant::from_legacy(3, RELAX_BIT).is_err());
        assert!(legacy_pp_eligible(3, RELAX_BIT).is_err());
        assert!(ScoreVariant::from_legacy(4, 0).is_err());
    }

    #[test]
    fn legacy_automation_stays_out_of_pp_in_both_partitions() {
        for mode in 0..=2 {
            for automation in [AUTOPLAY_BIT, AUTOPILOT_BIT] {
                assert_eq!(legacy_pp_eligible(mode, automation), Ok(false));
                assert_eq!(legacy_pp_eligible(mode, automation | RELAX_BIT), Ok(false));
            }
        }
    }
}
