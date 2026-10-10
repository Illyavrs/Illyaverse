---
title: "權限與角色管理"
---

每個帳號可以持有多個角色，權限取聯集。`*` 代表完整權限；其餘權限採
精確名稱比對，沒有隱含的階級或名稱前綴繼承。公開註冊自動取得 `player`
角色。不要將 `*` 指派給公開註冊角色。

網站管理面板依帳號持有的功能權限提供入口與分區。例如只持有
`chat.moderate` 或 `announcements.manage` 也能開啟對應區域，無須另外授予
`admin.view`。後端每個 API 獨立驗證權限；`admin.view` 仍授權讀取管理用
圖譜中繼資料，不會隱含任何寫入權限。

| 權限                                  | 可執行的操作                                       |
| ------------------------------------- | -------------------------------------------------- |
| `roles.manage`                        | 建立、編輯、指派或移除自己有權限管理的角色         |
| `users.moderate`                      | 查看帳號 email，限制或解除限制其他帳號             |
| `users.sessions.revoke`               | 撤銷自己有權限管理的帳號之網站、stable、lazer 登入 |
| `chat.moderate`                       | 查看最小玩家目錄、設定或解除限時聊天禁言           |
| `beatmaps.moderate`                   | 審核圖譜狀態                                       |
| `beatmaps.rank.request`               | 提交圖譜排名申請                                   |
| `beatmaps.rank.review`                | 查看排名申請佇列、拒絕申請；核准另須圖譜審核權限   |
| `scores.moderate`                     | 審核成績，以及透過 API 進行有上限的 RX pp 重算     |
| `announcements.manage`                | 建立公告草稿、編輯、發布、撤下與查看非公開公告     |
| `audit.read`                          | 查看管理操作紀錄                                   |
| `beatmaps.upload` / `beatmaps.import` | 上傳伺服器圖譜／從官方匯入圖譜                     |
| `chat.send` / `social.manage`         | 發送聊天訊息／管理自己的好友與封鎖關係             |

自訂工作人員角色只需授予其負責功能的權限。migration 0024 為內建
`moderator` 加入 `chat.moderate`；自訂角色須明確指派此權限。
Migration 0025 為內建 `player` 加入 `beatmaps.rank.request`，為內建
`moderator` 加入 `beatmaps.rank.request` 與 `beatmaps.rank.review`。
自訂角色的現有權限不會被替換或自動擴充。
內建 `player`、`moderator`、`administrator` 保留原名稱且不能刪除；
`administrator` 必須保留 `*`。自訂角色刪除前必須先從所有帳號解除指派，
系統不會默默移除玩家角色。每次角色或限制變更後，仍須保留至少一個
未受限制且持有 `*` 的管理員。

有限權限的角色管理者只能新增或移除自己持有的權限。他也不能把較高權限
帳號的角色換成較低權限角色，或撤銷較高權限帳號的登入。限制帳號需要
`users.moderate`；限制持有 `*` 的帳號另外需要 `*`，且不能限制自己。
上述規則由 API 執行，網頁上的唯讀或停用控制只是協助操作。

## 圖譜提名與排名申請

`beatmap_nominator` 是圖譜提名角色，預設權限只有 `chat.send`、
`beatmaps.upload`、`beatmaps.import`、`beatmaps.moderate`、
`beatmaps.rank.request` 與 `beatmaps.rank.review`。它沒有 `*`、帳號限制、
成績審核或角色管理權限；角色名稱不形成權限階級。若升級前已有同名自訂
角色，migration 0025 保留其原有權限，由管理員明確配置所需功能。

公開註冊的 `player` 已有 `beatmaps.upload`，新上傳的伺服器圖譜以
Graveyard 開始；升級不會重寫既有圖譜狀態。狀態屬於每一個難度，支援
Graveyard、WIP、Pending、Unranked、Ranked 與 Loved，同一圖譜組中的其他
難度不會因一個難度獲得 Ranked 而一起變更。

玩家可提出排名申請，目標包含自己上傳的有效、未排名本服圖譜，以及官方
Pending／Graveyard 圖譜。每個圖譜組最多有一筆待審申請；申請保存提交時的難度清單與身分
快照，審核只可核准其中明確選取的難度。未選取的難度保留原狀態。
申請者可查看自己的申請並撤回待審申請，之後失去提交權限也仍能管理已提交
申請。審核者以 `beatmaps.rank.review` 取得佇列和拒絕權限；核准申請還須
`beatmaps.moderate`。直接變更難度狀態由 `beatmaps.moderate` 授權，無須
`admin.view`。一般帳號最多保留 5 筆待審申請，每 24 小時最多提交 10 筆。

`beatmaps.status_revision`、伺服器難度狀態決策的 `revision`，以及申請的
`revision` 由資料庫維護。有效狀態變更會累加版本；直接重設版本不會
繞過檢查，即使狀態曾改走又改回也能辨識舊決策。申請的理由、審核內容
或核准難度清單變更也會增加申請版本。伺服器決策依公開難度 ID 與圖譜組
身分保存，官方難度稍後匯入或中繼資料更新時仍會保留該難度的伺服器狀態。
審核理由與操作者資料保留在管理用資料表與稽核紀錄，不混入公開圖譜欄位。

核准申請須帶入申請的 `expected_revision`，並為每個選取難度提供目前的
伺服器決策版本 `expected_revision` 和已匯入圖譜版本
`expected_status_revision`。後者必須明確為整數或 null；null 表示該官方
難度尚未匯入。直接修改難度狀態也使用相同的兩個版本，防止官方圖譜在
決策排隊期間匯入或其他工作人員先修改狀態時覆寫舊資料。版本衝突回傳 409，
不會自動重新送出決策。申請與審核理由去除首尾空白後須為 1–1000 字元，
最多 4000 bytes，且不得包含控制字元。

帳號清單依 ID 排序並分頁，可搜尋完整正整數 ID 或不分大小寫的名稱片段。
搜尋接受最多 64 個字元，不接受控制字元，`%` 和 `_` 作為一般字元處理。
僅持有角色管理或登入撤銷權限時，回應不包含 email、密碼雜湊或其他帳號的
權限陣列。每列提供目前可執行的操作旗標，寫入時仍重新驗證。

## 管理 API

所有路由都位於 `/api/v1`，使用網站登入 cookie 與既有 Origin 檢查。

| 路由                                                              | 方法與內容                                                                                           |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `/admin/users?page=1&page_size=20&search=...`                     | GET 分頁帳號與角色摘要                                                                               |
| `/admin/roles`                                                    | GET 角色、權限與持有人數；POST 建立角色                                                              |
| `/admin/roles/{id}`                                               | PATCH 更新；DELETE 刪除未指派的自訂角色                                                              |
| `/admin/users/{id}`                                               | PATCH `roles` 與／或 `is_restricted`                                                                 |
| `/admin/users/{id}/sessions/revoke`                               | POST 無需 request body，回傳 `revoked.web/stable/lazer` 筆數                                         |
| `/admin/chat-silences?page=1&page_size=24&search=...`             | GET 最小玩家目錄、有效禁言期限、`chat_silence_revision` 與操作旗標                                   |
| `/admin/users/{id}/chat-silence`                                  | PUT `expires_at`、`reason`、`expected_revision`；DELETE `reason`、`expected_revision`                |
| `/admin/beatmaps/{id}`                                            | PATCH `status`                                                                                       |
| `/admin/beatmapsets?page=1&page_size=24&q=...&availability=all`   | GET 本服上傳圖譜組，包含已撤下的圖譜；需 `beatmaps.moderate`                                         |
| `/admin/beatmapsets/{native_set_id}`                              | GET 驗證後的官方或本服圖譜組與逐難度狀態、版本                                                       |
| `/admin/beatmapsets/{native_set_id}/difficulties/{native_id}`     | PATCH `status`、`reason`、`expected_revision` 與 `expected_status_revision`                          |
| `/me/beatmap-rank-requests?page=1&page_size=24&status=all`        | GET 自己的申請，登入後可讀                                                                           |
| `/beatmap-rank-requests`                                          | POST `native_set_id` 或 `official_set_id` 擇一、`reason`，可選 `difficulty_ids`                      |
| `/beatmap-rank-requests/{id}`                                     | GET 自己的申請或具審核權限的申請                                                                     |
| `/beatmap-rank-requests/{id}/withdraw`                            | POST `expected_revision`，只可撤回自己的待審申請                                                     |
| `/admin/beatmap-rank-requests?page=1&page_size=24&status=pending` | GET 審核佇列，需 `beatmaps.rank.review`                                                              |
| `/admin/beatmap-rank-requests/{id}`                               | PATCH `decision`、`expected_revision`、`reason`；核准另須 `difficulty_ids` 與 `difficulty_revisions` |
| `/admin/scores/{id}`                                              | PATCH `verification` 與 `expected_revision`                                                          |
| `/admin/scores/recalculate-relax`                                 | POST `score_ids`，1–16 個不重複正整數 ID                                                             |
| `/admin/announcements`                                            | GET 依狀態分頁公告；POST 建立草稿                                                                    |
| `/admin/announcements/{id}`                                       | GET 指定公告的最新版本；PATCH 完整標題、內文、狀態與預期 `revision`                                  |

排名申請核准的 `difficulty_revisions` 是逐難度物件陣列，每個物件包含
`native_id`、`expected_revision` 與 `expected_status_revision`，須與
`difficulty_ids` 完全對應。`decision` 只接受 `approved` 或 `rejected`；
拒絕時不得提供核准難度欄位。舊 `/admin/beatmaps/{id}` 使用內部圖譜 ID，
保留無版本欄位的相容契約，但仍只變更該難度並保存私服狀態決策；新的
逐難度管理介面使用上述具版本檢查的路由。

登入撤銷不會限制帳號，玩家可重新登入。lazer 筆數代表登入紀錄，同一筆
紀錄的 access 與 refresh token 會一起失效。已開啟的長連線會在既有驗證
週期或下一次需要驗證的操作發現撤銷；這個回應不代表已同步關閉所有視窗。

公告公開 API 只回傳已發布內容，管理員也不能從公開路由讀取草稿或已撤下公告。
公告建立與更新會在交易中重新驗證權限和登入，再與稽核紀錄一同提交。
更新必須帶入目前 `revision`；版本不同回傳 409 `announcement_conflict`，
不會覆蓋其他人的修改。內建管理員維持單一 `*` 權限；一般玩家與版主不會
自動取得公告管理權限。公告 API、面板與暫存草稿的授權皆以
`announcements.manage` 為準；失去此權限、登出或換帳號會清除私密暫存。

禁言目錄只提供公開身分、有效期限、原始版本與 `can_silence`／`is_protected`，
不提供 email、密碼、角色權限或理由。`chat.moderate` 可處理一般玩家；不可
處理自己，其他持有 `chat.moderate` 或 `*` 的帳號須由持有 `*` 的管理員處理。
期限須為 RFC3339 時間，在取得權限表、排序後的使用者與登入鎖之後，以資料庫
`clock_timestamp()` 驗證為未來且不超過 30 天。理由去除首尾空白後須為
1–500 字元，輸入不得包含控制字元，僅寫入私密稽核紀錄。套用與解除都須帶
目前 `chat_silence_revision` 作為 `expected_revision`；過期版本回傳 409
`chat_silence_conflict`。已到期的期限在 API 顯示為 null，原始版本仍保留。
禁言只阻止新訊息，玩家仍能登入、遊玩、接收訊息與讀取原有歷史。

成績清單與詳情提供 `moderation_revision`。審核必須帶入此版本，取得成績鎖
後重新比對；其他管理員或原生發布流程改過狀態時回傳 409
`score_moderation_conflict`，即使狀態曾改走又改回也不會接受舊版本。
要求與目前相同的狀態回傳 400 `score_verification_unchanged`，不新增稽核決策。
成功審核的前後狀態與版本一同寫入稽核。一般 stable／lazer 成績原有的合格
原生提交自動發布流程維持不變；新增審核工具不會要求每筆新成績經人工批准。

角色變更、限制、登入撤銷、禁言、圖譜／成績審核與 RX 重算均在資料庫交易中
重新讀取操作者目前的登入、限制與權限。請求若在排隊期間被撤銷權限或
登出，會在寫入前拒絕。成功的寫入和稽核紀錄一同提交。
網頁會區分「寫入失敗」與「寫入成功但清單重新載入失敗」。

常見錯誤代碼為 `stronger_role`、`builtin_role`、`last_administrator`、
`role_assigned`、`invalid_search`、`forbidden` 與 `unauthorized`。
角色仍有人持有時刪除會回傳 HTTP 409；權限不足回傳 403；登入失效回傳 401。

2026-10-09 的新禁言與成績版本 API 已通過隔離 PostgreSQL 測試；網站分區與
操作介面已有獨立瀏覽器與 source build 驗證。migration 0023／0024 與這次變更尚未套用至保留的本機
部署，其資料庫仍為 0022。上述為目前原始碼契約，不是已部署或原生視窗實測宣告。
Migration 0025 的圖譜提名、難度狀態版本與排名申請同樣尚未套用至保留部署；
各項隔離測試結果以整合驗證紀錄為準。lazer 登入已由玩家確認，這不代表
搜尋、圖譜審核或新 schema 已完成原生客戶端驗證。

相關限制與驗證請參考 RX、[部署](/docs/deploy/deployment) 與
[驗證紀錄](/docs/reference/verification)。本機隔離測試不代表原生遊戲視窗已完成實測。
