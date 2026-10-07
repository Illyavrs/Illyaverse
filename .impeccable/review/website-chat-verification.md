# Website chat verification checkpoint

Checked 2026-10-07 UTC. This checkpoint covers the website chat extension described in `docs/website-chat.md`, not completion of the full Illyaverse goal.

## Source and persistence

- Workspace unit/CLI suites: 201 passed. Owned PostgreSQL suites: 56 Resonance and 65 Sanctuary tests passed, including participant exclusion, policy changes, commit notifications, shared retries and queued authorization.
- Formatting and workspace Clippy with `-D warnings` passed after two behavior-preserving lint corrections. Full integration suites ran before those corrections; the refresh condition and exact refresh assertion remain equivalent.
- A stale profile-preference fixture was updated to preserve an explicit true `pm_friends_only` value. An existing auth lock-wait timeout occurred on the first full attempt; its isolated and subsequent full run passed. Only its failure diagnostic changed; timeout and authorization fences were preserved.
- Node infrastructure guards: 32 passed. All 19 committed migration digests remain unchanged. This feature adds no migration.
- Serendipity lint and TypeScript checks passed. Rust commands disabled incremental compilation only in their process environment and restored it afterward.

## Browser and image integration

- Synthetic chat fixtures: 7/7 passed in 11.6 seconds on the final web image. Coverage includes older history, late IDs, sender-scoped UUIDs, retry/draft preservation, creation and preference recovery, policy cache removal, reconnect, permission changes and expired sessions.
- Real disposable-image community regression: 3/3 passed, covering registration, locale/theme/profile persistence, public chat and operator RBAC.
- Real disposable-image chat interoperability: 1/1 passed in 13.2 seconds. Website sends reach stable binary polling; native action messages and stable PM sends reach website history/live delivery. Concurrent website/native retries share one persisted UUID message. A block removes accessible history and private browser state.
- The initial synthetic retry corrected a test that checked the mobile toggle before account loading completed. The initial real-image retry corrected its account fixture's omitted locale; saved account locale correctly supersedes its cookie. Neither required an application source change.
- Every disposable browser project verified removal of its owned containers, networks, volumes and temporary environment. These tests do not touch retained community accounts or launch a game.
- Final-image native protocol regression passed 349 HTTP requests across 42 check categories, including stable encrypted submission, large replay, RX partitioning, native rooms and spectator transport. Its owned project and temporary environment were removed.

## Pinned images

| Service | Image |
| --- | --- |
| Sanctuary | `sha256:7ccb6e6338a360f36fd833a3a14a67dde89e29740b3071b40283bb62716c434b` |
| Resonance | `sha256:3eaea5b830e6631a2b41da9cc7550652880ac21865232b0770b992d95fa4d059` |
| Serendipity | `sha256:96d7cc60cc1afbf5132eecc4e31790281179f24d1b4fd18554404af1ea35fc4c` |

Resonance was rebuilt from current source and produced the same binary/image pin as the previous checkpoint.

## Design evidence

Two root UI render rounds; the final batch was retried after correcting test readiness with the same application image. The four final desktop/mobile zh-TW/en/ja captures and hashes are recorded in `website-chat-capture-evidence.json`. Mobile assertions require the composer/restriction bottom inside 390×844 and at least 80px of transcript.

The detector ran once before the mobile fix: 86 advisory findings, zero primary findings. The finish reviewer scored only the ordered mobile first-viewport fix and observed no regressions; disposition is `ship` for that fix. The dedicated reviewer/documenter roles were unavailable, so fresh generic agents followed their complete degraded instructions. `DESIGN.md` and its sidecar remain unchanged; the documentation verdict reports preexisting drift.

## Limits and retained evidence

Stable/lazer game UI, production load and full client compatibility are not established by isolated protocol fixtures. Actual stable score submission remains pending the human diagnostic retry. Native presence and numeric read-marker limitations are documented in `docs/website-chat.md`.

Local ignored logs, failed-run evidence and deployment backups remain under `data-website-chat/`. Compiler caches and workspace fixtures were not deleted. Deployment results will be recorded separately after that step succeeds.
