# Illyaverse

受《魔法少女伊莉雅》啟發的 osu! 社群伺服器 monorepo。網站使用 Next.js、Tailwind CSS 與 shadcn/ui；服務使用 Rust，資料持久化到 PostgreSQL 與共用媒體 volume。所有實作皆為原創，參考專案僅用於架構與協定研究。

目前是可持續開發的初始版本：網站／API 與 stable／lazer 的協定接口各有自己的完成程度。**尚未宣稱官方 stable／lazer 客戶端已通過完整連線測試。** 詳見 [協定相容性](docs/protocol-compatibility.md) 與 [連線教學](docs/connection.md)。

| 名稱 | 路徑 | 職責 |
| --- | --- | --- |
| Serendipity | `apps/serendipity` | 網站、landing、排行榜、譜面、聊天、個人頁與管理介面 |
| Sanctuary | `services/sanctuary` | 帳號、權限、偏好、媒體、譜面、分數、replay、聊天與稽核 API |
| Resonance | `services/resonance` | stable 封包與 legacy web、lazer OAuth／API v2 相容接口 |
| Kaleidoscope | `apps/kaleidoscope`、`tools/kaleidoscope` | 固定連到 Illyaverse 的 Windows 啟動器與 Rust 程序／清理核心 |
| Transfiguration | `tools/transfiguration` | 官方 lazer 的原創強制啟動外掛與相容性檢查 |
| 共用領域 | `crates/illya-domain` | 型別、驗證與 pp 計算 |
| 封包協定 | `crates/resonance-protocol` | 原創 stable wire parser／encoder |

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

請保留其他貢獻者的變更、不要複製參考專案程式碼，也不要加入 AI 生成圖片。Commit 使用帶 scope 的 Conventional Commits，例如 `feat(serendipity): add profile editor`、`fix(resonance): validate packet lengths`、`chore(infra): configure HTTPS deployment`。把已實作、整合測試通過、實際客戶端測試通過與待完成分開記錄。

參考研究來源：[Solar-System](https://github.com/SunriseCommunity/Solar-System)、[bancho.py](https://github.com/osuAkatsuki/bancho.py)、[g0v0-server](https://github.com/GooGuTeam/g0v0-server)、[ppy/osu](https://github.com/ppy/osu)。各自的版權與授權仍屬於原專案；此倉庫沒有 vendored reference source。
