---
name: Illyaverse — Serendipity
description: Illyaverse appearance system with a black default background and personal glass controls.
colors:
  primary: "#f28eb6"
  primary-foreground: "#111111"
  primary-hover: "color-mix(in oklab, var(--primary) 90%, transparent)"
  readable-accent: "color-mix(in srgb, var(--user-accent) 40%, #f5f5f5)"
  page-ground: "#000000"
  background: "#080808"
  foreground: "#f5f5f5"
  card: "#141414"
  popover: "#191919"
  secondary: "#262626"
  secondary-foreground: "#e8e8e8"
  muted: "#222222"
  muted-foreground: "#b5b5b5"
  accent: "#2c2c2c"
  accent-foreground: "#f5f5f5"
  destructive: "#f294a0"
  border: "#ffffff16"
  input: "#ffffff25"
  surface: "rgb(12 12 12 / var(--panel-opacity))"
  surface-strong: "rgb(12 12 12 / clamp(0, calc(var(--panel-opacity) + 0.15), 1))"
  ranked: "#93d7be"
  loved: "#edb0d1"
  pending: "#e8cd99"
  light-readable-accent: "color-mix(in srgb, var(--user-accent) 30%, #262626)"
  light-page-ground: "#f5f5f5"
  light-background: "#fafafa"
  light-foreground: "#262626"
  light-card: "#ffffff"
  light-secondary: "#e8e8e8"
  light-secondary-foreground: "#444444"
  light-muted: "#eeeeee"
  light-muted-foreground: "#606060"
  light-accent: "#e2e2e2"
  light-destructive: "#a1233e"
  light-border: "#00000020"
  light-input: "#00000030"
  light-surface: "rgb(255 255 255 / var(--panel-opacity))"
  light-surface-strong: "rgb(255 255 255 / clamp(0, calc(var(--panel-opacity) + 0.15), 1))"
  light-ranked: "#246346"
  light-loved: "#984168"
  light-pending: "#785018"
typography:
  display:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "40px"
    fontWeight: 800
    lineHeight: 1.35
    letterSpacing: "-0.035em"
  display-mobile:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "32px"
    fontWeight: 800
    lineHeight: 1.35
    letterSpacing: "-0.035em"
  headline:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontWeight: 800
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  title:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "1.4rem"
    fontWeight: 800
    lineHeight: 1.3
    letterSpacing: "-0.025em"
  body:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.7
  label:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "0.875rem"
    fontWeight: 500
    lineHeight: "1.25rem"
  field-label:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "13px"
    fontWeight: 800
    lineHeight: 1.7
  metadata:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.7
  badge:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "0.75rem"
    fontWeight: 500
    lineHeight: "1rem"
  read-body:
    fontFamily: "'Nunito Variable', 'Noto Sans TC', sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.95
rounded:
  sm: "6.3px"
  md: "9.1px"
  lg: "14px"
  xl: "16.8px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  control-gap: "12px"
  md: "16px"
  section-gap: "20px"
  lg: "24px"
  xl: "32px"
  panel: "36px"
  spacious: "40px"
  section: "48px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.primary-foreground}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
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
    typography: "{typography.badge}"
    rounded: "{rounded.full}"
    padding: "2px 8px"
  glass-panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.foreground}"
    rounded: "{rounded.lg}"
    padding: "{spacing.lg}"
  mode-control:
    backgroundColor: "{colors.background}"
    textColor: "{colors.muted-foreground}"
    rounded: "{rounded.md}"
    padding: "{spacing.xs}"
  avatar:
    backgroundColor: "color-mix(in srgb, var(--user-accent) 15%, var(--background))"
    textColor: "{colors.readable-accent}"
    rounded: "{rounded.md}"
    size: "32px"
---

# Design System: Illyaverse — Serendipity

## Overview

**Creative North Star: "Black Ground and Personal Glass"**

Illyaverse is a personal rhythm community with the familiarity of an osu! catalogue and rounded bilingual lettering. A pure-black default ground, translucent neutral gray-black glass, a player-selected accent and restrained silver outlines keep its anime-compatible interface readable. The landing page uses a compact introduction; community tools use compact rows, direct labels and measured headings.

Personalization is a material rule. Players can change the theme, accent, background blur and dim, panel blur and opacity, and glass radius independently, while semantic foreground colors keep the interface coherent. Background imagery comes only from player uploads; the default world carries no artwork. This document records the built Serendipity frontend, including its neutral light theme, rather than promising native-client compatibility.

**Key Characteristics:**

- Pure-black default ground with translucent neutral gray-black glass.
- Rounded Latin and Traditional Chinese typography with a strong reading hierarchy.
- A player-selected accent shared by actions, focus and readable highlights.
- Independent controls for background treatment, panel material and radius.
- Compact catalogue rows, quiet dividers and purposeful negative space.
- Responsive navigation, intact mode labels and locally scrollable dense tables.

The normative frontmatter records default values extracted from `apps/serendipity/src/app/globals.css`, the shadcn primitives and `providers.tsx`. Runtime CSS custom properties remain authoritative for personalized values. Typography entries with fluid sizes intentionally leave `fontSize` out of the portable token object; the actual formulas are recorded below. The companion `.impeccable/design.json` adds previews, motion, depth and breakpoint metadata without a second primitive-token catalogue. Its tonal ramps are preview aids, not additional shipping colors. Format reference: [official DESIGN.md specification](https://raw.githubusercontent.com/google-labs-code/design.md/main/docs/spec.md).

## Colors

The palette keeps the default raspberry accent within neutral black and gray surfaces; the light theme uses neutral gray and white replacements.

### Primary

- **Raspberry** (`primary`): the default user accent for primary actions, selected emphasis and keyboard focus. The appearance controls also offer lavender, mint and apricot choices; these are player choices, not competing brand accents.
- **Readable Raspberry** (`readable-accent`): the accent mixed toward the foreground for linked text, headline emphasis, avatar initials and score highlights. The mix changes in light mode through `light-readable-accent`.
- **Primary Ink** (`primary-foreground`): text on the default pale accent. The provider switches this value to white for a sufficiently dark custom accent, using its calculated relative luminance threshold (0.179).

### Neutral

- **Black Ground** (`page-ground`): the default canvas. `background` provides a slightly lifted opaque ground for fields, selectors and code.
- **Neutral Glass** (`surface`, `surface-strong`): the translucent container and denser sticky-header materials. Their opacity comes from the live panel-opacity control. `card` and `popover` serve opaque component layers.
- **Soft Silver** (`foreground`): primary text. **Secondary Gray** (`muted-foreground`) carries descriptions, metadata and secondary navigation.
- **Gray Layers** (`secondary`, `muted`, `accent`): selected controls, subdued regions and hover states. Their matching foreground tokens stay paired with them.
- **Silver Seams** (`border`, `input`): quiet dividers and field outlines; preserve their source alpha values.
- **Light Ground and Glass** (`light-*`): semantic replacements applied by the `.light` theme. Light card and popover surfaces are white; the page ground and supporting layers are neutral grays.

### Semantic status

Ranked mint, loved pink and pending amber label beatmap states. Their `light-*` counterparts darken the text in light mode. Destructive feedback uses the separate `destructive` role and its light counterpart. Status labels retain text as well as color.

**The Personal Accent Rule.** Use the shared runtime accent and readable-accent roles for new surfaces; do not hard-code raspberry into personalized actions or text.

**The Paired Theme Rule.** Choose semantic surface and foreground pairs so light mode and user-selected accents remain readable together.

## Typography

**Display and body font:** locally bundled Nunito Variable, followed by Noto Sans TC and a generic sans-serif fallback. The layout imports the variable Nunito package plus Noto Sans TC at weights 400 and 600. Latin letterforms are rounded; Chinese text follows the Noto fallback and the browser's available weight behavior. Do not claim a distinct Chinese display font.

**Technical text:** native monospace for commands and code. Numeric rankings, scores and slider outputs use tabular figures where their existing components request them.

### Hierarchy

- **Display:** the compact landing headline uses the `display` token, with the `display-mobile` token at 600px and below. The subtitle uses 17px, weight 700 on desktop; supporting copy uses 14px and line-height 1.8. These labels identify the site and its available actions directly.
- **Headline:** general page headings use `clamp(2rem, 3.6vw, 3.25rem)`, weight 800, line-height 1.3 and balanced wrapping. Small-screen page headings use 31px. Auth, profile and detail views have contextual heading sizes rather than a universal fixed h1.
- **Title:** common h2 headings use the `title` token. Section and form contexts generally range from 19px to 27px; keep their scale subordinate to the page heading.
- **Body:** the default body uses the `body` token, reduced to 14px at 600px and below. Supporting copy generally uses 13–16px with generous line-height (1.8–2). Guide paragraphs have a 70ch measure; hints have a 65ch measure.
- **Read body:** Markdown articles use the `read-body` token, with h1/h2/h3 sizes of 36px/24px/19px and larger vertical separation. Code remains 12px.
- **Label:** link-rendered shadcn buttons use the `label` token. Native buttons and fields inherit body typography through the global font shorthand unless a feature rule overrides it; their declared Tailwind type utilities do not win that cascade. Field labels are smaller and heavier; metadata uses the `metadata` token. Catalogue tags may shrink in dense rows, so they remain secondary information rather than primary controls.

**The Bilingual Breathing Rule.** Preserve the built line-height and wrapping behavior when translating; do not squeeze Chinese copy or break game-mode labels to force a desktop arrangement onto mobile.

## Layout

The shell is centered and fluid within explicit maximum widths. The header is capped at 1320px, landing and footer at 1256px, community pages at 1192px, narrow pages at 850px, Markdown articles at 920px and authentication at 1060px. Desktop page gutters are 32px; small-screen gutters are 16px. Common page padding is 52px above and 60px below, reduced to 32px and 40px on small screens.

Spacing is a practical rhythm rather than a rigid grid. Four- and eight-pixel steps hold tight controls; 12–16px gaps connect nearby actions; 20–24px gaps separate groups; 32–48px padding gives sections room. Glass content panels use 24px padding, settings panels 36px, and reading articles 48px. Small-screen content panels use 12px, settings panels 24px 18px, and reading articles 24px.

The desktop landing body pairs a flexible catalogue with a 290px connection column, reduced to 250px at 1100px. Profiles pair a 270px about column with a flexible score column; connection guides pair a 210px sidebar with a flexible reading area. Chat uses a 220px channel rail. These are surface-specific compositions, not a universal sidebar requirement.

The landing introduction is a compact glass panel with a 270px desktop minimum height and 32px 36px padding. At 1100px and below its padding is 32px; at 860px and below its minimum height is 260px; at 600px and below it has no minimum height and uses 24px padding. Its tools column groups ordinary community, account and appearance links before the connection guide.

### Responsive behavior

- **At 1100px and below:** header spacing and navigation type tighten; the landing connection column narrows and the username label hides.
- **At 860px and below:** desktop navigation becomes a menu sheet, header height drops from 76px to 68px, landing/profile/guide/auth layouts become single-column, and guide navigation becomes a horizontal rail.
- **At 600px and below:** gutters and type reduce, action groups wrap, authentication padding tightens and chat channels become a horizontally scrollable row. Profile score headings stack above full-width mode controls whose labels stay on one line.
- **Tailwind component threshold:** the sheet uses the `sm` threshold (40rem), alongside the page's explicit pixel breakpoints. Input classes also declare a medium-threshold type change, but the global font inheritance wins in the rendered fields.
- **Dense content:** data tables stay on one line in horizontal scroll wrappers. Markdown tables keep a minimum width of 640px inside labeled, keyboard-focusable regions. Commands retain intact strings with local horizontal scrolling. The page itself should not acquire horizontal overflow.

**The Local Overflow Rule.** Preserve meaningful labels and technical strings; scroll the bounded dense region before compressing its content or widening the document.

## Elevation & Depth

Depth comes primarily from translucency, tonal layering and quiet borders. Glass containers, the sticky header, sheets, select menus and the settings tab track share `backdrop-filter: blur(var(--panel-blur))`. Panel blur defaults to 8px and ranges from 0 to 48px. Panel opacity defaults to 55% and ranges from 0% to 100%, represented as 0–1 in preferences. The header's strong surface adds 0.15 to the same opacity and clamps the result to 0–1, giving a 70% default. Dark glass uses neutral 12/12/12 RGB; light glass uses white. Panel blur and opacity remain independent.

The default ground has no image; a chosen player background sits in a fixed cover layer, enlarged slightly (1.05), with a separate theme-aware dim veil. Background blur ranges from 0 to 48px and dim from 0 to 1; defaults are 0px and 0.76. These scene controls remain separate from panel controls and do not blur readable foreground content.

### Shadow vocabulary

- **Control seam:** the shadcn outline button and input carry the small Tailwind shadow (`0 1px 2px 0 rgb(0 0 0 / 0.05)`).
- **Selected tab:** default tabs add Tailwind's small composite shadow (`0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)`).
- **Sheet surface:** the sheet primitive declares a larger Tailwind shadow, but the shipped shared sheet/select rule overrides it with `box-shadow: none`. Sheets retain the half-black overlay and shared panel material.

Glass panels, sheets and select menus have no default box shadow. Reuse their material and seams for container depth.

**The Glass Before Shadow Rule.** Establish container depth through the shared translucent surface, panel blur and border; reserve the existing small shadows for control states.

**The Independent Material Controls Rule.** Keep background blur and dim separate from panel blur and opacity; use the shared panel variables for every surface that carries the glass material.

## Shapes

The default user radius is 14px. Component radii derive from that live value: small at 0.45 times, medium at 0.65 times, large at the full value and extra-large at 1.2 times. The frontmatter records their default resolved dimensions; implementations keep the runtime formulas so the appearance control's 0–40px range remains effective. Badges and swatches are fully rounded. Small inline permission and code labels retain their existing fixed 4px corners; the active navigation underline has 3px corners.

Profiles use joined banner and identity containers with complementary top and bottom rounding. Authentication joins its desktop decorative text panel to the form; the form regains complete rounding when the side panel disappears on mobile. These silhouettes express grouping rather than adding unrelated card treatments.

**The Shared Radius Rule.** New glass, fields and buttons follow the live radius family; preserve fixed corners only where the existing small label or navigation pattern calls for them.

## Components

### Buttons

Compact, clear actions with soft corners. The default size is 36px high with 8px vertical and 16px horizontal padding; icon-bearing buttons reduce horizontal padding to 12px. Large buttons and landing actions are 40px high. Landing actions use 20px horizontal padding, reduced to 16px on small screens. Primary actions use the live accent with calculated contrasting text; secondary actions use their paired neutral roles.

Computed native buttons inherit body type (15px, weight 400, line-height 1.7 on desktop; 14px on small screens). Link-rendered buttons retain the 14px, weight-500 utility type with 20px line-height. Feature styles add their own emphasis, such as the weight-700 landing actions. This native-versus-anchor distinction is an observed cascade effect, not a new hierarchy rule for future surfaces.

Primary hover reduces the fill opacity to 90%. Outline buttons use a quiet input-colored dark fill, a border and control seam; in light mode they use the opaque background role and accent hover. Ghost buttons are transparent at rest and acquire an accent-toned hover. Disabled controls lose pointer interaction and use 50% opacity. Destructive variants stay tied to destructive roles.

### Chips and segmented controls

Secondary badges are compact pills (2px 8px padding), carrying map difficulty or account-role text. Mode and theme controls sit inside a bordered background track with 4px padding and gap. Their 13px, weight-700 buttons use small-radius corners; the pressed state gains a secondary fill and foreground text. Mobile mode buttons use 12px type, intact labels and bounded overflow.

### Cards and containers

The reusable glass material combines the live `surface` role, a one-pixel `border` seam, the full live radius and shared panel blur. Padding belongs to the content pattern: 24px for general panels, 36px for settings, and larger reading padding for articles. Map catalogues use divider-led rows with 18px gaps and a restrained accent mix on hover (4%); dense tables use a 3% row hover mix. Do not infer that every content group needs a glass box.

### Inputs and feedback

Fields are full-width, 36px-high controls with medium-radius corners and quiet borders. Their computed type inherits the body (15px on desktop and 14px on small screens, with line-height 1.7). Dark fields use a 30% input-color fill; light fields are transparent. Search fields reserve 38px left padding for an inline 17px icon. Textareas share this vocabulary, with content-based sizing and a 64px minimum height; chat applies its own bounded height.

Focus combines a three-pixel half-opacity accent ring with the global two-pixel outline and four-pixel offset. Focused invalid fields use a destructive ring. The current unlayered universal border-color rule overrides the primitives' input, focus and invalid border-color utilities, so computed seams stay neutral; this cascade drift is recorded as evidence, not a rule for future error treatments. Form errors appear near the form as alert text; success feedback uses readable accent and a status role. Disabled fields use 50% opacity. Keep labels, hints and feedback explicit.

### Navigation and sheets

The sticky header uses stronger glass, a bottom seam and compact tools. Desktop links use 14px, weight 700 and secondary text; hover and active links brighten to foreground, with a two-pixel accent underline for the active route. Mobile navigation uses a menu sheet with padded links and a secondary active fill. The shell includes a visible-on-focus skip link. Locale choices are Traditional Chinese, English and Japanese through next-intl; preserve the translated labels and accessible names.

The appearance sheet is 390px wide, capped at the viewport, and scrolls vertically. It previews the whole site's material immediately, including theme, accent, background blur and dim, panel blur and opacity, and radius. Guests retain private appearance preferences in their browser; signed-in players explicitly save preferences to their account. Background upload requires an account. Persistence and load errors remain visible. Standard sheets use the existing dialog primitives for dismissal and focus behavior. Their source declares 500ms opening and 300ms closing durations, but the stylesheet does not import an animation utility package; do not treat a slide animation as guaranteed by those class names alone.

### Avatars, empty states and reading content

Avatars show player-uploaded images or a single initial. Initials use readable accent over a 15% accent mix with the background; normal avatars are 32px and large avatars 104px, reduced to 78px on small screens. Empty states use centered, bounded text and an outlined rhythm icon, with a meaningful action when available. Markdown retains semantic headings, underlined links, locally scrolling commands and intact tables.

### Motion and keyboard treatment

Motion follows state changes rather than decorating the ground: links change color over 0.18s, catalogue rows change background over 0.2s, and shadcn controls use Tailwind's 150ms standard transition easing. Loading spinners use a one-second linear rotation. Reduced-motion preference disables animations and transitions and changes smooth scrolling to immediate scrolling. Keep the global focus outline and component focus rings visible in both themes and with custom accents.

## Do's and Don'ts

### Do:

- **Do** build new surfaces from semantic colors, the live accent and the shared radius family.
- **Do** keep the default background plain black and use player-uploaded images for personal atmosphere.
- **Do** use rounded bilingual type with enough line-height for Chinese and translated copy.
- **Do** keep technical strings and mode labels intact in bounded, keyboard-accessible scroll regions.
- **Do** carry explicit hover, focus, disabled and feedback states into new controls.
- **Do** preserve the light-theme foreground pairs and the custom-accent contrast behavior.
- **Do** keep panel blur and opacity independent from background blur and dim, with immediate preview and explicit account saving.
- **Do** use direct, ordinary labels and explanatory copy for community tools and appearance controls.

### Don't:

- **Don't** add AI-generated artwork or a default illustrated background.
- **Don't** hard-code default raspberry or fixed panel radii where personalization should apply.
- **Don't** apply overlay shadows to every glass section or blur readable foreground content.
- **Don't** compress mobile reading tables into narrow, broken strings or split mode labels across lines.
- **Don't** replace Lucide-style SVG icons with glyph characters or introduce an unrelated display font.
- **Don't** turn compatibility status, empty states or activity figures into unsupported product claims.
- **Don't** replace direct product labels with poetic marketing taglines.
