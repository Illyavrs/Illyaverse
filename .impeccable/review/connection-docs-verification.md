# Connection and documentation verification

This is an incumbent Read refinement of the player connection guide and public Markdown reader. It fixes actual broken guide links and missing fragment destinations, and directs the lazer player tab to the fixed-server launcher guide. It does not certify native gameplay or resolve the separately pending stable submission diagnostic.

## Implemented behavior

The reader explicitly allowlists 25 public guides, including the five formerly missing appearance-state, profile-presentation, native-score-submission, mutation-authorization and website-chat routes. Only an own registry key supplies a filesystem filename; unknown, inherited and traversal inputs do not. Approved filename references support bare, `./`, `../docs/`, `docs/` and `/docs/` forms while retaining query and fragment suffixes. External source URLs stay external. Unsupported repository citations remain readable text rather than broken website links. Raw HTML remains escaped and unsafe URL schemes do not become actions.

An original per-document remark plugin creates deterministic Unicode heading IDs, including inline formatting and collision suffixes. All five authored fragment destinations now exist. A scoped 100px heading scroll offset keeps them below the sticky header; reading typography, material, layout and appearance controls remain incumbent.

Connect's lazer tab links to `/docs/kaleidoscope` in zh-TW, en and ja. Stable instructions describe the user's explicit Restart action. Operator DNS/CA preparation, the current fixed `illya.test` test package, developer source-build comparison and pending native validation have distinct factual descriptions. Changing server `.env` alone does not change a distributed launcher package.

## Source and final-image checks

| Gate | Result |
| --- | --- |
| `node --test apps/serendipity/tests/docs-navigation.test.mjs` | 10 cases passed in the owning worker |
| Full Serendipity ESLint and TypeScript | Passed; focused checks also passed after the spec-only selector corrections |
| Linux production Serendipity Docker build | Passed |
| Final-image Chromium docs/Connect browser gate | 12/12 passed |
| Single scoped Impeccable detector | 0 primary findings, 86 incumbent advisories |
| Independent finish review | `disposition: ship`, no material fixes |
| Independent design documenter | No canonical system changes; existing focus-preview drift recorded without repair |

The browser gate serves real shipped Markdown and Connect pages; only the guest `/api/v1/me` response is intercepted. It checks all three localized launcher CTAs, mouse and keyboard activation, approved document navigation, all five fragment clicks/native hashes/reloads, the five added guide responses, unknown/prototype/traversal 404s, external URLs, readable repository citations and mobile overflow. These are website checks, not game-client tests.

The first 12-case attempt passed 10 cases. Two selectors were wrong: the local `.md` assertion also counted a valid external GitHub source, and the CJK selector omitted normal URI encoding. Both were corrected only in the spec; the same production image then passed 12/12. Neither production code nor the image was changed to satisfy those assertions.

The successful isolated PostgreSQL 17 website fixture was `illya-web-smoke-eedf208707e14645`; its containers, network, volumes and generated temporary environment were verified removed. The failed earlier fixture `illya-web-smoke-05eda04cd0854214` was also verified removed. No retained community data was used as a browser fixture.

## Bounded finish evidence

Exactly two finalized viewport captures were opened together in one postbuild root inspection: desktop 1440×1000 dark English Connect with the lazer tab, and mobile 390×844 light zh-TW at the actual CJK fragment destination. [Capture metadata](connection-docs-capture-metadata.json) records exact paths, hashes, times, image pins and eleven production-source hashes. A separate pre-edit baseline inspection established incumbent visual truth. No production edits, second build or second detector followed the final captures.

The unavailable dedicated finishing roles were replaced by fresh independent generic agents following the full supplied role contracts. The [review](connection-docs-verdict.md) and [documenter report](connection-docs-documentation.md) retain their output and evidence limits. PRODUCT.md, DESIGN.md and the sidecar were preserved; no AI artwork or visual-world replacement was introduced.

## Retained local deployment

Only Serendipity was recreated in `illyaverse` and `illyaverse-client-test`, using `--no-deps --no-build --pull never`. All eight backend/database/proxy container and image identities remained unchanged, and all ten services are healthy. No backend writers were stopped, migrations applied, native clients operated, trust/hosts settings changed, or compiler caches/backups deleted.

| Service | Current image |
| --- | --- |
| Serendipity | `sha256:b07c455d2c98f0506dd7ad1c207e60bdae6f120d439aca404ce74c21b500298c` |
| Sanctuary, retained | `sha256:fdeba8f3767c41aae64e7335443f1a66c6fb0e808715e8436daf02d6952cdcac` |
| Resonance, retained | `sha256:f50992f1a2170ab38d1f830269c4eaf8876520a779a67146f0f665326a4ddb1c` |

The completed runtime gate made **60 read-only HTTP checks** across `http://localhost:3000` and `https://illya.test`: all 25 guides return 200 with real heading IDs, three unknown/prototype routes return 404, Connect returns 200 with current copy, and guest `/api/v1/me` returns 401 at each origin. It also checks the real Unicode target and shipped fixed-server protocol language. HTTPS uses the existing public Caddy root only for those curl requests; machine trust was not modified.

| Retained project | Users before/after | Maps before/after | Scores before/after | Schema |
| --- | --- | --- | --- | --- |
| `illyaverse` | 2 / 2 | 0 / 0 | 0 / 0 | Exactly successful 0001–0020; no failures |
| `illyaverse-client-test` | 2 / 2 | 44 / 44 | 0 / 0 | Exactly successful 0001–0020; no failures |

Detailed ignored evidence is retained in `data-connection-docs/frontend-lint.log`, `frontend-types.log`, `build-web.log`, `browser-final-image.log`, `browser-corrected.log`, `detector.json`, `deployment.log`, `runtime-verification.log` and before/after JSON summaries. These are development records, not required runtime files.

## Remaining native evidence

The earlier native stable failure remains unverified. This scope does not change the score parser, its scalar/replay limits, or diagnostic policy. The pending metadata-only native retry is still required to establish the real multipart format and a successful persisted score. Launcher GUI/game startup, gameplay, restart and post-exit plugin cleanup also remain user-operated native validation; website navigation and isolated protocol tests do not establish those outcomes.
