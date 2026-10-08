# 原生成績提交

Stable 的 `/web/osu-submit-modular-selector.php` 接收兩個 `score` multipart 項目：加密文字 envelope，以及帶 `filename` 參數的壓縮 replay。Resonance 解密後驗證帳號、登入 session、客戶端版本與 hash、圖譜 checksum 和成績 checksum，通過後由 Sanctuary 計算 PP 並保存成績。Passed 成績另外驗證 LZMA frames，使用伺服器產生的檔名保存 `.osr`；客戶端提供的 filename 不會成為檔案路徑。

## Multipart 相容性與限制

Content-Disposition 的 `name`、`filename` 參數名稱以 ASCII 大小寫不敏感方式解析，參數值保留大小寫。支援 quoted values、escaped characters、未加引號的值與 quoted value 中的分號；未知參數不參與欄位分類。同一個 header 內重複的 name 或 filename（包括不同大小寫）、不完整引號、control characters 與超出限制的內容回覆 400 `invalid_score_disposition`。Replay 不依賴 Content-Type；帶 filename 的 `score` 附件使用 replay 限制。

限制為已知文字參數與加密 envelope 32 KiB、壓縮 replay 16 MiB、所有欄位內容合計 20 MiB、單一 disposition header 8 KiB、解碼後欄位名稱 128 bytes。HTTP request body 上限為 20 MiB（包含 multipart framing），內容須留出標頭與 boundary 空間。沒有 name 的 filename 附件不會被猜測為 replay，也不會放寬加密文字欄位限制。

有效 `form-data` 中，非空 name、沒有 filename，而且 name 不是已知參數或其 ASCII 大小寫變體的普通附加欄位，逐塊讀完後丟棄。這些資料不轉成 UTF-8、不保存、不成為 envelope／Replay；每個 byte 仍計入原有 20 MiB 總量。未知附件、空名稱、其他 disposition kind 及已知名稱大小寫變體維持原先的嚴格路徑。這項處理不需要猜測未知欄位的名稱或用途；伺服器只使用固定的成績參數。參考協定欄位與雙 `score` 行為可見 [bancho.py 的原生提交入口](https://github.com/osuAkatsuki/bancho.py/blob/master/app/api/domains/osu.py)，實作為原創 Rust streaming reader。

Multipart 開始讀取與欄位內容讀取失敗時，保留 Axum 的 HTTP 狀態：框架 request-body 上限回覆 413 `score_upload_too_large`，畸形或截斷內容回覆 400 `invalid_score_multipart`，其他底層讀取失敗保留框架的 500。回應使用固定 error/message，不輸出解析器原始錯誤。這依循 [Axum MultipartError.status()](https://docs.rs/axum/latest/axum/extract/multipart/struct.MultipartError.html#method.status)；已固定的依賴版本為 0.8.9。這項狀態保留與先前第 14 個未知欄位的 scalar 413 是不同情況，不能視為真人送分問題已修好。

Multer 3.1 的內建參數辨識區分大小寫，會漏認 `Name`／`FileName`；Resonance 使用原創、有限制的 header resolver 處理這個差異。Multer 會在把 HeaderMap 提供給 handler 前合併重複 header lines，目前無法聲稱已拒絕所有原始 wire-level 重複 Content-Disposition lines。RFC 的大小寫規則參考 [RFC 2045 §5.1](https://www.rfc-editor.org/rfc/rfc2045#section-5.1)；未複製第三方解析器程式碼。

## 診斷與確認送分

新增的 multipart 拒絕事件只記錄固定欄位分類、標頭類型分類、參數存在與大小寫 booleans、byte counts、限制與 stage；送分拒絕事件只記錄固定 error code 和 HTTP status。這兩種事件不記錄欄位原始名稱、filename、header values、密碼、hash 值、decrypted envelope 或 replay 內容。

`parser_name_present`、`parser_filename_present` 分別表示 Multer 提供的 name／filename；既有 `name_present`、`filename_present` 保留相同 parser 意義。Header resolver 成功後，才記錄 `resolved_name_present`、`resolved_filename_present` 和 `parser_name_matches_resolved`。空字串參數仍算存在；比對保留 name 值大小寫，並區分不存在與空字串。Resolver 拒絕不合法 header 時不記錄這三個 booleans，避免將無法解析誤認為合法但沒有參數。這些欄位可分辨內建 parser 漏認不同大小寫的參數名稱，或誤認 quoted value 內的參數形狀字串；不會輸出該字串或調整分類與大小限制。

2026-10-07 本機人工作業確認 stable 有送出 POST，但收到 413，資料庫仍沒有成績。診斷顯示第 14 個項目沒有被內建 parser 辨識出 name／filename，超過 32 KiB 被拒絕。不同大小寫的附件參數會造成相同失敗，已由合成 fixture 重現並修正；該次真實客戶端的完整標頭分類與修正後送分仍待人工作業確認，不能將隔離通過記為 native client pass。

同日 13:58–13:59 UTC 的真人重試仍收到 413：第 14 個項目有五 bytes 的 parser name，但 resolver 的固定分類仍為 unknown；沒有 filename 參數或 Content-Type，內容約 33–64 KiB。因此先前的大小寫附件修正不能視為這次原生格式已修復。追加的安全診斷分別記錄 parser／resolver 固定分類、resolver name 長度，以及是否為 score 的 ASCII 大小寫變體；仍不記錄原始欄位名稱或 payload。

2026-10-08 07:01–09:15 UTC（台灣 15:01–17:15）的十四次真人 POST，均有相同的第十四個普通附加欄位拒絕：parser 與 resolver 都有五 bytes 名稱、兩者一致、不是 score 大小寫變體、`form-data` 且沒有 filename／MIME，內容為 33,901–80,486 bytes。這排除參數大小寫／parser 誤辨識；未知欄位的實際語意仍不作猜測。修正改為上述通用附加欄位排空，不再為伺服器不使用的資料分配 buffer 或套用已知文字參數限制。25 個 multipart tests 通過，包含實際觀察到的結構、opaque bytes、前後順序、已知參數與 Replay 保留、全部已知名稱大小寫變體拒絕及總量 exact／+1 byte。超過 32 KiB 的已丟棄欄位僅記錄固定 unknown 分類、index、bytes 及 envelope／Replay 是否已存在，不輸出名稱或內容。最終加密協定測試與部署後真人成功仍分開記錄。

測試服務更新後，登入 stable 並完成一張 Ranked／Loved 圖譜，或等待待送成績重試。網站自己的成績紀錄應出現成績與 replay；成績先保存為 pending review，管理員審核前不會加入公開排行榜。若仍失敗，提供發生時間即可，先使用上述安全診斷定位，不要貼密碼或 request body。圖譜辨識是另一項檢查，見[官方圖譜辨識](native-beatmap-discovery.md)。

獨立 Python Rijndael fixture 在 Docker 最終映像測試中送出超過 32 KiB 的合法 LZMA replay，涵蓋標準參數、不同大小寫參數、無 MIME 的 attachment、相同成績重試去重、replay round trip、錯誤密碼與 checksum 拒絕。來源 gate、映像 ID 和部署 checkpoint 見[驗證紀錄](verification.md)。

附加欄位修正另完成一輪 461 次 HTTP／56 類最終映像協定測試，其中新的第十四項 opaque 附加資料仍取得原有 score receipt、Replay bytes；錯誤密碼與 checksum 依然拒絕。兩套本機 Resonance 已在 2026-10-08 台灣 17:36 更新，備份與 schema 22 核對通過。實機修正後送分成功仍待確認；確切來源、映像與部署範圍見 [修正證據](stable-ancillary-evidence.json)。

同日 09:38:19／27／42 UTC 的三次真人重試已通過 multipart 與解密，改於 `StableSubmission::parse` 回覆 400 `invalid_score_envelope`；資料庫仍為零成績／Replay。這確認前述 413 阻擋已清除，但不代表完整送分成功。下一項是取得不含值的固定 parser failure category，以區分欄位數、版本、時間及其他 envelope 格式檢查。

成績最後一欄可包含版本與尾端 ASCII 空格旗標；[bancho.py 的提交資料模型](https://github.com/osuAkatsuki/bancho.py/blob/master/app/objects/score.py) 記錄了此格式。[旗標定義](https://github.com/osuAkatsuki/bancho.py/blob/master/app/constants/clientflags.py) 包含 128／256／512，因此含這些尾端空格的正常版本會被原先「整欄最多 128 bytes」誤拒絕。原創 Rust parser 現在先移除這個 ASCII 空格後綴，再套用原有版本長度限制與正常化；完整 envelope 的 16 KiB 上限保留，非空格 padding 未放寬。這是由 wire 格式支持的相容性修正，尚未證明是上述真人 400 的實際原因。

Envelope 拒絕診斷只輸出白名單固定 `parse_reason`、欄位數、附加欄位數、總 bytes，以及欄位數至少為 18 時 canonical 第 18 欄的原始／正常化長度和尾端 ASCII 空格數。公開回應維持 400 `invalid_score_envelope`。不輸出 parser error 原始字串、解密內容、版本值、玩家名稱或任何成績欄位值。

此版本完成 7 個核心 submission tests、原有 25 個 multipart tests、新的診斷隱私測試及 Clippy；新的隱私 fixture 一度誤把合法空格後綴當成超長版本，修正 fixture 後單獨通過，未重跑已通過的 reader tests。最終映像另通過一輪 465 HTTP／57 類隔離協定檢查，包含獨立加密 128／256／512 空格後綴重試、原 receipt／Replay 一致及超長版本拒絕。兩套本機 Resonance 已在 2026-10-08 台灣 18:00 更新，schema 22、其他八個容器與原有資料 ID 保留；更新時仍無原生成績／Replay。新的 source、映像、備份及驗證範圍見 [版本旗標修正證據](stable-version-flags-evidence.json)。真人成功依然需要對應請求與持久化證據。

2026-10-08 13:53:40–13:55:32 UTC（台灣 21:53–21:55）的五次真人提交，均於欄位數檢查回覆 400：`parse_reason=field_count`、19 欄、143 bytes，資料庫仍為零成績／Replay。這確定「剛好 18 欄」的檢查是當次阻擋點；先前的版本旗標修正不能當成這次已成功。同期讀取排行榜的八個請求為七次 200、一次 499，但 HTTP 狀態本身不證明原生 UI 解析成功。

[Sunrise 的固定欄位解析](https://github.com/SunriseCommunity/Sunrise/blob/f44fa316152a5b93e32cba6b4d48ca98535a2918/Sunrise.Shared/Extensions/Scores/ScoreExtensions.cs#L139) 接受至少 18 欄並讀取索引 0–17；[bancho.py 的資料模型](https://github.com/osuAkatsuki/bancho.py/blob/f10c03a3969c1b48a0cf9a423384653633c52fba/app/objects/score.py#L179) 也以固定索引讀取 canonical 資料。Resonance 以原創 `splitn(19, ':')` 實作有界前段解析：前 18 欄仍依原規則驗證，後續 opaque 附加內容保留為單一未使用 slice，整體 16 KiB 上限不變。附加內容不成為帳號、checksum、版本、PP、Replay 或審核依據。這不推定真人第 19 欄的具體語意；伺服器只使用協定中已知且經驗證的欄位。

有界前段修正完成 10 個核心 tests、更新後的診斷隱私測試與 Clippy；獨立 Python fixture 以 19 欄首次保存成績及 Replay，之後以 18 欄、空／decimal／opaque／多欄附加內容重試取得相同 receipt，17 欄截斷、錯誤 checksum／密碼仍被拒絕。最終映像通過一輪 472 HTTP／58 類隔離協定 gate，兩套本機 Resonance 在 2026-10-08 台灣 22:07 更新，十個容器健康、schema 22 與原有資料 ID 保留。來源、參考 revision、備份和原生觀察範圍見 [前段解析證據](stable-prefix-evidence.json)；隔離 fixture 成功仍不等同真人成功。
