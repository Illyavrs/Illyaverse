# Beatmap withdrawal verification

The feature adds reversible canonical-set withdrawal and restoration across the website, Sanctuary and Resonance. Ranking status remains independent. Packages, immutable IDs/files, saved scores/replays and frozen histories remain retained; active discovery, downloads, score admission and qualified pp participation follow availability.

This record separates implemented source, isolated integration, rendered UI, final Docker images, retained deployment and real native clients. The original reported stable submission failure is not established as fixed by synthetic requests.

## Source and PostgreSQL evidence

The final combined source gate passed with `CARGO_INCREMENTAL=0`, using the existing workspace target and one uniquely named, labelled tmpfs PostgreSQL 17 fixture. Generated credentials and all five process environment values were restored; the fixture identity was validated and its removal verified. Neither retained community database was used.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --workspace --all-targets` | 201 unit cases passed |
| `cargo test -p resonance -- --ignored --test-threads=1` | 63 PostgreSQL cases passed |
| `cargo test -p sanctuary -- --ignored --test-threads=1` | 78 PostgreSQL cases passed |
| `npm run test:availability:db` | 19 real PostgreSQL boundary assertions passed |
| Node infrastructure/helper/migration guards | 32 passed; 12 affected helper checks also passed after fixture corrections |
| Frontend ESLint and TypeScript | Passed |

The schema boundary fixture applied 0001–0019, seeded retained synthetic content, then applied only additive migration 0020. It checked both identity namespaces, all ranking statuses, preserved rows/files and identities, restoration, exact saved versus new/foreign-partition publication, revision/delete restrictions, event rollback, and actual admission/withdrawal lock waiters. Historical migrations were not changed.

Service tests cover warm and in-flight metadata, immutable ID/hash/filename resolution, official archive publication, authority rechecks, new versus saved solo/room/stable score admission, PP/best exclusion and restoration, retained replay/history and token lifetime. Exact receipt tests additionally reject changed judgement distributions under the same legacy online checksum, prevent missing-replay backfill with changed input, and compare winners published during set-lock or Sanctuary HTTP waits. Exact original inputs remain retryable after review/pp changes and withdrawal; derived performance state is not receipt identity.

Earlier failures were retained in local logs and corrected before this final gate: a missing test-fixture database environment, two fixtures predating the active-map view/private-reason split, and a test-only missing Router import. The first release HTTP run also exposed an insufficient saved-receipt comparison; the production semantic guards and independent PostgreSQL regressions were added before rebuilding both backends. These earlier attempts are not counted as passing evidence.

## Final production-image protocol evidence

The completed isolated release-image gate issued **438 HTTP requests across 51 check categories**, including **89 availability/receipt requests**. It used original synthetic encrypted stable submissions, binary bancho interactions, JSON/SSE lazer flows and real PostgreSQL/HTTP services. It did not launch either native client.

It checked availability authentication and authorization, compare-and-swap/no-op behavior, retained staff metadata without public reason leakage, warm native selectors and downloads, denied new publication versus exact saved retries, retained score/replay/frozen-room history, active pp/best exclusion with lifetime counts retained, restoration preserving IDs/bytes/receipts, deferred intent publication after restoration, and uncached official withdrawal without an upstream fallback. The same-checksum changed-judgement test now returns 409 before and after withdrawal without changing the saved score, replay or score count.

| Service | Exact image ID |
| --- | --- |
| Sanctuary | `sha256:fdeba8f3767c41aae64e7335443f1a66c6fb0e808715e8436daf02d6952cdcac` |
| Resonance | `sha256:908ffce02107d99219ed3d1386b49dbeff359c1bfcfb653fde5701c76de77d72` |
| Serendipity | `sha256:3045e410aa35efae360539d0e22a8b09dffc43e2cac3e426ae08c3d117faf5a9` |
| PostgreSQL 17 | `sha256:1bea307dfb3ee30541a7acf7de14b58bcd6948da98e5d31a04c627c4d35ec64b` |
| Caddy | `sha256:f77f856a30f0004200b36b322d61da17fade31e24875699d77fb968399b9eb77` |

The protocol project `illya-smoke-5a198e59f53d503b` was created only for this invocation. Its containers, network, volumes and generated temporary environment were verified removed. Compiler caches, retained community storage and backups were preserved. Local detailed evidence is in `data-beatmap-withdrawal/source-gate-final.log`, `schema-gate.log` and `protocol-receipt-final.log`; these ignored logs are not required runtime files.

## Frontend and bounded design finish

The new withdrawal browser fixture passed **10/10** with synthetic intercepted API requests. It covers whole-set request composition, required reasons, revision-conflict recovery, acknowledged-write refresh failure, permission/session loading, retained-data cache clearing, public withdrawal, filter/history/pagination and restoration without ranking-status changes.

Exactly three required captures were finalized: desktop dark zh-TW confirmation, mobile light ja read-only retained detail and mobile dark en public withdrawal. Their viewports, dimensions, exact paths/hashes, inspection rounds and capture-time image pins are in [capture metadata](beatmap-withdrawal-capture-metadata.json). The captures used the final web image above but earlier backend images; intercepted frontend evidence does not establish backend behavior. The rebuilt backend protocol gate is recorded separately above.

The initial capture attempt passed 7/10; three navigation timing assertions were corrected. A subsequent screenshot check found document scroll was not at the top; only the capture helper was corrected and the valid captures were inspected as one batch. After the reviewer copy fix, the same three paths were recaptured and passed the same 10-case gate. These corrections are not described as three independent successful visual gates.

The single detector invocation reported **0 primary findings and 86 incumbent advisories**. No second detector or global design repair was run. An independent generic substitute performed the [full review](beatmap-withdrawal-verdict.md), which requested explicit uncached-official scope in confirmations. Six localized hints were corrected. The independent [scored fix review](beatmap-withdrawal-fix-verdict.md) returned `ship` for that one fix, and the independent generic [documenter report](beatmap-withdrawal-documentation.md) records the five-line system summary and proof boundaries. Dedicated reviewer/documenter roles were unavailable. No AI artwork, visual-world replacement, global DESIGN or sidecar changes were made.

The assembled final images covered **63 distinct regression cases across eight related website files**: catalog workflows, local/official packages, score/map controls, native score details, profile best, RX and live website chat. The first terminal run passed 61 and failed two catalogue moderation cases that implicitly assumed `/admin` selected Scores. Availability intentionally makes the read-only Beatmaps section visible to `admin.view`; both failing snapshots were on that section. The production score moderation/pagination implementation was unchanged from baseline `1a979af`.

Only the two test fixtures were corrected to explicitly select Scores, assert its selected state and unique pending row, and assert the complete PATCH body and saved feedback while retaining the page-one clamp checks. The affected catalogue file then passed **9/9** against the same final images. The other 54 cases remained covered by the unchanged passing run. This is combined regression evidence, not a claim that the first eight-file run passed. Captures were disabled and no UI production source/image changed for this correction.

Both website fixture projects (`illya-web-smoke-1bda38cbbaad93f1` and `illya-web-smoke-d40fbc7f2244f956`) verified removal of their owned containers, network, volumes and temporary environment. Local logs are `browser-regression-final.log` and `browser-catalog-corrected.log` in `data-beatmap-withdrawal`.

## Retained deployment and native-client boundary

The retained main/client stacks have not yet been upgraded to migration 0020 for this feature. Deployment requires quiet native sessions, verified image pins and fresh database/shared-media backups, then fingerprint/count checks and read-only runtime verification. No test set is withdrawn in the retained community databases.

Real stable gameplay/submission and stock-lazer bridge behavior remain pending human client verification. The existing diagnostic retry request remains pending; no additional client run, hosts/CA change, trust modification or native process control was performed by these gates. This feature's passing server evidence does not complete the overall Illyaverse goal or prove all native protocol/client versions.
