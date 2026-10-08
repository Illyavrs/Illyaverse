# Map discovery

Target: `apps/serendipity/src/features/map-discovery.tsx`
Related: `apps/serendipity/src/styles/map-discovery.css`, beatmap detail.
Mode: Operate. Players find a set, compare its actual stored difficulties and open a difficulty. Preserve uploads, official imports, permissions, withdrawal and score variants.

## Direction contract

THESIS: A map catalogue whose recognizable unit is a cover, song information and a playable difficulty strip. Filters remain visible and results represent server-paginated sets rather than duplicate song rows.

OWN-WORLD: The existing black ground and personal neutral glass remain. Actual map covers supply imagery; status text and numeric difficulty colors supply useful hierarchy. Rounded Nunito/Noto type, user radius and restrained neutral dividers remain shared.

STORY: Search or choose mode, status and source; compare the matched count and stored count; open a difficulty directly. Share the URL or use browser history without losing the filter selection.

FIRST VIEWPORT: Compact page heading and authorized upload/import actions; labelled search followed by a single filter row; count and grid/list controls; a two-column dense catalogue with covers and full-width difficulty strips. Mobile stacks filters and uses one result column.

FORM: User-pinned Sunrise/osu! catalogue convention, translated into the established Illyaverse system. No concept seed: the user supplied the products and operational direction. Signature interaction is the colored, numerically labelled difficulty strip and the paginated known-difficulty selector on detail.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Truth and verification

Official cover URLs are formed only from safe positive official native set IDs below 2,000,000,000. Local maps and failed covers use a modest library glyph. No shipping raster is added or remote asset downloaded. Cover, state and difficulty colors retain textual alternatives. Selector lists known cached siblings without implying an upstream-complete archive. Public selector access failures discard previous contents. Browser fixtures are synthetic and intercept all API calls without writes, accounts or downloads. Review captures require document-top checks and per-image metadata.
