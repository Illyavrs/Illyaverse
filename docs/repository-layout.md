# Illyaverse 資料夾導覽

`osu-ps/Illyaverse` 是整合 parent 的根目錄。Serendipity、Sanctuary、Resonance、Kaleidoscope 與 Transfiguration 五個主要子專案各有 repository 邊界，parent 以 Git submodules 固定組合版本。五個子模組已登記，相關歷史保留；目前 remote 是本機 bare repositories，實際狀態以 `.gitmodules` 與 `git submodule status` 為準。根 Compose 組合部署網站、兩個 Rust 服務、PostgreSQL 與 Caddy；Windows 啟動器建成玩家電腦上的程式，不作為 Docker 服務。

## 子專案與共用來源

```text
Illyaverse/
├── package.json + package-lock.json  npm workspace 與共用依賴鎖定
├── Cargo.toml + Cargo.lock           Rust workspace 與共用依賴鎖定
├── compose*.yaml                    開發、本機客戶端測試與正式部署
├── apps/
│   └── serendipity/                  Serendipity 子 repository
│       ├── src/app/                  網站路由與共用樣式
│       ├── src/features/             個人頁、圖譜、聊天、設定等畫面
│       ├── src/components/ui/        shadcn/ui 元件
│       ├── messages/                 next-intl 翻譯
│       ├── public/                   發布的靜態資產與授權字體
│       └── tests/                    Playwright 網站測試
├── services/
│   ├── sanctuary/src/               Sanctuary 子 repository：社群 API
│   └── resonance/src/               Resonance 子 repository：客戶端協定
├── crates/
│   ├── illya-domain/src/            共用型別、驗證、分數與 mods 規則
│   └── resonance-protocol/src/      stable 封包 parser／encoder
├── tools/
│   ├── kaleidoscope/                Kaleidoscope 子 repository
│   │   ├── src/                     Rust 啟動、程序監護、復原與 CLI
│   │   ├── scripts/                 打包及 package 驗證
│   │   ├── tests/                   隔離的程序生命週期與 package fixtures
│   │   └── ui/
│   │       ├── src/                 同一產品的 Next.js 介面
│   │       ├── src-tauri/           Windows Tauri 外殼與獨立 Cargo manifest
│   │       └── tests/               啟動器介面測試
│   ├── transfiguration/             Transfiguration 子 repository：C# 橋接
│   └── archive-compatibility/        原生 ZIP 讀取器的隔離 proof 工具
├── migrations/                      PostgreSQL schema 的唯一版本來源
├── infra/                           Caddy、部署、備份與 smoke tests
├── docs/                            使用、開發、營運與驗證文件
├── PRODUCT.md                       產品需求與範圍
└── DESIGN.md                        網站的設計規則與材料參數
```

Kaleidoscope 的介面與核心集中在同一個產品 repository。`tools/kaleidoscope/ui` 處理玩家看到的視窗，`tools/kaleidoscope/src` 提供可在隔離 fixture 中測試的 Rust 核心。Tauri 用 `kaleidoscope = { path = "../.." }` 連接核心，沒有第二份啟動與清理邏輯。Transfiguration 由它在 lazer 啟動前載入，負責本次私服 HTTPS 網址、帳號隔離與相容性檢查；正式 bridge 不包含官方客戶端或其資源。

共用 `crates/`、`migrations/`、根 npm／Cargo lockfile、Compose、部署與整合文件由 Illyaverse parent 管理。子專案有各自的 Git 歷史與版本，仍透過完整 parent checkout 取得共同建置環境；單獨 clone 一個子專案不包含其他服務、共用 crates／schema 或 parent lockfile。

| 子 repository | Parent checkout 路徑 | 產品邊界 |
| --- | --- | --- |
| Serendipity | `apps/serendipity` | 網站來源、翻譯、靜態資產與網站 tests |
| Sanctuary | `services/sanctuary` | 社群 API 與其資料庫 tests |
| Resonance | `services/resonance` | 原生協定、即時接口與其 tests |
| Kaleidoscope | `tools/kaleidoscope` | Rust 核心、CLI、打包 scripts、`ui/` 與 Tauri |
| Transfiguration | `tools/transfiguration` | 原創 C# 橋接、相容性合約與 proofs |

## Workspace 邊界

| 建置系統 | 包含範圍 | 入口與 lockfile |
| --- | --- | --- |
| npm workspaces | Serendipity、Kaleidoscope 介面 | 根 `package.json`、`package-lock.json` |
| Cargo workspace | Sanctuary、Resonance、Illya Domain、Resonance Protocol、Kaleidoscope 核心 | 根 `Cargo.toml`、`Cargo.lock` |
| Tauri 原生外殼 | Kaleidoscope 的 Windows 桌面程式；本地 dependency 連到共用核心 | `tools/kaleidoscope/ui/src-tauri/Cargo.toml` 與同目錄 `Cargo.lock` |
| .NET | Transfiguration 及其 proofs；另有 archive compatibility proof | 各工具自己的 `.csproj`、固定版本設定與建置腳本 |

Tauri manifest 的 `[workspace]` 讓桌面原生依賴有獨立的建置邊界。根 `cargo test --workspace` 涵蓋共用核心，不會順便建置或啟動 Tauri 視窗；橋接也需要自己的 .NET 驗證。各語言的建置工具不同，由 parent 的 workspace、lockfile 與固定子專案 commit 協調整合。

## Git 協作

目前五個 URL 指向這台電腦 `.git/local-remotes/` 內的 bare repositories，其他電腦無法使用這些絕對路徑。從這台電腦的 parent checkout 初始化時，僅對這次指令允許本機 file transport：

```sh
git -c protocol.file.allow=always submodule update --init --recursive
```

之後改成可存取的 HTTPS／SSH 遠端時，使用 `git submodule update --init --recursive`，再安裝／建置依賴。子模組固定 parent 記錄的 commit；一般的遞迴 checkout 會以 detached HEAD 載入該版本（[Git submodule 文件](https://git-scm.com/docs/git-submodule)），開發前先切換到明確的分支。本機目前五個來源 checkout 都在 `main`。

功能變更先在所屬子 repository 提交，並讓目前的 origin 保存該 commit，再由 parent 固定新的 gitlink。本機 `origin` 是自己的 bare repository，子專案 commit 不會自動同步進去。以 Serendipity 為例，完成來源提交與驗證後：

```sh
git -C apps/serendipity push origin main
git add apps/serendipity
git commit -m "chore(illyaverse): update Serendipity revision"
```

子專案自己的來源提交同樣使用 scoped Conventional Commits，例如 `feat(serendipity): add profile editor`。若 origin 已改為外部遠端，推送／發布需由使用者授權後執行。跨子專案變更要把互相配合的 commit 一起固定，並從完整 parent checkout 執行整合驗證。Parent commit 只保存子模組版本指標，不會保存子模組內尚未提交的檔案。

### 設定實際遠端

使用者提供五個子倉庫的實際 URL 後，才將本機 URL 替換為那些遠端。下列 `<... URL>` 都是需要替換的佔位文字，不是已建立的 repository；這次沒有執行任何遠端設定或外部推送。先保留本機來源／bare 備份，並在使用者授權發布後，確認每個真正遠端已有 parent 所固定的 child commit，才發布更新後的 parent。

```sh
git submodule set-url apps/serendipity "<Serendipity URL>"
git submodule set-url services/sanctuary "<Sanctuary URL>"
git submodule set-url services/resonance "<Resonance URL>"
git submodule set-url tools/kaleidoscope "<Kaleidoscope URL>"
git submodule set-url tools/transfiguration "<Transfiguration URL>"
git -C apps/serendipity remote set-url origin "<Serendipity URL>"
git -C services/sanctuary remote set-url origin "<Sanctuary URL>"
git -C services/resonance remote set-url origin "<Resonance URL>"
git -C tools/kaleidoscope remote set-url origin "<Kaleidoscope URL>"
git -C tools/transfiguration remote set-url origin "<Transfiguration URL>"
git submodule sync --recursive
```

這些設定不會上傳來源。確認五個 origin 的固定 commit 可取得後，在 parent 提交 `.gitmodules`。其他 checkout 拉取 parent 設定變更後，先執行 `git submodule sync --recursive`，再執行 `git submodule update --init --recursive`。

### 備份 Git 來源

來源備份必須包含 Illyaverse parent 和全部五個子倉庫。只備份 parent 的 bundle 不會收集 gitlink 指向的其他 repository，也無法保存之後新增的 child commits。

暫停 Git 寫入後，可以保存完整 parent `.git/`，包括吸收後的 `.git/modules/` 子倉庫歷史與 `.git/local-remotes/` 本機 origins；或分別為 parent 和每個 child 建立並驗證 `git bundle create <備份檔> --all`。Child bundle 要從實際 child checkout 建立，才能包含尚未推到本機 origin 的分支。另行保存需要保留的未提交來源檔案；bundle 不包含工作樹變更（[Git bundle 文件](https://git-scm.com/docs/git-bundle)）。還原時也需恢復 URL／gitlink 的對應並驗證完整遞迴 checkout。營運資料庫、媒體、密鑰與憑證的備份仍依 [部署文件](deployment.md) 處理。

## 建置與測試入口

以下指令從 `Illyaverse` 根目錄執行。需要的環境變數、資料庫與測試範圍，以各工具文件為準；測試 fixture 與原生人工驗證分別記錄。

| 工作 | 入口 |
| --- | --- |
| 網站開發 | `npm run dev`，預設 Serendipity |
| 網站 lint／型別／建置 | `npm run lint`、`npm run typecheck`、`npm run build` |
| 啟動器介面開發 | `npm run dev --workspace @illyaverse/kaleidoscope` |
| 啟動器介面 lint／型別／建置 | `npm run lint --workspace @illyaverse/kaleidoscope`、`npm run typecheck --workspace @illyaverse/kaleidoscope`、`npm run build --workspace @illyaverse/kaleidoscope` |
| Rust workspace | `cargo test --workspace --locked`；資料庫案例另需指定隔離測試資料庫 |
| Migration 來源一致性 | `npm run test:migrations` |
| Docker 網站／協定隔離測試 | [瀏覽器測試](browser-testing.md) 與 `infra/disposable-web-smoke.mjs`、`infra/disposable-protocol-smoke.mjs` |
| 原創 bridge 相容性 proof | `tools/transfiguration/test.ps1`，範圍見 [工具說明](../tools/transfiguration/README.md) |
| 啟動器完整本機打包 | `tools/kaleidoscope/scripts/package-launcher.ps1`，見 [Kaleidoscope 指南](kaleidoscope-launcher.md) |

Sanctuary／Resonance 的 Rust 原始碼與 SQLx 查詢不要求建置時連到營運資料庫。Docker build 使用 repository 根目錄作為 context，以取得共用 crates 與 migrations；容器執行與路由關係見 [架構](architecture.md)。

## 原始碼、生成檔與持久化資料

| 路徑 | 性質與用途 |
| --- | --- |
| 五個子 repository、parent 的 `crates/`、`infra/`、`migrations/` | 各自版本管理的應用程式、共用來源與部署來源 |
| `apps/serendipity/public/fonts/` | 網站實際發布的授權字體、notice 與 provenance；即使由 generator 建立，仍是需保留的產品資產，見 [字體文件](fonts.md) |
| `docs/`、`PRODUCT.md`、`DESIGN.md`、`.impeccable/` | 文件、設計合約與驗證描述；較早 checkpoint 的報告保留其原始範圍 |
| `node_modules/`、`target/`、`.next/`、`.swc-cache/` | 可重建的依賴及編譯快取，忽略版本控制；不由新的任務複製多份大型 target |
| `tools/kaleidoscope/ui/out/`、工具的 `bin/`、`obj/`、`artifacts/`、`proof-output/` | 介面輸出、.NET 編譯與橋接 proof 產物；其是否可用由 verifier 判斷 |
| 根目錄 `data-*/`、`*.log`、生成的 `data-*.verification.json`、啟動器 `.zip` | 本機測試 fixture、來源下載、package 與部署證據；不列為子專案，也不進入 Git |
| `.env`、`.env.*.local` | 本機密鑰與部署設定，忽略版本控制；只提交 `.env.example` |
| Compose 的 PostgreSQL、shared media、Caddy volumes | 持久化帳號、圖譜、成績、Replay 與憑證；資料不存進 Git，備份與保留規則見 [部署文件](deployment.md) |

「忽略版本控制」不表示可以整批刪除。現有 `data-client-test` 保存本機 CA／連線狀態，部分 `data-*` 與 `.impeccable/review` 是已記錄 hash 的部署或測試證據；active runner 也可能仍在使用它們。清理先核對確切的產生者、完整路徑、任務身分與引用，再處理該次自有產物。不要對所有 `data-*`、Docker volumes 或玩家圖譜庫套用通配清理。

較舊腳本把產物直接放在根目錄，例如過去 migration-source tests 留下的 `data-migration-source-*` fixture；Kaleidoscope 打包預設仍建立新的 `data-kaleidoscope-package-*` 目錄與 ZIP。新的隔離產物可集中於忽略的 `.artifacts/` 並配合 run ownership manifest。現有已引用的證據路徑需保留，不能只為整理畫面搬移。

這次 Kaleidoscope 布局只搬移 23 個 Git 追蹤來源檔；原 `apps/kaleidoscope` 中的 `.next`、`out`、快取與未追蹤產物留在原位並繼續忽略。它不是目前來源或子 repository 的位置，也不會放入 Docker build context。

## 文件怎麼讀

| 對象 | 先讀 | 遇到特定問題再讀 |
| --- | --- | --- |
| 玩家 | [連線](connection.md)、[啟動器](kaleidoscope-launcher.md) | [本機測試](local-client-testing.md)、[RX](relax.md) |
| 營運者 | [部署](deployment.md)、[權限](permissions.md) | [帳號復原／SMTP](account-recovery.md)、[官方圖譜包](official-beatmap-archives.md)、[完成度](release-readiness.md) |
| 開發者 | 本文、[架構](architecture.md)、[API](api.md)、[產品](../PRODUCT.md)、[設計](../DESIGN.md) | 對應功能文件、[瀏覽器測試](browser-testing.md)、[協定相容性](protocol-compatibility.md) |
| 檢查驗證結果 | [完成度](release-readiness.md)、[驗證紀錄](verification.md) | 各功能的 review／evidence；它們記錄指定 source／image／fixture，不能當成其他版本或真人遊玩的通過證明 |
