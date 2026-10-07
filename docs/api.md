# 網站個人檔案 API

以下路由由 Sanctuary 提供。玩家使用有效的網站 session 操作自己的資料，不需要額外角色權限；cookie 寫入要求須來自設定的網站 origin。

## 個人資料與圖片

| 路由 | Body | 大小上限／行為 |
| --- | --- | --- |
| `GET /api/v1/me` | 無 | 自己的個人資料，包含私有帳號欄位 |
| `GET /api/v1/users/{id}` | 無 | 公開個人資料，不包含 email、permissions 或私有 appearance preferences |
| `PATCH /api/v1/me/profile` | JSON：`bio`、`country`、`accent_color` 的部分更新 | 32 KiB；bio 最多 4,000 字，country 為兩個大寫 ASCII 字母 |
| `POST /api/v1/me/media` | multipart：`file` 與 `kind=avatar\|banner\|background` | 保留 14 MiB request 上限；圖片本身最多 12 MiB，支援 PNG、JPEG、WebP |
| `DELETE /api/v1/me/media` | JSON：`{"kind":"avatar"}` 或 `{"kind":"banner"}` | 1 KiB；移除自己的指定圖片引用 |

DELETE 只接受上述兩種 `kind`，拒絕額外欄位；不接受目標玩家 ID、檔案 URL 或 background。成功回覆 HTTP 200：

```json
{"kind":"avatar","url":null}
```

重複移除已經沒有的圖片仍成功。服務以登入玩家 ID 清空 `avatar_url` 或 `banner_url`，保留另一張圖片與其他個人資料；這是資料庫引用的移除，不會刪除媒體檔案或其他玩家持有的引用。自己的 `/me` 與公開個人頁之後都回傳該欄位為 `null`。

交易先鎖定玩家再鎖定 session，在寫入時重新檢查帳號限制、session 存在與實際到期時間。排隊期間 session 被撤銷、到期或帳號遭限制的請求回覆 401，不會移除圖片。錯誤 origin 回覆 403，超過 DELETE body 上限回覆 413；錯誤 JSON 或不支援的欄位／kind 會遭拒絕。

## 設定流程

登入後從自己的個人頁選擇編輯，或開啟 `/settings?tab=profile`。文字欄位的「儲存變更」保存 biography 與 country；上傳、替換或移除 avatar／banner 是獨立的即時圖片操作，完成後重新讀取登入玩家資料。移除 avatar 後顯示名稱縮寫，移除 banner 後不再顯示上傳的封面。

圖片操作不提交或清除尚未儲存的 biography／country。操作期間停用圖片控制，失敗時顯示錯誤並保留文字草稿，成功時顯示圖片操作結果。個人頁只有自己的頁面提供編輯入口。

網站背景屬於私有 appearance preferences，既有移除背景操作先更新預覽，再由 appearance 的儲存操作保存；不要用 avatar／banner DELETE 代替它。

隔離瀏覽器測試見 [網站瀏覽器測試](browser-testing.md)。
