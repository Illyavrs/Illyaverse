//! Derived display scales and native-summary integrity checks.
//! Client-reported total/accuracy remain unverified until separate moderation.

use crate::{ApiError, ApiResult, legacy_scoring, scores::Submission};
use illya_domain::{score_math, score_mods::ValidatedMods};
use resonance_protocol::lazer::SoloScoreSubmission;
use rosu_pp::any::DifficultyAttributes;
use serde_json::{Value, json};

const BASIC: [&str; 6] = ["miss", "meh", "ok", "good", "great", "perfect"];
const RESULTS: [&str; 17] = [
    "none",
    "miss",
    "meh",
    "ok",
    "good",
    "great",
    "perfect",
    "small_tick_miss",
    "small_tick_hit",
    "large_tick_miss",
    "large_tick_hit",
    "small_bonus",
    "large_bonus",
    "ignore_miss",
    "ignore_hit",
    "slider_tail_hit",
    "legacy_combo_increase",
];

#[derive(Debug, Clone)]
pub(crate) struct NativeScales {
    pub standardised_score: Option<i64>,
    pub standardised_without_mods: Option<i64>,
    pub classic_score: Option<i64>,
    pub native_submission: Option<Value>,
    pub native_basic_maximum: Option<i32>,
    pub legacy_attributes: Option<Value>,
    pub status: &'static str,
    pub version: Option<String>,
    pub reason: Option<String>,
}

impl NativeScales {
    pub(crate) fn unavailable(reason: &str) -> Self {
        Self {
            standardised_score: None,
            standardised_without_mods: None,
            classic_score: None,
            native_submission: None,
            native_basic_maximum: None,
            legacy_attributes: None,
            status: "unavailable",
            version: Some("score_scale.unavailable.v1".into()),
            reason: Some(reason.chars().take(200).collect()),
        }
    }
}

/// Derive native basic opportunities from the same converted map and validated
/// mods used for PP. Native mania counts every hold head and tail, including CL.
pub(crate) fn basic_n(attributes: &DifficultyAttributes) -> ApiResult<u32> {
    let count = match attributes {
        DifficultyAttributes::Osu(a) => a
            .n_circles
            .checked_add(a.n_sliders)
            .and_then(|n| n.checked_add(a.n_spinners)),
        DifficultyAttributes::Taiko(a) => Some(a.max_combo),
        DifficultyAttributes::Catch(a) => Some(a.n_fruits),
        DifficultyAttributes::Mania(a) => a.n_objects.checked_add(a.n_hold_notes),
    }
    .ok_or_else(|| ApiError::bad("Native judgment count overflow."))?;
    if count > 1_000_000 {
        return Err(ApiError::bad("Native judgments exceed processing limits."));
    }
    Ok(count)
}

fn basic_sum(results: &std::collections::BTreeMap<String, u32>) -> u64 {
    BASIC
        .iter()
        .map(|key| u64::from(results.get(*key).copied().unwrap_or(0)))
        .sum()
}

/// Checks the binding before any input can be treated as a stored native context.
/// Map-derived maxima are checked separately after difficulty calculation.
pub(crate) fn validate_native_binding(score: &Submission) -> ApiResult<()> {
    let Some(native) = &score.native_submission else {
        return Ok(());
    };
    native
        .validate()
        .map_err(|_| ApiError::bad("Invalid native submission context."))?;
    if !(0..=3).contains(&score.mode)
        || !score.accuracy.is_finite()
        || native.ruleset_id != score.mode as u8
        || i64::try_from(native.total_score).ok() != Some(score.score)
        || (native.accuracy * 100.0 - score.accuracy).abs() > 1e-8
        || native.max_combo != score.max_combo
        || native.passed != score.passed
        || native.legacy_hit_counts() != json!(score.hit_counts)
    {
        return Err(ApiError::bad(
            "Native context does not match the score summary.",
        ));
    }
    let supplied = score
        .api_mods
        .as_ref()
        .ok_or_else(|| ApiError::bad("Native context requires structured mods."))?;
    let summary_mods = ValidatedMods::parse(native.ruleset_id, supplied).map_err(ApiError::bad)?;
    let context_mods =
        ValidatedMods::parse(native.ruleset_id, &json!(native.mods)).map_err(ApiError::bad)?;
    if summary_mods.mods != context_mods.mods || context_mods.legacy_bits != score.mods {
        return Err(ApiError::bad("Native context modifiers do not match."));
    }
    if native
        .statistics
        .keys()
        .chain(native.maximum_statistics.keys())
        .any(|key| !RESULTS.contains(&key.as_str()))
    {
        return Err(ApiError::bad("Unknown native judgment category."));
    }
    Ok(())
}

/// Build the accuracy expected by the pinned migration formula. Stable mania
/// has one result per note/hold; native mania's map maximum has both head/tail.
/// Catch's raw miss count combines fruit and large-droplet misses, while tiny
/// misses live in `katu`. Validate those partitions against the converted map.
fn legacy_summary(
    score: &Submission,
    attributes: &DifficultyAttributes,
) -> ApiResult<score_math::LegacySummary> {
    let h = &score.hit_counts;
    let a = u64::from(h.n300);
    let b = u64::from(h.n100);
    let c = u64::from(h.n50);
    let m = u64::from(h.misses);
    let g = u64::from(h.geki);
    let k = u64::from(h.katu);
    let complete = |actual: u64, maximum: u64| -> ApiResult<()> {
        if actual > maximum || (score.passed && actual != maximum) {
            return Err(ApiError::bad("Legacy judgments do not match this beatmap."));
        }
        Ok(())
    };
    let mut catch = score_math::CatchStatistics::default();
    let (numerator, denominator) = match attributes {
        DifficultyAttributes::Osu(_) => {
            complete(a + b + c + m, u64::from(basic_n(attributes)?))?;
            (300 * a + 100 * b + 50 * c, 300 * (a + b + c + m))
        }
        DifficultyAttributes::Taiko(attrs) => {
            complete(a + b + m, u64::from(attrs.max_combo))?;
            (300 * a + 150 * b, 300 * (a + b + m))
        }
        DifficultyAttributes::Catch(attrs) => {
            if a > u64::from(attrs.n_fruits) || b > u64::from(attrs.n_droplets) {
                return Err(ApiError::bad(
                    "Legacy fruit/droplet counts exceed this beatmap.",
                ));
            }
            complete(
                a + b + m,
                u64::from(attrs.n_fruits) + u64::from(attrs.n_droplets),
            )?;
            complete(c + k, u64::from(attrs.n_tiny_droplets))?;
            catch = score_math::CatchStatistics {
                maximum_fruit: attrs.n_fruits,
                maximum_tiny_droplets: attrs.n_tiny_droplets,
                tiny_droplet_hits: h.n50,
            };
            (a + b + c, a + b + c + m + k)
        }
        DifficultyAttributes::Mania(attrs) => {
            complete(a + b + c + m + g + k, u64::from(attrs.n_objects))?;
            // Native migration accuracy distinguishes Perfect (305) from Great
            // (300). The existing public stable accuracy display remains 300/300.
            (
                305 * g + 300 * a + 200 * k + 100 * b + 50 * c,
                305 * (a + b + c + m + g + k),
            )
        }
    };
    if denominator == 0 {
        return Err(ApiError::bad("Legacy summary has no basic judgments."));
    }
    if score.mode != 3 && u64::from(score.max_combo) + m > u64::from(attributes.max_combo()) {
        return Err(ApiError::bad(
            "Legacy combo and misses exceed this beatmap.",
        ));
    }
    Ok(score_math::LegacySummary {
        total_score: u64::try_from(score.score)
            .map_err(|_| ApiError::bad("Invalid legacy total score."))?,
        accuracy: numerator as f64 / denominator as f64,
        max_combo: score.max_combo,
        misses: h.misses,
        catch,
        passed: score.passed,
    })
}

/// Estimate a passed ScoreV1 score from the original map and a complete stable
/// summary. This is versioned official-style estimation, never replay evidence.
pub(crate) fn legacy_scales(
    score: &Submission,
    original_map: &rosu_pp::Beatmap,
    attributes: &DifficultyAttributes,
) -> ApiResult<NativeScales> {
    let summary = legacy_summary(score, attributes)?;
    let expected = basic_n(attributes)?;
    let scorev2 = score.mods & (1 << 29) != 0;
    // ScoreV2 preserves raw total and uses a separate source multiplier; its
    // original map maxima are still useful, without feeding raw V2 into V1.
    let context =
        match legacy_scoring::simulate_map(original_map, score.mode as u8, score.mods & !(1 << 29))
        {
            Ok(context) => context,
            Err(reason) => return Ok(NativeScales::unavailable(reason)),
        };
    let source_multiplier = if scorev2 {
        match legacy_scoring::source_multiplier(original_map, score.mode as u8, score.mods) {
            Ok(multiplier) => multiplier,
            Err(reason) => return Ok(NativeScales::unavailable(reason)),
        }
    } else {
        context.legacy_multiplier
    };
    let cached = json!({
        "accuracy_score":context.attributes.accuracy_score,
        "combo_score":context.attributes.combo_score,
        "bonus_score":context.attributes.bonus_score,
        "bonus_score_ratio":context.attributes.bonus_score_ratio,
        "max_combo":context.attributes.max_combo,
        "maximum_statistics":context.maximum_statistics,
        "legacy_multiplier":source_multiplier,
        "native_multiplier":context.native_multiplier,
        "simulator_version":legacy_scoring::SIMULATOR_VERSION,
        "simulator_client_commit":legacy_scoring::SIMULATOR_CLIENT_COMMIT,
        "formula_client_commit":score_math::UPSTREAM_SCORE_COMMIT,
    });
    let unavailable = |reason: &str| {
        let mut value = NativeScales::unavailable(reason);
        value.legacy_attributes = Some(cached.clone());
        value.native_basic_maximum = Some(expected as i32);
        value
    };
    if !score.passed {
        return Ok(unavailable("incomplete_legacy_score"));
    }
    let (standardised, without_mods, status) = if scorev2 {
        if source_multiplier <= 0.0 || !source_multiplier.is_finite() {
            return Ok(unavailable("scorev2_without_mods_multiplier_unavailable"));
        }
        let without_mods =
            match score_math::round_even_score(score.score as f64 / source_multiplier) {
                Ok(value) => value,
                Err(_) => return Ok(unavailable("scorev2_without_mods_overflow")),
            };
        (summary.total_score, without_mods, "legacy_scorev2")
    } else {
        match score_math::estimate_legacy_score(
            score.mode as u8,
            context.attributes,
            summary,
            source_multiplier,
            context.native_multiplier,
        ) {
            Ok(value) => (value.total, value.without_mods, "legacy_estimated"),
            Err(_) => return Ok(unavailable("legacy_estimate_outside_supported_domain")),
        }
    };
    let classic =
        match score_math::classic_score(score.mode as u8, standardised, u64::from(expected)) {
            Ok(value) => value,
            Err(_) => return Ok(unavailable("legacy_classic_outside_supported_domain")),
        };
    Ok(NativeScales {
        standardised_score: Some(standardised as i64),
        standardised_without_mods: Some(without_mods as i64),
        classic_score: Some(classic as i64),
        native_submission: None,
        native_basic_maximum: Some(expected as i32),
        legacy_attributes: Some(cached),
        status,
        version: Some(if scorev2 {
            "legacy_scorev2.v1".into()
        } else {
            format!(
                "legacy_estimate.v{}.simulator.v{}",
                score_math::LEGACY_ESTIMATE_VERSION,
                legacy_scoring::SIMULATOR_VERSION
            )
        }),
        reason: (status == "legacy_estimated").then(|| {
            "Official-style estimate from summary; ordered gameplay is unavailable.".into()
        }),
    })
}

pub(crate) fn validate_native_maximum(
    native: &SoloScoreSubmission,
    expected: u32,
) -> ApiResult<()> {
    let maximum = basic_sum(&native.maximum_statistics);
    let actual = basic_sum(&native.statistics);
    if maximum != u64::from(expected) || actual > maximum || (native.passed && actual != maximum) {
        return Err(ApiError::bad(
            "Native basic judgments do not match this beatmap.",
        ));
    }
    Ok(())
}

pub(crate) fn native_scales(
    score: &Submission,
    attributes: &DifficultyAttributes,
) -> ApiResult<NativeScales> {
    validate_native_binding(score)?;
    let expected = basic_n(attributes)?;
    if let Some(context) = &score.native_submission {
        validate_native_maximum(context, expected)?;
    }
    let total =
        u64::try_from(score.score).map_err(|_| ApiError::bad("Invalid native total score."))?;
    let classic = score_math::classic_score(score.mode as u8, total, u64::from(expected))
        .map_err(ApiError::bad)?;
    Ok(NativeScales {
        standardised_score: Some(score.score),
        standardised_without_mods: score
            .native_submission
            .as_ref()
            .map(|n| n.total_score_without_mods as i64),
        classic_score: Some(classic as i64),
        native_submission: score.native_submission.as_ref().map(|n| json!(n)),
        native_basic_maximum: Some(expected as i32),
        legacy_attributes: None,
        status: "native_reported",
        version: Some(format!(
            "native_classic.v{}",
            score_math::NATIVE_CLASSIC_VERSION
        )),
        reason: score.native_submission.is_none().then(|| {
            "Native context unavailable; without-mods score was not reconstructed.".into()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score() -> Submission {
        serde_json::from_value(json!({"beatmap_id":1,"mode":0,"score":1000000,"accuracy":100.0,"max_combo":2,"mods":0,"api_mods":[],"passed":true,"hit_counts":{"n300":2},"native_submission":{"ruleset_id":0,"passed":true,"total_score":1000000,"total_score_without_mods":1000000,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]}})).unwrap()
    }
    fn attrs() -> DifficultyAttributes {
        DifficultyAttributes::Osu(rosu_pp::osu::OsuDifficultyAttributes {
            n_circles: 2,
            max_combo: 2,
            ..Default::default()
        })
    }
    #[test]
    fn native_context_is_persisted_and_classic_uses_server_count() {
        let calculated = native_scales(&score(), &attrs()).unwrap();
        assert_eq!(calculated.standardised_score, Some(1_000_000));
        assert_eq!(calculated.classic_score, Some(100_130));
        assert_eq!(calculated.native_basic_maximum, Some(2));
        assert_eq!(calculated.standardised_without_mods, Some(1_000_000));
        assert!(calculated.native_submission.is_some());
        assert_eq!(calculated.status, "native_reported");
        let mut old = score();
        old.native_submission = None;
        let old = native_scales(&old, &attrs()).unwrap();
        assert_eq!(old.classic_score, Some(100_130));
        assert!(old.standardised_without_mods.is_none());
        assert!(old.reason.is_some());
    }
    #[test]
    fn inflated_or_incomplete_basic_statistics_are_rejected() {
        let mut inflated = score();
        inflated
            .native_submission
            .as_mut()
            .unwrap()
            .maximum_statistics
            .insert("great".into(), 2000);
        assert!(native_scales(&inflated, &attrs()).is_err());
        let mut incomplete = score();
        incomplete.hit_counts.n300 = 1;
        incomplete
            .native_submission
            .as_mut()
            .unwrap()
            .statistics
            .insert("great".into(), 1);
        assert!(native_scales(&incomplete, &attrs()).is_err());
        incomplete.passed = false;
        incomplete.native_submission.as_mut().unwrap().passed = false;
        assert!(native_scales(&incomplete, &attrs()).is_ok());
    }
    #[test]
    fn contexts_cannot_change_summary_or_modifier_parameters() {
        let mut forged = score();
        forged.native_submission.as_mut().unwrap().total_score += 1;
        assert!(validate_native_binding(&forged).is_err());
        let mut forged = score();
        forged.native_submission.as_mut().unwrap().ruleset_id = 3;
        assert!(validate_native_binding(&forged).is_err());
        let mut forged = score();
        forged.api_mods = Some(json!([{"acronym":"CL"}]));
        assert!(validate_native_binding(&forged).is_err());
        let mut forged = score();
        forged.native_submission.as_mut().unwrap().accuracy = f64::NAN;
        assert!(validate_native_binding(&forged).is_err());
    }
    #[test]
    fn every_ruleset_derives_its_native_basic_count() {
        let samples = [
            attrs(),
            DifficultyAttributes::Taiko(rosu_pp::taiko::TaikoDifficultyAttributes {
                max_combo: 4,
                ..Default::default()
            }),
            DifficultyAttributes::Catch(rosu_pp::catch::CatchDifficultyAttributes {
                n_fruits: 3,
                n_droplets: 7,
                n_tiny_droplets: 20,
                ..Default::default()
            }),
            DifficultyAttributes::Mania(rosu_pp::mania::ManiaDifficultyAttributes {
                n_objects: 4,
                n_hold_notes: 2,
                ..Default::default()
            }),
        ];
        for (attributes, expected) in samples.iter().zip([2, 4, 3, 6]) {
            assert_eq!(basic_n(attributes).unwrap(), expected);
        }
    }

    fn original_map(mode: u8) -> rosu_pp::Beatmap {
        let bytes = format!(
            "osu file format v14\n[General]\nMode:{mode}\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,0,100,1,0\n[HitObjects]\n64,192,1000,1,0\n256,192,2000,1,0\n"
        );
        rosu_pp::Beatmap::from_bytes(bytes.as_bytes()).unwrap()
    }

    #[test]
    fn original_two_object_maps_have_four_versioned_legacy_estimates() {
        for (mode, expected_total, expected_classic) in [
            (0, 985_000, 98_628),
            (1, 1_000_000, 102_218),
            (2, 1_000_000, 100_086),
            (3, 1_000_000, 1_000_000),
        ] {
            let mut input = score();
            input.mode = mode;
            input.native_submission = None;
            input.api_mods = None;
            input.client = Some("stable".into());
            input.score = if mode == 3 { 1_000_000 } else { 600 };
            if mode == 3 {
                input.hit_counts.n300 = 0;
                input.hit_counts.geki = 2;
            }
            let map = original_map(mode as u8);
            let attrs = rosu_pp::Difficulty::new().lazer(false).calculate(&map);
            let result = legacy_scales(&input, &map, &attrs).unwrap();
            assert_eq!(result.status, "legacy_estimated");
            assert_eq!(result.standardised_score, Some(expected_total));
            assert_eq!(result.standardised_without_mods, Some(1_000_000));
            assert_eq!(result.classic_score, Some(expected_classic));
            assert_eq!(result.native_basic_maximum, Some(2));
            let cached = result.legacy_attributes.unwrap();
            assert_eq!(cached["simulator_version"], 1);
            assert_eq!(
                cached["simulator_client_commit"],
                legacy_scoring::SIMULATOR_CLIENT_COMMIT
            );
            assert!(cached["maximum_statistics"].is_object());
            assert!(result.version.unwrap().contains("legacy_estimate.v1"));
        }
    }

    #[test]
    fn catch_partitions_and_mania_perfect_accuracy_use_native_migration_semantics() {
        let attrs = DifficultyAttributes::Catch(rosu_pp::catch::CatchDifficultyAttributes {
            n_fruits: 3,
            n_droplets: 2,
            n_tiny_droplets: 4,
            ..Default::default()
        });
        let mut input = score();
        input.mode = 2;
        input.native_submission = None;
        input.max_combo = 3;
        input.hit_counts = illya_domain::HitCounts {
            n300: 2,
            n100: 2,
            n50: 3,
            katu: 1,
            misses: 1,
            ..Default::default()
        };
        let summary = legacy_summary(&input, &attrs).unwrap();
        assert!((summary.accuracy - 7.0 / 9.0).abs() < 1e-12);
        assert_eq!(summary.catch.maximum_fruit, 3);
        assert_eq!(summary.catch.maximum_tiny_droplets, 4);
        assert_eq!(summary.catch.tiny_droplet_hits, 3);
        input.hit_counts.katu = 0;
        assert!(legacy_summary(&input, &attrs).is_err());
        input.hit_counts.katu = 1;
        input.hit_counts.n300 = 4;
        assert!(legacy_summary(&input, &attrs).is_err());
        let attrs = DifficultyAttributes::Mania(rosu_pp::mania::ManiaDifficultyAttributes {
            n_objects: 4,
            n_hold_notes: 2,
            max_combo: 6,
            ..Default::default()
        });
        input.mode = 3;
        input.hit_counts = illya_domain::HitCounts {
            geki: 1,
            n300: 1,
            katu: 1,
            n100: 1,
            ..Default::default()
        };
        let summary = legacy_summary(&input, &attrs).unwrap();
        assert!((summary.accuracy - 905.0 / 1220.0).abs() < 1e-12);
        input.hit_counts.geki = 3;
        assert!(legacy_summary(&input, &attrs).is_err());
    }

    #[test]
    fn scorev2_preserves_total_and_reverses_tested_source_multiplier() {
        let mut input = score();
        input.native_submission = None;
        input.api_mods = None;
        input.mods = (1 << 29) | (1 << 4) | (1 << 0);
        input.score = 880_000;
        let map = original_map(0);
        let attrs = rosu_pp::Difficulty::new()
            .mods(input.mods)
            .lazer(false)
            .calculate(&map);
        let result = legacy_scales(&input, &map, &attrs).unwrap();
        assert_eq!(result.status, "legacy_scorev2");
        assert_eq!(result.standardised_score, Some(880_000));
        assert_eq!(result.standardised_without_mods, Some(800_000));
        assert_eq!(result.legacy_attributes.unwrap()["legacy_multiplier"], 1.1);
        input.mods = (1 << 29) | (1 << 7);
        let result = legacy_scales(&input, &map, &attrs).unwrap();
        assert_eq!(result.status, "unavailable");
        assert_eq!(
            result.reason.as_deref(),
            Some("scorev2_without_mods_multiplier_unavailable")
        );
        assert!(result.standardised_score.is_none());
        assert!(result.version.is_some());
    }

    #[test]
    fn incomplete_and_impossible_summaries_have_distinct_results() {
        let mut input = score();
        input.native_submission = None;
        input.api_mods = None;
        input.score = 300;
        input.hit_counts.n300 = 1;
        input.max_combo = 1;
        input.passed = false;
        let map = original_map(0);
        let attrs = rosu_pp::Difficulty::new().calculate(&map);
        let result = legacy_scales(&input, &map, &attrs).unwrap();
        assert_eq!(result.status, "unavailable");
        assert_eq!(result.reason.as_deref(), Some("incomplete_legacy_score"));
        assert!(result.version.is_some());
        assert!(result.legacy_attributes.is_some());
        input.passed = true;
        assert!(legacy_scales(&input, &map, &attrs).is_err());
        input.hit_counts.n300 = 3;
        assert!(legacy_scales(&input, &map, &attrs).is_err());
    }
}
