# Score normalization research

**Status: research / pending implementation.** This document describes contracts and a proposed original implementation. Illyaverse does not currently convert stable ScoreV1 totals to standardized lazer totals, and has not verified a converter against native gameplay. Formula examples below are mathematical calculations, not client test results.

Research recorded on 2026-10-07. Reference source is pinned to:

- `ppy/osu`: [`6359741babba4ced42da1fcda738c35370fdf3f4`](https://github.com/ppy/osu/tree/6359741babba4ced42da1fcda738c35370fdf3f4).
- `ppy/osu-queue-score-statistics`: [`7558b004f33cb79196b63682daa9862cb9c23faa`](https://github.com/ppy/osu-queue-score-statistics/tree/7558b004f33cb79196b63682daa9862cb9c23faa).

These sources establish behavior. Implementations must be original; neither source code nor their server implementation should be copied into Illyaverse. Native-client request fixtures currently use a separately pinned client build; a future scoring implementation must explicitly select and test its own scoring version.

## Current protection and remaining work

Root has added source restrictions to the two map leaderboard queries:

| Endpoint | Current score source and scale |
|---|---|
| `/web/osu-osz2-getscores.php` | `client='stable'`, stable raw score |
| `/api/v2/beatmaps/{id}/scores` | `client='lazer'`, native standardized score |

This prevents ordering unrelated raw scales on the same map. It does not normalize either source. The independent HTTP smoke passed against rebuilt images and a fresh PostgreSQL 17 project: a reviewed stable score remained on the stable board; the native board stayed empty until its own lazer score was reviewed, then returned that score. The pending-score, moderation, privacy, ruleset and mod filters remain necessary.

PP can continue to select the best eligible result per map across client types using server-computed rosu-pp. That does not make the two raw total-score scales comparable. Current native profile aggregate formulas and stable score serialization still need separate review; this research did not change them. See [native mod policy](native-mods.md) for validated modifiers and ranked PP eligibility.

## Three different score meanings

1. **Legacy raw score:** the original stable score, dependent on map length, difficulty and ScoreV1 behavior. Preserve this value.
2. **Standardized score:** the native per-play score, usually near one million for a perfect no-mod result; mods and bonuses can change that ceiling.
3. **Classic display score:** a map-size-dependent display transformation of standardized score. It resembles the scale of legacy scoring but is not the inverse of stable ScoreV1.

The API's individual-score `total_score` has standardized semantics. `legacy_total_score` preserves the historical raw number. A native profile's `statistics.total_score` and `ranked_score` have Classic aggregation semantics; they are not sums of standardized per-play totals.

## Native gameplay formulas

Use the following quantities after applying the playable beatmap and judgments:

- `a`: achieved accuracy-weighted base score / maximum base score of judgments processed so far; use 1 if the denominator is zero.
- `q`: achieved combo portion / autoplay maximum combo portion; use 1 if the maximum is zero.
- `p`: processed accuracy-judgment count / autoplay maximum accuracy-judgment count; use 1 if the maximum is zero.
- `b`: accumulated native bonus portion.
- `M`: native ruleset/mod multiplier for the selected scoring version and original map difficulty context.
- `roundEven`: nearest integer, with exact half values rounded to the even integer.

| Ruleset | Unrounded score without mods |
|---|---|
| osu! | `500000*a*q + 500000*a^5*p + b` |
| taiko | `250000*q + 750000*a^3.6*p + b` |
| catch | `(1000000-400000*t)*q + 400000*t*d + b` |
| mania | `150000*q + 850000*a^(2+2*a)*p + b` |

For catch, `t = maximumSmallTickHit / (maximumSmallTickHit + maximumGreat)`, or 0 for a zero denominator. `d = smallTickHit / maximumSmallTickHit`, or 0 if no tiny droplets exist. Large droplets are intentionally excluded from the denominator defining `t`.

Calculate `withoutMods = roundEven(unrounded)`, then `total = roundEven(withoutMods*M)`. Do not combine the two rounding operations. Rust's ordinary floating-point `round()` is not an equivalent tie rule.

Sources: [shared ScoreProcessor](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/Rulesets/Scoring/ScoreProcessor.cs), [taiko processor](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Taiko/Scoring/TaikoScoreProcessor.cs), [catch processor](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Catch/Scoring/CatchScoreProcessor.cs), [mania processor](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Mania/Scoring/ManiaScoreProcessor.cs).

### Why counts and maximum combo do not determine native score

The combo portion depends on the sequence of judgments, not just the highest combo:

- osu! weights each scorable judgment by its maximum base value times `sqrt(comboAfterJudgment)`.
- taiko uses the achieved result's base value and a logarithmic combo factor capped at 400. Strong nested hits use an additional object-dependent factor.
- mania uses a logarithmic combo factor capped at 400. Perfect contributes 300 to combo but 305 to accuracy. Hold/tick ordering matters.
- catch uses fruit/droplet base values and a logarithmic combo factor capped at 200. Tiny droplets have their own score portion.

Judgment counts discard order, slider breaks and distinctions between some nested objects or bonus events. Maximum statistics describe opportunities, not the achieved combo trajectory. Exact reconstruction therefore needs complete ordered results with object context, replay simulation under the matching engine/ruleset version, or trustworthy processor state plus map autoplay maxima.

## Official legacy conversion is an estimate

The official [migration tool](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/Database/StandardisedScoreMigrationTools.cs) computes a deterministic standardized estimate from a legacy summary. Reproducing that estimate can be exact relative to a pinned migration version; it is not an exact reconstruction of native gameplay.

Define legacy beatmap attributes:

| Symbol | Meaning |
|---|---|
| `A` | maximum legacy accuracy score |
| `C` | maximum legacy combo score before the legacy mod multiplier |
| `B` | maximum legacy bonus score |
| `r` | ratio converting an estimated excess legacy bonus to native bonus |
| `N` | beatmap maximum legacy combo |

Also retain original legacy total `R`, achieved max combo `L`, misses `m`, native-recomputed accuracy `a`, legacy multiplier `l`, and current native multiplier `M`. Stable mods are converted and Classic is added. The pinned osu! multiplier gives Classic a factor of `.985`; the other three rulesets use 1. Modifier behavior and rounding must be versioned rather than assumed constant.

Common intermediate quantities:

```text
Cm = roundEven(C*l)
c  = max(R-A*a, 0)/(Cm+B)
b  = max(R-A-Cm, 0)*r
```

When `Cm+B` is zero, use `c=0` if `l=0`, otherwise `c=1`. Bonus and combo cannot generally be separated from the raw number. Do not introduce an unconditional `c<=1` clamp or a one-million final cap: neither is the upstream contract.

For taiko and mania, use the native formulas above with `q=c` and `p=1`. For catch, use its formula with the estimated combo factor described below. For osu!, use its estimated combo factor and `p=1`.

### osu! combo estimate

For `L=0` or `a=0`, the combo term is zero. When `Cm+B=0`, the special branch is `500000*c + 500000*a^5 + b`.

Otherwise define:

```text
V  = max(N^2*c/a, L^2)
k  = floor(V/L^2)
e1 = k*L^1.5 + (sqrt(V-k*L^2))^1.5
u  = N-L-m
x  = (V-L^2)/u, if u>0; otherwise 0
e2 = L^1.5 + u*sqrt(x)
```

Clamp each `e1` and `e2` into `[0,N^1.5]`. Let `lo` and `hi` be their minimum and maximum, then:

```text
e = min(0.3*lo+0.7*hi, 1.2*(lo+hi)/2)
q = e/N^1.5
withoutMods = roundEven(500000*a*q + 500000*a^5 + b)
```

The approximation treats slider opportunities and lower-value hits as uniformly distributed. A validated implementation must reject impossible inputs and non-finite intermediates rather than inventing a successful conversion.

### catch combo estimate

Return `q=1` if `N=0` or `N=L`, and `q=0` if `L=0`. Otherwise estimate the loss from misses evenly distributed outside the longest combo.

Define `F(n)` as the integral approximation of the capped native combo factor:

```text
F(0) = 1
F(n) = 0.5*n                                      for 0<n<=2
F(n) = 1+(n*(ln(n)-1)+2-ln(4))/ln(4)              for 2<n<=200
F(n) = F(200)+(n-200)*ln(200)/ln(4)               for n>200
G(n) = z*(1+ln(200)-ln(z))/ln(4), z=min(n,200)     for n>0
```

Let `u=N-L-m`, `s=floor(u/m)`. If `s>0`, loss is `floor(u/s)*G(s)` plus `G(u mod s)` when the remainder is positive. Otherwise loss is `F(N)-F(L)`. Then `q=1-clamp(loss/F(N),0,1)`.

Validate `m>0` before the division for a non-full-combo score; impossible summaries need an explicit unsupported/error result. This is an estimate of missed combo distribution. Stable catch combines large-droplet misses with fruit misses, which also makes the reconstructed fruit/tiny scale approximate.

### ScoreV2 and incomplete plays

The official local beatmap overload treats stable ScoreV2 separately: it preserves the original total and derives the without-mods number using the legacy multiplier. It does not run the ScoreV1 estimation above. Historical official imported score tables have a narrower context than arbitrary submitted Illyaverse scores; a new implementation must define eligibility for ScoreV2 explicitly.

The legacy estimate uses `p=1`. It should not be described as exact native progression for failed/incomplete plays. Initial mixed-client ranked normalization should target validated passed ScoreV1 plays with complete map context, and preserve an explicit unsupported state for other cases.

## A summary cannot uniquely recover exact score

Consider a nine-circle osu! fixture with legacy difficulty multiplier 5, no mods, no bonus, every successful hit Great. Its perfect legacy attributes are `A=2700`, `C=1680`, `N=9`.

| Play | Ordered results | Great / Miss | Highest combo | Legacy raw |
|---|---|---|---:|---:|
| A | `M,G,G,M,G,G,G,G,G` | 7 / 2 | 5 | 2460 |
| B | `G,M,G,M,G,G,G,G,G` | 7 / 2 | 5 | 2460 |

Legacy base score is 2100 in both. A five-hit run adds 360 combo points; the remaining two hits add none in either arrangement. Native combo weighting distinguishes a two-hit run from two isolated hits:

| Calculation | Play A | Play B |
|---|---:|---:|
| Exact native formula, without mods | 359793 | 351450 |
| Exact native formula with `.985` Classic | 354396 | 346178 |
| Official legacy estimate, without mods | 303348 | 303348 |
| Official estimate with `.985` Classic | 298798 | 298798 |

These results were calculated from the pinned formulas, not observed through client gameplay. The difference proves that even raw score, all hit counts, maximum combo, map and mods do not uniquely determine native score.

## Data and original implementation plan

Keep raw values immutable. Store a separate normalization result with a quality state such as `native_reported`, `legacy_estimated`, `server_replayed` or `unsupported`. These describe calculation provenance; none replaces moderation or anti-cheat verification.

Suggested versioned inputs and outputs:

- Original beatmap MD5, immutable `.osu` bytes/version, original and target rulesets, source build, original mod bits and validated structured native mods/settings.
- Raw legacy score, all legacy counts, achieved combo, passed/completion state, complete native statistics and maximum statistics when available.
- Cached legacy attributes `A,C,B,r,N`, original difficulty context and the version of their map converter/simulator. Converted mania key modes need their own context.
- `standardised_without_mods`, `standardised_total_score`, `classic_total_score`, normalizer version, multiplier version, and normalization status/reason.
- For an exact replay-based path, ordered result/object data or trusted processor aggregates (`BaseScore`, `MaximumBaseScore`, `AccuracyJudgementCount`, `ComboPortion`, `BonusPortion`) together with playable-map autoplay maxima and engine version.

The existing stable summary and stored map are enough to attempt the official estimate once the missing simulator/converter functionality exists. They are insufficient for exact native gameplay reconstruction. Existing native token submissions already retain maximum statistics; a normalized score record should persist them independently of token retention and validate them against playable-map opportunities.

Map simulators require nested sliders, ticks, timing points, object types, breaks and version-specific conversion behavior. Sources: [legacy attributes](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/Rulesets/Scoring/Legacy/LegacyScoreAttributes.cs), [osu simulator](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Osu/Difficulty/OsuLegacyScoreSimulator.cs), [taiko simulator](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Taiko/Difficulty/TaikoLegacyScoreSimulator.cs), [catch simulator](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Catch/Difficulty/CatchLegacyScoreSimulator.cs), [mania simulator](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game.Rulesets.Mania/Difficulty/ManiaLegacyScoreSimulator.cs).

The official import maps legacy judgments, reconstructs maximum statistics, and pads unrecorded combo opportunities with `LegacyComboIncrease`. Mania Perfect contributes 305 to native accuracy versus Great's 300; catch cannot restore separate fruit and large-droplet misses. These mappings must precede the conversion, not reuse a stored stable accuracy percentage. [Import contract](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Helpers/BatchInserter.cs).

rosu-pp computes performance. Its currently exposed osu legacy difficulty attributes are not a complete four-ruleset normalization implementation. Dependency reuse should be based on its public API and an explicit compatibility test, not an assumption that PP calculation also produces native score.

## Classic profile aggregation

Let `S` be standardized total score including native multipliers and `n` the sum of basic judgments in maximum statistics. The pinned display contract gives:

| Ruleset | Classic display score |
|---|---|
| osu! | `roundEven((32.57*n^2+100000)*S/1000000)` |
| taiko | `roundEven((1109*n+100000)*S/1000000)` |
| catch | `roundEven(21.62*(S*n/1000000)^2+S/10)` |
| mania | `S` |

This transformation is an approximation of the historical scale, not an inverse legacy converter. For `n=100,S=1000000`, results are respectively 425700, 210900, 316200 and 1000000. For the current two-basic-judgment native smoke fixture, osu!'s Classic value is 100130, not 1000000.

The official total-score processor adds Classic values for native plays; imported legacy scores do not add those values again. Ranked-score selection compares standardized totals, then adds the selected score's Classic display value. Consequently, fixing mixed map ordering does not by itself fix Illyaverse's profile aggregate semantics. A direct `SUM(stableRaw + lazerStandardised)` or mixed `MAX(score)` remains incorrect.

Until a tested normalizer exists, cross-client PP can remain shared while score-based totals are explicitly scoped/documented. A future lifetime total can preserve already-counted raw stable history and add native Classic values, following historical aggregation semantics. Cross-client best-per-map ranked-score selection still requires a common standardized comparison scale; adding both source-specific bests would double-count maps.

Sources: [Classic display transform](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/Scoring/Legacy/ScoreInfoExtensions.cs), [total score processor](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Processors/TotalScoreProcessor.cs), [ranked score processor](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Processors/RankedScoreProcessor.cs), [standardized best-score selection](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Helpers/DatabaseHelper.cs).

## Proposed verification vectors and acceptance conditions

The following are derived vectors for future tests. Synthetic algebra inputs are not assertions that a real play can achieve every combination.

| Input | Expected score without mods |
|---|---:|
| Any ruleset, perfect, no bonus | 1000000 |
| taiko synthetic `a=.5,c=.5,b=0` | 186852 |
| catch full combo, maximum fruit=100, maximum tiny=100, tiny hits=75 | 950000 |
| mania synthetic raw=900000, legacy multiplier=1, all Great (`a=300/305`) | 931049 |
| osu! nine-circle counterexample above | official estimate 303348 for both |

Required future tests include zero denominators, impossible/non-finite input, rounding ties (`2.5→2`, `3.5→4`), spinner/banana/strong-hit bonuses, slider/hold padding, all ruleset conversions, key mods, ScoreV2 policy, modifier version changes and failed/incomplete score policy. Native replay simulation and summary conversion must be tested as different calculation paths.

Before enabling a mixed-client score leaderboard, compare an original implementation against pinned native contract fixtures, backfill only convertible rows, retain unsupported rows outside the mixed ranking, and verify HTTP ordering with two clients' scores that disagree under raw-scale ordering. Mark the resulting converter as integration-tested only after those checks. Actual stable/lazer gameplay validation remains a separate requirement.
