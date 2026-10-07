//! Validate native mod parameters before performance calculation or persistence.
use crate::score_variant::ScoreVariant;
use rosu_mods::{GameMode, GameMods, serde::GameModsSeed};
use serde::de::DeserializeSeed;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct ValidatedMods {
    pub mods: GameMods,
    pub legacy_bits: u32,
    pub pp_eligible: bool,
    pub score_variant: ScoreVariant,
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
            if acronym == "RX" && ruleset == 3 {
                return Err("Relax is unavailable for osu!mania.");
            }
            if matches!(acronym, "AP" | "DA") {
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
            score_variant: ScoreVariant::from_legacy(ruleset, mods.bits())?,
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
    fn automation_and_unmodelled_difficulty_stay_out_of_pp() {
        assert!(ValidatedMods::parse(0, &json!([])).unwrap().pp_eligible);
        assert!(
            !ValidatedMods::parse(0, &json!([{"acronym":"AP"}]))
                .unwrap()
                .pp_eligible
        );
        assert!(
            !ValidatedMods::parse(0, &json!([{"acronym":"DA"}]))
                .unwrap()
                .pp_eligible
        );
        // Native autoplay remains unsupported, rather than receiving PP.
        assert!(ValidatedMods::parse(0, &json!([{"acronym":"AT"}])).is_err());
    }

    #[test]
    fn relax_projection_and_custom_speed_survive_native_validation() {
        for mode in 0..=2 {
            let rx = ValidatedMods::parse(mode, &json!([{"acronym":"RX"}])).unwrap();
            assert_eq!(rx.score_variant, ScoreVariant::Relax);
            assert_eq!(rx.legacy_bits, 128);
            assert!(rx.pp_eligible);
            assert_eq!(
                rx.score_variant,
                ScoreVariant::from_legacy(mode, rx.legacy_bits).unwrap()
            );
            let rated = ValidatedMods::parse(
                mode,
                &json!([{"acronym":"RX"},{"acronym":"DT","settings":{"speed_change":1.2}}]),
            )
            .unwrap();
            assert_eq!(rated.mods.clock_rate(), Some(1.2));
            assert_eq!(rated.legacy_bits, 128 | 64);
            assert_eq!(rated.score_variant, ScoreVariant::Relax);
            assert!(rated.pp_eligible);
            let adjusted =
                ValidatedMods::parse(mode, &json!([{"acronym":"RX"},{"acronym":"DA"}])).unwrap();
            assert_eq!(adjusted.score_variant, ScoreVariant::Relax);
            assert!(!adjusted.pp_eligible);
        }
        assert!(ValidatedMods::parse(3, &json!([{"acronym":"RX"}])).is_err());
    }

    // An original in-memory beatmap: no downloaded map or reference server code.
    fn relax_fixture() -> rosu_pp::Beatmap {
        let mut text = String::from(
            "osu file format v14\n\n[General]\nMode:0\n\n[Metadata]\nTitle:RX fixture\nArtist:Illyaverse\nCreator:Illyaverse\nVersion:Validation\n\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:8\nApproachRate:9\nSliderMultiplier:1.4\nSliderTickRate:1\n\n[TimingPoints]\n0,500,4,1,0,100,1,0\n\n[HitObjects]\n",
        );
        for index in 0..100 {
            let x = 64 + (index * 173 % 384);
            let y = 64 + (index * 97 % 256);
            let time = 1000 + index * 150;
            text.push_str(&format!("{x},{y},{time},1,0,0:0:0:0:\n"));
        }
        rosu_pp::Beatmap::from_bytes(text.as_bytes()).unwrap()
    }

    fn fixture_pp(mode: u8, mods: rosu_pp::GameMods, lazer: bool) -> f64 {
        let mode = match mode {
            0 => rosu_pp::model::mode::GameMode::Osu,
            1 => rosu_pp::model::mode::GameMode::Taiko,
            2 => rosu_pp::model::mode::GameMode::Catch,
            _ => panic!("RX fixture does not support this mode"),
        };
        let map = relax_fixture().convert(mode, &mods).unwrap();
        let attributes = rosu_pp::Difficulty::new()
            .mods(mods.clone())
            .lazer(lazer)
            .checked_calculate(&map)
            .unwrap();
        assert!(attributes.stars().is_finite());
        let combo = attributes.max_combo();
        rosu_pp::Performance::new(attributes)
            .mods(mods)
            .lazer(lazer)
            .combo(combo)
            .accuracy(98.0)
            .calculate()
            .pp()
    }

    #[test]
    fn library_relax_pp_is_finite_for_all_supported_modes_and_sources() {
        for mode in 0..=2 {
            let validated = ValidatedMods::parse(mode, &json!([{"acronym":"RX"}])).unwrap();
            let native = rosu_pp::GameMods::from(validated.mods);
            let legacy = rosu_pp::GameMods::from(validated.legacy_bits);
            let stable_pp = fixture_pp(mode, legacy, false);
            let projected_pp = fixture_pp(mode, native.clone(), false);
            let lazer_pp = fixture_pp(mode, native, true);
            for pp in [stable_pp, projected_pp, lazer_pp] {
                assert!(pp.is_finite() && pp > 0.0, "mode {mode}: {pp}");
            }
            assert!((stable_pp - projected_pp).abs() < 1e-9);
        }
    }

    #[test]
    fn custom_relax_speed_is_not_replaced_by_legacy_bits_during_pp_calculation() {
        let validated = ValidatedMods::parse(
            0,
            &json!([{"acronym":"RX"},{"acronym":"DT","settings":{"speed_change":1.2}}]),
        )
        .unwrap();
        let native = fixture_pp(0, rosu_pp::GameMods::from(validated.mods), true);
        let legacy_default = fixture_pp(0, rosu_pp::GameMods::from(validated.legacy_bits), true);
        assert!(native.is_finite() && legacy_default.is_finite());
        assert!((native - legacy_default).abs() > 1e-6);
    }
}
