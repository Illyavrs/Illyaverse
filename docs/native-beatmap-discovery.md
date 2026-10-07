# Stable 官方圖譜辨識

Stable 選擇圖譜後，登入玩家的 `/web/osu-osz2-getscores.php` 請求會先依 MD5 查詢本機圖譜。尚未收錄的 checksum 由 Resonance 交給 Sanctuary，使用營運者設定的 osu! OAuth 應用程式查詢官方 metadata，取得並驗證 `.osu` 後保存。後續請求使用本機資料；已收錄的私服圖譜與管理員指定的狀態具有優先權，不會被官方資料覆寫。

設定 `OSU_CLIENT_ID`、`OSU_CLIENT_SECRET` 及兩個服務共用、至少 32 bytes 的 `INTERNAL_SERVICE_KEY`。本機原生測試使用 `.env.client-test.local`，修改後需重新建立 Sanctuary 容器。密鑰留在私有環境檔，不要放入提交、網站或遊戲客戶端。正式服務的上游固定為 `https://osu.ppy.sh`；測試用 loopback fixture 只存在於 Rust test build。

## 身分、快取與錯誤

官方 difficulty ID 與資料庫內部 ID 是不同欄位。Stable 的 leaderboard、BeatmapInfo 和提交結果使用官方 ID；分數、replay 與網站圖譜頁使用本機資料庫 ID。`/osu/{id}` 與 BeatmapInfo 先查官方 ID，僅對沒有官方 ID 的私服圖譜使用本機 ID。BeatmapInfo 批次請求只查快取，不會同步匯入最多 100 張未知圖譜。

同一 selector 的併發請求合併，最多四個 resolver 同時處理；OAuth token、全域請求間隔、429 的 Retry-After 和短期失敗快取降低重複請求。官方 `.osu` 最多 8 MiB，metadata、檔案 checksum、ruleset 與官方 ID 均須一致。資料先寫入唯一檔案，再發布資料庫引用；競爭中的 loser 只移除自己的未使用檔案。不能確定 COMMIT 是否成功時保留檔案，避免已提交圖譜失去內容。

只有官方明確表示 checksum 不存在時，resolver 回覆 404 `beatmap_not_found`，stable 才得到原有的未知圖譜回應。未設定 API、授權失敗、上游服務失敗或限流分別回覆 503、502 或 429，不會假造 Ranked，也不會把這些失敗快取成永久的不存在。

同一官方 ID 已有另一個 checksum 時回覆 409 `official_version_conflict`。目前一個官方 ID 僅保存一個版本；不會改寫既有 checksum、分數或 replay 的圖譜身分。官方 Qualified 狀態目前映射為 unranked，資料庫仍只有 pending、ranked、loved、unranked 四種狀態。只有 filename、缺少 checksum 的未知圖譜辨識仍未完成。

## 內部路由

`POST /api/v1/internal/beatmaps/resolve` 使用 `x-illya-service-key`，JSON body 上限 1 KiB。要求有效且未受限制的 `user_id`，並且恰好提供一個 selector：

```json
{"user_id": 123, "checksum": "0123456789abcdef0123456789abcdef"}
```

或 `{"user_id":123,"official_id":456}`。拒絕 null、非正整數、錯誤 checksum 與額外欄位。成功回覆 `id`、`official_id`、`official_set_id`、`md5`、`status`、`source`、`mode`；不回傳檔案路徑或上游 token。普通玩家的原生請求可觸發辨識，網站手動匯入仍要求 `beatmaps.import`。Gateway 在認證與 score integrity checks 通過後才呼叫此路由，並驗證回覆與共用資料庫完全一致。

手動匯入也保留已收錄資料：重複匯入不會更新狀態；官方檔案與已上傳私服檔案 checksum 相同時，回傳既有資料，不會自動把它改成官方來源或補寫官方 ID。管理員狀態調整仍使用獨立 moderation 操作。這取代先前重複匯入可能覆蓋狀態與來源的行為。

## 原生驗證

使用已設定的本機 HTTPS 環境與獨立測試帳號：

1. 關閉舊 stable，再用 `-devserver illya.test` 啟動並登入。
2. 選擇一張已安裝的官方 Ranked 或 Loved 圖譜，等待線上 leaderboard 載入；首次查詢需要下載 metadata 與 `.osu`。
3. 確認狀態已顯示，實際完成一次遊玩，再確認網站自己的成績紀錄與 replay。
4. 若仍是 Not submitted，提供該 difficulty 的官方網址或 ID，以及發生時間。圖譜辨識與真正提交是分開的驗證；尚未嘗試送分不能判定為提交失敗。

分數目前先進入 pending review，不會立即出現在公開排行榜。隔離 HTTP／PostgreSQL fixture 不代表遊戲視窗、原生送分或 replay 播放已通過。完整檢查見 [相容性清單](protocol-compatibility.md) 與 [本機測試](local-client-testing.md)。

官方 API 契約參考 [Lookup Beatmap](https://osu.ppy.sh/docs/index.html#lookup-beatmap)；伺服器實作及 fixture 為原創。
