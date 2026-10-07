# Serendipity appearance and community navigation brief

Visitor mode: **Operate**. Scope: the homepage, shared navigation, localized interface copy and appearance settings in Serendipity. Preserve existing registration, profiles, account recovery, friends, chat, beatmap catalogue, rankings and permission-controlled administration.

## Direction contract

The user rejected the previous large promotional headline, empty hero space, poetic copy and opaque plum panels. That earlier review is not approval for this surface. The requested result is a normal, functional osu! community website: familiar catalogue layout, compact headings, direct labels and quickly discoverable workflows.

Retain the requested pure-black default canvas, thin black translucent glass, rounded lettering and user-selected accent. The default has no background image. Uploaded player images are optional; no AI artwork or decorative hero illustration belongs here. Keep the existing visual system and practical site structure rather than introducing a new concept.

The first viewport identifies Illyaverse as an osu! community server, provides beatmap browsing and account creation or profile access, shows only activity returned by the server, and leads into recent beatmaps. A compact side panel links to rankings, chat, appearance, and signed-in profile/account/friend settings. Beatmap upload and import appear only for accounts with the corresponding grants. The existing API remains responsible for authorization.

## Interaction and content contract

- Labels describe the actual page or action in Traditional Chinese, English and Japanese. Replace emotional slogans and invented empty-state stories with specific purpose, data availability and recovery instructions.
- Homepage activity comes from `/status`; loading and unavailable states have distinct copy. Beatmaps come from the existing catalogue endpoint. No illustrative counts, fake activity or unsupported download claims.
- Desktop navigation exposes Settings. The mobile navigation includes sign-in and registration for guests and profile access for members, while administration remains permission-controlled.
- Profile editing links directly to `?tab=profile`; email verification returns to `?tab=account`. Appearance and friends links use their matching settings tabs.
- Background blur/dim and glass panel blur/opacity are separate settings. The former affects an uploaded image; the latter controls panel material. The black canvas remains plain with no image. Default panel blur is 8px and opacity is 55%, with theme-aware foreground contrast.
- Appearance controls preview their effects immediately. Guests persist preferences in the browser; signed-in players can save preferences to their account. Uploaded backgrounds require sign-in. Reset restores the supplied defaults.
- The connection guide keeps the existing experimental native-client status, DNS/HTTPS conditions, launcher instructions and honest absence of a public launcher download. Clipboard failure displays a localized error and leaves the command selectable for manual copying.
- Existing forms, errors, permissions and game terminology retain their factual behavior. This change introduces no additional backend workflows or account actions.

## Finish boundary

Root owns the final bounded browser QA and finish verdict for the integrated changes. Check the compact homepage, navigation and settings on desktop and mobile using real translated copy, plus light/dark appearance and uploaded-background glass behavior. Also verify settings-tab navigation, permitted/hidden beatmap actions and clipboard failure. Preserve focus, loading, error and empty states without whole-page horizontal overflow.

Local static checks and final browser evidence are reported with the implementation. Native stable/lazer gameplay, real-account changes, deployment and production compatibility are outside this frontend verification; the connection guide states the current validation limit.
