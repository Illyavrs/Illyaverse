# 權限與角色管理

每個帳號可以持有多個角色，權限取聯集。`*` 代表完整權限；其餘權限採
精確名稱比對，沒有隱含的階級或名稱前綴繼承。公開註冊自動取得 `player`
角色。不要將 `*` 指派給公開註冊角色。

網站管理面板入口需要 `admin.view`。面板中的功能另外依照下列權限顯示，
後端每個 API 也會獨立驗證權限。只有 `admin.view` 的帳號不能修改資料。

| 權限 | 可執行的操作 |
| --- | --- |
| `roles.manage` | 建立、編輯、指派或移除自己有權限管理的角色 |
| `users.moderate` | 查看帳號 email，限制或解除限制其他帳號 |
| `users.sessions.revoke` | 撤銷自己有權限管理的帳號之網站、stable、lazer 登入 |
| `beatmaps.moderate` | 審核圖譜狀態 |
| `scores.moderate` | 審核成績，以及透過 API 進行有上限的 RX pp 重算 |
| `audit.read` | 查看管理操作紀錄 |
| `beatmaps.upload` / `beatmaps.import` | 上傳伺服器圖譜／從官方匯入圖譜 |
| `chat.send` / `social.manage` | 發送聊天訊息／管理自己的好友與封鎖關係 |

建議建立帶有 `admin.view` 與所需操作權限的自訂角色，再指派給工作人員。
內建 `player`、`moderator`、`administrator` 保留原名稱且不能刪除；
`administrator` 必須保留 `*`。自訂角色刪除前必須先從所有帳號解除指派，
系統不會默默移除玩家角色。每次角色或限制變更後，仍須保留至少一個
未受限制且持有 `*` 的管理員。

有限權限的角色管理者只能新增或移除自己持有的權限。他也不能把較高權限
帳號的角色換成較低權限角色，或撤銷較高權限帳號的登入。限制帳號需要
`users.moderate`；限制持有 `*` 的帳號另外需要 `*`，且不能限制自己。
上述規則由 API 執行，網頁上的唯讀或停用控制只是協助操作。

帳號清單依 ID 排序並分頁，可搜尋完整正整數 ID 或不分大小寫的名稱片段。
搜尋接受最多 64 個字元，不接受控制字元，`%` 和 `_` 作為一般字元處理。
僅持有角色管理或登入撤銷權限時，回應不包含 email、密碼雜湊或其他帳號的
權限陣列。每列提供目前可執行的操作旗標，寫入時仍重新驗證。

## 管理 API

所有路由都位於 `/api/v1`，使用網站登入 cookie 與既有 Origin 檢查。

| 路由 | 方法與內容 |
| --- | --- |
| `/admin/users?page=1&page_size=20&search=...` | GET 分頁帳號與角色摘要 |
| `/admin/roles` | GET 角色、權限與持有人數；POST 建立角色 |
| `/admin/roles/{id}` | PATCH 更新；DELETE 刪除未指派的自訂角色 |
| `/admin/users/{id}` | PATCH `roles` 與／或 `is_restricted` |
| `/admin/users/{id}/sessions/revoke` | POST 無需 request body，回傳 `revoked.web/stable/lazer` 筆數 |
| `/admin/beatmaps/{id}` | PATCH `status` |
| `/admin/scores/{id}` | PATCH `verification` |
| `/admin/scores/recalculate-relax` | POST `score_ids`，1–16 個不重複正整數 ID |

登入撤銷不會限制帳號，玩家可重新登入。lazer 筆數代表登入紀錄，同一筆
紀錄的 access 與 refresh token 會一起失效。已開啟的長連線會在既有驗證
週期或下一次需要驗證的操作發現撤銷；這個回應不代表已同步關閉所有視窗。

角色變更、限制、登入撤銷、圖譜／成績審核與 RX 重算均在資料庫交易中
重新讀取操作者目前的登入、限制與權限。請求若在排隊期間被撤銷權限或
登出，會在寫入前拒絕。成功的寫入和稽核紀錄一同提交。
網頁會區分「寫入失敗」與「寫入成功但清單重新載入失敗」。

常見錯誤代碼為 `stronger_role`、`builtin_role`、`last_administrator`、
`role_assigned`、`invalid_search`、`forbidden` 與 `unauthorized`。
角色仍有人持有時刪除會回傳 HTTP 409；權限不足回傳 403；登入失效回傳 401。

相關限制與驗證請參考 [RX](relax.md)、[部署](deployment.md) 與
[驗證紀錄](verification.md)。本機隔離測試不代表原生遊戲視窗已完成實測。
