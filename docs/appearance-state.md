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

A separate fresh generic documenter verified the five image and seven production-source hashes, compared the finished extension with the incumbent authority, and preserved `DESIGN.md` and its sidecar. Its [comparison](../.impeccable/review/appearance-state-documentation.md) reports the legacy contrast-threshold/focus descriptions and existing border cascade without promoting them into new rules or repairing system artifacts outside this scope.

## Local deployment

2026-10-08 Asia/Taipei: source commit `33750f4` was deployed by replacing only each stack's Serendipity container with the verified web image. Main and client-test PostgreSQL, Sanctuary, Resonance and Caddy container identities were checked and retained. Schema remains `0019`, with no failed migration. Before/after account and complete beatmap fingerprints match, as do user/map/score counts: 2/0/0 in main and 2/44/0 in client-test. No database migration or copy of compiler caches was needed.

Both new web containers match the pinned image and all ten stack containers are healthy. Read-only landing/settings/player GETs returned 200 on localhost and CA-verified `https://illya.test`; public profile defaults and privacy also passed. TLS used the existing CA file and request-local DNS override without changing hosts or trust. Raw update/runtime evidence is ignored under `data-appearance-state/`. This record was written after the image build and is repository evidence, not a claim about later prose bundled in that image.

## Native submission boundary

A separate bounded read-only inspection of the newly deployed client-test gateway/API logs found zero retained diagnostic events during 2026-10-07 19:21:48–19:27:10 UTC. Both logging drivers were enabled. This does not prove that no successful request occurred: response traces default to DEBUG, while deployment logging is INFO.

The latest actual stable failure remains the earlier 413: multipart part 14, five-byte parser name, unresolved `unknown` category, no filename/MIME, and 33–64 KiB exceeding the 32,768-byte scalar limit. No new field classification supersedes it. A fresh human-operated retry remains necessary to construct the corresponding redacted fixture; these appearance tests do not establish native score submission success.

## 2026-10-10 source contract

The preceding sections retain their historical deployment and test scope, including the old visual-field count and native failure snapshot. The current integration checkpoint supersedes those current-status statements: the user now reports stable/lazer submissions and chat working normally, while source-28 appearance changes and release verification remain pending.

Current appearance saves contain **11 visual fields**: `mode`, `accent_color`, `background_url`, `background_mode`, `background_color`, `background_blur`, `background_dim`, `panel_blur`, `panel_opacity`, `glass_level` and `border_radius`. They exclude `locale` and `pm_friends_only`. Color selection uses `react-colorful` 5.8.1. The frontend normalizes `background_mode` to `default`, `image` or `color`; the backend keeps this field optional so older preferences without a mode infer `image` from an existing background URL, otherwise `default`. A valid background color is exactly six hexadecimal digits after `#`, with `#000000` as the default.

Default mode uses the page ground; color mode uses the selected solid color and bypasses image blur/dim; image mode uses the retained background URL. Selecting a mode retains the saved URL and color. A successful background upload selects image mode and replaces the URL; removing it clears the URL and selects default. Reset restores visual defaults locally while retaining the locale, and Save performs the account PATCH. The existing shared save/upload lock and explicit upload retry remain in effect.

Compatibility is directional. The new backend can read older preferences, but an older backend using `deny_unknown_fields` without `background_mode`/`background_color` can reject both new PATCH bodies and stored preference JSON containing them. Rolling back to that backend requires a compatible implementation or explicit field cleanup; no runtime rollback has been tested. The final shared fixture UI gate is recorded in the current integration checkpoint; these source contracts are not a completed source-28 deployment.
