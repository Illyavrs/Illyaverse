# Rank-history finish record

The finish review disposition is **ship** for the rank-history extension. The review covers the existing public profile's new history anchor, controls, original SVG chart, exact-observation table, server collector and API. Unrelated map catalogue work and broader site changes are excluded. This is an ordinary extension of the established visual system: no new visual world, seed, approved composition or shipping raster was introduced.

The unavailable dedicated finish-reviewer and documenter roles were performed by fresh default-agent substitutions. Both substitutions are disclosed here and in the review handoff. The documenter read the dedicated role, its degraded fallback and the full document reference before checking the finished extension.

## Evidence and boundaries

The [surface contract](../.impeccable/surfaces/rank-history.md), [feature documentation](rank-history.md), [five-section finish verdict](../.impeccable/review/rank-history-verdict.md), [pinned evidence packet](../.impeccable/review/rank-history-evidence.json) and [capture metadata](../.impeccable/review/rank-history-captures.json) describe the same increment. The verdict includes persistence, fidelity, ceiling, material_fixes and keep; it requests no rank-history material fix.

All 17 source SHA-256 values in the packet and all four capture SHA-256 values matched the files checked by the documenter. Source gates and browser results were read from the existing evidence and logs, not rerun by this documentation pass. The documenter opened all four full-page PNGs and checked their visible locale, theme, state, top-of-page content and complete page shape. These captures use synthetic intercepted profile and history responses; they do not depict retained player history or a native osu! client.

| Capture | Dimensions | Evidence |
| --- | --- | --- |
| [Desktop dark, zh-TW](../.impeccable/review/rank-history-1440-dark-zh-TW-observed.png) | 1440 × 2485 | Seven observations over the full 90-day UTC window, missing-day and unranked interruptions, inherited custom profile accent |
| [Desktop light, en](../.impeccable/review/rank-history-1440-light-en-service-error.png) | 1440 × 2078 | History service error and retry while the profile remains available |
| [390px light, en](../.impeccable/review/rank-history-390-light-en-country-touch.png) | 390 × 3500 | Country placement, Oct 1/TW selection by touch, 30-day window, expanded locally scrolling daily table |
| [320px dark, ja](../.impeccable/review/rank-history-320-dark-ja-single-unranked.png) | 320 × 3098 | One zero-pp observation remains selectable and unranked, with no invented trend |

The three final images are pinned in the evidence packet:

| Service | Image ID |
| --- | --- |
| Serendipity | `sha256:88ad45fd2aecf22fbe6f7262b2a5d4b624e20f89fcae684675934eae4bae11c0` |
| Resonance | `sha256:8fac03733eb6713d2e322d07fc8e0431316a900d4adf7bc437afc19599092b0d` |
| Sanctuary | `sha256:a6f97d927c20348d32dbd47509062bcd6538caae8642d88fd30ad73b670cb80e` |

## Finished behavior

Identity and current statistics remain first. A profile navigation anchor leads to the full-width history section before About; best/recent scores and uploads retain their established placement. Global/country placement and 30/90/180-day controls use the selected profile mode and vanilla/RX partition. The selected observation's UTC date, placement and pp are visible above the chart.

The chart derives X positions from the exact requested UTC window, so a new server's short observation history occupies only its actual dates. Better placement rises. Missing days, unranked/zero-pp observations and sampled-country changes in the country view interrupt line segments. A single observation produces no trajectory. Mouse, touch, arrow keys, Home/End and the daily-table buttons select the same observations. The table preserves date, capture time, pp, both ranks and sampled country in a bounded horizontal scroll region.

History requests validate the returned partition and window, clear old content during partition changes, and separate service failure from profile availability. Viewer-specific cache keys and access revalidation prevent an earlier viewer's data from surviving denied access. Loading, empty first-day, single-point, denied and retry states have direct localized copy in zh-TW, en and ja.

The collector uses the existing live ranking query across all seven valid partitions: four vanilla modes and three RX modes, with no mania RX. It captures every 30 minutes when due, checks each minute and on startup, and preserves the latest successful observation per UTC date. Each capture is atomic across the population and partitions, with a cross-replica PostgreSQL fence. No past score-date reconstruction, fabricated backfill or third ranking algorithm is introduced. Restricted or missing profiles return 404, including administrator reads.

## Incumbent system preserved

The documenter compared `PRODUCT.md`, `DESIGN.md` and `.impeccable/design.json` with the finished rank stylesheet, shared globals, font imports, profile accent scope and sampled components. No new durable primitive token is needed. The original SVG chart is a data presentation component; its local measurement geometry does not replace the existing palette, typography, glass, radius or focus system.

| System rule | Finished extension |
| --- | --- |
| Personal Accent and Paired Theme | Chart strokes, selection and table interaction use shared readable-accent and semantic foreground/surface roles, including the public profile's accent and light-theme replacement |
| Bilingual Breathing | Nunito Variable/Noto Sans TC remain self-hosted; headings stay restrained, numeric values use tabular figures, and localized labels wrap without breaking game-mode names |
| Glass Before Shadow and Independent Material Controls | The section reuses shared neutral glass, live panel opacity/blur/radius and quiet seams; background blur and dim remain independent |
| Shared Radius | Personalized material and controls retain live radii; the chart's fixed 4px transparent focus geometry is a local measurement detail |
| Local Overflow | Compact controls reflow below 600px; the selected date spans a row and dense table content scrolls within its labeled, keyboard-focusable region |

`DESIGN.md` and `.impeccable/design.json` are preserved byte-for-byte. Their checked SHA-256 values are `42631947bcacb83e33ba13806f3d46d4a05a429ea13c4e124c012e32861eab33` and `5c4150a4843e51807f712abfc42f251c580c936870f400c52e659841b6a96a28`, respectively. This documentation pass writes only this file and appends runtime proof to the existing rank-history evidence packet.

Existing landing documentation refers to the older hero minimum heights while the current community introduction uses a different composition. Existing accent/font/border cascade behavior is also outside this rank-history increment; the documented universal border-color and native-control font inheritance were not repaired or promoted into new system guidance. These are pre-existing or excluded concerns, not new rank-history findings.

## Recorded verification

| Gate | Result |
| --- | --- |
| Sanctuary / Resonance unit tests | 75 / 56 passed |
| Rank-history PostgreSQL tests | 6 passed |
| Profile best / profile presentation / RX PostgreSQL tests | 3 / 1 / 2 passed |
| Migration-source / documentation checks | 6 / 10 passed |
| Rust format, Sanctuary Clippy, frontend lint and TypeScript | Passed |
| Browser first batch | 43 of 46 passed; 3 failures were test-only exact `getByLabel` selectors whose implicit-label text included select options; the actual Placement/Period accessible names were correct |
| Focused rank confirmation on the same image pins | 12 of 12 passed |
| Browser unique coverage across batches | 46 unique cases passed; this was **not** an unbroken 46-case run |
| Scoped detector | One run, zero primary findings, six advisories |

The detector advisories concern the selected 16px value, compact 10/11px chart metadata and the chart's 4px focus geometry. The reviewer accepted their component context; no second detector or design-token rewrite was used to suppress them. See `data-rank-history/detector.json`, `source-gates-confirmation.log`, `browser-round-one.log` and `browser-confirmation.log` for the recorded outputs.

## Runtime and remaining scope

Both retained stacks, `illyaverse` and `illyaverse-client-test`, now run the three application images pinned above. The root task's read-only runtime verifier passed 26 GET assertions, found all 10 service containers healthy, and confirmed the four PostgreSQL/Caddy full container IDs were preserved. Both databases contain successful migrations through 0022 with zero failed migrations. Core users/maps/aliases/scores, community, availability and announcements retain their protected counts and fingerprints. The documenter read `data-rank-history/deploy.log` and `runtime.log`, inspected the verifier's assertions and checked the matched baseline snapshots; this documentation pass did not rerun deployment or runtime tools.

The pre-deployment backup set uses the stem `before-rank-history-20261008T033727Z-{main,client}` and includes PostgreSQL dumps, shared-data tar archives and snapshot JSON for both stacks. Request-only CA/resolve options verified the client-test TLS endpoint without changing host resolution or the trust store. Backup paths and runtime log hashes are appended to the existing [evidence packet](../.impeccable/review/rank-history-evidence.json); original source/image/capture pins remain intact.

Each stack has exactly 14 rows for Oct 8, 2026 UTC: two unrestricted users across seven partitions, all written by the application collector. Main captured at `2026-10-08T03:37:44Z`; client-test captured at `2026-10-08T03:38:05Z`. There is no earlier history. Both retained databases still have zero actual scores and zero replay paths, so these first observations have zero pp and no placement. Public history responses were checked for all seven partitions and the exact UTC window; missing profiles return 404, invalid day ranges and mania RX return 400, and the private guide remains 404. This real initial-day state is distinct from the synthetic multi-day captures used to review the interface.

No shipping raster requires provenance: the chart is original SVG code, and the PNGs are review evidence. Native stable/lazer gameplay and score submission remain pending human client validation. The retained baseline has zero actual scores/replays, so this review makes no native-client compatibility claim. Other map UX and global changes require their own subsequent increment and review.
