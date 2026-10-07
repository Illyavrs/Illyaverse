# Website live chat and private conversations

Visitor mode: **Operate**. Ordinary extension of the existing `/chat` surface. The established Serendipity black ground, neutral adjustable glass, rounded fonts, shared controls, direct copy and light theme remain authoritative. Code-led implementation was chosen by the user. No artwork, generated assets, slogans, concept tournament or replacement visual world.

## Direction contract

THESIS: Players can read public channels and exchange private messages from the website, using the same persisted conversations as native clients.

OWN-WORLD: Inherit DESIGN.md and the current chat composition. Accent communicates actions and selection; background and panel material remain independently customizable. All visible states are translated in zh-TW, English and Japanese.

STORY: The sidebar separates public channels and private conversations. An inline username field starts a conversation. The selected conversation exposes its peer, message history and composer. Connection status describes current transport truth; failures preserve the draft and enable a safe retry. Friend-only messaging is an account preference, separate from appearance.

FIRST VIEWPORT: Desktop retains a compact navigation column beside the transcript. Mobile reflows the same controls without clipping or horizontal document overflow. The selected conversation and send action are findable immediately. Empty, denied, pending, failed and reconnecting states describe the next available action.

FORM: Precisely scoped extension of an existing Operate surface; no seed is required by new-work's local-extension rule. Preserve older-history loading, scroll anchoring, per-conversation drafts, IME-safe Enter and explicit authenticated access.

FINISH: A bounded desktop/mobile render pass, at most one root confirmation pass, one detector pass and a fresh finish reviewer establish the UI evidence. A fresh documenter checks the inherited system without repairing pre-existing drift. No new shipping raster is introduced.

## Behavior and evidence boundaries

Website PM writes enter the existing protocol-private table and its transactional bridge. PM reads require an actual participant, including for administrators. Both block directions, restrictions, current permissions and live sessions are enforced by the server. Guest readers retain public channels only.

Signed-in live delivery uses commit notifications rather than a maximum message ID. Retries carry the same client UUID for the same logical send. Cached historical messages are checked against current visibility, including older pages. Signing out or changing account clears private state.

Browser fixtures are synthetic and intercept API/socket traffic; they never modify real users. Isolated PostgreSQL/HTTP/WebSocket tests separately verify authorization, persistence, retry behavior, commit ordering and the native bridge. These tests do not prove real stable/lazer UI connectivity or resolve the pending human score-submission retry.
