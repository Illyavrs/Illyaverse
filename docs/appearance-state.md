# Appearance state resilience

The appearance provider owns one immediate save/upload lock shared by full settings, the quick sheet and conflicting navbar theme actions. Pending operations disable appearance edits, reset, another save and background selection. A completed save describes the submitted snapshot; editing again clears stale success. Upload failures retain the selected file for explicit retry, while file-input reset permits selecting the same file again.

Appearance PATCH requests contain only the eight visual fields. Language selection keeps its separate account PATCH and cookie/next-intl refresh, so a delayed appearance save cannot overwrite a concurrent language change. Reset restores visual defaults while preserving the current locale. Guest initialization and cached visual settings retain the runtime next-intl language, including a language cookie with no prior appearance cache.

Accent action text chooses the stronger contrast of white and `#111111`, with black as the fallback when neither reaches 4.5:1. This fixes the middle-gray boundary: `#767676` uses white and `#777777` uses black. Keyboard focus uses the existing theme-readable accent, including when the user selects pure black or white. Public profile colors remain independently scoped.

## Evidence

ESLint, TypeScript and the Linux production web build pass. The pinned Serendipity image is `sha256:e0123955893c87ffee8add7dd8330c00a567bd665b1a8947b90f6ffcaf264805`. Sanctuary/Resonance images and database schema remain the profile-presentation checkpoint's versions.

The first eight-test browser gate passed in disposable project `illya-web-smoke-35bfbd09486e5a04`. Five valid captures were opened: dark zh-TW desktop publication/full quick-sheet views, light English mobile save failure, dark Japanese mobile guest reset and upload failure. These tests use synthetic intercepted account/media data; community registration/permissions flows use the disposable API/database. Captures and current production source hashes are recorded in [capture evidence](../.impeccable/review/appearance-state-capture-evidence.json).

An expanded 16-test cross-profile run first passed 15 tests. Its failure was an old profile screenshot helper asserting scroll position even when capture was disabled. The helper now retains font/layout/overflow checks during functional runs and confines stabilized scrolling to actual screenshots. This was a fixture correction; no production UI change or replacement of the five appearance captures followed it. Raw first/failed/final browser logs remain ignored under `data-appearance-state/`; each disposable project's containers, network, volumes and temporary credentials are removed by its guarded helper.

The final expanded run passed **16/16** tests in `illya-web-smoke-0b3dc552202bc75b`, including actual rendered action-text contrast of at least 4.5:1 for middle gray, black and white, plus existing public profile/media workflows. Disposable cleanup was verified. No old profile review PNG was overwritten.

The detector ran once on the changed targets and reported 83 advisories, with no primary findings. The independent read-only provider review found no actionable in-scope concurrency defect. The interface is an ordinary incumbent extension: existing black ground, thin glass, rounded type, direct translations and design-system authority are preserved.

The fresh full interface reviewer returned `disposition: ship`, with no material in-scope fixes. It checked the five required appearance captures and incumbent contracts; its verdict does not cover other site surfaces or the later functional cross-profile rerun. The unavailable named role was substituted by a fresh generic agent using the supplied role contract; see [verdict](../.impeccable/review/appearance-state-verdict.md).

## Native submission boundary

A separate bounded read-only inspection of the newly deployed client-test gateway/API logs found zero retained diagnostic events during 2026-10-07 19:21:48–19:27:10 UTC. Both logging drivers were enabled. This does not prove that no successful request occurred: response traces default to DEBUG, while deployment logging is INFO.

The latest actual stable failure remains the earlier 413: multipart part 14, five-byte parser name, unresolved `unknown` category, no filename/MIME, and 33–64 KiB exceeding the 32,768-byte scalar limit. No new field classification supersedes it. A fresh human-operated retry remains necessary to construct the corresponding redacted fixture; these appearance tests do not establish native score submission success.
