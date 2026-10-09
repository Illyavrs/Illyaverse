# Illyaverse

受《魔法少女伊莉雅》啟發的 osu! 社群伺服器 monorepo。網站使用 Next.js、Tailwind CSS 與 shadcn/ui；服務使用 Rust，資料持久化到 PostgreSQL 與共用媒體 volume。所有實作皆為原創，參考專案僅用於架構與協定研究。

目前是可持續開發的初始版本：網站／API 與 stable／lazer 的協定接口各有自己的完成程度。**尚未宣稱官方 stable／lazer 客戶端已通過完整連線測試。** 詳見 [協定相容性](docs/protocol-compatibility.md) 與 [連線教學](docs/connection.md)。

## 專案結構

Illyaverse 採用整合 parent 與五個主要子專案 repository 的結構；子專案透過 Git submodules 固定版本，網站與啟動器前端仍由根 npm workspaces 管理，Rust 服務與共用核心仍由 Cargo workspace 管理。從 `osu-ps/Illyaverse` 的完整 checkout 開發或部署。此階段已整理來源與建置路徑，Git 子模組登記尚待完成，實際登記狀態以 `.gitmodules` 與 `git submodule status` 為準。

```text
Illyaverse/
├── apps/
│   └── serendipity/         Serendipity repository：玩家網站
├── services/
│   ├── sanctuary/          Sanctuary repository：Rust 社群 API
│   └── resonance/          Resonance repository：stable／lazer 協定服務
├── crates/
│   ├── illya-domain/       共用領域型別、驗證、分數與 mods 規則
│   └── resonance-protocol/ stable 封包解析與編碼
├── tools/
│   ├── kaleidoscope/       Kaleidoscope repository：Rust 核心、CLI、退出清理
│   │   └── ui/             同一啟動器的 Next.js 介面與 Tauri 外殼
│   ├── transfiguration/    Transfiguration repository：原創 C# lazer 橋接
│   └── archive-compatibility/ 原生 ZIP 讀取器的隔離驗證工具
├── infra/                  Docker、部署、備份與隔離測試腳本
├── migrations/             共用 PostgreSQL schema
└── docs/                   玩家、營運與開發文件
```

Kaleidoscope 的介面與核心集中在 `tools/kaleidoscope` 同一個產品 repository；Tauri 以本地 Rust dependency 使用核心。Transfiguration 是它在 lazer 啟動前載入的橋接，不是另一台伺服器。共用 crates、schema、Compose、lockfile 與整合文件留在 Illyaverse parent。完整 workspace 邊界、各入口與生成檔說明見 [資料夾導覽](docs/repository-layout.md)。

## 文件入口

| 你要做的事 | 從這裡開始 |
| --- | --- |
| 玩家註冊與連線 | [連線教學](docs/connection.md)、[Kaleidoscope 操作](docs/kaleidoscope-launcher.md) |
| 部署與營運伺服器 | [Docker／HTTPS／備份](docs/deployment.md)、[權限管理](docs/permissions.md)、[郵件與帳號復原](docs/account-recovery.md) |
| 開發與找程式碼 | [資料夾導覽](docs/repository-layout.md)、[架構與路由](docs/architecture.md)、[API](docs/api.md) |
| 確認完成度與測試範圍 | [完成度](docs/release-readiness.md)、[協定相容性](docs/protocol-compatibility.md)、[驗證紀錄](docs/verification.md) |

## Docker 啟動

需要 Docker Engine／Docker Desktop 與 Docker Compose 2.24.4 以上。從 `Illyaverse` 目錄執行：

Windows PowerShell：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File infra/setup.ps1
docker compose up -d --build --wait
```

Linux／macOS：

```sh
sh infra/setup.sh
docker compose up -d --build --wait
```

開啟 [http://localhost:3000](http://localhost:3000)。初始化腳本會建立忽略版本控制的 `.env`，每次安裝產生獨立密鑰；初始管理員的 username／password 也在該檔，請在本機讀取。成功登入後，可清空三項 `BOOTSTRAP_ADMIN_*` 變數並執行 `docker compose up -d --force-recreate sanctuary`，停止重複 bootstrap。

若要在這台 Windows 電腦測試原生客戶端，可使用獨立的 `illya.test` HTTPS 配置；它保留網站預覽，使用自己的資料庫與本機 CA。設定、hosts 預覽、憑證檢查及還原步驟見 [本機客戶端測試](docs/local-client-testing.md)。

預設只把網站的 `3000` 與協定測試的 `8081` 綁定到 `127.0.0.1`。Sanctuary／PostgreSQL 留在容器網路。正式網域、HTTPS、備份與開發流程見 [部署文件](docs/deployment.md)。首次 image build 會下載並編譯 Rust／Node dependencies，需要一些時間。

## 網站與資料

預設深色背景為純黑色，介面有毛玻璃、light／dark 切換與圓體字。使用者可上傳自己的背景／頭像／banner，調整模糊、dim、accent、圓角與 profile；網站不附 AI 生成插畫。帳號偏好透過 API 持久化，RBAC 由伺服器驗證，前端同步隱藏不可存取的區域。

官方譜面匯入需要自己的 `OSU_CLIENT_ID`／`OSU_CLIENT_SECRET`；未設定時，自訂譜面與其他功能仍可使用。匯入服務有節流與 `429` 處理。自訂譜面、score／replay 保存與查詢有獨立資料模型；客戶端提交格式與 replay frames 的驗證狀態以協定文件為準。

公開註冊支援郵件驗證與忘記密碼。營運者需設定自己的 SMTP；未設定時，網站會明確顯示郵件功能尚未開放。重設密碼會撤銷網站、stable、lazer 的登入憑證；一次性連結和加密寄送佇列見 [帳號復原](docs/account-recovery.md)。

帳號統一由網站註冊；stable／lazer 的遊戲內註冊入口會提示網站網址，遊戲只提供登入。

管理面板支援分頁帳號搜尋、有限權限的角色指派、自訂角色刪除、帳號限制與獨立的登入撤銷。內建角色與最後一位管理員受到保護；寫入會重新驗證目前的登入與權限。設定工作人員角色與 API 合約見 [權限管理](docs/permissions.md)。

Relax（RX）支援 osu!、taiko、catch，成績、pp、排名與個人統計各自與 Normal 分開；mania 不支援 RX。網站可切換模式與 Normal／RX，stable／lazer 協定也有對應分區。RX 保留原始分數與 replay，目前不提供可比較的標準化／Classic 總分。資格、歷史重算與原生驗證範圍見 [RX 模式](docs/relax.md)。

lazer 房間成績已有原創實作，包含凍結 round／參與者、綁定 token、提交復原、房間結果／排行榜及處理通知。目前僅能建立 HeadToHead 房間；playlist 聚合規則的 fixture 不代表支援建立 playlist 房間。四項獨立 PostgreSQL／HTTP／WebSocket 測試及 19 項原生 DLL 資料模型檢查已通過；實際原生遊戲、重連與結果顯示仍待人工驗證。詳見 [房間成績合約與驗證清單](docs/lazer-room-scores-plan.md)。

## 本機開發與貢獻

Node.js 22 以上與 Rust 1.96 以上：

```sh
npm ci
npm run dev
cargo test --workspace --locked
npm run typecheck
npm run lint
npm run build
```

啟動 Sanctuary／Resonance 前需設定 `DATABASE_URL` 與服務密鑰。可用 `compose.dev.yaml` 將 PostgreSQL `15432` 與 Sanctuary `8080` 僅暴露到 localhost。資料夾、環境變數與執行方式在 [架構文件](docs/architecture.md) 及部署文件。

根目錄的 `npm run dev/build/lint/typecheck` 預設操作 Serendipity。開發啟動器介面時使用 `npm run dev --workspace @illyaverse/kaleidoscope`；Tauri 原生外殼與 C# 橋接的建置／驗證入口另見 [資料夾導覽](docs/repository-layout.md#建置與測試入口)。

請保留其他貢獻者的變更、不要複製參考專案程式碼，也不要加入 AI 生成圖片。Commit 使用帶 scope 的 Conventional Commits，例如 `feat(serendipity): add profile editor`、`fix(resonance): validate packet lengths`、`chore(infra): configure HTTPS deployment`。把已實作、整合測試通過、實際客戶端測試通過與待完成分開記錄。

參考研究來源：[Solar-System](https://github.com/SunriseCommunity/Solar-System)、[bancho.py](https://github.com/osuAkatsuki/bancho.py)、[g0v0-server](https://github.com/GooGuTeam/g0v0-server)、[ppy/osu](https://github.com/ppy/osu)。各自的版權與授權仍屬於原專案；此倉庫沒有 vendored reference source。
