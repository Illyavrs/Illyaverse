# Verification record

Last local verification:2026-10-07. The running review stack is a local Docker Compose deployment; fixtures created by tests are not a production player community.

| Boundary | Evidence | Scope |
| --- | --- | --- |
| Serendipity | TypeScript, ESLint, Linux Docker build and native Windows build pass | Current source; UI uses account-isolated SWR caches |
| Browser flows |3 Chromium end-to-end tests pass | Registration, locale, themes, background upload, profile, social, chat, logout and RBAC |
| Responsive UI |18 desktop/mobile full-page captures,0 page errors,0 document overflows | Landing, appearance settings, map browse, chat, connection, registration, docs, own profile and admin |
| Design detector | Returned no findings | Ran once after second inspection; independent finish verdict recorded separately |
| Sanctuary | Core unit/domain tests and real PostgreSQL isolated-schema integration pass | Durable data, score/replay policy, permission escalation protection, password revocation, social ownership and private-room channel exclusion |
| Resonance gameplay/API | Independent live HTTP/binary/encrypted score smoke pass | Stable login, status, chat, friends/blocks, room authority/state flow, spectating, score integrity, PP, replay round-trip and deduplication; lazer OAuth, map lookup/archive download, solo scores, rankings and verified profile statistics |
| Resonance realtime | Real TCP JSON/MessagePack WebSocket and SSE integration pass | Negotiation, authentication/revocation, room authority/gameplay, native-format settings/slots, spectator frames/timestamps and metadata; isolated PostgreSQL schema |
| Protocol checks | 23 codec/model tests,11 gateway unit tests and all-targets Clippy pass | Bounded/truncated payloads, secret redaction, authoritative fields, realtime models, level thresholds and avatar boundaries |
| Installed lazer DTO serializers | Six native typed and six Rust-codec-to-native typed round-trips pass | Installed `2026.1005.1-tachyon+6cbcae…` DLLs and their actual SignalR resolver; offline synthetic public fixtures, no application launch |
| Kaleidoscope |15 tests, Clippy and Windows release build pass | Hosts preview/backup/restore, ownership/encoding preservation; actual system hosts were never changed |
| Docker | Compose validation and security invariants pass | Five services, local-only published ports, private PostgreSQL, persistent volumes; production Caddy configuration validated |
| Final Docker protocol images | Independent encrypted HTTP smoke passes against fresh PostgreSQL 17 | Random isolated project, network and volumes; all test resources removed afterwards; preview data untouched |
| Local client HTTPS | Ten `illya.test` hostname chains and routing checks pass | Independent `illyaverse-client-test` stack; explicit CA file and request-local DNS overrides; no system hosts or Windows trust changes |

Exact protocol coverage and remaining limitations are recorded in [client compatibility](protocol-compatibility.md) and [lazer hub compatibility](signalr-compatibility.md). The realtime test uses original synthetic native-format fixtures rather than loading the official client assemblies. No test above substitutes for native stable/lazer gameplay, host/TLS routing, reconnect and replay playback. Production domain, DNS/ACME validation and upstream OAuth credentials remain operator configuration.

CI runs Rust unit checks and the isolated PostgreSQL integration check, frontend build/type/lint, Docker startup, browser end-to-end checks and the independent gateway smoke through `infra/protocol-smoke.mjs`. The smoke launcher requires `ILLYA_ALLOW_DISPOSABLE_TEST=1`, generates a fixture player and reads local bootstrap credentials without printing them. Use it only with a disposable local Compose database.

For an existing developer installation, prefer `infra/disposable-protocol-smoke.mjs`, which pins the existing Docker images and creates its own fresh PostgreSQL, network, volumes and credentials before running the independent Python test. It removes only that random test project afterwards. Four launcher guard tests pass. On PowerShell, set `$env:ILLYA_ALLOW_DISPOSABLE_TEST='1'` before running the Node helper; install `services/resonance/tests/requirements.txt` in the selected Python environment first.

The final local workspace run passed 60 Rust unit/CLI tests, both opt-in isolated PostgreSQL integration tests, rustfmt and all-targets Clippy; all three browser flows passed against the rebuilt images. Browser fixture accounts/messages were removed by their exact known identities before the final 18 captures. The independent finish reviewer scored both requested mobile composition fixes resolved, with a `ship` verdict limited to that fix list. See `.impeccable/review/verdict.md` for its scope and generic-role substitution.

Local HTTPS verification used Windows Schannel curl with `--ssl-revoke-best-effort` because this explicit development CA has no public CRL distribution point. Hostname, chain and certificate validity checks remained enabled. A separate Linux curl request verified the same CA chain and HTTPS API without that Schannel option. [Local client testing](local-client-testing.md) records the commands; native game behavior is still unverified.

The initial HTTP port-80 check reached an existing WSL Ubuntu nginx rather than this test stack. Windows listener inventory did not show that forwarded listener; the setup preflight now also probes loopback TCP. The user explicitly authorized temporarily stopping Ubuntu's nginx. After that stop and recreation of the test Caddy, port 80 returned Caddy's `308` redirect to `https://illya.test/`, and port 443 passed the CA-verified HTTPS API check. Nginx's boot configuration was not changed; stop the test stack before restarting it with `wsl --distribution Ubuntu --user root --exec systemctl start nginx`.

Current runtime npm audit reports no vulnerabilities. Development-only transitive dependencies of `eslint-config-next` report five high advisories around `braces`; the inspected package release has no patched compatible version. This is recorded rather than hidden by changing the requested stack or suppressing audit output.

The Windows build launcher uses an ignored cache in the monorepo root to avoid both the machine's incompatible default SWC cache ACL and longer materialization paths. SWC still performs its own path trust and native image integrity checks; no operating-system ACL or native client configuration is changed. See [SWC native carriers](https://github.com/swc-project/swc/blob/main/docs/native-addon-carriers.md).
