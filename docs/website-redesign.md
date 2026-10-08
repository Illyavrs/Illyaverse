# Serendipity website redesign

The October 8 redesign replaces the rejected flat presentation with joined community records, distinct navigation and toolbar surfaces, aligned facts, and compact player and beatmap identity. It preserves real API content, public registration, permissions, URL navigation, three locales, both themes and independent appearance controls.

## Reference study

Only public source structure and rendered composition were studied. No upstream source, font, palette or artwork was copied into Illyaverse.

| Reference | Pinned revision | Observed pattern | Original implementation |
|---|---|---|---|
| [osu-web](https://github.com/ppy/osu-web/tree/8d395f2ae962857d0f4e48c233c0a92bba84d361) | `8d395f2ae962857d0f4e48c233c0a92bba84d361` | Separate player identity, performance facts and sticky sections; broad map identity beside narrow statistics; distinct primary and utility navigation. | Compact profile identity/mode/rank bands, connected chart/statistics composition, inline section navigation, map title/actions beside selected difficulty facts, visible route navigation. |
| [Sunset](https://github.com/SunriseCommunity/Sunset/tree/a0011369218d2b26c705494e2a16556148539b8f) | `a0011369218d2b26c705494e2a16556148539b8f` | Joined entity sections with immediate metadata, underlined tabs and consistent page widths; genuine map covers organize recognition. | Connected heading and content surfaces, scannable cover records and difficulty controls, aligned ranking metrics, short factual home introduction followed by real content. |

Primary composition sources: osu-web [profile](https://github.com/ppy/osu-web/blob/8d395f2ae962857d0f4e48c233c0a92bba84d361/resources/js/profile-page/main.tsx), [map header](https://github.com/ppy/osu-web/blob/8d395f2ae962857d0f4e48c233c0a92bba84d361/resources/js/beatmapsets-show/header.tsx), [navigation](https://github.com/ppy/osu-web/blob/8d395f2ae962857d0f4e48c233c0a92bba84d361/resources/views/layouts/_nav2.blade.php); Sunset [profile](https://github.com/SunriseCommunity/Sunset/blob/a0011369218d2b26c705494e2a16556148539b8f/app/%28website%29/user/%5Bid%5D/page.tsx), [maps](https://github.com/SunriseCommunity/Sunset/blob/a0011369218d2b26c705494e2a16556148539b8f/components/Beatmaps/Search/BeatmapsSearch.tsx), [header](https://github.com/SunriseCommunity/Sunset/blob/a0011369218d2b26c705494e2a16556148539b8f/components/Header/Header.tsx).

## Product constraints

The default dark canvas stays pure black. Panel blur, opacity and radius remain independently adjustable, with defaults of 8px, 55% and 14px. User background blur/dim remain separate. The player's public accent remains separate from the visitor's private appearance. Nunito Variable and Noto Sans TC remain locally packaged fonts.

No generated artwork or new shipping raster was introduced. Official map covers remain runtime content from validated `assets.ppy.sh` set IDs, with an original icon fallback; user avatars, banners and backgrounds remain user uploads. Screenshot artwork and activity fixtures are QA data, never production community content. Missing banners produce no placeholder artwork. Passed score letter grades are not inferred from accuracy when the API does not provide enough information.

## Validation

Lint, TypeScript and the production Docker build are required before the owned disposable browser suite. Browser fixtures and real HTTP workflows run only through `infra/disposable-web-smoke.mjs`, in its random PostgreSQL/network/volume environment. Captures cover desktop 1440px, the user's 1254px viewport, mobile 390px and 320px, zh-TW/en/ja and dark/light. Functional checks preserve filtering/history, permissions, private score isolation, best/recent collections, weights, replay operations and appearance persistence.

Deployment updates only the two retained Serendipity containers. The eight API/protocol/database/proxy containers and their full identities remain protected. A deployment manifest records the prior Web image for reviewable rollback, successful schema 22 and retained record IDs. No native client, hosts file or certificate trust store is changed by this redesign.

### Final result

The final image `sha256:2d70e54739a003aef5d6de9ddf61257d9b127309a0741cab1b56d2f29fe7fe8f` passed ESLint, TypeScript, the production build and all 191 browser tests in the owned disposable project `illya-web-smoke-c670d777f38370de`. Its containers, network, volumes and temporary environment were removed by the harness after verification. Test fixtures never entered either retained database.

One detector pass found 182 advisory differences from the former design document's token scale and no blocking anti-pattern findings. The design document and sidecar now record the actual reusable system; obsolete styles and isolated metadata sizes were not promoted into reusable rules to silence advisories. Independent review identified one material correction: mobile catalogue difficulty previews now show eight links in four two-column rows. The follow-up verdict was `ship` for that correction, with no regressions observed. Dedicated review/documenter roles were unavailable, so fresh independent agents performed both roles.

Both retained Web services were updated to this image. All ten services remained healthy; the eight non-Web container identities and schema 22 were preserved. The client-test database retained two users, 48 maps, two scores and one Replay; the main database retained two users. Read-only browser checks of the real home, player 2 and map 42 confirmed the new composition and existing 19.78 pp score/Replay links. This verifies the website redesign, without claiming completion of remaining native-client work.

Local deployment evidence: `data-community-redesign/deploy-20261008T155104Z-5666070117bb479aab0a2cedbbe0aafc/deployment-manifest.json`. Previous image pins and rollback instructions remain in that run directory. The normative design system is [DESIGN.md](../DESIGN.md), with supplemental metadata in `.impeccable/design.json`.
