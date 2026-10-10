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
  surface: "var(--content-surface)"
  surface-strong: "var(--content-surface-strong)"
  surface-raised: "var(--content-surface-raised)"
  surface-subtle: "var(--content-surface-subtle)"
  glass-surface: "rgb(24 24 27 / var(--panel-opacity))"
  glass-surface-strong: "rgb(32 32 36 / clamp(0, calc(var(--panel-opacity) + 0.15), 1))"
  glass-surface-raised: "rgb(46 46 52 / var(--panel-opacity))"
  glass-surface-subtle: "rgb(16 16 19 / var(--panel-opacity))"
  solid-surface: "rgb(24 24 27)"
  solid-surface-strong: "rgb(32 32 36)"
  solid-surface-raised: "rgb(46 46 52)"
  solid-surface-subtle: "rgb(16 16 19)"
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
  light-glass-surface: "rgb(255 255 255 / var(--panel-opacity))"
  light-glass-surface-strong: "rgb(255 255 255 / clamp(0, calc(var(--panel-opacity) + 0.15), 1))"
  light-glass-surface-raised: "rgb(246 246 246 / var(--panel-opacity))"
  light-glass-surface-subtle: "rgb(252 252 252 / var(--panel-opacity))"
  light-solid-surface: "rgb(255 255 255)"
  light-solid-surface-strong: "rgb(255 255 255)"
  light-solid-surface-raised: "rgb(246 246 246)"
  light-solid-surface-subtle: "rgb(252 252 252)"
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
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "32px"
    fontWeight: 800
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  display-mobile:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "26px"
    fontWeight: 800
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  headline:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "30px"
    fontWeight: 800
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  headline-mobile:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "26px"
    fontWeight: 800
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  title:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "19px"
    fontWeight: 900
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  section:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "17px"
    fontWeight: 800
    lineHeight: 1.4
    letterSpacing: "-0.025em"
  body:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.7
  body-mobile:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.7
  label:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "13px"
    fontWeight: 800
    lineHeight: 1.7
  metadata:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "12px"
    lineHeight: 1.7
  performance:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
    fontSize: "24px"
    fontWeight: 900
    lineHeight: 1.25
  read-body:
    fontFamily: "'Nunito Variable', var(--font-ui-cjk), sans-serif"
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
    backgroundColor: "{colors.surface-subtle}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "4px 12px"
    height: "36px"
  select-trigger:
    backgroundColor: "{colors.surface-subtle}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
    height: "38px"
  select-menu:
    backgroundColor: "{colors.glass-surface-strong}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "4px"
  dropdown-menu:
    backgroundColor: "{colors.glass-surface-strong}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "6px"
  floating-option-highlighted:
    backgroundColor: "{colors.selection-fill}"
    textColor: "{colors.readable-accent}"
    rounded: "{rounded.sm}"
    padding: "6px 8px"
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
    backgroundColor: "var(--section-surface-raised)"
    textColor: "{colors.foreground}"
    typography: "{typography.section}"
    padding: "12px 20px"
  catalogue-toolbar:
    backgroundColor: "var(--section-surface-strong)"
    textColor: "{colors.foreground}"
    rounded: "{rounded.lg}"
    padding: "12px 16px"
  catalogue-card:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.lg}"
    height: "124px"
  catalogue-card-mobile:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.lg}"
    height: "112px"
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
  profile-score-record:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.md}"
    padding: "1px 12px"
---

# Design System: Illyaverse — Serendipity

## Overview

**Creative North Star: "The Community Record"**

Illyaverse presents players, beatmaps and scores as connected community records. A pure-black default canvas supports thin neutral glass, joined identity and fact bands, aligned numeric values and rounded bilingual lettering. Strong headings, quieter metadata and accent underlines make location and selection clear without decorative copy.

Personalization is part of the material. The visitor can adjust theme, accent, background blur and dim, glass coverage, panel blur and opacity, and radius independently. Coverage selects which surface groups use glass; the remaining groups use deliberate solid counterparts. The viewed player’s public accent remains distinct from the visitor’s private appearance. Light mode uses neutral white and gray replacements, while zh-TW, en and ja share the same hierarchy and wrapping behavior.

The source composition studies of osu-web and Sunset inform recognizable community structure; Illyaverse’s source, palette and geometry are original. Runtime official beatmap covers and player-uploaded avatars, banners and backgrounds provide identity. The user-authorized transparent homepage character is a separate, original-byte asset above the plain black default background. Its provenance is recorded beside the image; it does not establish a decorative page background. A profile without a banner reserves no empty artwork region. Provenance is recorded in the website redesign record and the dense interface source record.

**Key Characteristics:**

- Pure-black default canvas with adjustable thin neutral glass and solid counterparts.
- Joined heading, toolbar, identity and record surfaces.
- Rounded Nunito Variable, TaiwanPearl and Zen Maru Gothic typography.
- Personal accent underlines, readable highlights and aligned performance facts.
- Real covers and player uploads with useful missing-image fallbacks.
- Paired light/dark semantic colors, meaningful labels and bounded overflow.
- Shared shadcn/Radix fields and floating menus with consistent material and keyboard states.
- Compact catalogue rows with difficulty detail available on demand.
- Purposeful route orientation and request feedback with reduced-motion alternatives.

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
- **Content Material** (`surface`, `surface-strong`, `surface-raised`, `surface-subtle`): live aliases select the content group’s glass or solid counterpart; a local identity, section, navigation or floating role rebinds the same four levels.
- **Neutral Glass** (`glass-surface`): the translucent base for selected surface groups.
- **Strong Glass** (`glass-surface-strong`): the persistent utility header, floating menus and prominent bands; opacity is increased and clamped.
- **Raised Glass** (`glass-surface-raised`): distinct section headings, toolbars and record hover emphasis.
- **Inset Glass** (`glass-surface-subtle`): route navigation, mode rails, inset performance columns and compact supporting content.
- **Solid Counterparts** (`solid-surface*`): the same neutral channels at full opacity, with no panel blur, for groups excluded by coverage.
- **Soft Silver and Secondary Gray** (`foreground`, `muted-foreground`): primary facts and subordinate labels.
- **Silver Seams** (`border`, `input`, `divider-strong`): quiet record divisions, field boundaries and major section separation.
- **Light Ground and Glass** (`light-*`): the semantic replacements for neutral white/gray surfaces and dark text. Keep the role mapping intact.

**The Personal Accent Rule.** Use the runtime accent and readable-accent roles for actions and emphasis; preserve the distinction between visitor appearance and a player’s public accent.

**The Separate Meanings Rule.** Location and selection use the personal accent. Status, grade and numeric difficulty use their own semantic roles and always retain readable labels.

## Typography

**Display and Body Font:** locally bundled Nunito Variable for Latin lettering, TaiwanPearl for Traditional Chinese and Zen Maru Gothic for Japanese. OFL-safe subset family names are `Illya Pearl` and `Illya Maru`; the locale selects their fallback order through `--font-ui-cjk`. TaiwanPearl uses weights 400 and 600, Zen Maru Gothic uses 400 and 700. Unicode-range WOFF2 subsets retain the eligible source repertoire for player names and dynamic content; the browser requests only the subsets needed on that page. Licensed source versions, full coverage and rebuild instructions are in [fonts.md](docs/fonts.md).

Rounded Latin, Traditional Chinese and Japanese lettering carry a compact, readable community interface. Keep Nunito for the Latin wordmark. Use native monospace for code and commands; comparable numeric data uses tabular figures where its component requests them.

### Hierarchy

- **Display:** the home wordmark overrides the base display roles with a contextual treatment (80px, weight 900, line-height 1.05, tracking -0.035em), reducing to 64px at 1000px, 56px at 800px, 36px at 600px and 32px at 360px. Solid foreground lettering forms one unbroken title, with the right-hand letters filling into readable accent. The supplied character has its own bounded decoration region beside the introduction.
- **Headline:** general page headings use `headline` and `headline-mobile`. The compact catalogue toolbar uses a contextual 24px heading with line-height 1.25; profile identity uses a contextual clamp from 26px to 34px and a 21px–26px clamp on mobile. Score detail has its own 26px–40px clamp.
- **Title:** profile sections use `title`; home record headings use the smaller `section` role, reducing to 16px on mobile.
- **Body:** the document base uses `body`, reducing to `body-mobile` at 600px. Supporting paragraphs use contextual 13–14px treatments; home copy uses line-height 1.6 and a 65ch measure, profile biography 75ch.
- **Label and Metadata:** mode labels use `label`; metadata commonly uses `metadata`. Shared selects use 13px semibold labels; link-rendered primitive buttons retain their 14px utility typography. Dense score metadata has contextual compact treatments rather than adding steps to the shared ramp.
- **Performance:** `performance` remains the prominent numeric role. The compact profile score list uses a contextual 20px value through desktop and mobile. Major score detail performance is 48px, reducing to 40px at 640px; keep its unit smaller.
- **Read Body:** Markdown prose uses `read-body`; article headings have the observed 36px/24px/19px hierarchy and code uses 12px.

**The Bilingual Breathing Rule.** Preserve line-height, meaningful mode labels and wrapping when translating; let the layout adapt before compressing text.

## Layout

Community pages and the persistent header share a centered maximum width (1200px), with desktop gutters (32px) and mobile gutters (16px). Main page padding is 32px above and 48px below; at 600px it becomes 24px above and 36px below. Narrow utility pages retain an 850px maximum, Markdown articles 920px and authentication 1060px. These are contextual reading widths, not extra community grids.

The utility header is 60px tall. Above 800px a separate route row adds 44px, with underlined active links. Its top seam and the header's bottom border make the complete shell 106px tall; mobile retains the 60px utility row and its one-pixel border. The shared header-height variable is therefore 106px desktop and 61px at 800px. Header actions, route links, action-menu options and sheet close controls retain a minimum 44px target; the compact language trigger is 38px tall on desktop and 44px at 700px and below. At 800px and below route navigation moves into the menu sheet. Profile section navigation joins the masthead and performance record, then sticks below the shared header height. Section anchors add 65px to that value, giving clearance of 171px desktop and 126px mobile.

Use tight 4–8px control spacing, 12–16px nearby-action gaps, 20–28px group gaps and 24–32px section padding. Joined headings usually have less vertical padding than their content. Home pairs flexible records with a 308px support column, narrowing to 276px at 1000px and stacking at 800px. Profile performance joins a flexible chart and a 294px statistics column, stacking at 900px. These compositions describe those surfaces rather than requiring every page to have a sidebar.

The dense home uses page padding (24px 32px 40px), joined record headings (12px 20px), and map rows with a minimum height (104px), a 72px cover and 12px gaps. At 600px map-row padding becomes 8px 16px while retaining that minimum height. Catalogue search and four filters share one toolbar: above 1050px the search and filters sit beside one another; below that width the search stacks above the four filters. The toolbar uses 12px vertical padding and one material layer. Catalogue results use two columns with a 10px gap and equal-height cards (`catalogue-card`); single-column results use an 8px gap at 780px. Grid and list presentation share the same compact card height.

### Responsive behavior

- **At 800px:** home content stacks; route navigation becomes a menu sheet; profile sticky navigation uses the mobile header offset. Profile scores give grade and a wider, two-line title the top row, with a footer spanning their area: mods and accuracy/combo remain at the left, performance/contribution at the right, and a 44px action target beside the record. Full dates, hit counts and extended facts remain available in the menu.
- **At 780px:** catalogue results become one column and map detail areas stack.
- **At 640px:** score detail identity, performance and fact bands rearrange into compact rows and a two-column fact grid.
- **At 600px:** gutters, padding and contextual type reduce; the utility header uses an 8px gap and displays the compact brand mark, with the account's direct profile link available through mobile navigation. Dense home page padding becomes 16px 16px 32px and home actions wrap. The hero's character region is 180px × 198px, with a 300px-wide original image rising above the introduction; descriptive copy and actions follow the wordmark and crop. Profile identity uses an 80px avatar rather than 108px, preserving half-overlap when a banner exists. Catalogue filters use two columns; cards use `catalogue-card-mobile` with an 82px cover, single-line identity tracks and a 44px difficulty-summary control. Complete read-only difficulty rows open in a viewport-bounded popover rather than expanding each card.
- **At 420px:** catalogue covers narrow to 70px. Supporting difficulty names truncate within their bounded rows while preserving the complete name in the title attribute. At 360px the home character region becomes 150px × 188px, with a 270px-wide image and a 32px wordmark.
- **Dense records:** rankings preserve a minimum width (650px); score tables preserve 860px, or 1040px for weighted best-score collections, inside local horizontal wrappers. Markdown tables preserve 640px. Do not shrink these factual columns into unreadable cells.

**The Local Overflow Rule.** Keep dense tables, technical strings and long navigation inside their own scrollable regions; the document must remain within the viewport.

## Elevation & Depth

Depth comes from neutral material levels, seams and joined structure. Glass coverage is an independent 0–100 slider, defaulting to 100. At 0–24, navigation and floating controls retain glass; identity joins at 25, section headings/toolbars and chart sections at 50, and content records at 75. Excluded groups use their opaque neutral counterparts and zero material blur. Coverage selects groups rather than changing opacity or blur.

Selected glass groups use the live panel blur. Panel blur defaults to 8px with a 0–48px range; opacity defaults to 55% with a 0–100% range. Strong glass adds 0.15 to the same opacity and clamps it to 0–1. Base, raised and inset levels share the chosen opacity with different neutral channels. Navigation, sheets, Select, DropdownMenu, popovers and dialogs share the floating/navigation glass roles even at minimum coverage; a portaled menu inherits the theme-root material variables. Avoid nesting a second backdrop filter on fields inside an already filtered container. Glass panels do not carry decorative shadows.

A player background uses a fixed cover layer scaled to 1.05, with separate scene blur and a theme-aware dim veil. Background defaults are 0px blur and 0.76 dim; ranges are 0–48px and 0–1. These effects leave foreground text outside the image filter.

### Shadow Vocabulary

- **Control seam:** outline buttons retain the small Tailwind shadow (`0 1px 2px 0 rgb(0 0 0 / 0.05)`); shared fields and floating controls explicitly remove it.
- **Selection underline:** the profile mode rail uses one accent bottom border (3px) and explicitly removes its selected-state shadow; this marks selection rather than elevation.
- **Overlay panels:** shared floating-control styling explicitly uses `box-shadow: none`; material, border and overlay establish separation.

State transitions stay short: links change color in 180ms and score-row emphasis in 160ms ease-out. Motion moves the active route underline with a shared layout spring (stiffness 450, damping 38). Supported browsers crossfade the complete root for 180ms with the implemented easing; the unchanged persistent header stays visually anchored. The main element has no permanent view-transition name, which would create a backdrop root and stop its glass panels sampling the global scene. The home wordmark keeps solid foreground letters visible while a single left-to-right wipe fills its right-hand letters into readable accent over 680ms, then remains fully filled. The supplied character remains static. The global reduced-motion preference presents the final wordmark and disables view-transition animation; Motion honors the same preference. Request feedback retains distinct pending and completion states without animated progress or opacity transitions.

**The Independent Material Rule.** Background blur and dim control the scene; coverage selects glass surface groups; panel blur and opacity control their material. Keep all five controls and effects independent.

**The Solid Counterpart Rule.** Every group excluded by coverage uses the corresponding opaque surface levels and zero material blur; never expose content directly over an arbitrary uploaded background.

## Shapes

The live radius defaults to 14px and ranges from 0–40px. Small and medium shared corners scale from it; large surfaces use it directly, and extra-large corners scale upward. Difficulty controls retain their implemented compact 8px corners. Avatars are rounded rectangles and badges are pills.

Joined sections keep rounding on the outer silhouette. Internal heading/content seams flatten their touching corners; profile identity, mode and rank bands share one enclosing record, and the section rail touches its masthead and performance record. A banner’s avatar overlaps its lower edge by half the avatar height, with a background-colored outline separating the image. Quiet one-pixel borders separate facts without converting every fact into another card.

**The Joined Record Rule.** Connect related heading, controls and content within one outer silhouette; use seams and surface levels to show internal structure.

## Components

### Buttons

Compact, direct actions use accent-filled primary, bordered outline and transparent ghost variants. Shared defaults are captured in frontmatter; home actions have a minimum height (36px) and wrap as needed. Catalogue search actions use 36px on desktop and 44px on phones; difficulty summaries, detail actions and persistent header actions retain 44px targets. Outline buttons use an opaque light background and an input-tinted dark surface. Hover shifts the assigned fill; focus retains an explicit ring; disabled controls reduce opacity and interaction. Button-rendered primitives inherit the body font, while anchor-rendered primitives carry the smaller utility label.

**The Inset Feedback Rule.** Dense score-menu and catalogue-summary controls preserve targets of at least 44px while insetting visual feedback by 6px. The square score-menu target therefore paints a 32px plane; wider summaries keep their width around the facts. This plane uses a 6% foreground veil on fine-pointer hover and an 8% veil while open or pressed, with a two-pixel readable-accent keyboard outline. Its corners cap at 8px while following the user's radius, and its 160ms fill transition disappears with reduced motion. Keep the target stationary and its outer background transparent.

### Chips

Secondary metadata badges use paired secondary colors and pill corners. Difficulty selectors join a tier marker, version and actual star value. Default selectors use the inset material; hover and current state tint the surface with the difficulty color and strengthen its border. Catalogue summaries combine up to eight colored ticks and a count in one 44px target. Their popover presents complete known difficulty metadata as read-only rows with mode, version and actual star value; it does not navigate or import. Selected difficulty controls on the full set selector retain `aria-current` or `aria-pressed` semantics.

### Cards / Containers

A record uses the base material for its assigned coverage group, a quiet border and the live radius. Joined section headings use the raised level; prominent identity and score headers use the strong level; support rails and key numeric columns use the inset level. Common content padding is 24px, with surface-specific 18–28px padding and tighter mobile values. Hover emphasizes a record with the appropriate existing surface level. The group determines glass or solid; the level determines its neutral tone.

Catalogue cards use a fixed compact height with a full-height cover beside four bounded single-line tracks: title, artist, mapper and facts. Their main grid has a bounded flexible row and zero minimum content height, keeping identity within the card. A bottom status/difficulty band reserves 44px. Long identity text truncates visually while titles and links retain complete identity. Actual official cover art also appears as a faint card backdrop (14% opacity); missing covers keep the icon fallback and geometry. Home map records place restrained neutral hover/focus feedback across the full row (4% foreground), with content and divider insets (20px desktop, 16px mobile). The home compact-record rules explicitly retain 72px desktop covers and 56px × 72px mobile covers despite later shared record styling. The containing panel supplies the material and blur; the row adds no second glass layer.

### Inputs / Fields

Fields use semantic foregrounds, inset material, input borders and medium corners. Shared shadcn/Radix Select triggers are at least 38px tall by default; chart contexts retain their contextual compact size. Catalogue search, submit and filter controls align at 36px on desktop and 44px at 600px and below. The search icon is vertically centered within its field. Labels sit above the fields, values truncate within bounded triggers, and keyboard focus, invalid and disabled states remain visible. Numeric outputs use tabular figures.

The catalogue toolbar uses its section group's strong material. Search and Select fields inside it use a thin neutral veil (4% foreground, 8% on Select hover), without another backdrop filter; supporting copy and placeholders retain the theme foreground. The section's coverage tier determines glass or solid material.

### Floating Controls

Use the shared shadcn/Radix Select and DropdownMenu primitives for option selection and action menus. Portaled content uses strong glass and medium corners, with no shadow and a maximum width of the viewport minus 24px. Select lists retain scroll controls and collision padding (12px); action menus retain their own nearby placement and are nonmodal by default. Select options retain a 36px minimum; action-menu options have a 44px minimum. Highlighted items use accent selection fill and an inset ring; checked options use readable accent, weight 800 and an SVG check. Disabled items retain their label and reduced emphasis. Preserve Radix keyboard navigation, selection, Escape dismissal and focus return.

The catalogue difficulty popover uses the smaller of 340px and Radix's available inline size, with 12px collision padding and the floating material. Its height is bounded by 380px and the available viewport height, with local vertical scrolling. Mouse hover, keyboard focus and touch activation open the same read-only preview without stealing focus. Official catalogue cards already carry their complete returned difficulties; retained server cards request the complete known set on opening. Closing, changing identity or retrying aborts and invalidates outstanding work. Pending and failed requests clearly label the stored-summary fallback, recoverable errors offer Retry, and withdrawn sets show no stale difficulty rows. The main card identity remains the set navigation action. The page reserves a stable native scrollbar gutter; modal sheet scroll locking does not add a second gutter, and nonmodal action menus leave page scrolling available.

### Navigation

The utility header groups site identity, locale, one Appearance button and account actions; the route row carries ordinary route names. Appearance opens the shared controls sheet, where dark, light and system remain available alongside the independent material controls. Active routes use readable accent text and a two-pixel accent underline that moves between links. Inactive client tabs remain plain; focus keeps its explicit keyboard outline. Mode selectors use underlines and meaningful, unbroken names; the profile's selected mode has one 3px accent border with no inset shadow. Profile section navigation is sticky, locally scrollable and tracks location with `aria-current`; mobile navigation remains available through the sheet.

Language selection uses the nonmodal shared DropdownMenu material, full option names (繁體中文, English, 日本語) and one selected SVG check. Opening focuses the selected option. Saving shows a busy disabled trigger and restores focus to the enabled trigger when focus has not moved elsewhere; the existing cookie and account preference remain authoritative. Country identity pairs a readable code with a self-hosted original Twemoji flag (24px × 18px), a localized accessible country name and image-failure fallback. Preserve the pinned Twemoji attribution and license with those assets.

Navigation retains Next.js anchor semantics and prefetching. Selecting the exact current URL is a no-op; hash-only changes retain ordinary anchor behavior. Same-origin route changes can use a native whole-root View Transition when supported, visible and motion is allowed; a slow route releases the snapshot after 700ms while loading feedback continues. A 3px top bar is visible from the first pending frame and reflects outstanding route work, initial SWR loads and explicit catalogue-preview/official-search requests. Cached SWR background revalidation does not activate it. The translated accessible pending state ends when actual work settles or unmounts. Visually, the bar starts at 12%, advances only below 90% while pending, then reaches completion after a minimum visible interval (220ms) and keeps a short completion tail (180ms). Reduced motion holds the pending segment still and shows completion without transitions; it does not replace real pending state with a full-width loading line.

### Homepage Wordmark

One accessible heading keeps Illya in solid foreground and places solid foreground verse beneath a readable-accent ink layer in the same inline grid cell. The decorative ink layer is hidden from assistive technology and wipes from left to right once over 680ms; its final opacity is 1 and its clip is fully open, leaving verse filled in readable accent. Reduced motion presents that final form immediately. The complete title stays visible without waiting for animation, and the wordmark uses no circle, vector arcs or text stroke.

### Homepage Character

Use the user-supplied transparent character only in the authorized home introduction. The asset preserves its original 700 × 1251 PNG bytes and alpha. A separate decoration layer rises above the rounded, clipped glass introduction and crops before the adjoining capability links. Desktop uses a 480px-wide region and image, beginning 112px above the panel with the image offset another 64px; at 1000px the region is 340px wide and image 400px wide, beginning 102px above the panel with a 56px image offset. At 800px the region narrows to 280px with a 360px image and a 50px image offset. Phone geometry follows Layout. CSS presentation leaves the plain default background intact. The static character has empty decorative alternative text, does not intercept pointer input, and stays separate from factual copy and actions. Preserve the adjacent provenance file when changing its presentation.

### Official Map Search

The import page joins a short search toolbar and divider-led result rows in one section material. Shared fields use 38px desktop controls and 44px phone targets, with thin neutral fills and no nested backdrop filter. Results pair small covers with bounded identity and tabular difficulty facts; selecting a result expands its details inline. Deliberate submit, cursor paging and localized loading/empty/error recovery keep feedback at the result location. Choosing a set or difficulty fills the existing manual import form and moves focus to the difficulty identifier or archive file field, with a localized confirmation. Official summaries remain separate from current storage and withdrawal facts. This metadata handoff preserves inactive drafts and archive choices; publication and a complete `.osz` remain explicit manual actions.

### Community Records

Player, map and score records put identity beside aligned facts and nearby actions. Rankings right-align comparable numeric columns and leave player identity to the left. Map records use genuine runtime covers with an original icon fallback. Profiles render a banner only when one exists. Score verification, grade and difficulty retain text and semantic color; unavailable facts stay unavailable rather than being visually inferred.

Map detail joins cover, identity and full-set actions to the current difficulty's fact rail with one shared outer silhouette and an internal seam; on narrow screens the same joined structure stacks. The normal download label describes a complete `.osz` set. A retained archive takes priority; official provider fallback identifies osu.direct and retains an official-page action, while a local raw-only map shows a disabled package action and an honest unavailable explanation.

### Profile Identity and Placement

The avatar occupies half its height over a real banner and remains in the identity grid when no banner exists. Identity, mode rail and rank band lead directly into the sticky section rail. Global and country placement values use the user-requested rank-dependent gradient text, with original paired colors for first, top ten, top hundred, top thousand, top ten thousand and other validated ranks. These are absolute placement bands, not population percentiles. An invalid rank or nonpositive performance renders the ordinary unranked label; forced-colors mode uses CanvasText. This explicit exception applies only to profile rank values and does not establish gradient text for headings, actions or other facts.

### Rank History

The compact chart plots the first through last genuinely observed dates, retaining their real date spacing. Its desktop height is 182px and mobile height 160px. Missing days break the line, unranked observations remain distinct, and a country change breaks country-rank continuity. The requested 30/90/180-day period remains available in the controls and expandable daily table, alongside collection details. Pointer selection, arrow keys and Home/End expose the same selected date and facts; the text/table representation remains available.

### Profile Score Records

Compact rows join a visible grade, numeric star difficulty, title/version/date, mod labels, aligned accuracy/combo/hit counts and right-aligned performance. Desktop rows retain a tight rhythm around their facts and a 44px action target. Mobile gives the title its own wider area with at most two lines, then groups the secondary facts in a footer; supporting labels remain readable rather than shrinking to preserve the desktop columns. A genuine runtime cover sits behind identity, masked toward the factual columns; it is subordinate to the assigned glass or solid material. Missing or failed cover requests leave the same readable row. Best-score position and weighted contribution appear only when provided. The action menu preserves score navigation, replay availability, raw score, complete mods, date, verification and hit counts when the compact row reduces them. Grade is taken from the API; an unavailable grade uses an SVG fallback rather than an invented letter.

## Do's and Don'ts

### Do:

- **Do** use semantic foreground/surface pairs and runtime accent variables in both themes.
- **Do** assign each surface its navigation/floating, identity, section or content material group and preserve its solid counterpart.
- **Do** use shared shadcn/Radix Select and DropdownMenu controls with visible checked, highlighted, focus and disabled states.
- **Do** connect entity identity, facts, controls and records with clear surface levels and seams.
- **Do** align comparable numeric values and use tabular figures for ranks, scores and control outputs.
- **Do** retain labels alongside status, grade and difficulty color, and show only facts provided by the running API.
- **Do** preserve 44px difficulty-summary and score action targets, complete read-only card previews and every available difficulty link in the full set selector.
- **Do** keep keyboard focus visible, honor reduced motion and contain dense horizontal content locally.
- **Do** use ordinary zh-TW/en/ja labels, real runtime covers, player-uploaded media and the explicitly authorized original homepage character.
- **Do** keep route motion purposeful and request feedback tied to outstanding work, including a reduced-motion presentation.

### Don't:

- **Don't** copy upstream source, design assets, palettes or exact geometry.
- **Don't** add AI-generated art, decorative default backgrounds or poetic marketing slogans; the authorized homepage character does not change that background rule.
- **Don't** merge coverage, background or panel controls, or replace user appearance with fixed visual values.
- **Don't** extend the explicitly requested profile rank gradient text to other typography or describe absolute rank bands as percentiles.
- **Don't** add an empty profile banner or decorative height when a player has no banner.
- **Don't** infer a passed score’s letter grade from accuracy when the API lacks the required information.
- **Don't** promote obsolete, unused styles or one-off small metadata values into the shared type system.
