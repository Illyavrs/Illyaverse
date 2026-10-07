# Serendipity appearance review

This is a fresh review of the user's correction to the homepage, shared navigation, localized copy and appearance settings. The shipped named reviewer role was unavailable; a fresh independent agent applied the supplied finish-reviewer contract without inheriting the build conversation.

Initial disposition: **fix**. The only material finding was persistence of the corrected design system. Native gameplay and complete product functionality are outside this verdict.

## persistence

PRODUCT.md exists and reflects the product. At the first review, DESIGN.md still prescribed plum neutrals, a large landing headline and 24px glass blur, contradicting the corrected screenshots and source defaults of neutral black, 8px blur and 55% opacity. Its token sidecar must be updated consistently.

All nine required captures exist and were opened successfully, including the actual 1254px user viewport. Screenshots are local ignored artifacts in this directory.

## fidelity

| Element | State | Evidence |
| --- | --- | --- |
| TYPE | match | Rounded lettering, compact homepage title and direct localized labels; the rejected oversized slogan is absent. |
| MATERIAL | match | Thin neutral glass; uploaded-image captures visibly distinguish panel blur/opacity from background dim. No fabricated texture or default illustration. |
| GROUND | match | Pure-black default canvas and neutral surfaces; pink remains an action accent. |
| Homepage reading order | match | Community identity, browsing/account actions, factual activity, recent beatmaps and useful links. |
| Empty community data | match | Clearly states no beatmaps; localhost activity shows 0 online, 1 registered, 0 ranked scores and 0 beatmaps. |
| Navigation | match | Settings is visible on desktop; English mobile menu exposes primary routes and guest account actions. |
| Appearance controls | match | Independent background/panel controls, displayed values, upload/remove, reset and save remain legible on desktop and mobile. |
| Locales and responsive layout | match | Traditional Chinese homepage, English mobile navigation and Japanese settings show intact labels without visible horizontal overflow. |
| Optional light theme | adaptation | Preserves light/dark personalization while keeping dark as default. |
| Uploaded fixture | adaptation | An original synthetic QA image and authenticated fixture demonstrate customization; neither is shipped default artwork. |

The supplied QA records no failures. All 87 detector findings were inspected: 63 type, 19 radius and 5 color advisories against the old design documentation; none establishes a material visual defect in these captures. No primary findings were reported. No QUALITY BAR card or approved comp was supplied for this explicitly retained, code-led direction.

## ceiling

Reached for the stated operational direction: clear hierarchy, restrained framing and readable personalization. No supplied QUALITY BAR card supports demanding additional ornament, imagery or promotional motion.

## material_fixes

1. Update DESIGN.md and its token sidecar to record the corrected neutral palette, compact homepage, independent panel controls and 8px/55% defaults; remove the superseded plum, large-hero and 24px prescriptions.

## keep

Keep the plain-black default, direct osu! community labels, truthful empty data, compact navigation and independently adjustable glass without adding artwork or promotional composition.

## Evidence

- `appearance-homepage-zhTW-desktop.png`
- `appearance-homepage-zhTW-mobile.png`
- `appearance-homepage-user-1254.png`
- `appearance-homepage-en-mobile-menu.png`
- `appearance-settings-ja-desktop.png`
- `appearance-settings-guest-light.png`
- `appearance-settings-guest-dark.png`
- `appearance-settings-uploaded-desktop.png`
- `appearance-settings-uploaded-mobile.png`
- [Synthetic browser behavior evidence](appearance-qa.json)

Guest behavior, synthetic authenticated PATCH/reload, decoded background and responsive checks are distinct from real-account changes and native-client validation.

## Final verdict pass

Disposition: **ship**, covering the scored documentation fix only.

| Fix | Score | Evidence |
| --- | --- | --- |
| Correct appearance documentation and sidecar | resolved | DESIGN.md, parsed schema-2 design.json and PRODUCT.md now match the neutral palettes, dynamic surfaces, 8px/55% panel defaults, independent background controls and compact 40px/32px homepage. Superseded plum and heavy-blur prescriptions are absent; plain black, no AI art and direct copy remain explicit. |

Remaining: clear. No regressions were identified within this documentation scope. UI and captures were unchanged; no recapture was needed. This verdict does not establish whole-product completeness or native-client compatibility.
