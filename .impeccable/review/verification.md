# Serendipity build evidence

Code-first, no approved image comp. User pinned osu! layout inspiration, pure-black default, dark glass, light/dark toggle, rounded lettering and player personalization; no AI artwork.

Inspection round 1: desktop1440×1000 and mobile390×844;10 captures covering landing/settings/maps/chat/connect. Material fixes were batched: mobile community copy compression, primary Link-button foreground contrast, account theme reload race, translated sheet close and save description.

Inspection round 2:18 full-page captures, same viewports, with registration, docs, own profile and administrator added. Every capture was opened and validated. Browser capture reported0 page errors and0 horizontal document overflows. Markdown docs are long; inspect at original detail if needed. UI screenshot counts/messages are actual disposable local QA fixtures, not a fabricated production community.

Functional Chromium tests:3 passed on the real Docker stack. They cover black default, next-intl cookie/account locale, mobile navigation, registration, background upload, persistent theme, profile bio, chat, social add/block/unblock, logout and server RBAC. No trace recording or credentials in captures.

Static verification: TypeScript and ESLint pass. Production Next.js build passes in Linux Docker and Windows native. A project-local SWC cache retains the native loader's trust/hash checks; no system ACL changes. Runtime dependency npm audit reports0 vulnerabilities; current development eslint transitive dependencies report5 high advisories with no patched braces release available at inspection time.

Detector ran once after round2 on apps/serendipity/src and returned[]. No further visual polishing by the build thread; material changes must come from the independent finish reviewer.

Post-capture functional changes: SWR child caches are keyed by authenticated account to clear private caches on session changes; documentation filesystem tracing is bounded by explicit Docker copying; Windows native build cache moved to the monorepo root to keep materialized DLL paths shorter. All three Chromium flows passed again against the final Docker images.

The independent reviewer returned `fix` for two mobile compositions. One correction batch added readable minimum-width Markdown tables in labeled keyboard-focusable horizontal scroll regions, and stacked the profile score heading above intact game-mode controls. The same 18 capture paths were regenerated and each opened for validity; browser capture again reported zero page errors and zero document overflows. The same reviewer scored both fixes `resolved` and returned `ship` at that two-fix scope. No second detector was run.

The harness exposes generic subagents rather than the named shipped review role. Review used a fresh generic substitute, without inherited build history, instructed to load the shipped reviewer definition. The subsequent documenter follows the same fresh-context substitution. This does not establish native client compatibility.

Known disposable browser fixture users and messages were removed by exact IDs, usernames and test emails before the verdict captures. The preview now contains only the real bootstrap administrator and no beatmaps or scores; its activity strip reports those actual values.
