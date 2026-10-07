# Public profile presentation and website chat visibility

Profile owners can save a public accent and horizontal/vertical banner positions with their biography and country. The public accent is independent of the visitor's private appearance settings. Settings and player pages use matching cover ratios at each breakpoint; missing banners hide the focal controls. Existing accounts receive centered positions through migration `0019`.

Public color, biography, country and position drafts survive failed saves and image operations. Save retries use the same form validation. Publication disables conflicting controls until the captured request finishes, and an image can be selected again after removal. Traditional Chinese, English and Japanese copy identifies each operation directly.

Public website channel history and authenticated WebSocket batches exclude restricted senders. Signed-in readers also exclude messages when either participant has blocked the other. History totals use the same visibility filter. Each stream batch refreshes authorization and visibility. This change does not implement website private messaging, a new frontend WebSocket subscription, or late-commit cursor recovery. See [chat contracts](native-chat.md).

## Verification checkpoint

These results belong to this change, rather than the historical initial-site checks:

| Boundary | Evidence | Limits |
| --- | --- | --- |
| Rust source | 193 unit/CLI checks; 118 isolated PostgreSQL cases and 2 existing native-artifact comparisons | The first aggregate run stopped at a test-only Clippy `len() > 0` assertion. After changing it to `!is_empty()`, rustfmt and workspace/all-target Clippy passed separately. |
| Profile API | Migration, typed partial updates, validation, media preservation, disclosure and queued authorization tests pass | Fresh isolated schemas; no actual player profile was edited. |
| Website chat | Real HTTP/WebSocket visibility and authorization integration passes | Synthetic identities; includes either block direction, restrictions, restored visibility, session closure and private-channel exclusion. |
| Infrastructure | 32 Node guard/checksum checks pass; all 19 Git/LF migration checksums pass | Owned fixture resources only. |
| Production images | Sanctuary, Resonance and Serendipity Linux Docker builds pass | Exact image IDs are recorded below. |
| Image protocol | 349 HTTP checks across 42 categories pass | Fresh PostgreSQL project `illya-smoke-27389e00407ec06a`; its containers, network, volumes and temporary credentials were removed. |
| Website | ESLint/typecheck and all 11 selected Chromium tests pass | Profile presentation/media use intercepted synthetic data; community flows use a disposable API/database. Dark desktop zh-TW, light mobile English and dark mobile Japanese were inspected. |

Early browser runs exposed fixture assertions for ambiguous status elements and Radix disabled attributes. The independent finish review then requested valid captures because smooth scrolling left the sticky header over form content. Only the capture helper changed: it disables smooth scrolling, clears focus, scrolls instantly to the document top, verifies the scroll position and waits two frames. The final 11-test run used `illya-web-smoke-ae8a215cad7bebc5`; cleanup was verified. All eight fresh PNGs were opened before the full re-review.

Capture dimensions, hashes, timestamps, source hashes and image provenance are in [capture evidence](../.impeccable/review/profile-presentation-capture-evidence.json). PNGs remain local ignored review artifacts. The detector ran once and returned 83 advisory entries with no primary finding; established-system drift was supplied to the independent reviewer rather than repaired outside this scope.

The fresh full finish reviewer returned `disposition: ship` for the scoped extension, with no material fixes. All 13 recorded image/source hashes matched. The dedicated role was unavailable, so a fresh generic agent performed the independent review using the supplied role contract; see [its verdict](../.impeccable/review/profile-presentation-verdict.md).

A separate fresh generic documenter compared the completed extension against the incumbent system and preserved `DESIGN.md` and `.impeccable/design.json`. Pre-existing drift was reported without repair or canonization; see [the comparison](../.impeccable/review/profile-presentation-documentation.md).

Pinned images:

- Sanctuary: `sha256:487dbf9cd46c6589b93abe57fa35b29c7be1b5cfca46264555a3dbae16d74a3c`
- Resonance: `sha256:3eaea5b830e6631a2b41da9cc7550652880ac21865232b0770b992d95fa4d059`
- Serendipity: `sha256:1f156b6306432c81c7f611a0c4228d75c624eb5bbb921f0ba8760a5ba293c839`
- Existing Caddy: `sha256:f77f856a30f0004200b36b322d61da17fade31e24875699d77fb968399b9eb77`

Actual stable score submission remains unverified after the reported `413` failure. This profile/chat checkpoint does not establish that native submission works. The human-operated diagnostic retry is still pending; no game process, hosts file or trust store was changed by these checks.
