# 原生成績提交

Stable 的 `/web/osu-submit-modular-selector.php` 接收兩個 `score` multipart 項目：加密文字 envelope，以及帶 `filename` 參數的壓縮 replay。Resonance 解密後驗證帳號、登入 session、客戶端版本與 hash、圖譜 checksum 和成績 checksum，通過後由 Sanctuary 計算 PP 並保存成績。Passed 成績另外驗證 LZMA frames，使用伺服器產生的檔名保存 `.osr`；客戶端提供的 filename 不會成為檔案路徑。

## Multipart 相容性與限制

Content-Disposition 的 `name`、`filename` 參數名稱以 ASCII 大小寫不敏感方式解析，參數值保留大小寫。支援 quoted values、escaped characters、未加引號的值與 quoted value 中的分號；未知參數不參與欄位分類。同一個 header 內重複的 name 或 filename（包括不同大小寫）、不完整引號、control characters 與超出限制的內容回覆 400 `invalid_score_disposition`。Replay 不依賴 Content-Type；帶 filename 的 `score` 附件使用 replay 限制。

限制為一般欄位 32 KiB、壓縮 replay 16 MiB、所有欄位內容合計 20 MiB、單一 disposition header 8 KiB、解碼後欄位名稱 128 bytes。HTTP request body 上限為 20 MiB（包含 multipart framing），內容須留出標頭與 boundary 空間。沒有 name 的 filename 附件不會被猜測為 replay，也不會放寬加密文字欄位限制。

Multipart 開始讀取與欄位內容讀取失敗時，保留 Axum 的 HTTP 狀態：框架 request-body 上限回覆 413 `score_upload_too_large`，畸形或截斷內容回覆 400 `invalid_score_multipart`，其他底層讀取失敗保留框架的 500。回應使用固定 error/message，不輸出解析器原始錯誤。這依循 [Axum MultipartError.status()](https://docs.rs/axum/latest/axum/extract/multipart/struct.MultipartError.html#method.status)；已固定的依賴版本為 0.8.9。這項狀態保留與先前第 14 個未知欄位的 scalar 413 是不同情況，不能視為真人送分問題已修好。

Multer 3.1 的內建參數辨識區分大小寫，會漏認 `Name`／`FileName`；Resonance 使用原創、有限制的 header resolver 處理這個差異。Multer 會在把 HeaderMap 提供給 handler 前合併重複 header lines，目前無法聲稱已拒絕所有原始 wire-level 重複 Content-Disposition lines。RFC 的大小寫規則參考 [RFC 2045 §5.1](https://www.rfc-editor.org/rfc/rfc2045#section-5.1)；未複製第三方解析器程式碼。

## 診斷與確認送分

新增的 multipart 拒絕事件只記錄固定欄位分類、標頭類型分類、參數存在與大小寫 booleans、byte counts、限制與 stage；送分拒絕事件只記錄固定 error code 和 HTTP status。這兩種事件不記錄欄位原始名稱、filename、header values、密碼、hash 值、decrypted envelope 或 replay 內容。

`parser_name_present`、`parser_filename_present` 分別表示 Multer 提供的 name／filename；既有 `name_present`、`filename_present` 保留相同 parser 意義。Header resolver 成功後，才記錄 `resolved_name_present`、`resolved_filename_present` 和 `parser_name_matches_resolved`。空字串參數仍算存在；比對保留 name 值大小寫，並區分不存在與空字串。Resolver 拒絕不合法 header 時不記錄這三個 booleans，避免將無法解析誤認為合法但沒有參數。這些欄位可分辨內建 parser 漏認不同大小寫的參數名稱，或誤認 quoted value 內的參數形狀字串；不會輸出該字串或調整分類與大小限制。

2026-10-07 本機人工作業確認 stable 有送出 POST，但收到 413，資料庫仍沒有成績。診斷顯示第 14 個項目沒有被內建 parser 辨識出 name／filename，超過 32 KiB 被拒絕。不同大小寫的附件參數會造成相同失敗，已由合成 fixture 重現並修正；該次真實客戶端的完整標頭分類與修正後送分仍待人工作業確認，不能將隔離通過記為 native client pass。

同日 13:58–13:59 UTC 的真人重試仍收到 413：第 14 個項目有五 bytes 的 parser name，但 resolver 的固定分類仍為 unknown；沒有 filename 參數或 Content-Type，內容約 33–64 KiB。因此先前的大小寫附件修正不能視為這次原生格式已修復。追加的安全診斷分別記錄 parser／resolver 固定分類、resolver name 長度，以及是否為 score 的 ASCII 大小寫變體；仍不記錄原始欄位名稱或 payload。目前尚待更新後的真人請求來辨識差異，未放寬未知文字欄位上限。

測試服務更新後，登入 stable 並完成一張 Ranked／Loved 圖譜，或等待待送成績重試。網站自己的成績紀錄應出現成績與 replay；成績先保存為 pending review，管理員審核前不會加入公開排行榜。若仍失敗，提供發生時間即可，先使用上述安全診斷定位，不要貼密碼或 request body。圖譜辨識是另一項檢查，見[官方圖譜辨識](native-beatmap-discovery.md)。

獨立 Python Rijndael fixture 在 Docker 最終映像測試中送出超過 32 KiB 的合法 LZMA replay，涵蓋標準參數、不同大小寫參數、無 MIME 的 attachment、相同成績重試去重、replay round trip、錯誤密碼與 checksum 拒絕。來源 gate、映像 ID 和部署 checkpoint 見[驗證紀錄](verification.md)。
