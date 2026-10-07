# Score normalization

**Status: implemented, with unit and isolated PostgreSQL validation; native gameplay remains unverified.** The original Rust converter preserves raw scores, estimates passed stable ScoreV1 results, handles ScoreV2 separately, and stores map-derived Classic display caches. Pinned native DLL fixtures validate the arithmetic and map simulator; they are headless contract tests, not successful game sessions.

Research recorded on 2026-10-07. Reference source is pinned to:

- `ppy/osu`: [`6359741babba4ced42da1fcda738c35370fdf3f4`](https://github.com/ppy/osu/tree/6359741babba4ced42da1fcda738c35370fdf3f4).
- `ppy/osu-queue-score-statistics`: [`7558b004f33cb79196b63682daa9862cb9c23faa`](https://github.com/ppy/osu-queue-score-statistics/tree/7558b004f33cb79196b63682daa9862cb9c23faa).

These sources establish behavior. Implementations must be original; neither source code nor their server implementation should be copied into Illyaverse. The separately built native fixture client is pinned to `2026.1005.1-tachyon`, commit `6cbcae7fbb764839a66c1adf564c2e1679242072`. Cached records retain both the formula and simulator identities.

## Implemented data and leaderboard policy

| Boundary | Current behavior |
|---|---|
| Stable map leaderboard | Stable raw scores only, with V1/V2 boards partitioned by the requested ScoreV2 flag; original replay totals are preserved. |
| Native map leaderboard | Native reported standardized totals and convertible stable estimates share one comparison scale. Unsupported rows are excluded. Stable results carry an implicit CL modifier. |
| Lifetime display total | Verified stable V1 raw history plus verified native/ScoreV2 Classic values. Missing native conversion context and unidentified public API score scales are excluded. |
| Ranked display total | Select one passed eligible verified result per ranked map using standardized score, then add that result's Classic value. |
| PP | Independently select the highest server-computed eligible PP per map and apply weighting. A score-scale cache does not grant approval. |

Native summaries persist independently of score-token retention. Core fields, structured mods and legacy counter projections must agree. The server derives the maximum basic judgment count from the same converted map/mod context used for PP:

| Ruleset | Maximum basic judgments |
|---|---|
| osu! | Circles + sliders + spinners |
| taiko | Scorable notes; strong bonuses, roll ticks and swells are excluded |
| catch | Fruits; large and tiny droplets are excluded |
| mania | Notes + holds; each hold has a basic head and tail, including CL |

A native maximum must equal this server-derived count. Actual basic judgments cannot exceed it, and a passed score must be complete. This validates the count used in Classic display conversion; it does not prove every bonus/tick maximum, reported total, accuracy, or ordered play. Native scores remain pending moderation. Native mania PP preserves all six judgment counts instead of collapsing CL hold heads/tails into the stable object count.

Migration `0012_score_scales.sql` adds separate raw-preserving caches. New ingestion computes them before insertion. Historical rows are recovered in batches of at most 16 every 30 seconds with PostgreSQL row locks and skip-locked replica coordination. Recovery updates only conversion context/caches; it never changes raw score, PP, ownership, review or replay bytes. Invalid/missing context receives a bounded unavailable marker instead of retrying forever. After restoring the missing context, an operator can clear only that row's `score_scale_version` while its status remains unavailable to request another recovery attempt.

States are `native_reported`, `legacy_estimated`, `legacy_scorev2`, and `unavailable`. Native caches use `native_classic.v1`; passed V1 estimates use `legacy_estimate.v1.simulator.v1`; V2 uses `legacy_scorev2.v1`. Cached legacy attributes include simulator/formula source pins, maximum statistics and separate source/native multipliers. Unidentified public API submissions retain PP/review functionality but do not receive a transport-specific score scale. See [native mod policy](native-mods.md) for PP eligibility.

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

The official local beatmap overload treats stable ScoreV2 separately: it preserves the original total and derives the without-mods number using the legacy multiplier. It does not run the ScoreV1 estimation above. Historical official imported score tables have a narrower context than arbitrary submitted Illyaverse scores. Illyaverse requires a passed complete summary and tested nonzero source multiplier for its ScoreV2 conversion cache.

The legacy estimate uses `p=1`. It should not be described as exact native progression for failed/incomplete plays. The implemented V1 path requires complete passed summaries and map context. Incomplete legacy results retain an unavailable cache. V2 preserves its original standardized total and divides by its separately tested V2 source multiplier for the without-mods value; zero source multipliers cannot be reversed and remain unavailable.

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

These results are formula/contract vectors. The two raw-summary estimates were also compared with a headless native migration call; neither ordered play was exercised through a game host. The difference proves that even raw score, all hit counts, maximum combo, map and mods do not uniquely determine native score.

## Context and remaining exact-replay work

Raw values remain immutable. Stored conversion provenance is separate from verification. A future `server_replayed` path would require ordered judgments and trusted processor aggregates; no current cache claims deterministic replay verification.

The versioned cache and future replay path require these inputs and outputs:

- Original beatmap MD5, immutable `.osu` bytes/version, original and target rulesets, source build, original mod bits and validated structured native mods/settings.
- Raw legacy score, all legacy counts, achieved combo, passed/completion state, complete native statistics and maximum statistics when available.
- Cached legacy attributes `A,C,B,r,N`, original difficulty context and the version of their map converter/simulator. Converted mania key modes need their own context.
- `standardised_without_mods`, `standardised_total_score`, `classic_total_score`, normalizer version, multiplier version, and normalization status/reason.
- For an exact replay-based path, ordered result/object data or trusted processor aggregates (`BaseScore`, `MaximumBaseScore`, `AccuracyJudgementCount`, `ComboPortion`, `BonusPortion`) together with playable-map autoplay maxima and engine version.

The stable summary and stored map support the implemented estimate. They remain insufficient for exact native gameplay reconstruction. Native maximum statistics now persist on the score itself; map-derived basic opportunities are checked before display conversion.

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

The implemented lifetime total preserves raw stable V1 history and adds native/ScoreV2 Classic values. Ranked-score selection uses the standardized comparison cache, then adds the selected result's Classic value; adding separate source-specific bests would double-count maps. The same aggregate queries serve stable status packets and native profiles. Native profiles also expose conversion coverage for verified lifetime plays.

Sources: [Classic display transform](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/Scoring/Legacy/ScoreInfoExtensions.cs), [total score processor](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Processors/TotalScoreProcessor.cs), [ranked score processor](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Processors/RankedScoreProcessor.cs), [standardized best-score selection](https://github.com/ppy/osu-queue-score-statistics/blob/7558b004f33cb79196b63682daa9862cb9c23faa/osu.Server.Queues.ScoreStatisticsProcessor/Helpers/DatabaseHelper.cs).

## Verification vectors and limits

The following are derived vectors used by the arithmetic tests. Synthetic algebra inputs are not assertions that a real play can achieve every combination.

| Input | Expected score without mods |
|---|---:|
| Any ruleset, perfect, no bonus | 1000000 |
| taiko synthetic `a=.5,c=.5,b=0` | 186852 |
| catch full combo, maximum fruit=100, maximum tiny=100, tiny hits=75 | 950000 |
| mania synthetic raw=900000, legacy multiplier=1, all Great (`a=300/305`) | 931049 |
| osu! nine-circle counterexample above | official estimate 303348 for both |

Current arithmetic and map fixtures cover zero denominators, impossible/non-finite input, rounding ties (`2.5→2`, `3.5→4`), spinner/banana/strong-hit bonuses, slider/hold counts, ruleset/key conversion samples, ScoreV2 multipliers and failed/incomplete policy. Wider map/conversion coverage and future modifier-version changes still need dedicated fixtures. Native replay processing and summary estimation remain different calculation paths.

The native fixture suite covers 93 typed Classic vectors, 12 original decoded/playable-map legacy attribute fixtures, 150 V1/V2 source/native multiplier combinations and eight native Mania PP vectors. Native overflow diagnostics are rejected outside Rust's safe integer domain instead of being reproduced. Portable tests retain representative golden values; optional local DLL fixture comparisons require the separately built pinned client.

PostgreSQL integration checks verify native-context persistence and binding, forged basic maxima rejection, pending-score privacy, token-independent recovery, 16-row batch bounds and unchanged raw/PP/review values. A second integration check selects the standardized winner when raw ordering disagrees, keeps PP's winning play independent, and excludes unavailable conversions from display sums. Fresh-image encrypted HTTP and native DTO smoke results are recorded in [verification](verification.md).

This is a versioned estimate over the exposed rosu-pp converter and original bounded legacy simulator. Twelve map fixtures cannot prove all maps, conversion edge cases or client releases. Actual four-ruleset gameplay, exact replay processing, and behavior after future upstream scoring changes remain separate validation work.
