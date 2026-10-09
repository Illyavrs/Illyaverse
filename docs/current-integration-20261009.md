# 2026-10-09 整合 checkpoint

先前 guarded deployment 與兩次 lazer 啟動失敗的修正證據已保留。最新來源改為沿用原本 `game.ini` 遊戲偏好，只分離私服帳號，並在 GUI 設定保存每次啟動使用的 HTTPS 網址；完整 managed／Rust／合成封裝檢查已通過，新的 Debug GUI 包已建成並通過內嵌介面與目錄／ZIP 完整性驗證。真人 GUI、lazer 登入／遊玩與原始社群目標仍未完成，保留部署仍停留在 migration 22。下列結果各有來源與映像邊界，不能合併宣稱真人 lazer 或完整專案驗證。先前 checkpoint 保留於 [release-readiness.md](release-readiness.md)。

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

來源已保存至現有本機 bare origins：Kaleidoscope `b05b56df3462efb879170f1bca0cf4782d05f08a`、Transfiguration `f02f8ebfe171e964336249b4bc7af7772cd3ec7c`，沒有外部 Git 發布。以上檢查不證明原生視窗、登入、送分、Replay、聊天、多人、重連或退出清理已通過。保留服務仍 HOLD，migration 23 僅有來源與可丟棄演練證據，尚未套用至原有 migration 22 部署。

## 部署狀態

2026-10-09 19:14:49（UTC+08:00；11:14:49 UTC）的 guarded deployment 已通過。兩套部署各重建 Sanctuary、Resonance、Serendipity，共六個 application services，十個容器均 running／healthy。部署使用前述兩個 backend 映像，以及通過最後 10 項 account-session 確認的 Web `sha256:36f52e19775bfc4825dcde1a82d5aa145cbff97943188cdd651ec983fe4114aa`。原資料與 stored bytes、schema 22、環境值、排名觀察，以及四個 PostgreSQL／Caddy 的容器身分、image、時間、health 和 mounts 的保留 guard 全部通過；沒有以健康狀態推論原生遊戲成功。

[最終 deployment manifest](../data-current-integration-20261009/integration-deploy-20261009T110622Z-6dcebea122f949e6942b2915b00c5c9b/deployment-manifest.json)的 SHA-256 為 `5dc274e2fcb7c2000dfcaf351627fb9e3143efe590d0057e869b55e7ea3ce73e`。最終 images、六個服務與十個容器、保留旗標和完成時間均來自此 manifest。

另以 .NET 10.0.12 預設 `SocketsHttpHandler` 對 `https://illya.test/` 做唯讀 HTTPS 檢查，沒有自訂憑證 callback，關閉自動 redirect，取得 200。Windows curl 的預設 revocation check 起初因本機 CA 缺少可用 CRL 回報 `CRYPT_E_NO_REVOCATION_CHECK`；僅在該次 curl 指令使用 `--ssl-revoke-best-effort` 後取得 200，hostname 與 chain validation 仍保留，沒有更改全域設定。這些 probe 未啟動遊戲，不能當作真人 lazer 登入證據。範圍與預設 .NET revocation mode 記在 [本機 HTTPS 證據](../.artifacts/builds/integration-20261009/local-https-check.json)。
