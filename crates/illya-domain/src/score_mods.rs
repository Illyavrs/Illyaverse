//! Validate native mod parameters before performance calculation or persistence.
use rosu_mods::{GameMode, GameMods, serde::GameModsSeed};
use serde::de::DeserializeSeed;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct ValidatedMods {
    pub mods: GameMods,
    pub legacy_bits: u32,
    pub pp_eligible: bool,
}

impl ValidatedMods {
    /// Settings remain native structured mods. Legacy bits are only a projection
    /// for stable presentation and must never replace the calculation input.
    pub fn parse(ruleset: u8, input: &Value) -> Result<Self, &'static str> {
        let mode = match ruleset {
            0 => GameMode::Osu,
            1 => GameMode::Taiko,
            2 => GameMode::Catch,
            3 => GameMode::Mania,
            _ => return Err("Unsupported ruleset."),
        };
        let values = input.as_array().ok_or("Mods must be an array.")?;
        if values.len() > 32 {
            return Err("Too many mods.");
        }
        let mut seen = HashSet::new();
        let mut eligible = true;
        for value in values {
            let object = value.as_object().ok_or("Each mod must be an object.")?;
            if object
                .keys()
                .any(|key| key != "acronym" && key != "settings")
            {
                return Err("Unknown mod field.");
            }
            let acronym = value["acronym"].as_str().ok_or("Missing mod acronym.")?;
            if !seen.insert(acronym) {
                return Err("Duplicate mod.");
            }
            if !matches!(
                acronym,
                "NF" | "EZ"
                    | "HD"
                    | "HR"
                    | "SD"
                    | "PF"
                    | "DT"
                    | "NC"
                    | "HT"
                    | "DC"
                    | "FL"
                    | "SO"
                    | "RX"
                    | "AP"
                    | "CL"
                    | "DA"
                    | "MR"
                    | "1K"
                    | "2K"
                    | "3K"
                    | "4K"
                    | "5K"
                    | "6K"
                    | "7K"
                    | "8K"
                    | "9K"
                    | "10K"
            ) {
                return Err("This mod is not supported by the performance adapter.");
            }
            if matches!(acronym, "RX" | "AP" | "DA") {
                eligible = false;
            }
            if let Some(settings) = object.get("settings") {
                let settings = settings
                    .as_object()
                    .ok_or("Mod settings must be an object.")?;
                if settings.len() > 16 {
                    return Err("Too many mod settings.");
                }
                for (key, value) in settings {
                    // Native serializers omit unset parameters. Explicit nulls
                    // have the same meaning and are accepted by rosu-mods.
                    if value.is_null() {
                        continue;
                    }
                    let valid = match (acronym, key.as_str()) {
                        ("DT" | "NC", "speed_change") => number_in(value, 1.0, 2.0),
                        ("HT" | "DC", "speed_change") => number_in(value, 0.5, 1.0),
                        ("DT" | "HT", "adjust_pitch") => value.is_boolean(),
                        (
                            "DA",
                            "circle_size" | "approach_rate" | "drain_rate" | "overall_difficulty",
                        ) => number_in(value, 0.0, 11.0),
                        ("DA", "scroll_speed") => number_in(value, 0.1, 10.0),
                        ("DA", "extended_limits" | "hard_rock_offsets") => value.is_boolean(),
                        ("HD", "only_fade_approach_circles") => value.is_boolean(),
                        (
                            "CL",
                            "no_slider_head_accuracy"
                            | "classic_note_lock"
                            | "always_play_tail_sample"
                            | "fade_hit_circle_early"
                            | "classic_health",
                        ) => value.is_boolean(),
                        ("MR", "reflection") => {
                            value.as_str().is_some_and(|v| matches!(v, "0" | "1" | "2"))
                        }
                        // rosu-pp has no custom flashlight area/delay model.
                        ("FL", "follow_delay") => value.as_f64() == Some(120.0),
                        ("FL", "size_multiplier") => value.as_f64() == Some(1.0),
                        ("FL", "combo_based_size") => value.as_bool() == Some(true),
                        _ => false,
                    };
                    if !valid {
                        return Err("Unsupported or out-of-range mod setting.");
                    }
                }
            }
        }
        let mods = GameModsSeed::Mode {
            mode,
            deny_unknown_fields: true,
        }
        .deserialize(input)
        .map_err(|_| "Mod settings are invalid for this ruleset.")?;
        // The library retains unknown mods as explicit Unknown variants.
        // Every accepted acronym must survive as a known, ruleset-specific mod.
        if mods.iter().any(|modifier| {
            matches!(
                modifier.intermode(),
                rosu_mods::GameModIntermode::Unknown(_)
            )
        }) {
            return Err("This mod is unavailable for the requested ruleset.");
        }
        if !mods.is_valid() {
            return Err("Incompatible mods.");
        }
        if mods
            .clock_rate()
            .is_none_or(|rate| !rate.is_finite() || !(0.5..=2.0).contains(&rate))
        {
            return Err("Unsupported mod clock rate.");
        }
        Ok(Self {
            legacy_bits: mods.bits(),
            mods,
            pp_eligible: eligible,
        })
    }
}

fn number_in(value: &Value, min: f64, max: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|number| number.is_finite() && (min..=max).contains(&number))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_rates_and_difficulty_parameters_survive() {
        for mode in 0..=3 {
            let mods = ValidatedMods::parse(
                mode,
                &json!([{"acronym":"DT","settings":{"speed_change":1.2,"adjust_pitch":false}}]),
            )
            .unwrap();
            assert_eq!(mods.mods.clock_rate(), Some(1.2));
            assert_eq!(mods.legacy_bits, 64);
            assert!(mods.pp_eligible);
        }
        let mods = ValidatedMods::parse(
            0,
            &json!([{"acronym":"DA","settings":{"approach_rate":9.5,"overall_difficulty":8.0}}]),
        )
        .unwrap();
        assert!(!mods.pp_eligible);
        assert_eq!(mods.legacy_bits, 0);
        assert!(format!("{:?}", mods.mods).contains("9.5"));
    }

    #[test]
    fn malformed_unknown_duplicate_conflicting_and_ignored_settings_fail() {
        for input in [
            json!([{"acronym":"ZZ"}]),
            json!([{"acronym":"HD"},{"acronym":"HD"}]),
            json!([{"acronym":"DT"},{"acronym":"NC"}]),
            json!([{"acronym":"EZ"},{"acronym":"HR"}]),
            json!([{"acronym":"DT","settings":{"speed_change":3.0}}]),
            json!([{"acronym":"DT","settings":{"speed_change":"1.2"}}]),
            json!([{"acronym":"DT","settings":{"speed_changes":1.2}}]),
            json!([{"acronym":"FL","settings":{"size_multiplier":2.0}}]),
            json!([{"acronym":"HD","settings":{"nested":{}}}]),
            json!([{"acronym":"HD","injected":true}]),
        ] {
            assert!(ValidatedMods::parse(0, &input).is_err(), "{input}");
        }
        assert!(ValidatedMods::parse(3, &json!([{"acronym":"SO"}])).is_err());
        assert!(ValidatedMods::parse(0, &json!([{"acronym":"4K"}])).is_err());
    }

    #[test]
    fn assisted_mods_stay_out_of_ranked_pp_and_no_mod_is_valid() {
        assert!(ValidatedMods::parse(0, &json!([])).unwrap().pp_eligible);
        assert!(
            !ValidatedMods::parse(0, &json!([{"acronym":"RX"}]))
                .unwrap()
                .pp_eligible
        );
        assert!(
            !ValidatedMods::parse(0, &json!([{"acronym":"AP"}]))
                .unwrap()
                .pp_eligible
        );
    }
}
