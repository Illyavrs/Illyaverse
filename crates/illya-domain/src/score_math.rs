//! Original arithmetic for the pinned osu! scoring contracts.
//!
//! These functions consume prepared statistics/attributes. They neither parse
//! beatmaps, simulate replays, select mod multipliers, nor verify a play.
//! Legacy ScoreV1 conversion is an estimate, not a native gameplay reconstruction.
//! See docs/score-normalization.md for derivation and the summary counterexample.

use serde::{Deserialize, Serialize};

pub const UPSTREAM_SCORE_COMMIT: &str = "6359741babba4ced42da1fcda738c35370fdf3f4";
/// Illyaverse arithmetic versions, independent of replay/container versions.
pub const NATIVE_SCORE_VERSION: u32 = 1;
pub const NATIVE_CLASSIC_VERSION: u32 = 1;
pub const LEGACY_ESTIMATE_VERSION: u32 = 1;
pub const STANDARDISED_BASE_SCORE: u64 = 1_000_000;

const EXACT_INTEGER_LIMIT: u64 = 1 << 53;
const SCORE_LIMIT_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreProvenance {
    NativeFormula,
    LegacyEstimate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardisedScore {
    pub without_mods: u64,
    pub total: u64,
    pub provenance: ScoreProvenance,
    pub version: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatchStatistics {
    pub maximum_fruit: u32,
    pub maximum_tiny_droplets: u32,
    pub tiny_droplet_hits: u32,
}

impl CatchStatistics {
    fn fractions(self) -> Result<(f64, f64), &'static str> {
        if self.tiny_droplet_hits > self.maximum_tiny_droplets {
            return Err("tiny droplet hits exceed maximum");
        }
        let count = u64::from(self.maximum_fruit) + u64::from(self.maximum_tiny_droplets);
        let share = if count == 0 {
            0.0
        } else {
            f64::from(self.maximum_tiny_droplets) / count as f64
        };
        let hit = if self.maximum_tiny_droplets == 0 {
            0.0
        } else {
            f64::from(self.tiny_droplet_hits) / f64::from(self.maximum_tiny_droplets)
        };
        Ok((share, hit))
    }
}

/// Ratios derived from ordered native judgments and map autoplay maxima.
/// Providing these values is not evidence that they were server-verified.
#[derive(Debug, Clone, Copy)]
pub struct NativeScorePortions {
    pub accuracy: f64,
    pub combo_progress: f64,
    pub accuracy_progress: f64,
    pub bonus: f64,
    pub catch: CatchStatistics,
}

/// Caller-supplied legacy map attributes, calculated with the same pinned map
/// conversion/simulator version. Mania's multiplier context is still external.
#[derive(Debug, Clone, Copy)]
pub struct LegacyAttributes {
    pub accuracy_score: u64,
    pub combo_score: u64,
    pub bonus_score: u64,
    pub bonus_score_ratio: f64,
    pub max_combo: u32,
}

/// A complete, passed stable ScoreV1 summary. Accuracy must already use native
/// judgment weights (including mania Perfect=305) and reconstructed maxima.
/// ScoreV2 must never be passed to this estimator.
#[derive(Debug, Clone, Copy)]
pub struct LegacySummary {
    pub total_score: u64,
    pub accuracy: f64,
    pub max_combo: u32,
    pub misses: u32,
    pub catch: CatchStatistics,
    pub passed: bool,
}

fn ruleset(value: u8) -> Result<u8, &'static str> {
    if value <= 3 {
        Ok(value)
    } else {
        Err("unsupported score ruleset")
    }
}

fn nonnegative(value: f64) -> Result<(), &'static str> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err("score input must be finite and nonnegative")
    }
}

fn fraction(value: f64) -> Result<(), &'static str> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err("score fraction is outside zero to one")
    }
}

fn exact_integer(value: u64) -> Result<f64, &'static str> {
    if value <= EXACT_INTEGER_LIMIT {
        Ok(value as f64)
    } else {
        Err("integer score input exceeds exact arithmetic range")
    }
}

/// Match the native nearest-even tie rule without saturating an invalid cast.
/// Results must fit PostgreSQL's signed BIGINT score storage.
pub fn round_even_score(value: f64) -> Result<u64, &'static str> {
    nonnegative(value)?;
    let rounded = value.round_ties_even();
    if rounded < SCORE_LIMIT_EXCLUSIVE {
        Ok(rounded as u64)
    } else {
        Err("score arithmetic overflow")
    }
}

fn finish(
    value: f64,
    multiplier: f64,
    provenance: ScoreProvenance,
    version: u32,
) -> Result<StandardisedScore, &'static str> {
    nonnegative(multiplier)?;
    let without_mods = round_even_score(value)?;
    let total = round_even_score(without_mods as f64 * multiplier)?;
    Ok(StandardisedScore {
        without_mods,
        total,
        provenance,
        version,
    })
}

fn formula(
    mode: u8,
    accuracy: f64,
    combo: f64,
    progress: f64,
    bonus: f64,
    catch: CatchStatistics,
) -> Result<f64, &'static str> {
    let value = match mode {
        0 => 500_000.0 * accuracy * combo + 500_000.0 * accuracy.powi(5) * progress,
        1 => 250_000.0 * combo + 750_000.0 * accuracy.powf(3.6) * progress,
        2 => {
            let (share, hit) = catch.fractions()?;
            (1_000_000.0 - 400_000.0 * share) * combo + 400_000.0 * share * hit
        }
        3 => 150_000.0 * combo + 850_000.0 * accuracy.powf(2.0 + 2.0 * accuracy) * progress,
        _ => return Err("unsupported score ruleset"),
    } + bonus;
    nonnegative(value)?;
    Ok(value)
}

pub fn native_score(
    mode: u8,
    portions: NativeScorePortions,
    multiplier: f64,
) -> Result<StandardisedScore, &'static str> {
    ruleset(mode)?;
    fraction(portions.accuracy)?;
    fraction(portions.combo_progress)?;
    fraction(portions.accuracy_progress)?;
    nonnegative(portions.bonus)?;
    let value = formula(
        mode,
        portions.accuracy,
        portions.combo_progress,
        portions.accuracy_progress,
        portions.bonus,
        portions.catch,
    )?;
    finish(
        value,
        multiplier,
        ScoreProvenance::NativeFormula,
        NATIVE_SCORE_VERSION,
    )
}

/// Classic display arithmetic only: `basic_maximum` is the sum of the six basic
/// maximum hit-result categories, excluding ticks, bonus and legacy combo padding.
pub fn classic_score(
    mode: u8,
    standardised_total: u64,
    basic_maximum: u64,
) -> Result<u64, &'static str> {
    ruleset(mode)?;
    if basic_maximum > i32::MAX as u64 {
        return Err("basic judgment count exceeds native integer range");
    }
    let score = exact_integer(standardised_total)?;
    let objects = exact_integer(basic_maximum)?;
    let value = match mode {
        0 => (32.57 * objects.powi(2) + 100_000.0) * score / 1_000_000.0,
        1 => {
            // The contract forms an int32 coefficient, multiplies it by an
            // int64 total, then converts that product to double for division.
            // Refuse the unchecked-overflow domain rather than emulate it.
            let coefficient = basic_maximum
                .checked_mul(1109)
                .and_then(|value| value.checked_add(100_000))
                .filter(|value| *value <= i32::MAX as u64)
                .ok_or("taiko classic coefficient overflow")?;
            let product = coefficient
                .checked_mul(standardised_total)
                .filter(|value| *value <= i64::MAX as u64)
                .ok_or("taiko classic product overflow")?;
            product as f64 / 1_000_000.0
        }
        2 => 21.62 * (score / 1_000_000.0 * objects).powi(2) + score / 10.0,
        3 => score,
        _ => unreachable!(),
    };
    round_even_score(value)
}

fn osu_combo_estimate(
    maximum: u32,
    longest: u32,
    misses: u32,
    legacy_ratio: f64,
    accuracy: f64,
) -> Result<f64, &'static str> {
    let n = f64::from(maximum);
    let length = f64::from(longest);
    let target = (n.powi(2) * legacy_ratio / accuracy).max(length.powi(2));
    nonnegative(target)?;
    let repetitions = (target / length.powi(2)).floor();
    let residual = (target - repetitions * length.powi(2)).max(0.0);
    let score_estimate = repetitions * length.powf(1.5) + residual.powf(0.75);
    let remaining = f64::from(maximum - longest - misses);
    let segment = if remaining > 0.0 {
        (target - length.powi(2)) / remaining
    } else {
        0.0
    };
    let object_estimate = length.powf(1.5) + remaining * segment.sqrt();
    nonnegative(score_estimate)?;
    nonnegative(object_estimate)?;
    let ceiling = n.powf(1.5);
    let first = score_estimate.clamp(0.0, ceiling);
    let second = object_estimate.clamp(0.0, ceiling);
    let low = first.min(second);
    let high = first.max(second);
    Ok((0.3 * low + 0.7 * high).min(1.2 * (low + high) / 2.0) / ceiling)
}

fn catch_combo_integral(combo: u32) -> f64 {
    if combo == 0 {
        return 1.0;
    }
    if combo <= 2 {
        return 0.5 * f64::from(combo);
    }
    let cap = f64::from(combo.min(200));
    let curved = 1.0 + (cap * (cap.ln() - 1.0) + 2.0 - 4.0f64.ln()) / 4.0f64.ln();
    curved + f64::from(combo.saturating_sub(200)) * 200.0f64.ln() / 4.0f64.ln()
}

fn catch_loss_integral(combo: u32) -> f64 {
    let cap = f64::from(combo.min(200));
    cap * (1.0 + 200.0f64.ln() - cap.ln()) / 4.0f64.ln()
}

fn catch_combo_estimate(maximum: u32, longest: u32, misses: u32) -> Result<f64, &'static str> {
    if maximum == 0 || maximum == longest {
        return Ok(1.0);
    }
    if longest == 0 {
        return Ok(0.0);
    }
    if misses == 0 {
        return Err("non-full catch combo requires misses");
    }
    let remainder = maximum - longest - misses;
    let length = remainder / misses;
    let maximum_score = catch_combo_integral(maximum);
    let loss = match remainder.checked_div(length) {
        Some(segments) => {
            let tail = remainder % length;
            f64::from(segments) * catch_loss_integral(length)
                + if tail > 0 {
                    catch_loss_integral(tail)
                } else {
                    0.0
                }
        }
        None => maximum_score - catch_combo_integral(longest),
    };
    Ok(1.0 - (loss / maximum_score).clamp(0.0, 1.0))
}

/// Calculate the pinned official-style ScoreV1 estimate from prepared attributes.
/// This deliberately does not select/create map attributes or mod multipliers.
pub fn estimate_legacy_score(
    mode: u8,
    attributes: LegacyAttributes,
    summary: LegacySummary,
    legacy_multiplier: f64,
    native_multiplier: f64,
) -> Result<StandardisedScore, &'static str> {
    ruleset(mode)?;
    if !summary.passed {
        return Err("incomplete legacy plays are unsupported");
    }
    fraction(summary.accuracy)?;
    nonnegative(legacy_multiplier)?;
    nonnegative(native_multiplier)?;
    nonnegative(attributes.bonus_score_ratio)?;
    let accuracy_score = exact_integer(attributes.accuracy_score)?;
    let combo_score = exact_integer(attributes.combo_score)?;
    let bonus_score = exact_integer(attributes.bonus_score)?;
    let raw = exact_integer(summary.total_score)?;
    if mode != 3
        && u64::from(summary.max_combo) + u64::from(summary.misses)
            > u64::from(attributes.max_combo)
    {
        return Err("legacy combo summary exceeds map maximum");
    }
    let scaled_combo = round_even_score(combo_score * legacy_multiplier)? as f64;
    let denominator = scaled_combo + bonus_score;
    nonnegative(denominator)?;
    let ratio = if denominator > 0.0 {
        (raw - accuracy_score * summary.accuracy).max(0.0) / denominator
    } else if legacy_multiplier == 0.0 {
        0.0
    } else {
        1.0
    };
    let bonus = (raw - accuracy_score - scaled_combo).max(0.0) * attributes.bonus_score_ratio;
    nonnegative(ratio)?;
    nonnegative(bonus)?;
    let value = match mode {
        0 if summary.max_combo == 0 || summary.accuracy == 0.0 => {
            500_000.0 * summary.accuracy.powi(5) + bonus
        }
        0 if denominator == 0.0 => 500_000.0 * ratio + 500_000.0 * summary.accuracy.powi(5) + bonus,
        0 => formula(
            mode,
            summary.accuracy,
            osu_combo_estimate(
                attributes.max_combo,
                summary.max_combo,
                summary.misses,
                ratio,
                summary.accuracy,
            )?,
            1.0,
            bonus,
            summary.catch,
        )?,
        2 => formula(
            mode,
            summary.accuracy,
            catch_combo_estimate(attributes.max_combo, summary.max_combo, summary.misses)?,
            1.0,
            bonus,
            summary.catch,
        )?,
        _ => formula(mode, summary.accuracy, ratio, 1.0, bonus, summary.catch)?,
    };
    finish(
        value,
        native_multiplier,
        ScoreProvenance::LegacyEstimate,
        LEGACY_ESTIMATE_VERSION,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portions() -> NativeScorePortions {
        NativeScorePortions {
            accuracy: 1.0,
            combo_progress: 1.0,
            accuracy_progress: 1.0,
            bonus: 0.0,
            catch: CatchStatistics::default(),
        }
    }
    fn attributes() -> LegacyAttributes {
        LegacyAttributes {
            accuracy_score: 2700,
            combo_score: 1680,
            bonus_score: 0,
            bonus_score_ratio: 0.0,
            max_combo: 9,
        }
    }
    fn summary() -> LegacySummary {
        LegacySummary {
            total_score: 2460,
            accuracy: 7.0 / 9.0,
            max_combo: 5,
            misses: 2,
            catch: CatchStatistics::default(),
            passed: true,
        }
    }

    #[test]
    fn native_perfect_all_modes_and_two_rounding_steps() {
        for mode in 0..=3 {
            let result = native_score(mode, portions(), 1.0).unwrap();
            assert_eq!(result.without_mods, 1_000_000);
            assert_eq!(result.total, 1_000_000);
            assert_eq!(result.provenance, ScoreProvenance::NativeFormula);
            assert_eq!(result.version, NATIVE_SCORE_VERSION);
        }
        assert_eq!(native_score(0, portions(), 0.985).unwrap().total, 985_000);
        let result = native_score(
            3,
            NativeScorePortions {
                accuracy: 0.0,
                combo_progress: 0.0,
                accuracy_progress: 0.0,
                bonus: 2.5,
                ..portions()
            },
            1.25,
        )
        .unwrap();
        assert_eq!((result.without_mods, result.total), (2, 2));
    }

    #[test]
    fn native_partial_progress_and_catch_tiny_partition() {
        assert_eq!(
            native_score(
                1,
                NativeScorePortions {
                    accuracy: 0.5,
                    combo_progress: 0.5,
                    ..portions()
                },
                1.0
            )
            .unwrap()
            .total,
            186_852
        );
        assert_eq!(
            native_score(
                0,
                NativeScorePortions {
                    accuracy_progress: 0.5,
                    ..portions()
                },
                1.0
            )
            .unwrap()
            .total,
            750_000
        );
        assert_eq!(
            native_score(
                2,
                NativeScorePortions {
                    catch: CatchStatistics {
                        maximum_fruit: 100,
                        maximum_tiny_droplets: 100,
                        tiny_droplet_hits: 75
                    },
                    ..portions()
                },
                1.0
            )
            .unwrap()
            .total,
            950_000
        );
        assert_eq!(
            native_score(
                2,
                NativeScorePortions {
                    catch: CatchStatistics {
                        maximum_fruit: 0,
                        maximum_tiny_droplets: 100,
                        tiny_droplet_hits: 0
                    },
                    ..portions()
                },
                1.0
            )
            .unwrap()
            .total,
            600_000
        );
    }

    #[test]
    fn classic_all_rulesets_and_ties_match_contract() {
        let expected = [425_700, 210_900, 316_200, 1_000_000];
        for (mode, expected) in expected.into_iter().enumerate() {
            assert_eq!(classic_score(mode as u8, 1_000_000, 100), Ok(expected));
        }
        assert_eq!(classic_score(0, 1_000_000, 2), Ok(100_130));
        assert_eq!(classic_score(1, 1_000_000, 2), Ok(102_218));
        assert_eq!(classic_score(2, 1_000_000, 2), Ok(100_086));
        let bonus_expected = [425_785, 210_942, 316_306, 1_000_200];
        for (mode, expected) in bonus_expected.into_iter().enumerate() {
            assert_eq!(classic_score(mode as u8, 1_000_200, 100), Ok(expected));
        }
        for mode in 0..=2 {
            assert_eq!(classic_score(mode, 5, 0), Ok(0));
            assert_eq!(classic_score(mode, 15, 0), Ok(2));
        }
        assert_eq!(classic_score(3, 15, 0), Ok(15));
        assert_eq!(round_even_score(2.5), Ok(2));
        assert_eq!(round_even_score(3.5), Ok(4));
    }

    #[test]
    fn classic_rejects_native_integer_overflow_before_float_conversion() {
        for mode in 0..=3 {
            assert!(classic_score(mode, 1, i32::MAX as u64 + 1).is_err());
        }
        let count_limit = (i32::MAX as u64 - 100_000) / 1109;
        assert_eq!(count_limit, 1_936_324);
        assert_eq!(classic_score(1, 1_000_000, count_limit), Ok(2_147_483_316));
        assert!(classic_score(1, 1_000_000, count_limit + 1).is_err());
        let score_limit = i64::MAX as u64 / 210_900;
        assert_eq!(score_limit, 43_733_390_407_087);
        assert_eq!(classic_score(1, score_limit, 100), Ok(9_223_372_036_855));
        assert!(classic_score(1, score_limit + 1, 100).is_err());
        assert!(classic_score(0, 1_000_000, i32::MAX as u64).is_err());
        assert!(classic_score(2, 1_000_000, i32::MAX as u64).is_err());
    }

    #[test]
    #[ignore = "requires separately generated pinned native DLL score vectors"]
    fn native_classic_contract_vectors() {
        let path = std::env::var("ILLYA_NATIVE_SCORE_VECTORS_PATH")
            .expect("set ILLYA_NATIVE_SCORE_VECTORS_PATH to the native probe JSON");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            fixture["pinned_source_commit"],
            "6cbcae7fbb764839a66c1adf564c2e1679242072"
        );
        assert_eq!(
            fixture["loaded_game_dll_sha256"],
            "E19D65CF648A575D1CA7061A8922016795D57CA769CA05583984D68F09EADDD5"
        );
        let mut compared = 0;
        let mut rejected = 0;
        for vector in fixture["vectors"].as_array().unwrap() {
            let domain = vector["server_input_domain"].as_str().unwrap();
            let inputs = vector["ruleset_id"]
                .as_u64()
                .and_then(|mode| u8::try_from(mode).ok())
                .zip(vector["standardised_input"].as_u64())
                .zip(vector["maximum_basic_judgements"].as_u64());
            if domain.starts_with("valid") {
                let ((mode, score), count) = inputs.unwrap();
                assert_eq!(
                    classic_score(mode, score, count),
                    Ok(vector["native_classic_output"].as_u64().unwrap()),
                    "{vector}"
                );
                compared += 1;
            } else if domain.starts_with("reject") {
                if let Some(((mode, score), count)) = inputs {
                    assert!(classic_score(mode, score, count).is_err(), "{vector}");
                }
                // Negative source values are unrepresentable in the typed API.
                rejected += 1;
            }
        }
        assert!(compared >= 62);
        assert!(rejected >= 23);
    }

    #[test]
    fn equal_legacy_summaries_do_not_reconstruct_native_combo_order() {
        let maximum = (1..=9).map(|n| f64::from(n).sqrt()).sum::<f64>();
        let score_for = |runs: &[u32]| {
            let achieved = runs
                .iter()
                .flat_map(|n| 1..=*n)
                .map(|n| f64::from(n).sqrt())
                .sum::<f64>();
            native_score(
                0,
                NativeScorePortions {
                    accuracy: 7.0 / 9.0,
                    combo_progress: achieved / maximum,
                    ..portions()
                },
                1.0,
            )
            .unwrap()
            .without_mods
        };
        assert_eq!(score_for(&[2, 5]), 359_793);
        assert_eq!(score_for(&[1, 1, 5]), 351_450);
        let estimated = estimate_legacy_score(0, attributes(), summary(), 1.0, 0.985).unwrap();
        assert_eq!(
            (estimated.without_mods, estimated.total),
            (303_348, 298_798)
        );
        assert_eq!(estimated.provenance, ScoreProvenance::LegacyEstimate);
        assert_eq!(estimated.version, LEGACY_ESTIMATE_VERSION);
    }

    #[test]
    fn legacy_four_mode_estimates_and_zero_denominator() {
        for mode in 0..=3 {
            let perfect = LegacySummary {
                total_score: 4380,
                accuracy: 1.0,
                max_combo: 9,
                misses: 0,
                ..summary()
            };
            assert_eq!(
                estimate_legacy_score(mode, attributes(), perfect, 1.0, 1.0)
                    .unwrap()
                    .total,
                1_000_000
            );
        }
        let mania = LegacyAttributes {
            accuracy_score: 0,
            combo_score: 1_000_000,
            max_combo: 0,
            ..attributes()
        };
        let input = LegacySummary {
            total_score: 900_000,
            accuracy: 300.0 / 305.0,
            ..summary()
        };
        assert_eq!(
            estimate_legacy_score(3, mania, input, 1.0, 1.0)
                .unwrap()
                .total,
            931_049
        );
        let taiko = LegacySummary {
            total_score: 2_190,
            accuracy: 0.5,
            ..summary()
        };
        assert_eq!(
            estimate_legacy_score(1, attributes(), taiko, 1.0, 1.0)
                .unwrap()
                .total,
            186_852
        );
        let catch = LegacySummary {
            max_combo: 9,
            misses: 0,
            catch: CatchStatistics {
                maximum_fruit: 100,
                maximum_tiny_droplets: 100,
                tiny_droplet_hits: 75,
            },
            ..summary()
        };
        assert_eq!(
            estimate_legacy_score(2, attributes(), catch, 1.0, 1.0)
                .unwrap()
                .total,
            950_000
        );
        assert_eq!(
            estimate_legacy_score(2, attributes(), summary(), 1.0, 1.0)
                .unwrap()
                .total,
            13_964
        );
        assert_eq!(catch_combo_estimate(0, 0, 0), Ok(1.0));
        assert_eq!(catch_combo_estimate(9, 0, 9), Ok(0.0));
        assert!(
            (catch_combo_estimate(9, 1, 8).unwrap() - 0.5 / catch_combo_integral(9)).abs() < 1e-14
        );
        assert!(catch_combo_estimate(1000, 600, 1).unwrap() > 0.0);
        let empty_combo = LegacyAttributes {
            combo_score: 0,
            ..attributes()
        };
        let perfect = LegacySummary {
            accuracy: 1.0,
            max_combo: 9,
            misses: 0,
            ..summary()
        };
        assert_eq!(
            estimate_legacy_score(0, empty_combo, perfect, 1.0, 1.0)
                .unwrap()
                .total,
            1_000_000
        );
        assert_eq!(
            estimate_legacy_score(0, empty_combo, perfect, 0.0, 0.0)
                .unwrap()
                .without_mods,
            500_000
        );
    }

    #[test]
    fn bonuses_and_legacy_ratio_are_not_capped_at_one_million() {
        let attrs = LegacyAttributes {
            accuracy_score: 0,
            combo_score: 1_000_000,
            bonus_score: 100,
            bonus_score_ratio: 0.2,
            max_combo: 0,
        };
        let result = estimate_legacy_score(
            3,
            attrs,
            LegacySummary {
                total_score: 1_001_000,
                accuracy: 1.0,
                ..summary()
            },
            1.0,
            1.0,
        )
        .unwrap();
        assert!(result.without_mods > 1_000_000);
        assert_eq!(
            native_score(
                0,
                NativeScorePortions {
                    bonus: 500.0,
                    ..portions()
                },
                1.0
            )
            .unwrap()
            .total,
            1_000_500
        );
    }

    #[test]
    fn invalid_inputs_never_saturate_or_produce_successful_nan() {
        for accuracy in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(
                native_score(
                    0,
                    NativeScorePortions {
                        accuracy,
                        ..portions()
                    },
                    1.0
                )
                .is_err()
            );
            assert!(
                estimate_legacy_score(
                    0,
                    attributes(),
                    LegacySummary {
                        accuracy,
                        ..summary()
                    },
                    1.0,
                    1.0
                )
                .is_err()
            );
        }
        assert!(native_score(0, portions(), f64::MAX).is_err());
        assert!(native_score(4, portions(), 1.0).is_err());
        assert!(classic_score(4, 100, 100).is_err());
        assert!(classic_score(0, 1 << 54, 100).is_err());
        assert!(classic_score(0, 1_000_000, u64::MAX).is_err());
        assert!(round_even_score(SCORE_LIMIT_EXCLUSIVE).is_err());
        assert!(round_even_score(f64::NEG_INFINITY).is_err());
        assert!(
            estimate_legacy_score(
                0,
                attributes(),
                LegacySummary {
                    passed: false,
                    ..summary()
                },
                1.0,
                1.0
            )
            .is_err()
        );
        assert!(
            estimate_legacy_score(
                0,
                attributes(),
                LegacySummary {
                    max_combo: 8,
                    ..summary()
                },
                1.0,
                1.0
            )
            .is_err()
        );
        assert!(
            estimate_legacy_score(
                2,
                attributes(),
                LegacySummary {
                    misses: 0,
                    ..summary()
                },
                1.0,
                1.0
            )
            .is_err()
        );
        assert!(
            native_score(
                2,
                NativeScorePortions {
                    catch: CatchStatistics {
                        tiny_droplet_hits: 1,
                        ..Default::default()
                    },
                    ..portions()
                },
                1.0
            )
            .is_err()
        );
    }
}
