# Website features

The website uses real server records. Empty sections mean that no eligible content is available; player counts, activity, scores and maps are never populated with demonstration data on a running server.

| Area              | Available behavior                                                                                                                                                                                        |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Landing           | Server status and counts, recent maps, public verified scores, osu! vanilla ranking preview, published announcement preview, registration and client connection steps, permission-aware feature links     |
| Player profile    | User banner and crop, avatar, public accent, biography, selected game mode and vanilla/RX partition, global/country rank, observed rank history, pp, accuracy, plays, combo, hit counts, available score totals and grade counts |
| Profile scores    | Best qualifying score per Ranked difficulty ordered by pp, server weights and weighted pp; recent submissions; pagination; score detail and replay downloads                                              |
| Uploaded beatmaps | Local maps attributed to the actual uploading account, independent loading/retry, and the complete filtered catalogue; this does not establish ownership of the free-text mapper name                     |
| Social            | Friends, mutual relationships, blocks and private messaging; Message on a profile opens a recipient form, and the player explicitly requests the conversation                                             |
| Personalization   | Light/dark theme, accent, uploaded background, independent background blur/dim, independent glass blur/opacity, live radius, profile media and public accent                                              |
| Map management    | Official import subject to API configuration/rate limits, local upload, package/difficulty downloads and permission-controlled moderation/withdrawal                                                      |
| Map discovery     | Grouped catalogue, grid/list views, URL search/filter/sort/paging, official covers, known-difficulty switching and truthful .osu/.osz downloads; see [map discovery](map-discovery.md)                       |
| Accounts          | Public website registration, email/username login, password recovery, email verification, locale and account settings                                                                                     |
| Administration    | Server-enforced permissions and restricted account, role, map, score, announcement and audit areas                                                                                                        |

Announcements have a persistent draft/published/archived workflow, explicit publishing, version conflicts and transactional audit. Public pages return only published entries; the editor retains failed/conflicting drafts and reloads the latest private record only after explicit discard confirmation. Content is plain text, and no example announcements are created. See [announcements](../apps/docs/content/docs/operate/announcements.md).

Recent pending/rejected submissions remain subject to the existing owner and moderation access policy. The landing requests only verified submissions. Public uploader-filtered map lists keep the existing active-map visibility rule, including for signed-in administrators. Administrative listings keep their explicit availability filters.

Grade counts describe verified submissions, including repeated attempts; they are not counts of each map's best grade. Global and country rank are unavailable until eligible pp exists. RX score totals are unavailable, and incomplete vanilla totals are labelled instead of being presented as complete.

[Rank history](rank-history.md) records the latest real observation per UTC day across all seven mode/category partitions, with a minimum 30-minute collection interval. Missing dates and unranked observations remain gaps; no earlier history is fabricated. Measured playtime, profile visits, achievements/medals and curated featured maps do not yet have complete persistence and workflows. They are not displayed as working features. Native stable/lazer gameplay and stable score submission still require the separate real-client verification described in the [verification guide](../apps/docs/content/docs/reference/verification.md).

## Current source additions

These additions are implemented in source and await deployment. They do not extend the recorded live-browser or native-client evidence.

- The authenticated notification bell and paged inbox show rank-request approvals/rejections and newly published announcements. Mark-read affects only system notices, never private-message read state. Explicit ID requests accept at most 100 IDs; a signed batch snapshot covers at most 1,000 observed notification UUIDs. Later arrivals require another explicit action. See [notification contracts](../services/sanctuary/src/notifications.rs).
- Official map discovery offers a modified-only filter and includes server Ranked overrides in Ranked results. Matching uses each difficulty's effective status; modified-only results stay within the finite server decisions and do not fetch an upstream search page. The Local tab remains uploaded-only.
- Map audio starts on an explicit play click through the private preview proxy. Starting another preview stops the previous one; browsing cards does not preload or autoplay every preview. See [preview controls](../apps/serendipity/src/features/map-audio-preview.tsx).

Layout reference: [Sunrise](https://sunrize.uk/) for landing content coverage and [the official osu! profile](https://osu.ppy.sh/users/2) for player information hierarchy. Illyaverse's code and visual assets are original; the default background stays black with no generated artwork.
