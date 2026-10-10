---
title: "Illyaverse 資料夾導覽"
---

`osu-ps/Illyaverse` is the integration parent. Serendipity, Sanctuary, Resonance, Kaleidoscope and Transfiguration have separate repository histories, and the parent pins their commits through submodules. The parent origin is `Illyavrs/Illyaverse`; all five `.gitmodules` URLs and child origins use canonical HTTPS GitHub URLs under `Illyavrs`. Exact previous child bare origins remain as `local-cache`, and the parent's previous origin remains as `previous-origin`. Root Compose deploys the website, two Rust services, PostgreSQL and Caddy; the Windows launcher runs on the player's computer.

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

The configured submodule URLs use HTTPS. Once every pinned child commit is available on its GitHub origin, initialize a checkout with:

```sh
git submodule update --init --recursive
```

A normal recursive checkout loads the parent's pinned commits in detached HEAD; switch to an explicit branch before development. The preserved `local-cache` remotes retain the exact previous local bare repositories.

Commit a feature in its owning child repository, preserve that commit in the local bare cache, then update the parent gitlink. `origin` identifies GitHub; `local-cache` identifies the preserved bare repository. For a local checkpoint:

```sh
git -C apps/serendipity push local-cache main
git add apps/serendipity
git commit -m "chore(illyaverse): update Serendipity revision"
```

Child source commits use scoped Conventional Commits, such as `feat(serendipity): add profile editor`. GitHub source pushes and release publication require user authorization. Pin cooperating commits together for changes across projects and verify them from the complete parent checkout. A parent commit saves gitlinks, not uncommitted child source.

### Configured repository URLs

The parent uses `https://github.com/Illyavrs/Illyaverse.git`. The five child origins and `.gitmodules` entries use the corresponding HTTPS URLs for `Serendipity`, `Sanctuary`, `Resonance`, `Kaleidoscope` and `Transfiguration` under `Illyavrs`.

This checkpoint configures URLs and preserves previous remotes. It does not push source to GitHub, publish releases or configure signing keys. Availability of the pinned commits on GitHub must be established before claiming a fresh recursive clone works.

After receiving a parent URL change, run `git submodule sync --recursive` before updating submodules.

### 備份 Git 來源

來源備份必須包含 Illyaverse parent 和全部五個子倉庫。只備份 parent 的 bundle 不會收集 gitlink 指向的其他 repository，也無法保存之後新增的 child commits。

Pause Git writes before backing up the complete parent `.git/`, including absorbed child histories in `.git/modules/` and preserved local bare caches in `.git/local-remotes/`. Alternatively, create and verify `git bundle create <backup file> --all` separately for the parent and every child. Create child bundles from the actual child checkouts so they include branches not yet preserved in `local-cache`. Save required uncommitted files separately; bundles omit working-tree changes ([Git bundle documentation](https://git-scm.com/docs/git-bundle)). Restore URL/gitlink mappings and verify a complete recursive checkout. Follow the [deployment guide](/docs/deploy/deployment) for databases, media, secrets and certificates.

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
| Docker 網站／協定隔離測試 | 瀏覽器測試 與 `infra/disposable-web-smoke.mjs`、`infra/disposable-protocol-smoke.mjs` |
| 原創 bridge 相容性 proof | `tools/transfiguration/test.ps1`，範圍見 [工具說明](../tools/transfiguration/README.md) |
| 啟動器完整本機打包 | `tools/kaleidoscope/scripts/package-launcher.ps1`，見 [Kaleidoscope 指南](/docs/clients/kaleidoscope-launcher) |

Sanctuary／Resonance 的 Rust 原始碼與 SQLx 查詢不要求建置時連到營運資料庫。Docker build 使用 repository 根目錄作為 context，以取得共用 crates 與 migrations；容器執行與路由關係見 [架構](/docs/deploy/architecture)。

## 原始碼、生成檔與持久化資料

| 路徑 | 性質與用途 |
| --- | --- |
| 五個子 repository、parent 的 `crates/`、`infra/`、`migrations/` | 各自版本管理的應用程式、共用來源與部署來源 |
| `apps/serendipity/public/fonts/` | 網站實際發布的授權字體、notice 與 provenance；即使由 generator 建立，仍是需保留的產品資產，見 字體文件 |
| `docs/`、`PRODUCT.md`、`DESIGN.md`、`.impeccable/` | 文件、設計合約與驗證描述；較早 checkpoint 的報告保留其原始範圍 |
| `node_modules/`、`target/`、`.next/`、`.swc-cache/` | 可重建的依賴及編譯快取，忽略版本控制；不由新的任務複製多份大型 target |
| `tools/kaleidoscope/ui/out/`、工具的 `bin/`、`obj/`、`artifacts/`、`proof-output/` | 介面輸出、.NET 編譯與橋接 proof 產物；其是否可用由 verifier 判斷 |
| 根目錄 `data-*/`、`*.log`、生成的 `data-*.verification.json`、啟動器 `.zip` | 本機測試 fixture、來源下載、package 與部署證據；不列為子專案，也不進入 Git |
| `.env`、`.env.*.local` | 本機密鑰與部署設定，忽略版本控制；只提交 `.env.example` |
| Compose 的 PostgreSQL、shared media、Caddy volumes | 持久化帳號、圖譜、成績、Replay 與憑證；資料不存進 Git，備份與保留規則見 [部署文件](/docs/deploy/deployment) |

「忽略版本控制」不表示可以整批刪除。現有 `data-client-test` 保存本機 CA／連線狀態，部分 `data-*` 與 `.impeccable/review` 是已記錄 hash 的部署或測試證據；active runner 也可能仍在使用它們。清理先核對確切的產生者、完整路徑、任務身分與引用，再處理該次自有產物。不要對所有 `data-*`、Docker volumes 或玩家圖譜庫套用通配清理。

較舊腳本把產物直接放在根目錄，例如過去 migration-source tests 留下的 `data-migration-source-*` fixture；Kaleidoscope 打包預設仍建立新的 `data-kaleidoscope-package-*` 目錄與 ZIP。新的隔離產物可集中於忽略的 `.artifacts/` 並配合 run ownership manifest。現有已引用的證據路徑需保留，不能只為整理畫面搬移。

這次 Kaleidoscope 布局只搬移 23 個 Git 追蹤來源檔；原 `apps/kaleidoscope` 中的 `.next`、`out`、快取與未追蹤產物留在原位並繼續忽略。它不是目前來源或子 repository 的位置，也不會放入 Docker build context。

## 文件怎麼讀

| 對象 | 先讀 | 遇到特定問題再讀 |
| --- | --- | --- |
| 玩家 | [連線](/docs/clients/connection)、[啟動器](/docs/clients/kaleidoscope-launcher) | [本機測試](/docs/clients/local-client-testing)、RX |
| 營運者 | [部署](/docs/deploy/deployment)、[權限](/docs/operate/permissions) | [帳號復原／SMTP](/docs/operate/account-recovery)、[官方圖譜包](/docs/operate/official-beatmap-archives)、[完成度](/docs/reference/release-readiness) |
| 開發者 | 本文、[架構](/docs/deploy/architecture)、[API](/docs/reference/api)、[產品](../PRODUCT.md)、[設計](../DESIGN.md) | 對應功能文件、瀏覽器測試、[協定相容性](/docs/reference/protocol-compatibility) |
| 檢查驗證結果 | [完成度](/docs/reference/release-readiness)、[驗證紀錄](/docs/reference/verification) | 各功能的 review／evidence；它們記錄指定 source／image／fixture，不能當成其他版本或真人遊玩的通過證明 |
