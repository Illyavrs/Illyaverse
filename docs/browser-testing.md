# 網站瀏覽器測試

完整 Playwright suite 包含會建立帳號、上傳圖片與送出聊天訊息的真實 HTTP 流程，以及攔截 API 的瀏覽器 fixtures。用獨立的 disposable 環境執行，避免把測試資料寫入既有預覽或玩家資料庫。

## 執行完整 suite

在 monorepo 根目錄執行：

```sh
ILLYA_ALLOW_DISPOSABLE_TEST=1 node infra/disposable-web-smoke.mjs
```

PowerShell：

```powershell
$env:ILLYA_ALLOW_DISPOSABLE_TEST = '1'
node infra/disposable-web-smoke.mjs
```

需要支援 `--wait` 與 config JSON 的 Docker Compose、Node.js、已安裝的 workspace npm dependencies 與 Playwright Chromium。應先準備包含待測程式碼的本機應用程式 images：`illyaverse/sanctuary:local`、`illyaverse/resonance:local`、`illyaverse/serendipity:local`，以及既有 `postgres:17-alpine`、`caddy:2-alpine`。helper 將這五個 tags 固定為當次 image ID，不 build／pull，也不替換既有服務。

helper 使用隨機 Compose project、全新的 PostgreSQL 17、專屬 network／volumes 與臨時產生的測試登入資料及密鑰。它不使用專案的實際 `.env` 或既有資料庫。網站只綁定 loopback，預設網址為 `http://localhost:19000`；若該 port 已被占用，選擇空閒 port：

```powershell
$env:ILLYA_WEB_SMOKE_PORT = '19001'
node infra/disposable-web-smoke.mjs
```

完成或失敗後 helper 清理當次隨機 project 的容器、network、volumes 與臨時環境檔。請以測試 exit status 判斷結果；最新實測證據另外記錄在 [verification](verification.md)。

Playwright 的標準結果放在 `apps/serendipity/test-results`。選用 `ILLYA_WEB_SMOKE_CAPTURE=1` 可啟用既有 RBAC、recovery、RX、score 與 workflow 擷取；圖片依各測試儲存在 repo 的 `data-*-review` 或 `.impeccable/review` 目錄。只擷取這次個人圖片、審核與圖譜流程時，設定 `ILLYA_CAPTURE_WORKFLOW_REVIEW=1`。這些擷取使用測試 fixtures，不能當成正式社群活動。

選用 `ILLYA_WEB_SMOKE_TEST_FILES=catalog-workflows.spec.ts` 可在全新 disposable 環境只重跑指定檔案；接受最多 8 個逗號分隔、符合 `^[a-z][a-z-]*\.spec\.ts$` 的檔名，省略此變數則執行完整 suite。

## 個人圖片的 focused checks

`apps/serendipity/tests/profile-media.spec.ts` 使用攔截 API 的 fixtures，檢查移除 avatar／banner、保留未儲存 biography／country、錯誤回饋與登入狀態。它不會改動真實社群帳號。

後端的 request guards 不需要資料庫：

```sh
cargo test -p sanctuary profile_media_removal_request_guards
```

持久化與排隊授權測試要求 `ILLYA_TEST_DATABASE_URL` 指向自己管理的 disposable PostgreSQL 17；每次建立新的隨機 schema：

```sh
cargo test -p sanctuary profile_media_removal_persistence_and_queued_authorization -- --ignored --nocapture
```

網站與 HTTP 測試不代表 native stable／lazer 客戶端相容性已驗證；相關狀態仍以 [協定相容性](protocol-compatibility.md) 為準。
