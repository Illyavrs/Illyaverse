# Official map search

Visitor mode: **Operate**. Local extension of `/beatmaps/import`, inheriting the existing Serendipity system and the manual import flows in `map-packages.md`. The user chose direct implementation with browser checks, dense controls, shadcn primitives, plain labels, pure black and adjustable neutral glass. No new artwork or design-system change.

## Direction contract

THESIS: Find an official set and carry its exact identity into the existing import form, with an explicit confirmation before any publication.

OWN-WORLD: Compact shared fields, one joined search/result surface, divider-led rows, small covers, tabular difficulty facts and restrained selection accent. Inherit all live glass, typography, focus and light/dark roles.

STORY: Search by text, mode and status; inspect official difficulty summaries and current local storage/withdrawal; choose a difficulty or set; confirm the filled import form. Inactive drafts and selected archive files survive.

FIRST VIEWPORT: The existing page heading leads to a short search toolbar and dense results; the manual ID/import-type form follows. Result selection expands inline. Loading preserves the toolbar, and empty/error states explain recovery at the result location.

FORM: Code-led extension of the established Operate surface; no concept seed applies. The signature interaction is the explicit result-to-form handoff with focus and localized confirmation. Search is submitted deliberately, with cursor paging and stale-request cancellation.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Quality and behavior boundary

- Search requires `beatmaps.import` in both the UI and API. It performs no map, score, audit or media publication.
- Official summaries stay separate from fresh local storage and withdrawal facts. A cached result cannot make a withdrawn set selectable.
- Complete archives remain explicit uploads until the operator chooses and configures a permitted automatic package provider. Finding metadata does not imply an original full `.osz` was downloaded or all assets were verified.
- Use the existing shared FieldSelect and Button controls, with keyboard focus, visible busy/disabled states and localized recovery. No native select imitation, extra theme icon, fixed heavy scrim or menu-induced page shift.
- Explicit submit avoids upstream calls on every keystroke. Starting a newer query aborts/invalidates older requests. Paging preserves the selected filters, encodes opaque cursors and avoids duplicate rows or repeated cursors.
- Verify zh-TW, en and ja; desktop, the user's 1280-pixel viewport and phone; dark/light; initial, loading, populated, selected, empty, error and restricted states in one bounded capture round.
- API/client fixtures are synthetic. Real metadata access, pinned deployment, retained data and native client validation carry their own evidence scopes.
