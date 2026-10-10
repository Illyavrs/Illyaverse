# 圖譜搜尋與難度切換

目錄以伺服器實際收錄的圖譜組顯示，每組可以包含多個難度。搜尋會比對曲名、歌手、mapper 和難度名稱；模式、狀態、來源與上傳者篩選會先選出符合條件的難度，再計算圖譜組數量與分頁。同名歌曲不一定是同一組，同一組符合篩選的難度數量也可能少於它的已收錄數量。

可以切換網格／列表，依最近加入、曲名、星數或長度排序。「最近加入」是伺服器收錄時間，不是官方提交或 Ranked 日期。搜尋、篩選、排序、檢視方式與頁碼保留在網址中，可分享或使用瀏覽器上一頁／下一頁返回。清除上傳者條件會保留其他條件。搜尋最長為 100 個 UTF-8 bytes。

每組顯示模式、實際狀態、星數與長度範圍、符合條件及已收錄的難度數量，並提供最多八個符合條件的難度入口。難度顏色同時搭配文字與星數。詳情頁以精簡的難度入口切換，完整難度資料可另行展開；兩者使用相同分頁，可查看該組實際收錄的所有難度。它不代表官方完整難度清單，也不會在瀏覽時自動抓取官方 API。切換難度會保留可用的 Vanilla／RX 選擇；mania 只使用 Vanilla。返回目錄會保留先前的條件。

官方封面由 `https://assets.ppy.sh/beatmaps/{native_set_id}/covers/cover.jpg` 在瀏覽器載入；網址只使用有效的官方圖譜組 ID。圖片載入失敗或本地圖譜沒有封面時使用原有風格的圖示。封面是圖譜內容，網站預設背景仍是純黑。沒有使用 AI 生圖。

下載按鈕依伺服器實際保存的檔案顯示：`.osz` 是已保存的圖譜包，`.osu` 是單一難度。只有單一 `.osu` 時不會宣稱有音樂或圖片等其他檔案。已下架的整組圖譜不會出現在公開目錄，公開難度查詢回傳 410；管理員登入也不會繞過此限制。原有管理頁和權限規則保留。

公開 API：

- `GET /api/v1/beatmapsets`：支援 `q`、`mode=0..3`、`status=ranked|loved|pending|unranked`、`source=official|local`、正整數 `uploaded_by`、`sort=recent|title|stars|length`、`page` 和 `page_size`。分頁以 canonical `native_set_id` 分組，回傳 `items`、圖譜組 `total`、`page`、`page_size`。即使超出最後一頁，`total` 仍是符合條件的圖譜組數量。
- 每項包含 `native_set_id`、既有公開圖譜 DTO `representative`、`matching_count`、`stored_count`、`matching_modes`、`matching_statuses`、`stars_min/max`、`length_min/max`、`added_at` 和最多八項 `preview`。狀態與模式描述符合篩選的難度，不會把混合狀態合成一個組狀態。
- `GET /api/v1/beatmaps/{id}/set/difficulties?page=1&page_size=24`：以該難度所屬的 canonical 圖譜組查詢已收錄且可用的難度，與是否有 `.osz` 無關。依模式、星數、ID 排序，回傳既有精簡難度 DTO 與實際成員數量。

兩項查詢不新增資料庫遷移、不修改圖譜、權限或官方 rate limit；資料和計數在同一個查詢快照中讀取。既有 `/beatmaps` 單一難度 API 與圖譜包 API 保持相容。Native stable／lazer 遊戲與送分驗證仍依[驗證說明](../apps/docs/content/docs/reference/verification.md)處理。
