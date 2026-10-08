---
name: "Illyaverse — Serendipity"
description: "Original community records on pure black with personal neutral glass."
colors:
  primary: "#f28eb6"
  primary-foreground: "#111111"
  readable-accent: "color-mix(in srgb, var(--user-accent) 40%, #f5f5f5)"
  page-ground: "#000000"
  background: "#080808"
  foreground: "#f5f5f5"
  secondary: "#262626"
  secondary-foreground: "#e8e8e8"
  muted-foreground: "#b5b5b5"
  accent: "#2c2c2c"
  border: "#ffffff16"
  input: "#ffffff25"
  surface: "rgb(24 24 27 / var(--panel-opacity))"
  surface-strong: "rgb(32 32 36 / clamp(0, calc(var(--panel-opacity) + 0.15), 1))"
  surface-raised: "rgb(46 46 52 / var(--panel-opacity))"
  surface-subtle: "rgb(16 16 19 / var(--panel-opacity))"
  divider-strong: "#ffffff30"
  selection-fill: "color-mix(in srgb, var(--user-accent) 16%, var(--background))"
  status-success: "#91d8b1"
  status-warning: "#edce84"
  status-info: "#9cbff0"
  destructive: "#f294a0"
  grade-gold: "#edcf86"
  grade-silver: "#c5d5e8"
  difficulty-easy: "#86c3ee"
  difficulty-normal: "#86d5ad"
  difficulty-hard: "#e8c77e"
  difficulty-insane: "#e59dc7"
  difficulty-expert: "#b5a1f2"
  difficulty-extreme: "#f39f95"
  light-readable-accent: "color-mix(in srgb, var(--user-accent) 30%, #262626)"
  light-page-ground: "#f5f5f5"
  light-background: "#fafafa"
  light-foreground: "#262626"
  light-secondary: "#e8e8e8"
  light-secondary-foreground: "#444444"
  light-muted-foreground: "#606060"
  light-accent: "#e2e2e2"
  light-border: "#00000020"
  light-input: "#00000030"
  light-surface: "rgb(255 255 255 / var(--panel-opacity))"
  light-surface-strong: "rgb(255 255 255 / clamp(0, calc(var(--panel-opacity) + 0.15), 1))"
  light-surface-raised: "rgb(246 246 246 / var(--panel-opacity))"
  light-surface-subtle: "rgb(252 252 252 / var(--panel-opacity))"
  light-divider-strong: "#00000030"
  light-selection-fill: "color-mix(in srgb, var(--user-accent) 12%, var(--background))"
  light-status-success: "#256b48"
  light-status-warning: "#785414"
  light-status-info: "#345e93"
  light-destructive: "#a1233e"
  light-grade-gold: "#80580f"
  light-grade-silver: "#476078"
  light-difficulty-easy: "#24658b"
  light-difficulty-normal: "#326d43"
  light-difficulty-hard: "#785b10"
  light-difficulty-insane: "#953d6b"
  light-difficulty-expert: "#6848a2"
  light-difficulty-extreme: "#9c4436"
typography:
  display:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "32px"
    fontWeight: 800
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  display-mobile:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "28px"
    fontWeight: 800
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  headline:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "30px"
    fontWeight: 800
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  headline-mobile:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "26px"
    fontWeight: 800
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  title:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "19px"
    fontWeight: 900
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  section:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "17px"
    fontWeight: 800
    lineHeight: 1.4
    letterSpacing: "-0.025em"
  body:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.7
  body-mobile:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.7
  label:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "13px"
    fontWeight: 800
    lineHeight: 1.7
  metadata:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "12px"
    lineHeight: 1.7
  performance:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "24px"
    fontWeight: 900
    lineHeight: 1.25
  read-body:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.95
rounded:
  sm: "calc(var(--radius) * 0.45)"
  md: "calc(var(--radius) * 0.65)"
  lg: "14px"
  xl: "calc(var(--radius) * 1.2)"
  difficulty: "8px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  control-gap: "12px"
  md: "16px"
  group: "20px"
  lg: "24px"
  record-gap: "28px"
  xl: "32px"
  section: "48px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  button-outline:
    backgroundColor: "color-mix(in oklab, var(--input) 30%, transparent)"
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  input:
    backgroundColor: "color-mix(in oklab, var(--input) 30%, transparent)"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "4px 12px"
    height: "36px"
  badge-secondary:
    backgroundColor: "{colors.secondary}"
    textColor: "{colors.secondary-foreground}"
    rounded: "{rounded.full}"
    padding: "2px 8px"
  glass-panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.lg}"
    padding: "{spacing.lg}"
  record-heading:
    backgroundColor: "{colors.surface-raised}"
    textColor: "{colors.foreground}"
    typography: "{typography.section}"
    padding: "15px 20px"
  mode-control:
    backgroundColor: "transparent"
    textColor: "{colors.muted-foreground}"
    typography: "{typography.label}"
    padding: "8px 14px"
  mode-control-selected:
    backgroundColor: "{colors.selection-fill}"
    textColor: "{colors.readable-accent}"
  difficulty-chip:
    backgroundColor: "{colors.surface-subtle}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.difficulty}"
    padding: "7px 11px"
    height: "48px"
---

# Design System: Illyaverse — Serendipity

## Overview

**Creative North Star: "The Community Record"**

Illyaverse presents players, beatmaps and scores as connected community records. A pure-black default canvas supports thin neutral glass, joined identity and fact bands, aligned numeric values and rounded bilingual lettering. Strong headings, quieter metadata and accent underlines make location and selection clear without decorative copy.

Personalization is part of the material. The visitor can adjust theme, accent, background blur and dim, panel blur and opacity, and radius independently. The viewed player’s public accent remains distinct from the visitor’s private appearance. Light mode uses neutral white and gray replacements, while zh-TW, en and ja share the same hierarchy and wrapping behavior.

The source composition studies of osu-web and Sunset inform recognizable community structure; Illyaverse’s source, palette and geometry are original. Runtime official beatmap covers and player-uploaded avatars, banners and backgrounds provide identity. The default background is plain black, and a profile without a banner reserves no empty artwork region. Provenance is recorded in [the website redesign record](docs/website-redesign.md).

**Key Characteristics:**

- Pure-black default canvas with adjustable thin neutral glass.
- Joined heading, toolbar, identity and record surfaces.
- Rounded Nunito Variable and Noto Sans TC typography.
- Personal accent underlines, readable highlights and aligned performance facts.
- Real covers and player uploads with useful missing-image fallbacks.
- Paired light/dark semantic colors, meaningful labels and bounded overflow.

The normative frontmatter captures current reused defaults from the global, home, profile, score and map stylesheets and the shared component primitives. Live CSS custom properties remain authoritative for personal appearance. The sidecar carries extensions and self-contained previews; its tonal ramps are preview aids, not additional shipping colors.

## Colors

A pale raspberry personal accent sits over black and neutral glass; light mode replaces the neutral materials and darkens semantic foregrounds.

### Primary

- **Raspberry** (`primary`): the default accent for actions, orientation and performance emphasis.
- **Readable Raspberry** (`readable-accent`): the accent mixed toward the theme foreground for text and focus.
- **Primary Ink** (`primary-foreground`): the provider chooses dark or white text from the custom accent’s luminance.
- **Accent Selection** (`selection-fill`): restrained selected-control fill; navigation and mode selection also retain an underline.

### Secondary

- **Success Mint, Warning Amber and Information Blue** (`status-*`): community availability, score verification and supporting semantic facts. Destructive feedback has its separate role.
- **Grade Gold and Grade Silver** (`grade-*`): grade summaries and leading rank distinctions. Grade labels remain visible.
- **Difficulty Blue, Green, Amber, Pink, Violet and Coral** (`difficulty-*`): numeric star tiers shared by catalogue, map records and difficulty selection. Light-mode foreground partners use the matching `light-difficulty-*` entries. They do not identify moderation status.

### Neutral

- **Black Ground** (`page-ground`): the default canvas. The slightly lifted `background` is an opaque field and code ground.
- **Neutral Glass** (`surface`): content records and containers at the chosen opacity.
- **Strong Glass** (`surface-strong`): the persistent utility header, identity bands and data headers; opacity is increased and clamped.
- **Raised Glass** (`surface-raised`): distinct section headings, toolbars and record hover emphasis.
- **Inset Glass** (`surface-subtle`): route navigation, mode rails, inset performance columns and compact supporting content.
- **Soft Silver and Secondary Gray** (`foreground`, `muted-foreground`): primary facts and subordinate labels.
- **Silver Seams** (`border`, `input`, `divider-strong`): quiet record divisions, field boundaries and major section separation.
- **Light Ground and Glass** (`light-*`): the semantic replacements for neutral white/gray surfaces and dark text. Keep the role mapping intact.

**The Personal Accent Rule.** Use the runtime accent and readable-accent roles for actions and emphasis; preserve the distinction between visitor appearance and a player’s public accent.

**The Separate Meanings Rule.** Location and selection use the personal accent. Status, grade and numeric difficulty use their own semantic roles and always retain readable labels.

## Typography

**Display and Body Font:** locally bundled Nunito Variable, with Noto Sans TC and generic sans-serif fallbacks. Nunito is variable; the layout bundles Noto Sans TC at weights 400 and 600.

Rounded Latin lettering and the Traditional Chinese fallback carry a compact, readable community interface. Use native monospace for code and commands; comparable numeric data uses tabular figures where its component requests them.

### Hierarchy

- **Display:** the factual home introduction uses `display` and becomes `display-mobile` at 600px.
- **Headline:** general page headings use `headline` and `headline-mobile`. The catalogue uses 32px; profile identity uses a contextual clamp from 26px to 34px and becomes 25px on mobile. Score detail has its own 26px–40px clamp.
- **Title:** profile sections use `title`; home record headings use the smaller `section` role, reducing to 16px on mobile.
- **Body:** the document base uses `body`, reducing to `body-mobile` at 600px. Supporting paragraphs use 13–14px with line-height 1.8; home copy has a 65ch measure, profile biography 75ch.
- **Label and Metadata:** mode labels use `label`; metadata commonly uses `metadata`. Dense supporting facts may be 10–11px, subordinate to record names and actions. Native fields inherit body typography; link-rendered primitive buttons retain their 14px utility typography.
- **Performance:** profile score values use `performance`, reducing to 21px on mobile. Major score detail performance is 48px, reducing to 40px at 640px; keep its unit smaller.
- **Read Body:** Markdown prose uses `read-body`; article headings have the observed 36px/24px/19px hierarchy and code uses 12px.

**The Bilingual Breathing Rule.** Preserve line-height, meaningful mode labels and wrapping when translating; let the layout adapt before compressing text.

## Layout

Community pages and the persistent header share a centered maximum width (1200px), with desktop gutters (32px) and mobile gutters (16px). Main page padding is 32px above and 48px below; at 600px it becomes 24px above and 36px below. Narrow utility pages retain an 850px maximum, Markdown articles 920px and authentication 1060px. These are contextual reading widths, not extra community grids.

The utility header is 60px tall. Above 800px a separate route row adds 40px, with underlined active links. At 800px and below route navigation moves into the menu sheet. Profile section navigation sits below the header at 100px desktop and 60px mobile; section anchor clearance is 165px and 125px respectively so headings remain visible.

Use tight 4–8px control spacing, 12–16px nearby-action gaps, 20–28px group gaps and 24–32px section padding. Joined headings usually have less vertical padding than their content. Home pairs flexible records with a 308px support column, narrowing to 276px at 1000px and stacking at 800px. Profile performance joins a flexible chart and a 294px statistics column, stacking at 900px. These compositions describe those surfaces rather than requiring every page to have a sidebar.

### Responsive behavior

- **At 800px:** home content stacks; route navigation becomes a menu sheet; profile sticky navigation uses the mobile header offset.
- **At 780px:** catalogue results become one column and map detail areas stack.
- **At 640px:** score detail identity, performance and fact bands rearrange into compact rows and a two-column fact grid.
- **At 600px:** gutters, padding and contextual type reduce; home actions wrap; profile score records preserve identity beside compact performance; catalogue filters and difficulty previews use two columns. Compact difficulty controls retain a minimum height (44px), so eight previews occupy four rows.
- **At 420px:** map cover tracks narrow and supporting difficulty names truncate within their bounded controls. Complete accessible link labels retain identity.
- **Dense records:** rankings preserve a minimum width (650px); score tables preserve 860px, or 1040px for weighted best-score collections, inside local horizontal wrappers. Markdown tables preserve 640px. Do not shrink these factual columns into unreadable cells.

**The Local Overflow Rule.** Keep dense tables, technical strings and long navigation inside their own scrollable regions; the document must remain within the viewport.

## Elevation & Depth

Depth comes from neutral glass levels, seams and joined structure. Shared glass, sticky navigation, sheets and select menus use the live panel blur. Panel blur defaults to 8px with a 0–48px range; opacity defaults to 55% with a 0–100% range. Strong glass adds 0.15 to the same opacity and clamps it to 0–1. Base, raised and inset levels share the chosen opacity with different neutral channels. Glass panels do not carry decorative shadows.

A player background uses a fixed cover layer scaled to 1.05, with separate scene blur and a theme-aware dim veil. Background defaults are 0px blur and 0.76 dim; ranges are 0–48px and 0–1. These effects leave foreground text outside the image filter.

### Shadow Vocabulary

- **Control seam:** outline button and input primitives retain the small Tailwind shadow (`0 1px 2px 0 rgb(0 0 0 / 0.05)`).
- **Selection underline:** the profile mode rail uses an inset bottom line (`inset 0 -2px var(--readable-accent)`); this marks selection rather than elevation.
- **Overlay panels:** shared sheet/select styling explicitly uses `box-shadow: none`; material, border and overlay establish separation.

State transitions stay short: links change color in 180ms, score-row emphasis in 160ms ease-out, and the route underline reveals in 240ms with the implemented easing. The global reduced-motion preference removes animation/transitions and smooth scrolling.

**The Independent Material Rule.** Background blur and dim control the scene; panel blur and opacity control surfaces. Keep their controls and effects independent.

## Shapes

The live radius defaults to 14px and ranges from 0–40px. Small and medium shared corners scale from it; large surfaces use it directly, and extra-large corners scale upward. Difficulty controls retain their implemented compact 8px corners. Avatars are rounded rectangles and badges are pills.

Joined sections keep rounding on the outer silhouette. Internal heading/content seams flatten their touching corners; profile identity, mode and rank bands share one enclosing record. Quiet one-pixel borders separate facts without converting every fact into another card.

**The Joined Record Rule.** Connect related heading, controls and content within one outer silhouette; use seams and surface levels to show internal structure.

## Components

### Buttons

Compact, direct actions use accent-filled primary, bordered outline and transparent ghost variants. Shared defaults are captured in frontmatter; home actions become 40px tall and catalogue/detail actions keep a minimum height (44px). Outline and input primitives use a transparent light surface and an input-tinted dark surface. Hover shifts the assigned fill; focus retains an explicit ring; disabled controls reduce opacity and interaction. Button-rendered primitives inherit the body font, while anchor-rendered primitives carry the smaller utility label.

### Chips

Secondary metadata badges use paired secondary colors and pill corners. Difficulty controls join a tier marker, version and actual star value. Default controls use inset glass; hover and current state tint the surface with the difficulty color and strengthen its border. Compact catalogue previews use transparent defaults and 44px targets. Selected controls retain `aria-current` or `aria-pressed` semantics.

### Cards / Containers

A record uses base glass, a quiet border and the live radius. Joined section headings use raised glass; prominent identity and score headers use strong glass; support rails and key numeric columns use inset glass. Common content padding is 24px, with surface-specific 18–28px padding and tighter mobile values. Hover emphasizes a record with the appropriate existing surface level.

### Inputs / Fields

Fields use semantic foregrounds, input borders and medium corners. Native selects are 38px tall by default; catalogue search and actions use 52px desktop and 48px mobile. Labels sit above the fields, and keyboard focus, invalid and disabled states remain visible. Numeric outputs use tabular figures.

### Navigation

The utility header groups site identity, locale, appearance and account actions; the route row carries ordinary route names. Active routes use readable accent text and a two-pixel accent underline. Mode selectors use underlines and meaningful, unbroken names. Profile section navigation is sticky, locally scrollable and tracks location with `aria-current`; mobile navigation remains available through the sheet.

### Community Records

Player, map and score records put identity beside aligned facts and nearby actions. Rankings right-align comparable numeric columns and leave player identity to the left. Map records use genuine runtime covers with an original icon fallback. Profiles render a banner only when one exists. Score verification, grade and difficulty retain text and semantic color; unavailable facts stay unavailable rather than being visually inferred.

## Do's and Don'ts

### Do:

- **Do** use semantic foreground/surface pairs and runtime accent variables in both themes.
- **Do** connect entity identity, facts, controls and records with clear surface levels and seams.
- **Do** align comparable numeric values and use tabular figures for ranks, scores and control outputs.
- **Do** retain labels alongside status, grade and difficulty color, and show only facts provided by the running API.
- **Do** preserve 44px catalogue action targets and the two-column mobile difficulty preview.
- **Do** keep keyboard focus visible, honor reduced motion and contain dense horizontal content locally.
- **Do** use ordinary zh-TW/en/ja labels and real runtime covers or player-uploaded media.

### Don't:

- **Don't** copy upstream source, design assets, palettes or exact geometry.
- **Don't** add AI-generated art, decorative default imagery or poetic marketing slogans.
- **Don't** merge background controls with panel controls or replace user appearance with fixed visual values.
- **Don't** add an empty profile banner or decorative height when a player has no banner.
- **Don't** infer a passed score’s letter grade from accuracy when the API lacks the required information.
- **Don't** promote obsolete, unused styles or one-off small metadata values into the shared type system.
