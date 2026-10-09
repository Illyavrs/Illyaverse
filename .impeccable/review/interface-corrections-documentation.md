# Interface corrections documentation proof

Role substitution: fresh independent generic subagent substituted for the unavailable Impeccable Documenter role, following `reference/degraded/documenter.md` and `reference/document.md` in full.

This is an authorized narrow merge into the incumbent Serendipity system. The user's requested local behaviors and the independent finish verdict establish the correction scope; no world replacement, comp, concept seed or QUALITY BAR applies. The frozen source and reviewed interface are the ground truth. Only `DESIGN.md`, its extension sidecar and this proof were written. No product source, runtime, browser, captures, builds or Docker operations were changed or run.

## Sources and reconciliation

Read `PRODUCT.md`, `DESIGN.md`, `.impeccable/design.json`, the three surface contracts (`hero-tabs-refinement`, `card-preview-navigation`, `official-map-search`) and `interface-corrections-verdict.md`. Sampled the named component/style responsibilities rather than walking the application tree.

| Recorded behavior | Frozen source evidence | Documentation result |
| --- | --- | --- |
| Hero wordmark and original character | `src/features/home.css`: wordmark 64/52/36/32px; image widths 400/340/250/220px; mobile crop regions 160×200 and 120×180. `home.tsx` uses the original PNG as a separate decorative layer. | Contextual geometry replaces obsolete 32/26/24px and square-region instructions; existing shared type tokens stay intact. |
| Plain inactive tabs and one profile selection line | `src/app/globals.css` mode selection and `src/features/profile.css` selected profile state: one 3px border, `box-shadow: none`. | Removed the inset-shadow prescription and retained explicit focus behavior. |
| Catalogue search/filter alignment | `src/styles/map-discovery.css`: 36px controls, 44px at 600px; one toolbar material and no nested field backdrop filter. | Corrected control dimensions and documented the centered search icon. |
| Complete read-only difficulty preview | `src/features/map-card-preview.tsx`: hover/focus/touch open, complete-set request, abort/stale guard, labelled stored fallback, retry/withdrawal, `li` rows without links. `map-discovery.css` bounds width and scrolling. | Removed first-link focus, close-button and eight-link instructions; eight ticks remain only the summary marker limit. The card identity still opens the set. |
| Whole-root route crossfade and real request feedback | `src/components/navigation-motion.tsx`, `src/styles/navigation-motion.css`: root capture, 700ms slow-snapshot release, first-frame 12% pending segment, request settlement, 220ms visibility policy and 180ms completion tail. Reduced motion disables creep/transitions. | Replaced route-content-only and static full-width loading claims in Markdown and sidecar motion metadata. |
| Language and country identity | `src/components/shell.tsx`, `country-flag.tsx`, `src/styles/country-language.css`: nonmodal menu, full language options, one check, selected-option focus and guarded trigger-focus restoration; original self-hosted flags with country-code fallback. | Documented full names, persistence/focus behavior, desktop/mobile trigger sizing and 24×18px Twemoji treatment with attribution. |
| Official search handoff | `src/features/official-map-search.tsx`, `beatmaps.tsx`, `src/styles/official-map-search.css`: deliberate submit, cancellation/generation guards, cursor paging, inline rows and focus into the existing manual form; 38px desktop/44px phone controls. | Added the current component description while preserving the explicit metadata-to-form and manual publication/package boundary. |

The sidecar retains schema version 2, existing color metadata, tonal ramps, shadows, breakpoints and component primitives. Only matching display-context, motion, catalogue-specimen and Do wording changed. Normative palette, shared type ramp, radii, spacing and frontmatter component tokens were preserved; canonical Markdown sections retain their order.

Static verification passed: the sidecar parses as JSON, schema version 2 is intact, its Do narrative matches the Markdown verbatim, the obsolete component prescriptions are absent, and the edited design files pass `git diff --check`.

## Evidence boundary

The independent finish verdict already visually inspected all 21 required representatives and machine-validated all 96 current confirmation PNGs; this documentation pass did not repeat those inspections or claim new visual certification. Latest user screenshot references remain in `data-official-search-20261009/review-inputs.json`. The verdict's scoped browser evidence concerns synthetic interface fixtures and does not certify upstream package availability, backend correctness, native client connectivity or score submission.

The original 700×1251 RGBA hero PNG and its adjacent provenance were untouched. Its reviewed SHA-256 remains `d32cc7271ff33364e247c8d8988c16f61e50d7715cd500b6cbdece29c53c70e4`. No AI artwork or replacement raster was introduced. Original attributed Twemoji assets and their license were untouched.

## System summary

Palette: pure black, neutral configurable glass, pale raspberry accent and paired light-theme semantic roles.
Type: Nunito Variable/Noto Sans TC shared hierarchy, with the current hero's contextual 64/52/36/32px wordmark override.
Material: Independent Material and Solid Counterpart rules keep scene treatment, coverage, blur and opacity separate.
Structure: Joined Record and Local Overflow rules preserve connected surfaces and bounded dense content.
Meaning: Personal Accent, Separate Meanings and Bilingual Breathing rules retain readable selection, facts and localized wrapping.

Not canonized: obsolete linked-eight preview, inset selection shadow, route-content capture and full-width loading descriptions were removed; unrelated detector advisories were left outside this authorized component-description merge.
