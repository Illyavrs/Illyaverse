# 原生客戶端官方圖譜辨識

Stable 選擇圖譜後，登入玩家的 `/web/osu-osz2-getscores.php` 請求會先依 MD5 查詢本機圖譜。尚未收錄的 checksum 由 Resonance 交給 Sanctuary，使用營運者設定的 osu! OAuth 應用程式查詢官方 metadata，取得並驗證 `.osu` 後保存。後續請求使用本機資料；已收錄的私服圖譜與管理員指定的狀態具有優先權，不會被官方資料覆寫。

設定 `OSU_CLIENT_ID`、`OSU_CLIENT_SECRET` 及兩個服務共用、至少 32 bytes 的 `INTERNAL_SERVICE_KEY`。本機原生測試使用 `.env.client-test.local`，修改後需重新建立 Sanctuary 容器。密鑰留在私有環境檔，不要放入提交、網站或遊戲客戶端。正式服務的上游固定為 `https://osu.ppy.sh`；測試用 loopback fixture 只存在於 Rust test build。

Lazer 的已認證 checksum lookup、difficulty ID 查詢、以 difficulty 查詢圖譜集，以及單人成績 token 建立也使用同一 resolver。Lookup 同時帶 checksum、ID、filename 時先依 checksum 取得圖譜，並要求已提供的正整數 ID 與回傳的原生 ID 一致；filename 不會覆蓋 checksum。只有 filename 的 lookup 新增本機精確檔名對照，未命中才透過官方 filename lookup 查詢並驗證內容。以官方圖譜集 ID 查詢現在可取得完整原始 difficulty 的 metadata；搜尋仍只使用本機已收錄資料。

具有 `beatmaps.import` 權限的帳號也可以提供完整官方 `.osz` 與 set ID，經整組官方 metadata 核對後保存原始封包與全部 difficulties。這是網站的明確匯入動作；原生 set lookup 不會自動下載整包。匯入成功後，stable osu!direct 與 lazer set download 可下載相同的完整 bytes，網站也可瀏覽同封包難度。驗證範圍、重試與版本限制見[官方封包匯入](official-beatmap-archives.md)。

## Lazer 官方圖譜集資訊

`GET /api/v2/beatmapsets/{id}` 與以 difficulty 查詢 set 的路由均要求有效 bearer。未保存完整 archive 的官方 set，使用一次受限的官方 set metadata 請求，驗證完整原始 difficulties 的 ID、set、checksum、ruleset 與必要日期、難度、物件數等資料。最多 512 張，忽略 `converts` relation，拒絕 original list 內的 convert／deleted 成員及重複 ID、checksum。這個讀取不新增圖譜、檔名對照或檔案；之後查看分數或遊玩時，個別 `.osu` 仍依原有 resolver 取得並核對。

回覆保留 canonical 官方 IDs，既有完全一致的 difficulty 使用本站目前的狀態與已保存資料。版本、set 或來源衝突回覆 409，不把不同內容拼成同一組。上游資料快取不含本站 moderation，管理員調整後的下次回覆重新讀取資料庫。官方 mapper 帳號 ID 不等於本站玩家 ID，因此不轉發 `user`、`owners`、`related_users` 等外服身分；保留 creator 文字，沒有身分映射時 `user_id` 為 0。`uploaded_by` 仍保存原 ingestor，不推論為 mapper 關聯。

官方圖片、preview、外部網址與玩家專用欄位不轉發。只有實際保存可驗證 archive 時，nested `availability.download_disabled` 才會為 false；metadata 本身不能讓整包變成可下載。本站 local package 與已驗證完整官方 archive 直接使用本機資料。上游失敗時，已有圖譜的 set 保留本機已收錄難度的回覆；這個 fallback 不代表整組 metadata 已取得。

Sanctuary 的 metadata 工作總預算 6 秒，Gateway 內部 HTTP 預算 8 秒，均包含等待；等待後重新確認有效帳號／bearer。正向 metadata 快取 5 分鐘，最多 32 組及 16 MiB，單回覆最多 2 MiB；404 快取 60 秒，暫時失敗 5 秒，429 沿用全域 Retry-After。共用既有四個 resolver slots 與至少一秒的全域請求間隔。快取或上游 metadata 尚未取得時不回傳假造的成功資料。

此處回覆原生 `APIBeatmapSet`，沒有自訂 202 job 或輪詢格式。官方 [GetBeatmapSetRequest](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Online/API/Requests/GetBeatmapSetRequest.cs)、[APIBeatmapSet](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Online/API/Requests/Responses/APIBeatmapSet.cs) 與 [APIBeatmap](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Online/API/Requests/Responses/APIBeatmap.cs) 的身分及欄位契約作為參考，實作為原創。原生遊戲畫面仍需要真人驗證。

## 身分、快取與錯誤

對外 difficulty ID 與資料庫內部 ID 是不同欄位。Stable 的 leaderboard、BeatmapInfo 和提交結果使用 canonical native ID；分數、replay 與網站圖譜頁使用本機資料庫 ID。官方 native ID 保留在 `1..1_999_999_999`，私服 difficulty／set 分別使用 `2_000_000_000..2_147_483_647`，不是把內部 primary key 當別名。未知高區段 ID 不會觸發官方查詢。BeatmapInfo 最多接收 100 個 selector，只查快取，不會同步匯入整批未知圖譜。

Lazer 使用相同的 canonical difficulty／set ID。已匯入官方圖譜的資料庫 primary key 不接受為另一個原生 ID；私服套件與其 difficulties 有獨立且持久的對外 ID。官方 metadata 若進入本站管理的高區段則明確拒絕，這個區段是本站政策而非 osu! 官方保留範圍。升級與舊圖譜限制見[套件契約](local-map-packages.md)。

單人成績 token 先依 checksum 辨識，再綁定本機圖譜、對外 ID、ruleset 與玩家。辨識等待結束後，在寫入 token 的短交易內重新檢查 bearer session 和帳號狀態；提交與重試只使用已綁定身分，不會重新辨識或接受官方圖譜的 primary-key 別名。

同一 selector 的併發請求合併，最多四個 resolver 同時處理；OAuth token、全域至少一秒的請求間隔、429 的 Retry-After 和短期失敗快取降低重複請求。官方 `.osu` 最多 8 MiB，metadata、檔案 checksum、ruleset 與官方 ID 均須一致。資料先寫入唯一檔案，再發布資料庫引用；競爭中的 loser 只移除自己的未使用檔案。不能確定 COMMIT 是否成功時保留檔案，避免已提交圖譜失去內容。

只有官方明確表示 checksum 不存在時，resolver 回覆 404 `beatmap_not_found`，stable 才得到原有的未知圖譜回應。未設定 API、授權失敗、上游服務失敗或限流分別回覆 503、502 或 429，不會假造 Ranked，也不會把這些失敗快取成永久的不存在。

同一官方 ID 已有另一個 checksum 時回覆 409 `official_version_conflict`。目前一個官方 ID 僅保存一個版本；不會改寫既有 checksum、分數或 replay 的圖譜身分。官方 Qualified 狀態目前映射為 unranked，資料庫仍只有 pending、ranked、loved、unranked 四種狀態。

Lazer 的 `BeatmapOnlineStatus` 沒有 `Unranked` 成員。本站資料庫的 unranked 在原生 JSON 使用 `pending`、數值 rank 0，保存狀態不變；它不會被宣告為 Ranked 或 Qualified。此 wire 標籤規則同樣用於本機 set、difficulty 與 playlist serializer。第一手 enum：[BeatmapOnlineStatus](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Beatmaps/BeatmapOnlineStatus.cs)。

## 精確檔名對照

新 `.osu` 上傳保留原始 basename；新 `.osz` 上傳將每張 difficulty 與封包內解碼後的 basename 一起保存，checksum 排序不會打散其關聯。`beatmap_filename_aliases` 使用精確、區分大小寫及 Unicode bytes 的資料庫鍵；名稱不經 trim、case folding 或 Unicode normalization。它是查詢資料，不會組成檔案路徑，也不會在每次 lookup 解壓封包。公開圖譜回覆不新增檔名、對照表或檔案路徑欄位。

本站 filename selector 限制為 5–1024 UTF-8 bytes、`.osu` 副檔名（副檔名驗證忽略 ASCII 大小寫），禁止斜線、反斜線、冒號與 control characters。驗證允許 `.OSU` 不代表查詢會把它改成 `.osu`；整個名稱仍精確比對。這是本站的 basename 政策，不是官方 API 宣告的完整字元規則。新 ZIP 上傳在建立索引前驗證原始項目，依 UTF-8 flag、CP932 或有效 Unicode Path 解碼，再處理分隔符；畸形編碼不使用替代字元修補。見[封包檔名政策](local-map-packages.md#archive-filename-validation)。舊 `.osz` 可使用下述明確維護操作；實際 native 匯入仍需要真人驗證。

同名可以屬於不同 difficulty；lookup 若命中兩張以上，回覆 409 `beatmap_filename_ambiguous`，不依 ID 大小、owner、官方來源或狀態任意選一張，也不繼續查官方。新的 alias 與圖譜在同一 publication transaction 發布，並使用 authority → filename → checksum／resource 的鎖順序。完整同 owner 重試不會以新的 standalone 檔名擴充既有 alias。Migration 0017 只建對照表，不猜測舊名稱、不改任何 retained bytes／checksum／ID。具有 `beatmaps.moderate` 的管理員可逐個 package 預覽、驗證並套用[舊 `.osz` 檔名修復](filename-alias-recovery.md)；它驗證原始 archive、完整 manifest 與 retained member bytes，只新增 aliases 及一次有效寫入的 audit。未知 legacy fingerprint 保持未知；standalone `.osu` 缺少可驗證原始名稱時明確拒絕。

官方 filename 結果仍須通過 metadata／下載 checksum、ruleset、difficulty ID 和 set ID 驗證，並在發布時重新檢查本機檔名。它不能把已有私服 checksum 收養成官方圖譜；衝突回覆 409 `beatmap_filename_conflict`。既有完全一致的官方圖譜可以增加已驗證的 alias，同時保留原 owner、狀態與 bytes。Gateway 另驗回覆與共用資料庫的 alias／圖譜身分一致。

Stable 的 filename-only BeatmapInfo（`Ids` 空陣列）保留原輸入的 zero-based index；未收錄名稱省略，後面的索引不重新壓縮，且整批查詢不連官方。混合 `Filenames`／`Ids` 的原生 wire indexing 尚無可靠證據，本輪保留既有 ID 分支及其索引，不宣稱混合 filename 支援完成。

第一手契約：[官方 Lookup Beatmap](https://osu.ppy.sh/docs/index.html#lookup-beatmap)、[GetBeatmapRequest](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Online/API/Requests/GetBeatmapRequest.cs)、[APIBeatmapMetadataSource](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Beatmaps/APIBeatmapMetadataSource.cs)、[BeatmapInfo.Path](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Beatmaps/BeatmapInfo.cs) 與 [BeatmapImporter](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Beatmaps/BeatmapImporter.cs)。正常 stock 匯入流程傳保留的頂層檔名，不從曲名重建。官方 [lookup controller](https://github.com/ppy/osu-web/blob/75829ba32e0d9e9ed199dd459bb2b123a3a5d23f/app/Http/Controllers/BeatmapsController.php) 另有 checksum → filename → ID fallback；本站保留較強的 checksum／ID 一致性規則，不把未知 checksum 回退成另一張內容。Filename-only index／省略規則由 [bancho.py 的第一手實作](https://github.com/osuAkatsuki/bancho.py/blob/f10c03a3969c1b48a0cf9a423384653633c52fba/app/services/maps.py) 佐證，不能以它認定官方 mixed wire contract。伺服器程式為原創，沒有複製參考實作。

## 內部路由

`POST /api/v1/internal/beatmaps/resolve` 使用 `x-illya-service-key`，JSON body 上限 4 KiB。要求有效且未受限制的 `user_id`，並且恰好提供一個 selector：

```json
{"user_id": 123, "checksum": "0123456789abcdef0123456789abcdef"}
```

或 `{"user_id":123,"official_id":456}`、`{"user_id":123,"filename":"Artist - Title (Mapper) [Easy].osu"}`。拒絕 null、多 selector、非正整數、錯誤 checksum／filename 與額外欄位。成功回覆 `id`、`native_id`、`native_set_id`、`official_id`、`official_set_id`、`md5`、`status`、`source`、`mode`；不回傳檔名、檔案路徑或上游 token。普通玩家的原生請求可觸發辨識，網站手動匯入仍要求 `beatmaps.import`。Gateway 在認證與相應 integrity checks 通過後才呼叫此路由，並驗證回覆與共用資料庫完全一致。

手動匯入也保留已收錄資料：重複匯入不會更新狀態；官方檔案與已上傳私服檔案 checksum 相同時，回傳既有資料，不會自動把它改成官方來源或補寫官方 ID。管理員狀態調整仍使用獨立 moderation 操作。這取代先前重複匯入可能覆蓋狀態與來源的行為。

`POST /api/v1/internal/beatmapsets/metadata` 使用相同 service key 與 4 KiB body 上限，僅接受 `{"user_id":123,"official_set_id":456}`。拒絕額外欄位、null 與非正整數／官方高區段 ID。回覆明確定義的共用 metadata DTO，不帶檔案路徑、上游授權或 foreign 帳號／URL。此路由是資訊讀取，不是前述圖譜內容發布路由。

## 原生驗證

使用已設定的本機 HTTPS 環境與獨立測試帳號：

1. 關閉舊 stable，再用 `-devserver illya.test` 啟動並登入。
2. 選擇一張已安裝的官方 Ranked 或 Loved 圖譜，等待線上 leaderboard 載入；首次查詢需要下載 metadata 與 `.osu`。
3. 確認狀態已顯示，實際完成一次遊玩，再確認網站自己的成績紀錄與 replay。
4. 若仍是 Not submitted，提供該 difficulty 的官方網址或 ID，以及發生時間。圖譜辨識與真正提交是分開的驗證；尚未嘗試送分不能判定為提交失敗。

分數目前先進入 pending review，不會立即出現在公開排行榜。隔離 HTTP／PostgreSQL fixture 不代表遊戲視窗、原生送分或 replay 播放已通過。完整檢查見 [相容性清單](protocol-compatibility.md) 與 [本機測試](local-client-testing.md)。

官方 API 契約參考 [Lookup Beatmap](https://osu.ppy.sh/docs/index.html#lookup-beatmap)；伺服器實作及 fixture 為原創。
