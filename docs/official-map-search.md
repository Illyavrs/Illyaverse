# 官方圖譜搜尋

有 `beatmaps.import` 權限的帳號可在 `/beatmaps/import` 搜尋 osu! 官方圖譜組。
搜尋只取得官方 metadata，並顯示本站保存的難度數量、完整封包與撤下狀態。
選擇結果會把 difficulty ID 或 set ID 填入原有匯入表單；確認並提交表單後才匯入。
切換匯入方式會保留另一份草稿及已選取的 `.osz`。

## API 與使用限制

`GET /api/v1/beatmaps/official/search` 需要本站登入與 `beatmaps.import`。
伺服器先驗證權限再解析 query，也會在等待上游、快取或資料庫查詢後重新驗證。

| 參數            | 值與預設                                                                    |
| --------------- | --------------------------------------------------------------------------- |
| `q`             | 最多 200 個字元、800 UTF-8 bytes，不接受控制字元；預設空字串                |
| `mode`          | `all`（預設）、`0` osu!、`1` taiko、`2` catch、`3` mania                    |
| `status`        | `ranked`（預設）、`any`、`qualified`、`loved`、`pending`、`graveyard`       |
| `sort`          | `ranked_desc`（預設）、`date_desc` 最近更新、`title_asc`、`difficulty_desc` |
| `cursor_string` | 上一頁回覆的 opaque cursor；最多 2048 個可列印 ASCII bytes                  |

不接受未列出的欄位或個人收藏／遊玩篩選。`date_desc` 轉成官方的 `updated_desc`，
不是本站加入日期。URL query encoding 保留 Unicode 搜尋詞和 cursor 裡的 `+`、`/`、`=`。

回覆是 `{items,total,cursor_string}`。每筆有官方 set ID、標題、演出者、圖師、
官方狀態、固定官方 cover／set URL、官方 download-disabled 標記、原始非 convert
的 difficulty 摘要，以及獨立的 `local` 保存數量／封包／withdrawn 摘要。
不回傳上游 token、原始 JSON、儲存路徑或撤下原因。官方狀態與本站 moderation 是不同資料。

## 限流與快取

沿用既有營運者 `OSU_CLIENT_ID`／`OSU_CLIENT_SECRET` 的 `public` client-credentials token。
正式程式固定連到 `https://osu.ppy.sh`，不接受玩家指定上游 URL。
共享至少一秒的官方請求間隔、四個 discovery slots 與 Retry-After/backoff。
搜尋不會在每個按鍵輸入時請求官方。

同一 query/cursor 的同時請求合併。搜尋正向快取最多 16 筆、總共 8 MiB、60 秒；
失敗短暫快取，官方 429 的等待時間不縮短。上游回覆最多 4 MiB，整個上游搜尋有六秒 deadline。
每頁最多 50 個 set、每個 set 最多 512 個原始 difficulty，身份及欄位皆經檢查。

快取只保存官方摘要。每次回覆重新查詢本站保存與撤下狀態，因此撤下圖譜的快取結果
會顯示不可選取。搜尋不建立或修改圖譜、封包、filename alias、成績、audit 或 media。

## 問題處理

| 狀態／code                                                                   | 處理                                         |
| ---------------------------------------------------------------------------- | -------------------------------------------- |
| 400 `invalid_request`                                                        | 確認搜尋詞長度、篩選與 cursor                |
| 401／403                                                                     | 重新登入或請管理員確認匯入權限               |
| 429 `official_resolution_busy`／`official_rate_limited`                      | 等候後再搜尋                                 |
| 503 `official_api_not_configured`                                            | 營運者設定官方 API 憑證                      |
| 503 `official_search_timeout`                                                | 官方搜尋未在期限內完成，稍後重試             |
| 502 `official_search_invalid`／`official_api_failed`／`official_auth_failed` | 上游資料、服務或營運者授權問題；不當成零結果 |

完整 `.osz` 自動下載需要另外的授權或明確設定的封包來源；搜尋成功不代表已下載音樂、
背景或全部難度。現在可使用原有的[完整封包匯入](official-beatmap-archives.md)。
搜尋與完整封包的 metadata 驗證不會宣稱各個音樂／圖片資產由官方逐檔驗證。

## 公開圖譜卡片的難度預覽

`GET /api/v1/beatmaps/{本站 difficulty ID}/set/preview` 提供唯讀完整難度摘要。
必須從已保存、未撤下的本站圖譜進入；任意官方 set ID 不能用這個端點搜尋或匯入。
官方圖譜會取得整組原始 difficulty 的名稱、模式、星數與狀態，因此只保存兩個難度時，
卡片仍能預覽官方的完整清單。本地圖譜則列出本站同組保存的難度。

回覆為 `{native_set_id,source,items,total,stored_count}`；未保存在本站的項目 `id` 為 `null`。
不包含 checksum、儲存路徑或下載 URL，也不新增圖譜或封包。沿用既有官方 metadata
快取、請求合併、限流與撤下檢查；回覆使用 `no-store`。

卡片以 hover、鍵盤聚焦或觸控開啟名稱與星數預覽，項目沒有導覽連結。
完整清單暫時不可用時會明示只顯示已保存的摘要並提供重試；已撤下／找不到的圖譜
不再顯示舊摘要。卡片的主要標題連到圖譜頁，在圖譜頁才選擇要看的難度。

第一手契約：官方 [API 文件](https://osu.ppy.sh/docs/index.html)、
[search 的 public scope 與 response](https://github.com/ppy/osu-web/blob/master/app/Http/Controllers/BeatmapsetsController.php#L348)、
[搜尋參數](https://github.com/ppy/osu-web/blob/master/app/Libraries/Search/BeatmapsetSearchRequestParams.php#L58)
及 [difficulty transformer](https://github.com/ppy/osu-web/blob/master/app/Transformers/BeatmapTransformer.php)。
實作與 fixture 為原創程式；這些參考用於行為契約，沒有複製程式碼。
