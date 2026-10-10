//! Relax performance adjustment.
//!
//! The approach follows the Relax calculation used by Sunrise
//! (https://github.com/SunriseCommunity/Sunrise, MIT License, Copyright (c) 2024
//! Sunrise Community): rosu-pp's per-skill pp values are recombined with Relax
//! specific exponents, an accuracy penalty on aim, and rosu-pp's own overall
//! multiplier recovered from its result. See THIRD_PARTY_NOTICES.md.
//!
//! Only osu!standard and osu!catch are adjusted; Relax taiko keeps rosu-pp's value.

const EASY_BIT: u32 = 2;
const HARD_ROCK_BIT: u32 = 16;

/// rosu-pp values for one osu!standard score. Accuracy is a percentage (0–100).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsuRelaxInput {
    pub pp: f64,
    pub aim_pp: f64,
    pub speed_pp: f64,
    pub accuracy_pp: f64,
    pub flashlight_pp: f64,
    pub aim_difficulty: f64,
    pub aim_difficult_strain_count: f64,
    pub speed_difficult_strain_count: f64,
    pub misses: u32,
    pub accuracy: f64,
    pub legacy_mods: u32,
}

/// Accuracy percentage of an osu!standard score from its hit counts.
pub fn osu_accuracy(n300: u32, n100: u32, n50: u32, misses: u32) -> f64 {
    let total = f64::from(n300) + f64::from(n100) + f64::from(n50) + f64::from(misses);
    if total == 0.0 {
        return 100.0;
    }
    (300.0 * f64::from(n300) + 100.0 * f64::from(n100) + 50.0 * f64::from(n50)) / (300.0 * total)
        * 100.0
}

fn pp_multiplier(i: &OsuRelaxInput) -> f64 {
    let sum = (i.aim_pp.powf(1.1)
        + i.speed_pp.powf(1.1)
        + i.accuracy_pp.powf(1.1)
        + i.flashlight_pp.powf(1.1))
    .powf(1.0 / 1.1);
    i.pp / sum
}

fn miss_penalty(i: &OsuRelaxInput) -> f64 {
    if i.aim_difficulty <= 0.0 {
        return 0.0;
    }
    let denominator = 4.0 * i.aim_difficulty.ln().powf(0.94);
    if !denominator.is_finite() {
        return 0.0;
    }
    2.0 / (f64::from(i.misses) / denominator + 1.0)
}

fn streams_nerf(i: &OsuRelaxInput) -> f64 {
    (i.aim_difficult_strain_count / i.speed_difficult_strain_count * 100.0).round() / 100.0
}

/// Relax pp for osu!standard. Invalid intermediate values produce 0.
pub fn osu_relax_pp(i: &OsuRelaxInput) -> f64 {
    let mut multiplier = pp_multiplier(i);
    let mut aim = i.aim_pp;
    let mut depression = 1.0;
    if streams_nerf(i) < 1.09 {
        depression = (0.86 - (100.0 - i.accuracy) / 100.0).max(0.5);
        aim *= depression;
    }
    if i.legacy_mods & HARD_ROCK_BIT != 0 {
        multiplier *= (miss_penalty(i) / 1.85).clamp(1.0, 2.0);
    }
    let pp = (aim.powf(1.15)
        + i.speed_pp.powf(0.65 * depression)
        + i.accuracy_pp.powf(1.1)
        + i.flashlight_pp.powf(1.13))
    .powf(1.0 / 1.1)
        * multiplier;
    if pp.is_finite() { pp } else { 0.0 }
}

/// Relax pp for osu!catch: Easy lowers the rosu-pp value.
pub fn catch_relax_pp(pp: f64, legacy_mods: u32) -> f64 {
    if legacy_mods & EASY_BIT != 0 {
        pp * 0.67
    } else {
        pp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> OsuRelaxInput {
        OsuRelaxInput {
            pp: 300.0,
            aim_pp: 150.0,
            speed_pp: 120.0,
            accuracy_pp: 60.0,
            flashlight_pp: 0.0,
            aim_difficulty: 3.0,
            aim_difficult_strain_count: 100.0,
            speed_difficult_strain_count: 80.0,
            misses: 0,
            accuracy: 98.0,
            legacy_mods: 128,
        }
    }

    #[test]
    fn accuracy_follows_the_osu_formula() {
        assert_eq!(osu_accuracy(10, 0, 0, 0), 100.0);
        assert_eq!(osu_accuracy(0, 0, 0, 0), 100.0);
        assert!((osu_accuracy(8, 1, 1, 0) - 85.0).abs() < 1e-9);
    }

    #[test]
    fn stream_maps_skip_the_accuracy_penalty_and_others_apply_it() {
        let mut stream = input();
        stream.aim_difficult_strain_count = 120.0; // ratio 1.5
        let mut jumps = input();
        jumps.aim_difficult_strain_count = 80.0; // ratio 1.0
        assert!(osu_relax_pp(&jumps) < osu_relax_pp(&stream));
    }

    #[test]
    fn lower_accuracy_lowers_pp_but_the_penalty_has_a_floor() {
        let mut high = input();
        high.aim_difficult_strain_count = 80.0;
        let mut low = high;
        low.accuracy = 90.0;
        let mut floor = high;
        floor.accuracy = 0.0;
        let mut floor_again = high;
        floor_again.accuracy = -50.0;
        assert!(osu_relax_pp(&low) < osu_relax_pp(&high));
        assert_eq!(osu_relax_pp(&floor), osu_relax_pp(&floor_again));
    }

    #[test]
    fn hard_rock_only_helps_and_is_capped_at_double() {
        let base = input();
        let mut hr = input();
        hr.legacy_mods |= HARD_ROCK_BIT;
        assert!(osu_relax_pp(&hr) >= osu_relax_pp(&base));
        hr.misses = 0;
        hr.aim_difficulty = 1.0001;
        assert!(osu_relax_pp(&hr).is_finite());
        assert!(osu_relax_pp(&hr) <= osu_relax_pp(&base) * 2.0 + 1e-9);
    }

    #[test]
    fn invalid_inputs_yield_zero_instead_of_nan() {
        let mut bad = input();
        bad.pp = 0.0;
        bad.aim_pp = 0.0;
        bad.speed_pp = 0.0;
        bad.accuracy_pp = 0.0;
        assert_eq!(osu_relax_pp(&bad), 0.0);
    }

    #[test]
    fn catch_easy_is_reduced() {
        assert_eq!(catch_relax_pp(100.0, 128), 100.0);
        assert!((catch_relax_pp(100.0, 128 | EASY_BIT) - 67.0).abs() < 1e-12);
    }
}
