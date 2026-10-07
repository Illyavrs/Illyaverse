//! Original bounded map adapter for versioned legacy score estimates.
//!
//! Public rosu-pp attributes supply already-converted object counts. Remaining
//! legacy bonus and ordering rules are expressed as independent arithmetic here.
//! These maxima are not replay verification. No client implementation is copied.

use illya_domain::score_math::LegacyAttributes;
use rosu_pp::model::{
    hit_object::{HitObjectKind, Slider},
    mode::GameMode,
};
use rosu_pp::{
    Beatmap, Difficulty, GameMods, any::DifficultyAttributes, catch::CatchGradualDifficulty,
};
use serde_json::{Value, json};

pub const SIMULATOR_VERSION: u32 = 1;
pub const SIMULATOR_CLIENT_COMMIT: &str = "6cbcae7fbb764839a66c1adf564c2e1679242072";
const MAX_OBJECTS: usize = 1_000_000;
const MAX_EVENTS: u64 = 2_000_000;
const MAX_DURATION: f64 = 86_400_000.0;
const KEY_BITS: u32 = (31 << 15) | (1 << 24) | (7 << 26);

#[derive(Debug)]
pub struct LegacyContext {
    pub attributes: LegacyAttributes,
    pub legacy_multiplier: f64,
    pub native_multiplier: f64,
    /// Map-derived accuracy/combo statistics only; not a claim about every
    /// ignored/bonus judgement in a full native score processor.
    pub maximum_statistics: Value,
}

#[cfg(test)]
fn simulate(bytes: &[u8], played_ruleset: u8, mods: u32) -> Result<LegacyContext, &'static str> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("map_simulation_body_limit");
    }
    let map = Beatmap::from_bytes(bytes).map_err(|_| "invalid_simulation_map")?;
    simulate_map(&map, played_ruleset, mods)
}

pub fn simulate_map(map: &Beatmap, mode: u8, mods: u32) -> Result<LegacyContext, &'static str> {
    if map.hit_objects.is_empty() || map.hit_objects.len() > MAX_OBJECTS {
        return Err("map_simulation_object_limit");
    }
    map.check_suspicion().map_err(|_| "unsafe_simulation_map")?;
    if [map.hp, map.cs, map.od, map.ar]
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=20.0).contains(value))
        || !map.slider_multiplier.is_finite()
        || map.slider_multiplier <= 0.0
        || !map.slider_tick_rate.is_finite()
        || map.slider_tick_rate <= 0.0
        || map.timing_points.iter().any(|point| {
            !point.time.is_finite()
                || !point.beat_len.is_finite()
                || !(6.0..=60_000.0).contains(&point.beat_len)
        })
        || map.difficulty_points.iter().any(|point| {
            !point.time.is_finite()
                || !point.bpm_multiplier.is_finite()
                || point.bpm_multiplier <= 0.0
        })
    {
        return Err("invalid_simulation_difficulty");
    }
    if map
        .hit_objects
        .iter()
        .any(|o| !o.start_time.is_finite() || o.start_time.abs() > MAX_DURATION)
    {
        return Err("invalid_simulation_time");
    }
    if mods & (1 << 29) != 0 {
        return Err("scorev2_requires_separate_normalization");
    }
    if mods & (1 << 31) != 0 {
        return Err("unsupported_legacy_modifier");
    }
    let target = game_mode(mode)?;
    let converted = map
        .convert_ref(target, &GameMods::from(mods))
        .map_err(|_| "unsupported_map_conversion")?;
    let attrs = Difficulty::new().mods(mods).calculate(&converted);
    let (attributes, maximum_statistics) = match attrs {
        DifficultyAttributes::Osu(ref a) => {
            let repeats: u64 = map
                .hit_objects
                .iter()
                .filter_map(|obj| match &obj.kind {
                    HitObjectKind::Slider(s) => Some(s.repeats as u64),
                    _ => None,
                })
                .sum();
            let ticks = u64::from(a.n_large_ticks)
                .checked_sub(repeats)
                .ok_or("invalid_slider_counts")?;
            let base = 300 * u64::from(a.n_circles + a.n_sliders + a.n_spinners)
                + 30 * (2 * u64::from(a.n_sliders) + repeats)
                + 10 * ticks;
            let (bonus, native_bonus) = osu_bonus(map)?;
            (
                LegacyAttributes {
                    accuracy_score: base,
                    combo_score: checked_integer(a.maximum_legacy_combo_score)?,
                    bonus_score: bonus,
                    bonus_score_ratio: ratio(native_bonus, bonus),
                    max_combo: a.max_combo,
                },
                json!({"great":a.n_objects(),"large_tick_hit":a.n_large_ticks+a.n_sliders,"small_tick_hit":a.n_sliders}),
            )
        }
        DifficultyAttributes::Taiko(ref a) => {
            let attributes = taiko_attributes(map, &converted)?;
            if attributes.max_combo != a.max_combo {
                return Err("taiko_conversion_count_mismatch");
            }
            (attributes, json!({"great":a.max_combo}))
        }
        DifficultyAttributes::Catch(ref a) => {
            let mut combo_score = 0_u64;
            let mut fruit = 0;
            let mut droplet = 0;
            let stars = peppy_stars(map, None)? as u64;
            let iterator =
                CatchGradualDifficulty::checked_new(Difficulty::new().mods(mods), &converted)
                    .map_err(|_| "unsafe_catch_simulation")?;
            for (combo, current) in iterator.enumerate() {
                let combo = combo as u64;
                if combo >= MAX_EVENTS {
                    return Err("map_simulation_event_limit");
                }
                let next_fruits = current.n_fruits - fruit;
                let next_droplets = current.n_droplets - droplet;
                if next_fruits + next_droplets != 1 {
                    return Err("invalid_catch_event_order");
                }
                if next_fruits == 1 {
                    combo_score += combo.saturating_sub(1) * 12 * stars;
                }
                fruit = current.n_fruits;
                droplet = current.n_droplets;
            }
            if fruit != a.n_fruits || droplet != a.n_droplets {
                return Err("catch_conversion_count_mismatch");
            }
            let bananas = banana_count(map)?;
            let bonus = bananas * 1100;
            (
                LegacyAttributes {
                    accuracy_score: 300 * u64::from(a.n_fruits)
                        + 100 * u64::from(a.n_droplets)
                        + 10 * u64::from(a.n_tiny_droplets),
                    combo_score,
                    bonus_score: bonus,
                    bonus_score_ratio: ratio(bananas * 200, bonus),
                    max_combo: a.max_combo(),
                },
                json!({"great":a.n_fruits,"large_tick_hit":a.n_droplets,"small_tick_hit":a.n_tiny_droplets}),
            )
        }
        DifficultyAttributes::Mania(ref a) => (
            LegacyAttributes {
                accuracy_score: 0,
                combo_score: 1_000_000,
                bonus_score: 0,
                bonus_score_ratio: 0.0,
                max_combo: 0,
            },
            json!({"perfect":a.n_objects.checked_add(a.n_hold_notes).ok_or("map_simulation_event_limit")?}),
        ),
    };
    let legacy_multiplier = source_multiplier(map, mode, mods)?;
    let native_mods = legacy_api_mods(mode, mods)?;
    let native_multiplier = native_multiplier(map, mode, &native_mods, true)?;
    Ok(LegacyContext {
        attributes,
        legacy_multiplier,
        native_multiplier,
        maximum_statistics,
    })
}

fn game_mode(mode: u8) -> Result<GameMode, &'static str> {
    match mode {
        0 => Ok(GameMode::Osu),
        1 => Ok(GameMode::Taiko),
        2 => Ok(GameMode::Catch),
        3 => Ok(GameMode::Mania),
        _ => Err("invalid_simulation_ruleset"),
    }
}
fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
fn checked_integer(value: f64) -> Result<u64, &'static str> {
    if !value.is_finite() || value < 0.0 || value >= (1_u64 << 53) as f64 || value.fract() != 0.0 {
        Err("invalid_simulation_integer")
    } else {
        Ok(value as u64)
    }
}
fn duration(value: f64) -> Result<f64, &'static str> {
    if !value.is_finite() || !(0.0..=MAX_DURATION).contains(&value) {
        Err("invalid_simulation_duration")
    } else {
        Ok(value)
    }
}
fn osu_bonus(map: &Beatmap) -> Result<(u64, u64), &'static str> {
    let mut legacy = 0;
    let mut native = 0;
    for object in &map.hit_objects {
        if let HitObjectKind::Spinner(spinner) = object.kind {
            let seconds = duration(spinner.duration)? / 1000.0;
            let half = (seconds * (477.0 / 60.0) * 2.0).trunc() as u64;
            let threshold = (seconds * 3.0).trunc() as u64 + 3;
            let bonus = half.saturating_sub(threshold) / 2;
            let small = half / 2
                - if threshold.is_multiple_of(2) {
                    bonus
                } else {
                    0
                };
            legacy += bonus * 1100 + small * 100;
            native += bonus * 50 + small * 10;
        }
    }
    Ok((legacy, native))
}
fn banana_count(map: &Beatmap) -> Result<u64, &'static str> {
    let mut count = 0;
    for object in &map.hit_objects {
        let length = match object.kind {
            HitObjectKind::Spinner(s) => s.duration,
            HitObjectKind::Hold(h) => h.duration,
            _ => continue,
        };
        let mut spacing = duration(length)? as f32;
        while spacing > 100.0 {
            spacing /= 2.0;
        }
        if spacing <= 0.0 {
            continue;
        }
        let mut time = object.start_time.trunc() as f32;
        let end = (object.start_time + length).trunc() as i64;
        while f64::from(time) <= end as f64 {
            count += 1;
            if count > MAX_EVENTS {
                return Err("map_simulation_event_limit");
            }
            let next = time + spacing;
            if next <= time {
                return Err("invalid_simulation_spacing");
            }
            time = next;
        }
    }
    Ok(count)
}
fn peppy_stars(map: &Beatmap, cs: Option<f64>) -> Result<u32, &'static str> {
    let first = map
        .hit_objects
        .first()
        .ok_or("empty_simulation_map")?
        .start_time;
    let last = map
        .hit_objects
        .last()
        .ok_or("empty_simulation_map")?
        .start_time;
    let breaks: f64 = map
        .breaks
        .iter()
        .map(|b| b.end_time.round_ties_even() - b.start_time.round_ties_even())
        .sum();
    let drain = ((last.round_ties_even() - first.round_ties_even() - breaks) / 1000.0).trunc();
    let density = if drain == 0.0 {
        16.0
    } else {
        (map.hit_objects.len() as f64 / drain * 8.0).clamp(0.0, 16.0)
    };
    let value =
        ((f64::from(map.hp) + f64::from(map.od) + cs.unwrap_or(f64::from(map.cs)) + density)
            / 38.0
            * 5.0)
            .round_ties_even();
    if !value.is_finite() || value < 0.0 {
        return Err("invalid_legacy_difficulty");
    }
    Ok(value as u32)
}
#[derive(Clone, Copy)]
enum DrumObject {
    Hit { strong: bool },
    Roll { end: f64, strong: bool },
    Swell { end: f64 },
}
fn timing(map: &Beatmap, time: f64) -> f64 {
    map.timing_points
        .iter()
        .rev()
        .find(|p| p.time <= time)
        .map_or(1000.0, |p| p.beat_len)
}
fn kiai(map: &Beatmap, time: f64) -> bool {
    map.effect_points
        .iter()
        .rev()
        .find(|p| p.time <= time)
        .is_some_and(|p| p.kiai)
}
fn slider_duration(map: &Beatmap, start: f64, slider: &Slider) -> Result<f64, &'static str> {
    let beat = timing(map, start);
    let adjusted = map
        .difficulty_points
        .iter()
        .rev()
        .find(|p| p.time <= start)
        .map_or(1.0, |p| p.bpm_multiplier);
    let length = slider.expected_dist.unwrap_or(0.0);
    let scale = f64::from(1.4_f32);
    let distance = (length * scale) * slider.span_count() as f64;
    let scoring = 100.0 * (map.slider_multiplier * scale) / map.slider_tick_rate;
    duration((distance / (scoring * map.slider_tick_rate) * (beat * adjusted)).trunc())
}
fn taiko_attributes(source: &Beatmap, played: &Beatmap) -> Result<LegacyAttributes, &'static str> {
    let stars = peppy_stars(source, Some(2.0))?;
    let mut objects = Vec::with_capacity(played.hit_objects.len());
    for (idx, obj) in played.hit_objects.iter().enumerate() {
        if !obj.start_time.is_finite() {
            return Err("invalid_simulation_time");
        }
        let strong = played
            .hit_sounds
            .get(idx)
            .is_some_and(|s| u8::from(*s) & 4 != 0);
        let kind = match &obj.kind {
            HitObjectKind::Circle => DrumObject::Hit { strong },
            HitObjectKind::Slider(slider) => DrumObject::Roll {
                end: obj.start_time + slider_duration(played, obj.start_time, slider)?,
                strong,
            },
            HitObjectKind::Spinner(s) => DrumObject::Swell {
                end: obj.start_time + duration(s.duration)?,
            },
            HitObjectKind::Hold(h) => DrumObject::Swell {
                end: obj.start_time + duration(h.duration)?,
            },
        };
        objects.push((obj.start_time, kind));
    }
    let mut attributes = LegacyAttributes {
        accuracy_score: 0,
        combo_score: 0,
        bonus_score: 0,
        bonus_score_ratio: 0.0,
        max_combo: 0,
    };
    let mut native_bonus = 0;
    let mut events = 0;
    let delay = |start: f64| {
        let beat = timing(played, start);
        let divisor = if played.version >= 8 && [1.5, 3.0, 6.0].contains(&played.slider_tick_rate) {
            6.0
        } else {
            8.0
        };
        let mut interval = beat / divisor;
        while interval < 60.0 {
            interval *= 2.0;
        }
        while interval > 120.0 {
            interval /= 2.0;
        }
        interval
    };
    for (idx, (start, kind)) in objects.iter().enumerate() {
        match *kind {
            DrumObject::Roll { end, strong } => {
                let interval = delay(*start);
                let next = objects.get(idx + 1).map(|(time, kind)| match kind {
                    DrumObject::Roll { .. } => *time - delay(*time),
                    _ => *time,
                });
                let end =
                    if next.is_none_or(|next| next - (end + interval.trunc()) > interval.trunc()) {
                        end + interval.trunc()
                    } else {
                        end
                    };
                let mut time = *start;
                while time < end {
                    events += 1;
                    if events > MAX_EVENTS {
                        return Err("map_simulation_event_limit");
                    }
                    let mut score = if kiai(played, *start) {
                        (300.0_f32 * 1.2) as u64
                    } else {
                        300
                    };
                    if strong {
                        score += score / 5;
                        score *= 2;
                    }
                    attributes.bonus_score += score;
                    native_bonus += 10;
                    time += interval;
                }
            }
            DrumObject::Hit { .. } | DrumObject::Swell { .. } => {
                let swell = matches!(kind, DrumObject::Swell { .. });
                let strong = matches!(kind, DrumObject::Hit { strong: true });
                let mut value = 300
                    + 300 / 35
                        * 2
                        * u64::from(stars + 1)
                        * u64::from(attributes.max_combo.min(100) / 10);
                let at = match kind {
                    DrumObject::Swell { end } => *end,
                    _ => *start,
                };
                if kiai(played, at) {
                    value = ((value as f32) * 1.2) as u64;
                }
                let mut combo = value - 300;
                let double = swell || strong;
                if double {
                    value *= 2;
                    combo *= 2;
                }
                attributes.combo_score += combo;
                if let DrumObject::Swell { end } = *kind {
                    let length = duration(end - start)? / 1000.0;
                    let base = (length * 7.5).trunc() as u64;
                    let hits = ((base as f32) * 1.65).trunc().max(1.0);
                    let hits = (hits * 1.5).trunc().max(1.0) as u64;
                    events += hits + 1;
                    if events > MAX_EVENTS {
                        return Err("map_simulation_event_limit");
                    }
                    attributes.bonus_score += (hits + 1) * 300 + (value - combo);
                    native_bonus += 50;
                } else {
                    attributes.accuracy_score += value - combo;
                    attributes.max_combo += 1;
                }
            }
        }
    }
    attributes.bonus_score_ratio = ratio(native_bonus, attributes.bonus_score);
    Ok(attributes)
}

fn original_columns(map: &Beatmap) -> u32 {
    if map.mode == GameMode::Mania {
        return map.cs.round_ties_even().max(1.0) as u32;
    }
    let count = map.hit_objects.len();
    let ends = map.hit_objects.iter().filter(|o| !o.is_circle()).count();
    let special = ends as f64 / count.max(1) as f64;
    let od = map.od.round_ties_even();
    if special < 0.2 {
        7
    } else if special < 0.3 || map.cs.round_ties_even() >= 5.0 {
        if od > 5.0 { 7 } else { 6 }
    } else if special > 0.6 {
        if od > 4.0 { 5 } else { 4 }
    } else {
        (od as u32 + 1).clamp(4, 7)
    }
}
fn selected_columns(map: &Beatmap, mods: u32) -> Result<u32, &'static str> {
    let keys = [
        (26, 1),
        (28, 2),
        (27, 3),
        (15, 4),
        (16, 5),
        (17, 6),
        (18, 7),
        (19, 8),
        (24, 9),
    ];
    let active: Vec<_> = keys
        .iter()
        .filter(|(bit, _)| mods & (1 << bit) != 0)
        .collect();
    if active.len() > 1 {
        return Err("incompatible_legacy_keys");
    }
    let keys = active
        .first()
        .map_or(original_columns(map), |(_, keys)| *keys);
    Ok(keys * if mods & (1 << 25) != 0 { 2 } else { 1 })
}
/// Stable-source multiplier; bit 29 selects its distinct ScoreV2 rules.
/// This does not make ScoreV2 eligible for the ScoreV1 attribute estimator.
pub fn source_multiplier(map: &Beatmap, mode: u8, mods: u32) -> Result<f64, &'static str> {
    game_mode(mode)?;
    let on = |bit: u32| mods & (1_u32 << bit) != 0;
    if mode != 3 && mods & (KEY_BITS | (1 << 25)) != 0 {
        return Err("ruleset_invalid_key_modifier");
    }
    if (on(6) || on(9)) && on(8) {
        return Err("incompatible_speed_modifiers");
    }
    if mode < 3 && on(7) || mode == 0 && on(13) {
        return Ok(0.0);
    }
    let mut value = 1.0;
    let score_v2 = on(29);
    if on(0) {
        value *= if score_v2 { 1.0 } else { 0.5 };
    }
    if on(1) {
        value *= 0.5;
    }
    if on(8) {
        value *= if mode == 3 { 0.5 } else { 0.3 };
    }
    if mode < 3 {
        if on(3) {
            value *= if mode == 2 && score_v2 { 1.0 } else { 1.06 };
        }
        if on(4) {
            value *= match mode {
                0 if score_v2 => 1.10,
                2 => 1.12,
                _ => 1.06,
            };
        }
        if on(6) || on(9) {
            value *= match mode {
                0 if score_v2 => 1.20,
                2 => 1.06,
                _ => 1.12,
            };
        }
        if on(10) {
            value *= 1.12;
        }
        if mode == 0 && on(12) {
            value *= 0.9;
        }
    } else if map.mode != GameMode::Mania {
        let original = original_columns(map);
        let selected = selected_columns(map, mods)?;
        if selected > original {
            value *= 0.9;
        } else if selected < original {
            value *= 0.9 - 0.04 * f64::from(original - selected);
        }
    }
    Ok(value)
}

fn legacy_api_mods(mode: u8, bits: u32) -> Result<Value, &'static str> {
    let mut list = Vec::new();
    for (bit, acronym) in [
        (0, "NF"),
        (1, "EZ"),
        (3, "HD"),
        (4, "HR"),
        (6, "DT"),
        (7, "RX"),
        (8, "HT"),
        (9, "NC"),
        (10, "FL"),
        (12, "SO"),
        (13, "AP"),
        (15, "4K"),
        (16, "5K"),
        (17, "6K"),
        (18, "7K"),
        (19, "8K"),
        (24, "9K"),
        (26, "1K"),
        (27, "3K"),
        (28, "2K"),
    ] {
        if bits & (1 << bit) != 0 && !(bit == 6 && bits & (1 << 9) != 0) {
            if mode == 3 && ["HD", "HR", "FL", "SO", "AP", "RX"].contains(&acronym) {
                continue;
            }
            list.push(json!({"acronym":acronym}));
        }
    }
    Ok(json!(list))
}

/// Current pinned native multiplier for the accepted mod/settings subset.
/// `implicit_classic` adds default CL exactly as legacy replay migration does.
pub fn native_multiplier(
    map: &Beatmap,
    mode: u8,
    mods: &Value,
    implicit_classic: bool,
) -> Result<f64, &'static str> {
    game_mode(mode)?;
    let list = mods
        .as_array()
        .filter(|m| m.len() <= 32)
        .ok_or("invalid_native_mods")?;
    let mut value = 1.0;
    let mut classic = false;
    for entry in list {
        let acronym = entry["acronym"].as_str().ok_or("invalid_native_modifier")?;
        let settings = entry.get("settings").unwrap_or(&Value::Null);
        let number = |name: &str, default: f64| -> Result<f64, &'static str> {
            match settings.get(name) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or("invalid_native_setting"),
            }
        };
        let boolean = |name: &str, default: bool| -> Result<bool, &'static str> {
            match settings.get(name) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v.as_bool().ok_or("invalid_native_setting"),
            }
        };
        value *= match acronym {
            "NF" => 0.5,
            "EZ" => {
                if mode == 0 {
                    0.8
                } else {
                    0.5
                }
            }
            "DT" | "NC" | "HT" | "DC" => {
                let rate = number(
                    "speed_change",
                    if matches!(acronym, "HT" | "DC") {
                        0.75
                    } else {
                        1.5
                    },
                )?;
                if !(0.5..=2.0).contains(&rate) {
                    return Err("invalid_native_rate");
                }
                if mode == 0 {
                    if rate < 1.0 {
                        (rate * 20.0).trunc() / 20.0 * 1.4 - 0.5
                    } else {
                        let floor = (rate * 10.0).trunc() / 10.0;
                        1.0 + (floor - 1.0) * 0.46
                            - if floor != 1.5 && floor != 1.0 {
                                0.01
                            } else {
                                0.0
                            }
                    }
                } else if mode == 3 && matches!(acronym, "DT" | "NC") {
                    1.0
                } else {
                    let rounded = (rate * 10.0).trunc() / 10.0 - 1.0;
                    if rate >= 1.0 {
                        1.0 + rounded / 5.0
                    } else {
                        0.6 + rounded
                    }
                }
            }
            "HR" => match mode {
                0 => 1.09,
                1 => 1.06,
                2 => 1.12,
                _ => 1.0,
            },
            "HD" => match mode {
                0 => {
                    1.04 - if boolean("only_fade_approach_circles", false)? {
                        0.02
                    } else {
                        0.0
                    }
                }
                1 | 2 => 1.06,
                _ => 1.0,
            },
            "FL" => {
                if mode == 0 {
                    1.2
                } else if mode < 3 {
                    1.12
                } else {
                    1.0
                }
            }
            "SO" => {
                if mode == 0 {
                    0.95
                } else {
                    1.0
                }
            }
            "RX" | "AP" => 0.1,
            "CL" => {
                classic = true;
                if mode == 0 {
                    if boolean("classic_note_lock", true)? {
                        0.985
                    } else {
                        0.96
                    }
                } else {
                    1.0
                }
            }
            "DA" => {
                if mode == 0 {
                    let diffs = [
                        ("circle_size", map.cs),
                        ("drain_rate", map.hp),
                        ("overall_difficulty", map.od),
                        ("approach_rate", map.ar),
                    ];
                    let mut result = 1.0;
                    for (key, base) in diffs {
                        result *= (1.0
                            - (number(key, f64::from(base))? - f64::from(base)).abs() * 0.5)
                            .max(0.1);
                    }
                    result.max(0.1)
                } else {
                    0.5
                }
            }
            "1K" | "2K" | "3K" | "4K" | "5K" | "6K" | "7K" | "8K" | "9K" | "10K" => {
                if mode == 3 {
                    0.9
                } else {
                    return Err("ruleset_invalid_key_modifier");
                }
            }
            "SD" | "PF" | "MR" | "AT" | "CN" | "TD" | "DS" | "FI" | "V2" | "SV2" => 1.0,
            _ => return Err("unsupported_native_multiplier"),
        };
    }
    if implicit_classic && !classic && mode == 0 {
        value *= 0.985;
    }
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err("invalid_native_multiplier")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn map(mode: u8, objects: &str) -> String {
        format!(
            "osu file format v14\n[General]\nMode:{mode}\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,0,100,1,0\n[HitObjects]\n{objects}\n"
        )
    }
    #[test]
    fn base_four_ruleset_attributes_are_bounded() {
        for mode in 0..4 {
            let text = map(mode, "128,192,1000,1,0\n256,192,2000,1,0\n384,192,3000,1,0");
            let context = simulate(text.as_bytes(), mode, 0).unwrap();
            assert!(context.attributes.combo_score + context.attributes.accuracy_score > 0);
            assert_eq!(context.legacy_multiplier, 1.0);
            assert_eq!(
                context.native_multiplier,
                if mode == 0 { 0.985 } else { 1.0 }
            );
        }
    }
    #[test]
    fn sliders_spinners_and_bananas_keep_bonus_separate() {
        let text = map(
            0,
            "128,192,1000,1,0\n256,192,2000,2,0,L|396:192,2,280\n256,192,5000,8,0,7000",
        );
        let osu = simulate(text.as_bytes(), 0, 0).unwrap();
        assert!(osu.attributes.accuracy_score > 900);
        assert!(osu.attributes.bonus_score > 0);
        assert!(osu.attributes.bonus_score_ratio > 0.0);
        let catch = simulate(text.as_bytes(), 2, 0).unwrap();
        assert!(catch.attributes.bonus_score > 0);
        assert_eq!(catch.attributes.bonus_score_ratio, 200.0 / 1100.0);
        let taiko = simulate(text.as_bytes(), 1, 0).unwrap();
        assert!(taiko.attributes.bonus_score > 0);
    }
    #[test]
    fn portable_original_map_goldens_match_pinned_native_simulators() {
        // These are original tiny maps, checked through the installed pinned
        // client's public Decoder/playable-map/simulator pipeline. Values are
        // independent evidence, not output recomputed by this adapter.
        let slider = map(
            0,
            "64,64,1000,1,0\n64,192,2000,2,0,L|344:192,2,280\n256,192,5000,8,0,6000",
        );
        for (mode, accuracy, combo, bonus, count) in [
            (0, 1010, 360, 4700, 7),
            (2, 1680, 216, 18700, 6),
            (3, 0, 1_000_000, 0, 0),
        ] {
            let result = simulate(slider.as_bytes(), mode, 0).unwrap();
            assert_eq!(result.attributes.accuracy_score, accuracy);
            assert_eq!(result.attributes.combo_score, combo);
            assert_eq!(result.attributes.bonus_score, bonus);
            assert_eq!(result.attributes.max_combo, count);
        }
        let strong = map(
            1,
            "64,192,1000,1,0\n64,192,1500,1,4\n64,192,2000,1,2\n64,192,2500,1,6",
        )
        .replace(
            "0,500,4,2,0,100,1,0",
            "0,500,4,2,0,100,1,0\n1900,500,4,2,0,100,1,1",
        );
        let taiko = simulate(strong.as_bytes(), 1, 0).unwrap();
        assert_eq!(taiko.attributes.accuracy_score, 1800);
        assert_eq!(taiko.attributes.combo_score, 180);
        assert_eq!(taiko.attributes.max_combo, 4);
        let roll = map(1, "64,192,1000,2,4,L|344:192,1,280\n256,192,2500,8,0,4000");
        let taiko = simulate(roll.as_bytes(), 1, 0).unwrap();
        assert_eq!(taiko.attributes.bonus_score, 21240);
        assert!((taiko.attributes.bonus_score_ratio - 0.010357815442561206).abs() < 1e-14);
        let holds = map(
            3,
            "64,192,1000,1,0\n192,192,1250,1,0\n320,192,1500,128,0,2250:0:0:0:0:\n448,192,1750,128,0,2500:0:0:0:0:",
        );
        assert_eq!(
            simulate(holds.as_bytes(), 3, 0).unwrap().maximum_statistics["perfect"],
            6
        );
        for (bits, legacy) in [(1 << 15, 0.86), (1 << 18, 0.9)] {
            let mania = simulate(slider.as_bytes(), 3, bits).unwrap();
            assert!((mania.legacy_multiplier - legacy).abs() < 1e-12);
            assert_eq!(mania.native_multiplier, 0.9);
            assert_eq!(mania.maximum_statistics["perfect"], 7);
        }
    }
    #[test]
    fn native_defaults_and_structured_rates_are_distinct_from_legacy() {
        let text = map(0, "128,192,1000,1,0");
        let map = Beatmap::from_bytes(text.as_bytes()).unwrap();
        assert_eq!(source_multiplier(&map, 0, 64).unwrap(), 1.12);
        let default = native_multiplier(&map, 0, &json!([{"acronym":"DT"}]), false).unwrap();
        let custom = native_multiplier(
            &map,
            0,
            &json!([{"acronym":"DT","settings":{"speed_change":1.2}}]),
            false,
        )
        .unwrap();
        assert!((default - 1.23).abs() < 1e-12);
        assert!((custom - 1.082).abs() < 1e-12);
        assert_eq!(
            native_multiplier(
                &map,
                0,
                &json!([{"acronym":"CL","settings":{"classic_note_lock":false}}]),
                false
            )
            .unwrap(),
            0.96
        );
    }
    #[test]
    fn separate_scorev2_and_resource_guards() {
        let text = map(0, "128,192,1000,1,0");
        assert_eq!(
            simulate(text.as_bytes(), 0, 1 << 29).unwrap_err(),
            "scorev2_requires_separate_normalization"
        );
        assert!(simulate(text.as_bytes(), 7, 0).is_err());
        assert!(duration(f64::INFINITY).is_err());
        assert!(duration(MAX_DURATION + 1.0).is_err());
        let parsed = Beatmap::from_bytes(text.as_bytes()).unwrap();
        for mode in 0..=3 {
            assert_eq!(
                source_multiplier(&parsed, mode, (1 << 29) | 1).unwrap(),
                1.0
            );
        }
        assert_eq!(source_multiplier(&parsed, 0, (1 << 29) | 16).unwrap(), 1.10);
        assert_eq!(source_multiplier(&parsed, 0, (1 << 29) | 64).unwrap(), 1.20);
        assert_eq!(source_multiplier(&parsed, 2, (1 << 29) | 8).unwrap(), 1.0);
        let mut parsed = Beatmap::from_bytes(text.as_bytes()).unwrap();
        parsed.slider_tick_rate = 0.0;
        assert_eq!(
            simulate_map(&parsed, 1, 0).unwrap_err(),
            "invalid_simulation_difficulty"
        );
        parsed.slider_tick_rate = 1.0;
        parsed.od = f32::NAN;
        assert_eq!(
            simulate_map(&parsed, 0, 0).unwrap_err(),
            "invalid_simulation_difficulty"
        );
    }

    #[test]
    #[ignore = "requires original ignored native DLL fixture harness output"]
    fn native_dll_legacy_fixture_parity() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data-native-probe/legacy-sim");
        let vectors: Value =
            serde_json::from_slice(&std::fs::read(root.join("vectors.json")).unwrap()).unwrap();
        assert_eq!(vectors["pinned_source_commit"], SIMULATOR_CLIENT_COMMIT);
        let mut checked = 0;
        for fixture in vectors["fixtures"].as_array().unwrap() {
            let label = fixture["label"].as_str().unwrap();
            assert!(
                label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            );
            let bytes = std::fs::read(root.join("fixtures").join(format!("{label}.osu"))).unwrap();
            let mode = fixture["target_mode"].as_u64().unwrap() as u8;
            let mods: Vec<Value> = fixture["mods"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| json!({"acronym":s.as_str().unwrap()}))
                .collect();
            let bits = mods.iter().fold(0, |bits, m| {
                bits | match m["acronym"].as_str().unwrap() {
                    "4K" => 1 << 15,
                    "7K" => 1 << 18,
                    _ => 0,
                }
            });
            let context = simulate(&bytes, mode, bits).unwrap_or_else(|e| panic!("{label}: {e}"));
            let expected = &fixture["legacy_attributes"];
            assert_eq!(
                context.attributes.accuracy_score,
                expected["accuracy_score"].as_u64().unwrap(),
                "{label} A"
            );
            assert_eq!(
                context.attributes.combo_score,
                expected["combo_score"].as_u64().unwrap(),
                "{label} C"
            );
            assert_eq!(
                context.attributes.bonus_score,
                expected["bonus_score"].as_u64().unwrap(),
                "{label} B"
            );
            assert!(
                (context.attributes.bonus_score_ratio
                    - expected["bonus_score_ratio"].as_f64().unwrap())
                .abs()
                    < 1e-14,
                "{label} ratio"
            );
            assert_eq!(
                context.attributes.max_combo,
                expected["max_combo"].as_u64().unwrap() as u32,
                "{label} max combo"
            );
            let basic_key = if mode == 3 { "perfect" } else { "great" };
            assert_eq!(
                context.maximum_statistics[basic_key].as_u64().unwrap_or(0),
                fixture["processor_maximum_statistics"][basic_key]
                    .as_u64()
                    .unwrap_or(0),
                "{label} basic maximum"
            );
            if mode == 2 {
                for key in ["large_tick_hit", "small_tick_hit"] {
                    assert_eq!(
                        context.maximum_statistics[key],
                        fixture["processor_maximum_statistics"][key],
                        "{label} {key} maximum"
                    );
                }
            }
            assert!(
                (context.legacy_multiplier
                    - fixture["applied_legacy_multiplier"].as_f64().unwrap())
                .abs()
                    < 1e-12,
                "{label} legacy multiplier"
            );
            let map = Beatmap::from_bytes(&bytes).unwrap();
            let native = native_multiplier(&map, mode, &json!(mods), false).unwrap();
            assert!(
                (native - fixture["applied_native_score_multiplier"].as_f64().unwrap()).abs()
                    < 1e-12,
                "{label} native multiplier"
            );
            for multiplier in fixture["multipliers"].as_array().unwrap() {
                let flag = multiplier["legacy_flags"].as_str().unwrap();
                let bits = match flag {
                    "None" => 0,
                    "NoFail" => 1,
                    "Easy" => 2,
                    "Hidden" => 8,
                    "HardRock" => 16,
                    "DoubleTime" => 64,
                    "HalfTime" => 256,
                    "Flashlight" => 1024,
                    "ScoreV2" => 1 << 29,
                    _ => panic!("unknown native fixture flag"),
                };
                // ScoreV2 is deliberately excluded from the estimator, but
                // source multiplier still describes its legacy mod projection.
                if bits & (1 << 29) == 0 {
                    let source = source_multiplier(&map, mode, bits).unwrap();
                    assert!(
                        (source - multiplier["legacy_multiplier"].as_f64().unwrap()).abs() < 1e-12,
                        "{label} {flag} source multiplier"
                    );
                }
                let mods: Vec<Value> = multiplier["mods"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| json!({"acronym":s.as_str().unwrap()}))
                    .collect();
                let native = native_multiplier(&map, mode, &json!(mods), false).unwrap();
                assert!(
                    (native - multiplier["native_score_multiplier"].as_f64().unwrap()).abs()
                        < 1e-12,
                    "{label} {flag} native multiplier"
                );
            }
            checked += 1;
        }
        assert!(checked >= 12);
        let rows = vectors["score_v2_multiplier_rows"].as_array().unwrap();
        assert!(rows.len() >= 150);
        for row in rows {
            let label = row["fixture"].as_str().unwrap();
            assert!(
                label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            );
            let bytes = std::fs::read(root.join("fixtures").join(format!("{label}.osu"))).unwrap();
            let map = Beatmap::from_bytes(&bytes).unwrap();
            let mode = row["ruleset_id"].as_u64().unwrap() as u8;
            let bits = u32::try_from(row["legacy_flags_bits"].as_u64().unwrap()).unwrap();
            assert_eq!(bits & (1 << 29) != 0, row["source_v2"].as_bool().unwrap());
            let actual = source_multiplier(&map, mode, bits).unwrap();
            assert!(
                (actual - row["legacy_multiplier"].as_f64().unwrap()).abs() < 1e-12,
                "{label} {bits} source multiplier {actual}"
            );
            let mods: Vec<_> = row["mods"]
                .as_array()
                .unwrap()
                .iter()
                .map(|acronym| json!({"acronym":acronym.as_str().unwrap()}))
                .collect();
            let actual = native_multiplier(&map, mode, &json!(mods), false).unwrap();
            assert!(
                (actual - row["native_score_multiplier"].as_f64().unwrap()).abs() < 1e-12,
                "{label} {bits} native multiplier {actual}"
            );
        }
    }
}
