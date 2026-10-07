# Community workflows: design persistence review

Date: 2026-10-07. A fresh independent documenter substitute performed this bounded pass because the named role was unavailable. This is a code-led ordinary extension in **Operate** mode; the direction contract in `../surfaces/community-workflows.md` preserves the incumbent world.

**Persistence verdict: retain the incumbent system.** The scoped additions introduce no reusable color, type, spacing, radius, depth, imagery or motion decision that requires a new token or rule. `DESIGN.md`, `.impeccable/design.json` and `PRODUCT.md` remain unchanged by this pass. Only this review note is written.

## Comparison evidence

| Workflow | Finished evidence | Comparison with the incumbent system |
|---|---|---|
| Own avatar/banner removal | `settings.tsx:235` adds the image-specific action; `settings.tsx:269` and `:279` use the shared ghost Button, the existing action group and pending disable state. The desktop zh-TW capture shows removal controls beside uploads with biography/country drafts; the mobile Japanese capture shows wrapping actions, retained draft text and a readable localized retry error. | Existing settings tabs, panel padding, preview silhouettes, shared radius, button scale, destructive alert and readable-accent status remain in place. No new visual component or copy style is needed. |
| Moderation after the last page empties | `admin.tsx:635` changes collection revalidation and clamps an invalid page after its own response. The desktop/mobile captures show the resulting 24-row collection and saved-status text. | This is data-state behavior within the incumbent divider-led table and shared controls. The layout JSON records document widths equal to their 1440px and 390px viewports, with both captures settled at scroll position zero. Mobile keeps the dense columns within the existing scroll region. |
| Real map-board title/difficulty and supported modes | `beatmaps.tsx:171` resolves native mode and variant context; `:243` passes the supported modes to the existing selector. `common.tsx:169` adds native disabled semantics. Both native-map captures display the same fixture map title and difficulty in the header and score row, select taiko, dim unsupported modes and retain the normal/RX control. | Shared segmented-control material and intact mode labels persist. The mobile filter groups stack and the table remains locally scrollable; the native layout JSON records 1440px/390px document widths without page widening. Existing source keeps mania on the vanilla variant and standard-map conversion choices available; the taiko captures do not visually certify every ruleset. |

The source sample covered the four modified web files (`settings.tsx`, `admin.tsx`, `beatmaps.tsx`, `common.tsx`), the shared Button and relevant global CSS. `globals.css` and `button.tsx` have no working-tree changes. Existing source supplies the defaults and formulas: pure-black page ground, neutral glass with independent panel blur/opacity, live accent and radius, rounded bilingual fonts, semantic error/status colors, and local dense-table overflow. The sidecar's shared button, glass, mode-selector and avatar previews still represent this vocabulary.

## Captures checked

- [Moderation desktop, English](workflow-catalog-moderation-desktop-en.png) and [mobile, English](workflow-catalog-moderation-mobile-en.png), with `workflow-catalog-moderation-layout.json`.
- [Native beatmap desktop, English](workflow-native-beatmap-desktop-en.png) and [mobile, English](workflow-native-beatmap-mobile-en.png), with `workflow-native-beatmap-layout.json`.
- [Profile media desktop, Traditional Chinese](workflow-profile-media-desktop-zhTW.png) and [mobile retry error, Japanese](workflow-profile-media-mobile-ja-error.png).

These are synthetic fixture captures, not live community activity. The blue profile-image blocks are fixture media, not additions to the default background or shipping artwork. No new shipping raster was added. The default remains pure black with thin neutral glass and direct labels; no AI artwork, slogan, concept replacement or new visual world appears in this scope.

## Five-line system summary

1. Palette: pure-black ground, neutral dark glass and silver seams; shared live accent and semantic foreground/status pairs.
2. Type: bundled Nunito Variable with Noto Sans TC fallback; measured page/section headings, body text and compact metadata retain the existing ramp.
3. Layout: centered fluid pages, existing settings padding, wrapping action groups and bounded dense-table overflow preserve the Local Overflow and Bilingual Breathing rules.
4. Material: shared radius and independent panel blur/opacity preserve the Shared Radius, Glass Before Shadow and Independent Material Controls rules.
5. Controls: ghost image actions, native pressed/disabled mode buttons and localized alert/status feedback preserve the Personal Accent and Paired Theme rules.

## Drift and limits

**Not canonized or repaired:** the pre-existing global `font: inherit` overrides native-control utility typography (`globals.css:117`), and the unlayered universal border-color rule overrides primitive border utilities (`globals.css:83`). `DESIGN.md` already records both as cascade observations rather than future design rules. They are outside this ordinary-extension write boundary; no new token-level divergence was found in the scoped additions.

`community-workflows-qa.json` supplies functional context: 40 browser passes, one opt-in visual skip, 27 PostgreSQL integration tests and two native artifact vectors. Those counts are not a UI-quality claim. This pass inspected the six supplied dark-default captures and source evidence; it did not run a browser, change source/runtime files, certify every theme/personalization state, or assess native client login/gameplay, production deployment, whole-product completeness or the separate native map-import issue.
