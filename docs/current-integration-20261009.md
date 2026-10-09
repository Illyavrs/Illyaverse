# 2026-10-09 整合 checkpoint

The retained `illyaverse-client-test` completed **schema 25 -> 27 at 2026-10-09 19:37:09 UTC**. Its three application services now include private-message receipts, the effective catalogue and archive acquisition. Main remains schema 22. The runtime images were built from frozen parent `0b4bec3a05fe0136cff8192e461aa3b4f608626f`; this later documentation update does not change that build identity.

The previous lazer login success is a player report. No new human native result has arrived: Normal scores, gameplay, replay, chat, Relax, multiplayer, reconnect and exit cleanup remain pending on the latest environment. The Debug GUI test package is unchanged; the Windows error 5 release-build limitation remains unresolved. Historical checkpoints and failures below retain their original scope.

## Retained schema-27 deployment

| Boundary | Current state |
| --- | --- |
| Client-test | Schema 27; three application services deployed. |
| Main | Schema 22; protected runtime and data preserved. |
| Runtime image source | Frozen parent `0b4bec3a05fe0136cff8192e461aa3b4f608626f`; later docs-only commits are separate. |

| Service | Deployed image |
| --- | --- |
| Sanctuary | `sha256:780dc57bae6fa7c414e0702d8155567bc852659d572b37ec5826697e71864da4` |
| Resonance | `sha256:57565bd01eb363fb2919d317709a00eb1372fc81048f3b61ad120328abc46c4e` |
| Serendipity | `sha256:d39a15a55f7dbc768a32ce0c87301f3553d5f2e83961d2ce1b6a9324a59e610a` |

The [build receipt](../.artifacts/builds/integration-images-27-d101fa4ddabb4289913763689f89b8ca/verification.json) binds all three zero-exit builds, five child pins, unchanged physical COPY inputs and 27 exact LF migrations. The [release aggregate](../.artifacts/builds/client-community-deploy27-5d0a0889c57145b6b36610e7e9bf4a30/release-gates27.json) binds the final gates: isolated startup passed nine check groups, protocol passed 593 HTTP requests / 68 categories with final cleanup, and recovery matched 45 tables / 72 synthetic rows / 19 sequences and three binary fixtures with UID/GID 10001. Categories are not assertion counts. The primary 14 selected browser cases and isolated branch suites remain separate; two startup harness failures, the recovery SQL-dollar harness failure and the protocol immediate-port-release wrapper failure are preserved.

The [reviewed read-only Preflight checkpoint](../.artifacts/builds/client-community-deploy27-5d0a0889c57145b6b36610e7e9bf4a30/run-preflight-20261009T193216Z-ed98ba354d954e87b2e0c2563a0dc1cb/reviewed-checkpoint.json) bound ten retained containers, all 14 quiet predicates, 60 client / four main files and root ownership. The [actual Deploy manifest](../.artifacts/builds/client-community-deploy27-5d0a0889c57145b6b36610e7e9bf4a30/run-deploy-20261009T193340Z-17b52a123bca4127959a5fe334ec39f2/manifest-0010.json) is `passed` / `complete`: three services verified, all ten checks passed and private backup completed. PM receipt backfill, initial catalogue state and the exact schema-27 ledger passed; original core rows, stored bytes/ownership, roles/sequences, rank history, client PostgreSQL/Caddy, all main runtime and the public CA were preserved. Protected environments and existing application values were preserved except the two declared client-test API provider additions. No build, pull, retag, manual SQL or native-client control occurred during Deploy.

The [post-deployment HTTPS receipt](../.artifacts/tests/client-https27-20261010-192510-root/verification-8ac0452768c54a25abe8b7d03f07efc6.json) passed **10/10** with default Windows trust and no override. `/users/2` and `/admin` returned 200; the players alias returned 308 with its exact `mode=0&variant=relax` query preserved; the local server catalogue API returned 200 and unauthenticated `/api/v2/me` returned 401. These status checks do not certify authenticated administration or native gameplay.

One [real anonymous archive GET](../.artifacts/tests/live-archive27-44642251e3594731b0cfca477720d840/verdict-20261009T193800Z-54fecc9b09064fcaafa5647dfbaa8507.json) through the deployed server route for set 1 passed: HTTP 200 in 5.509 seconds, 2,840,455 compressed bytes, matching server SHA-256 and `.osz` attachment headers, three ZIP entries, one chart, one nonempty referenced audio file and verified CRCs. It used normal TLS with no retry or redirect and persisted no raw archive. This single transfer did not independently verify the complete upstream manifest or database preservation, and did not test import, playback or a native client; deployed server metadata validation and the separate backend fixtures retain their own scope. The earlier release aggregate's HEAD302-only provider basis remains historical.

The client-test API uses `OSU_ARCHIVE_URL_TEMPLATE=https://osu.direct/api/d/{set_id}` and `OSU_ARCHIVE_REDIRECT_HOSTS=storage.osu.direct`; the general `.env.example` defaults remain empty. A separate [post-cutover configuration receipt](../.artifacts/tests/client-https27-20261010-192510-root/provider-configuration.json) records only these two public keys added to ignored `.env.client-test.local`, with all other original bytes, UTF-8 and ACL preserved. That file update did not change the running environment or main configuration. Private backup contents and other environment contents/hashes remain unpublished. Exact receipt SHA-256 references are in the [evidence index](current-integration-20261009-evidence.json).

## Production frontend and native evidence follow-up

One [anonymous production browser run](../.artifacts/tests/production-web27-functional-bbd1c24f0c3141a0b655897970507bc1/verdict.json) passed 26 assertions across `/users/2`, `/beatmaps` and `/beatmaps/official/1` in 5.262 seconds using default TLS and real public API responses. It checked nonempty profile score rows, the finite server catalogue, all original set-1 difficulties and the exact enabled full `.osz` link. No archive GET, authentication, screenshot, API write, native action or container change occurred. These checks used the deployed image parent `0b4bec3a05fe0136cff8192e461aa3b4f608626f`; the observed docs-only HEAD was `686ad0d5e2c99e85c2174660f564db1c6690b0b2`.

The original receipt preserves one uncategorized console error per route and failed prefetch requests; it records zero uncaught page errors and zero caught load-error UI. A separate [profile-only trace](../.artifacts/tests/production-web27-functional-bbd1c24f0c3141a0b655897970507bc1/console-diagnostic-4cf7339b6f6d4324b8e3cd3615c01e7e/appendix.json) directly correlates its sole console error to the expected anonymous `/api/v1/me` 401. The same cause on the other two original routes is an inference. Seven aborted non-navigation fetches occurred before context close; their cause is not attributed to cleanup. The [post-exit receipt](../.artifacts/tests/production-web27-functional-bbd1c24f0c3141a0b655897970507bc1/console-diagnostic-4cf7339b6f6d4324b8e3cd3615c01e7e/post-exit-cleanup.json) confirms the owned Node process and children were absent, browser/context closed and no server ports created. This is not a globally clean console/network claim.

A single [read-only server observation](../.artifacts/audits/native-observation27-b55bc533fefa4c1d9246e898958d0f0b.json) at 19:51:19 UTC found four total scores, no new user-2 score since cutover and zero pending lazer, replay-recovery and scale-recovery records. That instantaneous snapshot cannot prove a client opened, login was attempted or gameplay/cleanup succeeded. Latest human native verification remains pending.

## Retained schema-25 deployment (historical)

這次部署固定 parent `de4653e464796d5e38af6d7caa0e710a80f9d070`，只替換 client-test 的三個 application containers。部署前完成私人 backup；client-test PostgreSQL／Caddy，以及主部署所有 containers 的 IDs、StartedAt、mounts、環境值與保留 guard 均通過。原 core rows、stored file bytes／ownership、sequences 與 rank history 保留，client-test 帳號 `illya`（公開 id `2`）的 administrator role 未改變。主資料庫的 schema 22 未改變，client-test 的 migration 1–25 ledger 精確相符。映像不重新建置、pull 或 retag；只有正常 application startup 套用資料庫 migration。

| 已部署服務 | 固定映像 |
| --- | --- |
| Sanctuary | `sha256:fd8bf8d14b8dc44e1a015bd00f7ea5423f2b17ceb2a88dc4a7607738ddf8b31b` |
| Resonance | `sha256:2ca20da6acd0f00c8bd7c4931cc17e4a04d050b7b65ec0db496343ce18d80694` |
| Serendipity | `sha256:82f0bafbdb72db72d4b67e8d5e61a937fb415f55da7d271b9dad32be45ae2c80` |

[實際 deployment manifest](../.artifacts/builds/client-ranking-deploy-20261009T162512Z-e73784ed224b42f38e60d6a240b85445/run-deploy-20261009T182149Z-714b465ed7d1404cb3607fdae7b209d4/manifest-0010.json) 記錄 `status: passed`、`stage: complete`，SHA-256 為 `80596875067a818fe9964936fa302ad6fff13464017a8b1f9698a6e6a43ea26f`。文件只保存私人 backup manifest 的 SHA-256 `664c969862c6b1aa49365226df65b32ce1bd534090331f59dfa64209141833c7`，不公開 backup records、環境內容、帳號憑證或密鑰。

[source-25 gate aggregate](../.artifacts/builds/client-ranking-deploy-20261009T162512Z-e73784ed224b42f38e60d6a240b85445/release-checkpoint-20261009T181653Z-3cd11c3ff7b3401ebdca71f9a27f8804-release-gates-v1.json) SHA-256 為 `bb529d82ca43838d5445043f481e07d3055def75fe435ef4c0ae54b9649e14af`，保存固定來源、建置與各 gate 引用。網站是 **128 個未改動的原案例，加上 6 個明確修正 copied harness 的等價案例，共 134 logical cases**；不是所有原 suites 都通過。協定是 593 HTTP requests／68 check categories，類別數不是 assertion 數，原 wrapper delayed-cleanup failure 保留，最終清理證據另存。固定 schema-25 recovery、migration source、Rust 與 startup fixture gate 各自保留其範圍，不能當作 source-26／27 或真人測試結果。

原 coordinator 的兩次 read-only Preflight 因既有 API／protocol 的 `RUST_LOG` 與 frozen Compose render 不同而在 activation 前失敗，未改動保留環境。後續 private override 只沿用這兩個既有 logging 值，before／after 所有環境值仍精確相同。另一個 empty-root files dictionary checkpoint serialization 缺陷也在 Deploy 前被發現；修正後的 115 offline guard cases、實際 read-only checkpoint/file proof 與新的 Preflight 才支援這次成功部署。失敗 manifest 與修正證據均保留於 aggregate，沒有覆寫為成功。

部署後 **18:25:33 UTC** 的 [HTTPS receipt](../.artifacts/builds/client-ranking-deploy-20261009T162512Z-e73784ed224b42f38e60d6a240b85445/live-https-20261009T182533Z-02851906cfee4a5c9d0ce28a29f3d595.json) SHA-256 為 `579b189b78a5c87b04a8b68fa0439e6406ddc6cc5296a786941cc4125bc77cc4`，9／9 通過，使用正常 Windows trust、沒有 TLS bypass、cookies 或 credentials。`/users/2` 與 `/admin` 回傳 200；`/players/2?mode=0&variant=relax` 回傳 308，`Location` 精確為 `/users/2?mode=0&variant=relax`；`/api/v2/me/` 回傳預期未登入的 401。這些唯讀 HTTPS checks 不證明登入後的管理權限或原生遊玩，沒有控制真人 client。

## Source-26/27 integration before deployment (historical)

後續私訊 read receipts、effective catalogue 與官方完整 archive acquisition 已合入目前來源，migration 來源為 1–27；保留 client-test 仍為 schema 25，主部署仍為 schema 22。新的固定 schema-27 cohort 尚待完成，以下隔離來源收據不能取代最終 composition gates，也不能回填成現有部署或真人 client 證據。

| 範圍 | Schema | 狀態 |
| --- | --- | --- |
| 保留 client-test | 25 | 已部署；上節 manifest 與 HTTPS 收據。 |
| 主部署 | 22 | 保留未變。 |
| 目前整合來源 | 27 | 來源檢查；新映像、startup、protocol、recovery 與保留部署仍待完成。 |

| 隔離來源檢查 | 已確認範圍 |
| --- | --- |
| Source 26 私訊 | [Backend 收據](../.artifacts/workspaces/pm-receipts-67b996f3fda34add873e8ee3a6448543/.artifacts/tests/pm-receipts-67b996f3fda34add873e8ee3a6448543/verification-confirmed.json)：2 個實際 PostgreSQL cases、7 個 unit cases 與 Clippy；低 ID 的晚提交訊息仍保持未讀。原始 before assertion 與 passing phase 後的 wrapper failure 保留。[UI 收據](../.artifacts/workspaces/pm-receipts-67b996f3fda34add873e8ee3a6448543/.artifacts/tests/pm-equal-marker-ui/verification-confirmed.json)：27 個 intercepted Chromium cases，原 26 passed／1 timeout 與 label-only harness 修正保留。 |
| Source 27 catalogue | [Backend aggregate](../.artifacts/workspaces/effective-catalogue-bd3966bd302142f1baf6dcb05af35640/.artifacts/handoff/effective-catalogue-62-5bfb3384c353494e96b91b501f8c0d90/aggregate-verification.json)：62 unique cases＝9 unit＋53 owned PostgreSQL，Clippy／format 通過；隔離 ledger 是 1–25 加 27，排除 26。[網站 fixture 收據](../.artifacts/workspaces/effective-catalogue-bd3966bd302142f1baf6dcb05af35640/.artifacts/tests/server-catalogue-webgate/verification-final.json)：最終 82／82 browser cases，原 64／78 與 14 個 failures 保留，不累加為 160 個通過案例。 |
| 連續 schema 26／27 | [Composition verdict](../.artifacts/tests/schema27-20261009T184727314Z-24c409a5-0dab-4735-9df4-67ef5b9460be/composition-verdict.json)：6 個同步實際 PostgreSQL cases，26 fresh／24／25 與 27 fresh／25／26 全部通過，完整 1–27 ledger 與 checksums 相符；歷史 1–25 LF bytes 相同。[原 Node fixture failure](../.artifacts/tests/schema27-20261009T184315797Z-601ab314-3d1c-4f59-8e94-cb8ce0e5fc5e/verification.json)保留，沒有覆寫。 |
| Source 27 archive 網站 | [最終收據](../.artifacts/workspaces/archive-acquisition-91d6606738714598b6f6b9fca8496b7d/.artifacts/tests/website-archive-downloads-20261010-8b7dc5b882d841efb3dc0b17d63a48ed/verification-final.json)：18 focused cases＝6 transport unit＋12 browser，另有 6 existing browser cases；兩次 bounded capture batches 確認 7 張圖。原失敗 reports 保留，僅用 owned loopback ZIP fixture，沒有測試真實 provider。 |
| Source 27 archive backend | [Step-level composite](../.artifacts/workspaces/archive-acquisition-91d6606738714598b6f6b9fca8496b7d/.artifacts/tests/archive-acquisition-20261009T185912Z-9ae65c4a810f435dbda233ccd3588b0d/composite-verification.json)：115 actual PostgreSQL cases、13 unit cases 與兩個服務的 Clippy。四份原 whole receipts 仍為 failed；兩項 style／fixture delta 與 passing steps 分別保存，沒有把原收據全部標成通過。 |

合併來源的 TypeScript、scoped ESLint 與三語系 1099 keys parity 已通過。[33-file source manifest](../.artifacts/archive-source-integration/final-source-manifest.json)保存五份 TS／TSX 的 whitespace／normalized AST equality 與三份 locale JSON 的 deep value equality；最後 12 個 owned files 的 Prettier check 通過。[18:54 composition record](../.artifacts/tests/source26-27-web-composition-c7042bc9f36b48f88c2a18269e217afa/run-meta.txt)的五個檔案 format warnings 保留為歷史結果。

Primary 的 selected browser composition 為 **14／14**：appearance 4、chat 2、server catalogue 3、public official 2、archive 3。前四個範圍見[共同 run record](../.artifacts/tests/source26-27-web-composition-f5b5acbb29984d4faee073d7faaaeec8/run-meta.txt)，archive 見[最終 API rewrite fixture 收據](../.artifacts/tests/source26-27-web-composition-ac630d4e47dc4547817aa15945ef39d0/run-meta.txt)；初次 missing-API-rewrite harness failure／partial archive run 保留，沒有修改 application 或 tests 來完成重跑。這是 selected intercepted/loopback composition，不是所有 browser suites。

[Primary Rust verdict](../.artifacts/tests/source27-primary-rust-composition-0afb48fbfb0e424ca56a433a5322248f/verdict.json) 為 PASS：Sanctuary／Resonance `check --locked --tests`、兩個服務加 domain 的 all-target Clippy（`-D warnings`）、workspace rustfmt 與 6 個 pure `official_set` tests 通過。176 份 Rust／Cargo／toolchain／SQL inputs（包括 migration 1–27）前後雜湊相同；沒有來源修改或 PostgreSQL 重跑。[Preservation index](../.artifacts/tests/source27-primary-rust-composition-0afb48fbfb0e424ca56a433a5322248f/preservation.json)保存 29 份 exact copied receipts／logs，verdict 中的原 Temp 路徑僅為歷史來源。這些 primary code gates 不等於 runtime deployment：新的 schema-27 映像、startup、full protocol、recovery 與保留部署仍待完成。

Archive acquisition 的來源設定是 optional `OSU_ARCHIVE_URL_TEMPLATE`：HTTPS path 包含一個 `{set_id}`，預設空字串，未設定時 metadata-only sets 仍不能下載完整 `.osz`。額外 redirect hosts 可用 `OSU_ARCHIVE_REDIRECT_HOSTS`。Website 收據驗證 bounded transport、stored archive 與 plain download anchor 的 fixture 行為；初次 availability metadata 仍需要 JavaScript，沒有證明完整無 JavaScript 流程、真實 mirror、stock client 或真人送分。所有新引用與 SHA-256 保存在[證據索引](current-integration-20261009-evidence.json)，舊 source-25 deployment 與 gates 保持原樣。

## 先前 checkpoint 的歷史範圍

The following sections retain their original source, image and runtime states, including schema 22/25, HOLD and pre-deployment descriptions. Current client-test state comes from the schema-27 manifest above; old gates and failed receipts have not been rewritten as new-source evidence.

## 本輪來源與功能

Serendipity、Sanctuary、Resonance、Kaleidoscope 與 Transfiguration 已各有獨立 Git 歷史，Illyaverse parent 登記五個 submodules。根 Compose、migrations、共用 crates 與 workspace lockfiles 繼續由 parent 管理；Kaleidoscope 的 UI 位於 `tools/kaleidoscope/ui`。Origins 指向本機 `.git/local-remotes/`，尚未設定可跨電腦 clone 的遠端。[repository-layout.md](repository-layout.md)記錄實際邊界。

公開圖譜頁預設查詢官方 osu! API；伺服器儲存／社群圖譜另有明確範圍。卡片提供完整回傳難度的唯讀 preview，詳細頁保留完整 `.osz` 下載來源與 unavailable 狀態。帳號設定可檢視和撤銷本人 website／stable／lazer 登入。字體採本機授權子集：Latin Nunito、Traditional Chinese TaiwanPearl、Japanese Zen Maru Gothic。成績選單保留 44px 方形點擊範圍，以 32px 內縮平面顯示 hover；較寬的難度摘要同樣內縮 6px。

## 已完成的自動檢查

| 檢查 | 證據與範圍 |
| --- | --- |
| 整合網站 | Web `sha256:49580deecc6d49e0d83d83853b4802dc989de6bcf101d1647c001c2b27dc4cd8`：112 passed、0 failed。涵蓋字體、公開官方搜尋、登入 sessions、profile best、圖譜庫、導航、國旗／語言與首頁；API／封面 fixtures 不代表實際玩家內容。 |
| 後端隔離協定 | Sanctuary `sha256:d84d71bb9398b8d63df6b9dba5a7b2b5f784aaf174d47b1475c652d9a7037363`、Resonance `sha256:ec8a0058a2a40c553abe52256f6a336aadbad191233a2f63f4b3f0575341667c`：552 HTTP requests、66 check categories，runner exit 0，fixture 清理通過。類別數不等於 assertion 數。 |
| Rust workspace | `cargo test --workspace --locked`：269 passed、204 ignored、0 failed；workspace rustfmt 和 all-target Clippy 通過。這輪發生在後續 CLI package 新增案例之前，不能替代其獨立測試。 |
| 新 CLI package | 普通 debug build 成功；30 項 CLI integrity、41 項 GUI package regression、59 項核心／CLI／生命週期案例與 Clippy 通過。使用 production hostfxr 啟動／監護實作和強制橋接；沒有啟動真人遊戲。 |

112 項瀏覽器檢查完成後新增的 session 撤銷成功回饋修正，已在 Web `sha256:36f52e19775bfc4825dcde1a82d5aa145cbff97943188cdd651ec983fe4114aa` 通過 10 項針對性案例，保留相同七個場景的有效截圖，並取得這項單一視覺修正範圍的 ship 審查。未在此新映像重跑全部 112 項，兩次結果維持分開記錄。CLI 獨立來源審查已確認其請求範圍沒有剩餘 finding，13 份 payload 與原 ZIP 的雜湊保持相同；沒有以來源審查替代真人操作。

原始本機證據：

- [網站映像與 112 項確認](../.artifacts/builds/integration-20261009/web-disposable-smoke-confirmation-20261009T104000Z.json)。
- [552 HTTP／66 類隔離協定](../.artifacts/builds/integration-20261009/disposable-protocol-evidence.json)。
- [Rust workspace log](../data-current-integration-20261009/cargo-test-workspace-20261009.log)。
- [CLI package 驗證](../.artifacts/packages/kaleidoscope-cli-test-20261009-105203-46ed5f615ec7444cb0ed693ce3714969.verification-final.json)。
- [完整 GUI release 建置診斷](../.artifacts/builds/integration-20261009/launcher-package-diagnosis.json)。

這些 `.artifacts`／`data-*` 證據為本機忽略產物，不包含在普通 Git clone；[本輪證據索引](current-integration-20261009-evidence.json)保留引用與 SHA-256，沒有搬移或覆寫舊紀錄。

## 玩家可以測試的產物與未驗證範圍

目前測試產物是沿用遊戲偏好、只隔離帳號且可設定部署網址的 [Debug GUI ZIP](../.artifacts/packages/kaleidoscope-gui-test-20261009-141323-4ae3b865e72a4f75a7d1010d10e73040.zip)，SHA-256 `4dc11a2f7a87fddcc0b50d945b2610504d5643d422a6637f588845f60881bdec`。完整解壓後按[啟動器指南](kaleidoscope-launcher.md)操作。先前 `105203`、`115044`、`124928` CLI 包及驗證保留為歷史證據；其中 `124928` 仍使用整份 `game.illyaverse.ini`，不適用本次帳號隔離要求，不推薦用它驗證新的流程。

首次玩家啟動的 bounded Windows Application event 在 11:26:43 UTC 記錄 `transfiguration_client_assembly_location_mismatch` 與必要 startup hook 的呼叫。啟動器後續狀態為 error，外掛已清理；[安全失敗證據](../.artifacts/builds/integration-20261009/native-launch-failure-20261009.json)只保留固定錯誤碼與事件 metadata。原因是 Guard 只正規化預期路徑，已載入 assembly 的 `\\?\C:\…` 路徑未使用相同規則。修正後同一位置可接受兩種表示法，不同目錄、UNC、device 與 relative path 仍拒絕，原版本與 SHA-256 gate 保留。

此前 `115044` 路徑修正包完成 70 Rust tests／Clippy／rustfmt、70 managed diagnostic checks、8 synthetic hostfxr boundaries、31 項 package integrity。production policy 36、stock contracts 110、六種拒絕案例與 mandatory native fixture 111 為各自 proof 範圍，不相加宣稱真人測試。[該包驗證檔](../.artifacts/packages/kaleidoscope-cli-test-20261009-115044-bf5c60e932ed48ceab31612559fdc290.verification-final.json)記錄 EXE／DLL 與所有 payload hashes、來源及 logs。當時 CLI 等待 Guard ready 或 finalized cleanup/error，失敗固定分類會在退出和清理後保留；沒有自動啟動玩家遊戲或重新部署保留服務。

第二次玩家使用 `115044` 包啟動後回報沒有遊戲視窗。該包的 `running` 只確認 Guard ready 與當時 child 存活；後續狀態為 error、退出碼 `-1073741819`，外掛已清理。Windows Application 1000 在 12:01:00 UTC 記錄 `KERNELBASE.dll` 的 `c0000005`，並有 PID 46680 的現有 crash dump。兩個實際 PE 的 CETCompat 均為 false，不能將崩潰歸因於 CET。[第二次安全失敗紀錄](../.artifacts/builds/integration-20261009/native-launch-window-failure-20261009.json)保留固定分類和 metadata，沒有複製或公開原始 dump。後續啟動狀態須等原始 draw 成功完成才提升為 running；首幀完成仍不等於登入或遊玩通過。

現有 dump 的前三個 native frames 已由相同 PE identity、x64 unwind 與 call-return 位置驗證，指向 AuthNative 的 `VerQueryValueW` 呼叫。靜態追蹤確認它取目前 process main image 的 RT_VERSION，沒有檢查資源載入失敗；舊 CLI 的 VERSIONINFO 為零個，官方 apphost 有一個。[安全版本資源診斷](../.artifacts/builds/integration-20261009/native-version-resource-diagnosis-20261009.json)只記錄 module offsets／metadata，較深的 candidate frame 明確保留為未驗證。當時的 `124928` CLI 使用自己的 Kaleidoscope 名稱及 `0.1.0.0` VERSIONINFO，沒有移除 Auth loader。

`124928` 歷史包通過 80 Rust tests、Clippy／rustfmt、135 diagnostic/progress checks、九項 stock startup metadata contracts、八項 synthetic host boundaries，以及 42 package integrity checks（含真實 Win32 版本資源查詢）。14 個檔案與 ZIP 逐 byte 對應，當時 production DLL 為 `952fff6deb673f5d8d8adc13705e6aaabc05b4993320d1b0b1bd26c71d9ff4a2`。[該歷史包驗證](../.artifacts/packages/kaleidoscope-cli-test-20261009-124928-1abdafafcf364a248a47000d6531d907.verification-final.json)記錄來源、所有 payloads 和分開的 proof scopes；沒有執行玩家遊戲、AuthNative 或 GPU。

本輪新增的來源修正各自完成隔離驗證，尚未部署到保留服務：網站聊天補回與權限撤銷競態通過 17 項瀏覽器案例；自訂 wildcard role 編輯通過 16 項 RBAC 案例。Stable 重試和 replay bounds 通過四項 PostgreSQL fixture；lazer solo durable recovery／exact callback 通過三項 HTTP／WebSocket fixture與獨立審查。使用實際 guarded backup helper 的 recovery 演練通過 83 項離線案例，並在可丟棄部署比對 40 tables、38 rows、18 sequences、22 migration checksums、完整 normalized schema 和三個檔案的 bytes／ownership。這些結果不取代真人客戶端驗證。

先前三次 GUI Release 建置因 Windows error 5 失敗，原因未確認；本次使用現有 `target/debug` 進行不同的 GUI 建置，仍嵌入 production 介面並啟用 `custom-protocol`。CLI 的存在不等於 GUI 包已建成，包雜湊／核心與 headless 測試也不證明遊戲視窗、登入、聊天、遊玩、Score／Replay、多人、重新連線或退出清理已通過真人操作。正式 remote、公開 domain、完整 native checklist 和原始社群功能目標也尚未完成。

追加來源 checkpoint：網站共用語言儲存鎖通過 18 項瀏覽器案例；stable 離線／跨客戶端私訊通過五項新增與十項回歸 PostgreSQL/TCP 整合案例，另有 61 項 nonignored tests、83 ignored。Website 私訊互通在此驗證的是正式共用資料列 bridge，沒有呼叫 Sanctuary 的網站 HTTP writer。Migration 23 的六項來源／checkout／archive 檢查通過，versions 1–22 保持原始 bytes。這些來源變更尚未部署到保留服務。

使用新隔離 Sanctuary image `sha256:7370a94b71a287b8c56e46785188db39ef60d54f2a7de51a2b01994b9ef102a6` 的 recovery 演練通過 91 項離線案例及實際 backup／restore：23 exact migration checksums、40 tables、39 rows、18 sequences、normalized schema 和三個檔案的 bytes／ownership 相符，還原 readiness 通過，兩個自有 projects 及其資源均清理完成。[新演練證據](../.artifacts/recovery-drill-23-20261009-ba6df5a590f84ebc8c0b900971cdfd1f/verification.json)保留新 image 和來源檢查點；原來 22-migration 演練、保留部署和 image alias 保持原樣。

## GUI、帳號隔離與可設定網址的來源 checkpoint

Kaleidoscope 的 GUI 可讀取、驗證與保存部署網址，預設 `https://illya.test`，僅接受私服 HTTPS DNS origin；stable 限 443，lazer 可使用自訂 HTTPS port。儲存影響下一次啟動，目前遊戲保持其 session 捕捉的網址，面板分別顯示 saved／active endpoint。Transfiguration 在入口與網路邊界驗證同一 session 網址，拒絕官方目的地與 fallback。`.kaleidoscope.lock` 與舊 `.illyaverse-launcher.lock` 同時持有，空檔案刻意保留以維持新舊啟動器互斥。

圖譜與 Realm 沿用原本資料位置；音訊、輸入、顯示與 UI 偏好沿用 `game.ini`。七項帳號／伺服器快取值在 `auth.illyaverse.json` 中按 canonical origin 分開保存，官方帳號行保留原始 bytes，舊 `game.illyaverse.ini` 不載入或遷移。這次支援每份資料目錄一個 lazer session；啟動前檢查不能阻止之後手動開啟另一份遊戲。原子保存採樂觀衝突檢查，不支援外部程式同時替換設定檔。

| 檢查 | 證據與實際邊界 |
| --- | --- |
| Managed 端點／設定／啟動 | [完整 managed log](../.artifacts/builds/integration-20261009/managed-auth-endpoint-20261009T140216Z-0a6dff0043bd4d1f8df9bad3ab235203.log)：193 source-linked endpoint policy、135 startup diagnostic/progress、9 pinned startup metadata、38 source-linked auth/file checks；另有 production policy 36、direct stock 135、mandatory bootstrap 136。這些是分開的 proof scopes，64 項 stock shared-preference/auth-only config checks 包含在後兩種 stock scopes 中，不再相加。原生設定 parser／serializer 以自有 fixture 執行，帳號範圍的 API worker 在入口截停且無網路流量；傳輸範圍使用固定 loopback HTTP／WebSocket fixtures。未執行 GameHost、玩家 Realm、GPU、GUI、真實帳號或官方網路。 |
| 固定 managed 來源與 DLL | [來源清單](../.artifacts/builds/integration-20261009/transfiguration-auth-source-final-20261009T140627Z-73a615453f4c4fefaf01dd390a7b3cdb.json)保留 11 份來源 SHA、proof logs 與原子保存限制。production DLL 為 `e29618115a8d083bacadeafc5b99b7adb42059ed8fe1c9c639ce109280899956`；compatibility SHA 為 `7ab9a8d99119b21d737b4af27b2ff1f6457c226fd275bc8e762364ea3153a01f`。 |
| Rust 核心與 synthetic host | [最後 Rust log](../.artifacts/builds/integration-20261009/launcher-endpoint-auth-final-20261009T140737Z-4cfe2b18032e4fefab76c6342b8bb581.log)：90 passed、0 failed（library 69、CLI unit 3、CLI integration 6、lifecycle 12），Clippy／rustfmt 通過；8 項 synthetic hostfxr boundaries，沒有啟動玩家遊戲。 |
| 合成封裝檢查 | [77 項 package checks](../.artifacts/builds/integration-20261009/launcher-endpoint-auth-font-package-20261009T140606Z-7b18db0c52b34b5a9f1ed88569a1884e.log)涵蓋 GUI 字體授權、完整 metadata、檔案／ZIP 拒絕與 debug profile。使用自有合成 payload，不執行 EXE／DLL，不能當作新 GUI 實際封裝成功。 |
| GUI 介面 | [GUI 檢查](../.artifacts/tests/launcher-gui-endpoint-41898b4095854bc38506f686e95050e5/verification.json)：10 項 Playwright Chromium 案例，800×720／390×844 截圖、HTTPS 輸入、儲存失敗、下次啟動設定、清理與 slow-poll 競態。全部使用 synthetic native IPC，沒有真正 Tauri 視窗或玩家遊戲。原 verification 中 cleanup 欄位誤寫為 null；[另存清理收據](../.artifacts/tests/launcher-gui-endpoint-41898b4095854bc38506f686e95050e5/cleanup-receipt-20261009T132930Z-3a55a8385f84464ea903c15e6b6a43c0.json)確認自有 argv 程序與 port 3017 listener 均為零，原始 OS PID 未記錄。 |
| 實際 Debug GUI 封裝 | [封裝收據](../.artifacts/builds/integration-20261009/gui-package-verification-20261009T141530Z-7fb00ef86a9e4f8bb43071d331e569c7.json)、[建置 log](../.artifacts/builds/integration-20261009/gui-package-20261009-141323-4ae3b865e72a4f75a7d1010d10e73040.log)及[package manifest](../.artifacts/packages/kaleidoscope-gui-test-20261009-141323-4ae3b865e72a4f75a7d1010d10e73040/package.json)：producer exit 0，內嵌 HTML／JS／CSS 檢查 exit 0，目錄／ZIP 一致性通過。共 17 個檔案、16 份 payload，包含 Nunito／Noto Sans TC 授權與獨立 GUI font notice；EXE SHA 為 `8a04abad926e5b3b083e795492ed9315f45797cfcf870bb8231ea8d5498431a4`。Debug profile 仍使用 production Next.js export 與 Tauri `custom-protocol`，檢查只執行無視窗 proof，沒有開啟真正 Tauri 視窗或遊戲。 |

來源已保存至現有本機 bare origins：Kaleidoscope `b05b56df3462efb879170f1bca0cf4782d05f08a`、Transfiguration `f02f8ebfe171e964336249b4bc7af7772cd3ec7c`，沒有外部 Git 發布。以上自動檢查不能替代原生登入、送分、Replay、聊天、多人、重連或退出清理的完整真人驗證。後端／網站更新、一般 dev Caddy 重載與 migration 23／24 仍 HOLD；指定 client-test Caddy 的登入路由修正為下節記錄的單次例外，沒有更新 application services 或 migration-22 資料庫。

## lazer 登入路由 hotfix

新版 `141323` GUI 的玩家測試在 14:37:41 UTC 收到 `POST /oauth/token` 200，隨後四次 `GET /api/v2/me/` 均為空的 404。這個尾端斜線是 pinned stock `GetMeRequest` 在未指定 ruleset 時產生的正常路徑；`GetUserRequest` 同樣會產生 `/api/v2/users/<lookup>/`。保留 gateway 當時只有對應的無尾端斜線路由。[安全診斷](../.artifacts/builds/integration-20261009/lazer-login-route-diagnostic-20261009T144907Z-e3705c4032e945a28ecc2a5d9049d07e.json)僅記錄固定路徑、UTC、method、host、status、bytes 與映像身分，沒有帳號、token、body、header 或 URL query。原始 [GetMeRequest](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/API/Requests/GetMeRequest.cs)／[GetUserRequest](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/API/Requests/GetUserRequest.cs) 及[協定契約](protocol-compatibility.md#nullable-ruleset-lazer-profile-paths)記錄這項路徑假設。

三份 Caddy 來源設定僅對 `GET`／`HEAD` 且符合 `^/api/v2/(me|users/[^/]+)/$` 的請求移除最後一個斜線，再交由既有 authenticated handler；沒有 redirect，Authorization 與 query 保留。Explicit ruleset、成績路徑、重複尾端斜線及寫入 method 不受這條規則影響。隔離 fixture 通過三份 config adaptation、696 HTTP checks，以及使用 internal TLS、保持相同 container／StartedAt／mounts 和 `admin off` 的 `SIGUSR1` 重載；自有容器與 network 清理完成。這些是 proxy fixtures，沒有啟動真人遊戲。[fixture 收據](../.artifacts/tests/lazer-path-compat-20261009T150051Z-f1ffa6ef91e14685b1ce5ebd6b00fbd3/verification.json) SHA-256 為 `953b0c46cb38a854eae021f9913dd5b770780631251f48d82473b55c97fcab41`。實作依據為 [Caddy URI 操作](https://caddyserver.com/docs/caddyfile/directives/uri)與[signal 重載契約](https://caddyserver.com/docs/command-line#signals)。

2026-10-09 **15:04:16.1312462 UTC** 僅向原有 `illyaverse-client-test` Caddy `d065387aa23f7fb8447d3effe239b7ff0d242a3a45f425a6305777face91aef4` 發送 `SIGUSR1`。所選 config SHA-256 為 `06c3c30e7e93e98cabc584f9df6a2c35f0b9ee815257e9473332cfc5f0beffd6`；all-ten-container 保留檢查通過，IDs、images、StartedAt、mounts、health 與 restart state 維持原樣，兩份資料庫重載前後均為 22 migrations。沒有重建後端、網站或 Caddy，沒有套用 migration 23／24；一般 dev Caddy 仍使用先前的 active config。

重載後的實際 HTTPS unauthenticated probes 確認 aliases 與 canonical paths 均取得既有 401、65-byte 錯誤回覆，沒有 `Location`，採預設 .NET hostname／chain 驗證，沒有自訂 TLS callback 或信任庫變更。[本次重載收據](../.artifacts/builds/integration-20261009/lazer-route-hotfix-20261009T145619Z-ab56370b579e45d385e2f587e12ae597/verification.json) SHA-256 為 `8080ae4511d0f490c29aa2103c401760aa1f10b53c102aa42ee78b83d395dd9e`。這份自動收據證明請求已到達認證 handler；玩家之後在本次對話回報使用同一份 GUI 成功登入。人員回報沒有被轉寫成 agent 的遊戲觀察，完整 profile DTO、聊天、遊玩、Score／Replay、多人、重連與退出清理仍需各自驗證。未重新封裝 GUI；後續 gateway direct aliases 未隨此次 proxy 修正部署。

## 後續聊天與管理來源

後續來源加入期限禁言和具版本檢查的成績審核。`chat.moderate` 可獨立授權管理員目錄與禁言操作，不會因此取得 email 或帳號限制權限；對自己與有同等管理權限的目標有保護。每次操作重新檢查有效權限、期限及 `expected_revision`，將前後狀態和私人原因寫入稽核。成績狀態沒有改變時回覆明確錯誤，不會留下影響原生自動公開政策的空操作。正常原生成績的自動公開政策維持原契約。

網站、stable、lazer 只阻擋新的聊天發送；既有可讀歷史、接收訊息、已接受的 UUID 重試及遊戲 session 繼續使用。stable 回報實際剩餘禁言秒數；lazer 使用期限錯誤回覆，不以 silence arrays 清除玩家聊天歷史。網站編輯器保留草稿，確認權限失敗時停用發送，期限到達後重新確認權限。排名和圖譜成績列表也改成保留 URL 的分區／模式／國家／頁碼，支援上一頁、下一頁和瀏覽器歷史。

以下檢查各自對應來源與 synthetic fixtures，沒有部署到保留服務或操作真人遊戲：

- Sanctuary：90 ordinary tests、domain 32 tests、9 個新增 PostgreSQL moderation tests，以及 4 個既有 RBAC／聊天／mutation／成績與 Replay 回歸通過；Clippy 與格式通過。[確認收據](../.artifacts/tests/sanctuary-moderation-20261009T145152Z-f51b158f346544efa51d66a7174bd432/verification-confirmed.json)。
- Resonance：64 ordinary tests、6 個期限禁言 PostgreSQL／TCP／WebSocket tests、1 個 authenticated nullable-ruleset alias test，和 7 個既有 native chat tests 通過；Clippy 與格式通過。[確認收據](../.artifacts/tests/native-chat-silence-20261009T150401Z-075b846d8d9443b493cde418b5604f4a/verification.json)。
- Migration 24：6 個來源／Git index／LF checkout／archive tests 通過，前 23 份 migration bytes 保持原樣。[確認收據](../.artifacts/tests/migration-24-source-20261009T150755669Z-06bd6a3a869149aa885e7ecda8cb02ec/verification.json)。
- 排名導覽：14 個瀏覽器案例、scoped lint／format、桌面與手機 containment 確認通過。當時全域 typecheck 因 4 個同時修改的成績 fixtures 缺少版本欄位而失敗；fixtures 已補齊，之後的聊天 cohort 全域 typecheck 通過，原始失敗證據保留。[導覽收據](../.artifacts/tests/ranking-navigation-20261009T223814-cbabedf8833b4db6a1faebe906411e16/verification.json)。
- 網站聊天：13 個新增案例及 17 個既有案例，共 30 passed；lint、全域 typecheck、format 與兩輪桌面／手機確認通過。首輪 26／30 的失敗 log 保留：3 個測試使用錯誤的重試按鈕名稱，另 1 個 reconnect case 受到同時修改 providers 的 Fast Refresh full reload 影響；凍結所有網站來源後完整重跑通過。[確認收據](../.artifacts/tests/chat-silence-20261009T150521Z-118362f40feb1/verification-confirmed.json)。

管理介面先通過 16 個瀏覽器案例與兩輪桌面／手機確認；後續權限競態與 staff 入口修正另外通過 9 個針對性案例。兩組是不同來源時點的證據，不合併宣稱全部案例在最終來源重跑。最後 9 個案例後只有測試檔的 Prettier 排版，application source 保持原樣。

正式 Next.js source build 通過，590 份 application 檔案的雜湊已固定；974 個翻譯鍵在三個語系一致且格式通過。[source build 收據](../.artifacts/builds/moderation-web-20261009T151922Z-8ade655014e74a48ba50dce0b499eb08/verification-confirmed.json)不是新的 Docker image 或保留部署證據。成績審核的 protocol runner 已改成使用實際版本與明確的狀態轉換，2 份 Python syntax 和 8 個離線案例通過；[這份 runner 收據](../.artifacts/tests/protocol-score-cas-20261009T152018Z-c23e65d1f1de416e9c513687f1050cf3/verification.json)本身未執行新映像的 live probes，後續實跑另記於下一節。

Migration 24 與對應的 API／native source 必須一起部署。上述來源檢查之後，固定映像與 migration-24 recovery gate 已完成，範圍見下一節；目前保留資料庫仍為 migration 22，來源與隔離結果不能當作現有網站或真人聊天功能已更新的證據。

## 固定來源的 migration-24 整合檢查

本節只記錄 parent `012ef18e55de60759994dae018ebca6973b735dc` 與其五個 child commits 的已建成映像。測試 helper 來源為 parent `3882ca3021888b75c5a98faa8ae8d1a88d524d44`；它們不在這些 Dockerfile 的 COPY inputs 內。之後進行中的官方搜尋、玩家搜尋／管理、hover、BN 圖譜排名提案與 migration 25 都不屬於本節的 runtime 驗證範圍。

| 元件 | 固定映像與個別建置收據 |
| --- | --- |
| Sanctuary | `sha256:e25936877d07df06cdab818cfb1d1f9411de642c893ff0d751788ce843e5430d`；[收據](../.artifacts/builds/integration-images-20261009T153322Z-6ea175d46a6a4566ae45e77b55e95751/sanctuary.verification.json)。 |
| Resonance | `sha256:ccd36d0cf9a4869b4b46be97ca707400d8bd6621940e8998787ca7192050436f`；[收據](../.artifacts/builds/integration-images-20261009T153322Z-6ea175d46a6a4566ae45e77b55e95751/resonance.verification.json)。 |
| Serendipity | `sha256:2f2ecc5878b92e363b7c3ffbc2df68f5ccd6e14e2dc43b783be5b22ecaff9bbc`；[收據](../.artifacts/builds/integration-images-20261009T153322Z-6ea175d46a6a4566ae45e77b55e95751/serendipity.verification.json)。 |

[整組建置收據](../.artifacts/builds/integration-images-20261009T153322Z-6ea175d46a6a4566ae45e77b55e95751/verification.json) SHA-256 為 `b5cf63fe8010bb12deb369290097504545e08e3269a5a649b7a2849291cfb080`。三次建置均 exit 0，使用既有 locked BuildKit caches 與獨立 nonce tags；934 個實際輸入檔案、五個 child commits、24 份 migration checksums 前後一致，既有 `:local` aliases 保持原樣。18 個既有 Git-ignored 產物仍在這次實際 context 內，其個別 hashes 已記錄；沒有宣稱這次 context 與乾淨 clone 相同，也沒有刪除 cache 或修改 `.dockerignore`。

Disposable helper 的[最終離線收據](../.artifacts/tests/disposable-image-inputs-20261009T154323Z-67b93141073c4c6286b8f8f1fd4d492f/verification.json) SHA-256 為 `612d1e714f0c90fbd9325299a37284d9f2233a8844d1df5e784f257c2bcf228a`：50 passed、0 failed（24 個新 helper cases、26 個既有 guards）。檢查 immutable image override 的嚴格驗證、inspect 身分不符拒絕、沒有 fallback／retag／pull、rendered configuration 與成功／失敗後的 temp cleanup。這些使用 injected command／browser runners，不是真實 Docker 或瀏覽器測試。

固定 Sanctuary image 的[實際 backup／restore 收據](../.artifacts/tests/recovery-moderation-live-20261009T154257Z-d84f17f72f9549ddb2718ecfe6f1c5cd/verification.json) SHA-256 為 `7430f7c98360d6d10455d138f4cefa867757b57c7379ab50d6ad212506e72a8c`。自有可丟棄部署使用正式 guarded backup helper，還原後的 24 exact migration checksums、40 tables／45 synthetic rows、18 sequences、normalized schema，以及三個檔案的 bytes／ownership 全部相符。另確認 active silence、native silence cursor、私人 audit、user／score revisions、no-op／ABA trigger 行為和 rollback 保留，還原 readiness 通過。兩個自有 projects 的 containers／volumes／networks 均為零，temporary directory 已移除；沒有使用保留資料庫或玩家資料。

固定兩個 backend images 的[完整隔離協定最終收據](../.artifacts/tests/protocol-full-callback-20261009T1556Z-97b3ea41761b4cb484003908ee721fff/final-verification.json) SHA-256 為 `059a892023626ac250cb3ef28007a74ef4eed1a3bbfb5247d1668e66f5ecac66`。協定 subprocess 通過 591 HTTP requests／68 check categories；category 數不是 assertion 數。涵蓋 stable／lazer synthetic auth、一般／RX 成績、server PP、Replay、聊天／多人、圖譜 package／archive、withdrawal 與 score-review no-op／stale／ABA 契約。實際自有 PostgreSQL 的 migration 1–24 均成功且 checksums 相符。此 run 的 Python runner SHA-256 為 `7f0a93c85b9cb361155ee1740728e0e3ff066067d48e71cfc55f129245b9741f`；後續 application source 不在這次驗證範圍。

首次[協定失敗收據](../.artifacts/tests/protocol-full-baseline-20261009T154940Z-acf945707fc748dc87aa18272a8cc94d/verification.json)保留：fixture 對 solo completion 誤用 `no_event("UserScoreProcessed")`。僅修正 callback identity 與 exact-retry 不重複 callback 的期待後，對相同映像完整重跑，沒有藉此修改 production images。第二次[原始 wrapper 收據](../.artifacts/tests/protocol-full-callback-20261009T1556Z-97b3ea41761b4cb484003908ee721fff/verification.json)仍保留 failed，因 Docker cleanup 後 Windows loopback bind 短暫回報 `EADDRINUSE`；協定 subprocess 本身已通過。[首次清理復查](../.artifacts/tests/protocol-full-baseline-20261009T154940Z-acf945707fc748dc87aa18272a8cc94d/cleanup-verification.json)確認該次完整清理；[第二次清理復查](../.artifacts/tests/protocol-full-callback-20261009T1556Z-97b3ea41761b4cb484003908ee721fff/cleanup-verification.json)仍記錄 loopback port 尚不可 bind，`complete` 為 false。[最終清理收據](../.artifacts/tests/protocol-full-callback-20261009T1556Z-97b3ea41761b4cb484003908ee721fff/cleanup-final-verification.json)才確認 resources 為零、ports 可 bind、temporary environment／directory 已移除。原始失敗與中間清理紀錄沒有覆寫。

保留 runtime 的兩套資料庫仍為 migration 22，以上三個 application images 尚未部署。玩家另已授權將 client-test 的測試帳號 id `2` 從 `player` 加入 `administrator`；2026-10-09 15:53:32 UTC 的[私人操作收據](../.artifacts/tests/illya-admin-grant-20261009T155331Z-5dad11a0762d48b397e5d3b256fb01e0/verification.json) SHA-256 為 `aa11efdb50faba83eaaecf19f72b29c76d0a37e137686fb2b7445d4909ac80f7`。本文件只摘要公開帳號 ID 與 role names；沒有公開 SQL／grant dump、email、session 或密鑰，也沒有重新部署服務或套用 migration。真人登入成功的依據仍是玩家在本次對話使用相同 GUI 的回報，這些隔離檢查沒有啟動遊戲或檢查玩家檔案。

## 11:14 UTC 部署狀態（歷史）

2026-10-09 19:14:49（UTC+08:00；11:14:49 UTC）的 guarded deployment 已通過。兩套部署各重建 Sanctuary、Resonance、Serendipity，共六個 application services，十個容器均 running／healthy。部署使用前述兩個 backend 映像，以及通過最後 10 項 account-session 確認的 Web `sha256:36f52e19775bfc4825dcde1a82d5aa145cbff97943188cdd651ec983fe4114aa`。原資料與 stored bytes、schema 22、環境值、排名觀察，以及四個 PostgreSQL／Caddy 的容器身分、image、時間、health 和 mounts 的保留 guard 全部通過；沒有以健康狀態推論原生遊戲成功。

[最終 deployment manifest](../data-current-integration-20261009/integration-deploy-20261009T110622Z-6dcebea122f949e6942b2915b00c5c9b/deployment-manifest.json)的 SHA-256 為 `5dc274e2fcb7c2000dfcaf351627fb9e3143efe590d0057e869b55e7ea3ce73e`。最終 images、六個服務與十個容器、保留旗標和完成時間均來自此 manifest。

另以 .NET 10.0.12 預設 `SocketsHttpHandler` 對 `https://illya.test/` 做唯讀 HTTPS 檢查，沒有自訂憑證 callback，關閉自動 redirect，取得 200。Windows curl 的預設 revocation check 起初因本機 CA 缺少可用 CRL 回報 `CRYPT_E_NO_REVOCATION_CHECK`；僅在該次 curl 指令使用 `--ssl-revoke-best-effort` 後取得 200，hostname 與 chain validation 仍保留，沒有更改全域設定。這些 probe 未啟動遊戲，不能當作真人 lazer 登入證據。範圍與預設 .NET revocation mode 記在 [本機 HTTPS 證據](../.artifacts/builds/integration-20261009/local-https-check.json)。
