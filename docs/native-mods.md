# Native lazer mods and performance

Lazer sends a ruleset-specific list of mod objects, each with an acronym and optional settings. Illyaverse preserves those objects in `scores.api_mods`; `scores.mods` remains a legacy bit projection for stable-facing metadata. Performance calculation uses the original structured `rosu_mods::GameMods`, never the projection as a replacement for custom settings. [Native score submission contract](https://github.com/ppy/osu-infrastructure/blob/master/score-submission.md), [rosu-mods serialization](https://docs.rs/rosu-mods/0.4.1/rosu_mods/serde/enum.GameModsSeed.html).

## Supported parameters

The adapter validates the requested ruleset, mod compatibility, unique acronyms and setting types. It accepts at most 32 mods with 16 parameters per mod.

| Mods | Supported settings |
| --- | --- |
| DT / NC | `speed_change` from 1.0 to 2.0; DT also accepts boolean `adjust_pitch` |
| HT / DC | `speed_change` from 0.5 to 1.0; HT also accepts boolean `adjust_pitch` |
| DA | Available ruleset difficulty settings: CS / AR / HP / OD from 0 to 11, scroll speed from 0.1 to 10, boolean extended limits / Hard Rock offsets |
| HD | Boolean `only_fade_approach_circles`, where available |
| CL | Available ruleset classic behavior flags, including slider-head accuracy |
| MR | Available reflection setting `0`, `1` or `2` |
| FL | Default settings only: delay 120 ms, size 1.0, combo-based size enabled |
| NF / EZ / HR / SD / PF / SO / RX / AP / 1K through 10K | Supported only where the ruleset defines them; unsupported parameters are rejected |

Unknown fields, duplicate or incompatible mods, wrong setting types and unsupported settings produce an explicit validation error. A valid acronym in one ruleset is not automatically valid in another. Variable-speed, arbitrary experimental mods and customized Flashlight area/delay are not accepted by this adapter. The Flashlight defaults follow the [official osu!standard mod](https://github.com/ppy/osu/blob/master/osu.Game.Rulesets.Osu/Mods/OsuModFlashlight.cs); rosu-pp does not model those custom parameters.

## Ranking policy

Submitted scores remain pending until moderator review. Illyaverse excludes DA, AP and legacy autoplay from ranked pp via the server-owned `pp_eligible` column. Valid RX scores in osu!/taiko/catch belong to a separate `relax` partition with their own PP, rankings and statistics; mania RX is rejected. Review does not override other mod exclusions. Historical RX eligibility is restored only through explicit audited recalculation. See [RX policy and API](relax.md). These are Illyaverse policies, not official osu! ranking eligibility.

The server validates that the legacy bit projection matches the structured mods. Public web score submissions cannot impersonate native lazer ingestion by supplying structured mods. Native beatmap mod filters compare the full acronym set for stored structured mods, so DA is not mistaken for NM because both have a zero legacy projection. Filter acronyms do not constrain custom parameter values.

The website shows stored mod acronyms, individual parameters and pp exclusion information in all supported interface languages.

## Verification scope

Tests cover custom DT rates across four rulesets, ruleset-invalid and conflicting mods, unsupported or out-of-range settings, assisted-mod eligibility and native parameter persistence. A server calculation test verifies that DT 1.2× differs from default DT 1.5× and that changing OD affects pp. An isolated PostgreSQL/API test checks preserved parameters, projection mismatch rejection, pending-score privacy and web-ingestion restrictions. Browser fixtures validate the parameter display and explanation on desktop and mobile without inserting community rows.

These checks do not prove deterministic gameplay integrity or actual native client play. Scores still require review. Native map score leaderboards compare native standardized totals with convertible passed stable estimates. Stable raw boards partition V1 and V2 by the requested ScoreV2 flag. Ranked PP remains independent because Sanctuary calculates it from each original ruleset/mod configuration. Shared profile display totals use Classic caches and raw stable V1 history. [Score normalization](score-normalization.md) records the versioned conversion, unavailable states and the distinction between an estimate and exact gameplay reconstruction.
