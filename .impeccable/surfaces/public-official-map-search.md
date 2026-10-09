# Public official beatmap discovery

Target: `apps/serendipity/src/features/public-official-maps.tsx`, `/beatmaps` and `/beatmaps/official/[setId]`.
Related: existing `map-discovery.tsx`, `map-card-preview.tsx`, `map-discovery.css`, `official_search.rs`.
Mode: Operate. This is a code-led extension of the committed dense catalogue, not a replacement visual world.

## Direction contract

THESIS: Players can search the official osu! catalogue directly, then inspect every native difficulty without an import or account requirement.

OWN-WORLD: Keep the existing pure black/light grounds, neutral configurable glass, user accent/radius, cover-led equal-height dense records, shadcn controls and shared typography. No shipping raster, illustration or decorative slogan is added.

STORY: Open Beatmaps to official results; submit a title/artist/mapper query, refine mode/status/sort, inspect the complete read-only hover/focus/touch preview and open set details. A single Server beatmaps source link opens retained/custom maps. Existing uploader links retain their server scope. Detail selection shows factual metadata and links to local score pages only for verified retained identities.

FIRST VIEWPORT: Two compact source links sit above the existing heading and short toolbar. A labelled search field and three filters lead immediately into dense two-column records; mobile uses one column and touch-sized fields. No expanded import form sits in the player search flow.

FORM: Existing catalogue components are shared rather than a separate styling system. The difficulty trigger remains a read-only summary; only the set page navigates. Detail combines cover/title/download and a complete difficulty list with selected facts. Unknown facts display an em dash; no generated activity, local score IDs or invented data is shown.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Behavior and provenance

- `/official/beatmapsets/search` and `/official/beatmapsets/{id}` return bounded official metadata. The server owns OAuth, fixed upstream URLs, request quotas, cache, single flight and backoff. Browser search does not import or mutate maps, scores, files or audit data.
- Staff search/import continues to require import permission even when a public query has warmed the shared cache. Withdrawal is merged afresh and removes unavailable results and retained identifiers.
- Results include all returned native difficulties; no hover network request or mutation is needed for official cards. The retained catalogue retains its established on-demand complete-preview behavior.
- Public detail links to retained scores only when the API verifies set ID, difficulty ID and checksum. Official metadata alone does not imply an existing local leaderboard.
- Complete set downloads reuse the already disclosed osu.direct provider when a retained `.osz` is unavailable. No new provider policy, server-side mirror fetching, automatic import or `.osu` download is added. Official download-disabled status suppresses download links. Retained archive and existing provider paths remain distinct.
- New requests participate in the existing top loading bar. Abort/stale suppression, busy/retry/empty states, URL filters, opaque cursor paging and browser history remain functional.
- Primary behavioral references, read only and no code copied: [Sunrise public search controller](https://github.com/SunriseCommunity/Sunrise/blob/master/Sunrise.API/Controllers/BeatmapController.cs), [Sunrise beatmap service](https://github.com/SunriseCommunity/Sunrise/blob/master/Sunrise.Shared/Services/BeatmapService.cs), [Sunset search hook](https://github.com/SunriseCommunity/Sunset/blob/main/lib/hooks/api/beatmap/useBeatmapsetSearch.ts), [Sunset player search](https://github.com/SunriseCommunity/Sunset/blob/main/components/Beatmaps/Search/BeatmapsSearch.tsx), [Sunset beatmap card](https://github.com/SunriseCommunity/Sunset/blob/main/components/Beatmaps/BeatmapSetCard.tsx), [osu! API documentation](https://osu.ppy.sh/docs/index.html).
- Playwright fixtures intercept all API and map-cover requests; content and cover rectangles are synthetic test data, never shipping artwork. Desktop1280/1440, mobile390/320, zh-TW/en/ja and light/dark captures must be reviewed against the final combined source/image before shipping. Functional source checks do not establish deployed or native-client compatibility.
