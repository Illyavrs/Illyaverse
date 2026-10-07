# Relax score-partition surface brief

Scope: the shared score selection in `apps/serendipity/src/components/score-selection.tsx`, rankings, player profiles, beatmap boards, score rows and score detail. Visitor mode: **Operate**.

Audience and job: players comparing Normal and Relax (RX) results within a game mode, inspecting the corresponding player statistics and board, and opening a result or replay with its scoring context intact. This is a code-first extension of the existing Serendipity world.

## Direction contract

THESIS: Keep the active score partition explicit from rankings to the individual result, so players understand which statistics and ordering they are comparing.

OWN-WORLD: Reuse the incumbent personalized black ground, plum-black glass, rounded Nunito/Noto Sans TC lettering, semantic accent, compact segmented controls and secondary badges. No new identity or artwork.

STORY: Select a game mode and Normal or RX, compare its rankings and profile statistics, inspect a map board, then open a score or replay without losing the partition's meaning.

FIRST VIEWPORT: Retain familiar page headings and glass filters. Rankings place game and variant controls before the table. Profiles place selection above the profile grid, with four statistics in two columns and the active variant badge at the score heading. Map boards keep their controls and ordering explanation together. Score detail names RX beside performance and verification, with replay access nearby.

FORM: A precisely scoped code-first extension of the existing catalogue and profile compositions. No approved comp, critique reference or QUALITY BAR card applies; no new concept seed or identity replacement was required.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Interaction and content constraints

- Use the shared URL selection for `mode` and `variant`. Normal maps to `vanilla`; RX maps to `relax`. Selecting a mode or variant resets pagination. Rankings, profile statistics, profile scores and map-board requests include the effective selection.
- Carry selection into player links from rankings and map boards. Score detail derives its player and map return links from the result's own mode and variant. Keep replay actions available when the result has a replay.
- Mania uses the Normal partition and disables RX with localized explanatory copy. A disabled RX control remains visibly named rather than disappearing.
- Explain map-board ordering: RX ranks by pp, while Normal ranks by available standardised score; each board keeps one best score per player.
- Name RX on score rows and detail. Preserve a raw score of zero when supplied, and explain unavailable conversion context without inventing a comparable converted score.
- Keep the four profile metrics legible in a two-column grid. Preserve complete translated labels and the established local horizontal scrolling for dense tables.
- Keep loading, empty and error states honest. Fixture values and names in review screenshots are QA evidence, not live community activity.

## URL assumption

Query parameters are the current implementation choice after the optional URL-preference prompt remained unanswered through the reasonable response window. They are an implementation assumption, not a newly confirmed user preference. The shared helper owns this policy so a later explicit preference can be applied consistently.

## Incumbent comparison

The extension introduces no new palette, font family, radius, elevation or motion tokens. Its selector and RX badges reuse the existing control and pill vocabulary. The narrow stylesheet changes reuse the established 50% disabled treatment, 12px metadata size and 20px/24px spacing; the four-stat grid is a surface composition rather than a new global primitive.

One global prose mismatch remains: `DESIGN.md` describes mobile profile score headings above full-width mode controls. Current source instead places game/variant selection in a filter bar above the profile grid and places a variant badge beside the score heading. This brief records the current composition. The global file is preserved, and the mismatch is reported without repair. The sidecar's existing game-mode preview also does not demonstrate the new disabled RX state; its primitive values remain current.

## Finish evidence and boundary

The independent fresh generic reviewer returned scoped `ship` with no material finish fix remaining. The named shipped finish role was unavailable, so a generic substitute followed its instructions. The builder and reviewer individually inspected all eight final production-build captures in `data-relax-review`: rankings, profile, beatmap and score, each at desktop 1440 × 1000 in Traditional Chinese and mobile 390 × 844 in Japanese. Four browser fixture checks, frontend lint, type checking and the production build passed; the detector ran once and returned `[]`. The two bounded inspection rounds are closed. See [Relax verdict](../review/relax-verdict.md).

These captures and fixtures support the frontend presentation and selection behavior. Visual review did not independently exercise live API partitioning. Native GUI, live gameplay, stock-client behavior and deployment remain outside this scoped verdict. The existing `PRODUCT.md`, `DESIGN.md` and `.impeccable/design.json` are retained; this handoff adds no global tokens and ships no artwork.
