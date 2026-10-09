# 自己的登入工作階段

帳號設定列出目前仍有效的網站、stable 與 lazer 登入。使用者可以撤銷一個登入，或登出其他登入並保留目前網站登入；這些操作不需要管理員權限。

## API

- `GET /api/v1/me/sessions`：僅回傳自己的有效登入，最多 100 筆，目前網站登入排在最前面。`has_more` 表示還有其他登入；回覆使用 `private, no-store`。
- `DELETE /api/v1/me/sessions/{client}/{id}`：`client` 是 `web`、`stable` 或 `lazer`，`id` 是工作階段 UUID。其他人的、已失效或不存在的登入均不可撤銷。撤銷目前網站登入會清除登入 Cookie。
- `POST /api/v1/me/sessions/revoke-others`：保留目前網站登入，撤銷其他網站、stable 與 lazer 登入。

每筆資料包含 `id`、`client`、`current`、`created_at`、`expires_at` 與可能為 `null` 的 `last_seen_at`。只有 lazer 記錄最後活動時間；不補造其他客戶端的活動、裝置、IP 或位置資訊。工作階段 ID 是資料列識別碼，回覆不包含登入 token、token hash 或密碼。

lazer 使用 refresh token 的期限判斷登入是否仍有效；access token 過期但仍可 refresh 的登入不會消失。Refresh 會更換 access／refresh token，但保留工作階段 ID 與建立時間，所以更新 token 後仍能撤銷原先看到的登入。重新使用密碼登入會建立新的工作階段。

## 授權與撤銷

寫入使用既有使用者、工作階段鎖定順序，並在等待後和提交前檢查授權與到期時間。網站寫入套用同源 CSRF 檢查。只操作經過驗證的使用者自己的資料列；稽核紀錄不包含 token。

單純登出不取消密碼復原或驗證郵件。密碼變更／重設繼續使用原有完整撤銷流程，包含失效復原 token 與取消相應郵件。

撤銷後，後續網站授權、stable poll、lazer access／refresh 會拒絕舊登入；網站聊天連線也重新檢查登入。已被伺服器接受的操作可能完成。這不是取消已完成或正在處理的成績。

## 驗證範圍

隔離 PostgreSQL HTTP fixture 涵蓋自己的資料、跨使用者識別碼、有效期限、列數限制、CSRF、目前登入／其他登入、復原郵件保留，以及排隊期間的撤銷、限制與到期。另有 lazer refresh UUID、token 輪替、併發及撤銷 fixture。

網站操作、正式映像與原生遊戲畫面是不同的驗證範圍。瀏覽器測試和部署證據由本次整合紀錄補充；此文件不宣稱已完成真正的 stable／lazer 遊戲測試。
