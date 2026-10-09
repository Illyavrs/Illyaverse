# 官方圖譜集封包匯入

具有 `beatmaps.import` 權限的帳號可以在網站的圖譜匯入頁提供完整 `.osz`，
並填入官方圖譜集 ID。伺服器向官方查詢一次整組 metadata，核對每張原始
difficulty 的官方 ID、set ID、ruleset 與 MD5，再原子保存原始封包及所有難度。
單一 difficulty ID 匯入仍可使用；它只保存 `.osu`，不會自動取得音訊或背景。

## 操作

1. 使用本站帳號登入，開啟 `/beatmaps/import`。
2. 選擇完整圖譜集匯入，填入官方 set ID，選擇完整 `.osz`。
3. 確認結果列出的所有難度與目前狀態。已存在的官方難度保留原本的審核狀態。
4. 使用結果中的下載連結取得保存的完整封包。圖譜頁可以瀏覽同套件難度；
   stable osu!direct 與 lazer set 下載也使用相同的原始封包 bytes。

每個 `.osu` 必須在封包最上層；音訊、圖片等其他 assets 可以放在子目錄。
單一檔案最多 64 MiB，HTTP body 最多 68 MiB（包含 multipart framing），
難度最多 512 張。既有 ZIP 路徑、符號連結、重複路徑、檔案與解壓總量限制
同樣適用。非完整封包、額外難度、重複難度或不一致的身分不會部分匯入。

需要營運者設定 `OSU_CLIENT_ID`、`OSU_CLIENT_SECRET`。密鑰留在部署環境，
不輸入網站或遊戲。官方請求共用 OAuth token、全域至少一秒的請求間隔、
bounded singleflight、四個 discovery slots、短期失敗快取及 429 Retry-After。
匯入也有 HTTP upload 限流與有限同時處理名額。

## 驗證範圍

官方 metadata 證明 difficulty 身分與 checksum。音訊、圖片、影片及其他
assets 是營運者所提供的原始內容；metadata 無法逐一證明這些 assets 是官方
下載的原物。伺服器保存上傳封包的 SHA-256、長度、完整難度 manifest 及
原始 bytes，不重包、不改寫 `.osu`。這個手動匯入流程不使用 mirror 或官方玩家密碼。

官方 set response 的 `beatmaps` 是原始成員集合；`converts` 不當作額外 `.osu`。
已標示 convert 或 deleted 的 member、畸形 metadata 和 checksum 不一致會拒絕。
新封包使用共用的原始 ZIP 項目與檔名驗證：UTF-8 flag、CP932 fallback、
有效 Unicode Path override 與解碼後的路徑檢查。原始封包不改寫。
畸形編碼的拒絕規則與 stock reader 的差異見[檔名政策](local-map-packages.md#archive-filename-validation)；
實際原生匯入仍待人工驗證。

現有 `public` client-credentials token 可查 metadata。官方完整 `.osz` 下載
另有登入帳號與 privilege 檢查，並重新導向 mirror；本流程讓營運者提供封包，
沒有宣稱現有 token 可以自動下載完整官方封包。網站的官方 metadata 搜尋可帶入匯入表單。
另有預設關閉的[完整封包暫存取得](official-archive-acquisition.md)，必須由營運者設定
固定 HTTPS provider；它不發布圖譜、不指定 uploader，也不更改這個持久匯入契約。

## 重試與衝突

相同 set、相同完整 bytes、相同完整 manifest 重試會回傳既有難度及目前狀態，
`idempotent=true`，不新增建立 audit。任何目前仍有 `beatmaps.import` 的本站
帳號都可以重試這份公開官方內容；最初匯入者及各 difficulty owner 不會改變。
重試驗證實際 retained archive 與各 `.osu`，不只相信資料庫 fingerprint。

既有完全相同的官方難度可以重用；保留 internal/native IDs、owner、狀態與
`.osu` 路徑。相同 MD5 的私服難度、同官方 difficulty ID 的不同 checksum、
同 set 的不同原始封包或不同 member 集合都整包拒絕。現有版本政策是一個
官方 difficulty ID 保存一個 checksum；完整版本歷史與封包更新尚未實作。
發布了 archive 的 set 也不會由單 difficulty discovery 暗中加入新 member。

新圖譜、精確檔名 alias、archive manifest/member 引用及 audit 在同一交易發布。
Publication 重新檢查帳號、session、權限與期限，再依 authority → official set →
sorted filename → checksum/resource 的順序取鎖。明確 rollback 只清除本次擁有的
未引用 staging files；COMMIT 結果不明時保留檔案，避免已提交資料失去內容。
Migration 0018 不為舊 `archive_path` 自動建立已驗證 manifest。

首次發布完整 archive 也在同一交易新增既有 lazer update cursor event，
包括所有 difficulties 原本已快取的情況；精確重試不另發 availability event。

## API

匯入頁也提供[官方圖譜搜尋](official-map-search.md)：依文字、模式與狀態取得官方摘要，
再把選中的 difficulty／set ID 帶入表單。搜尋本身不發布內容，也不取得完整封包資產。

`POST /api/v1/beatmaps/import/archive` 接收兩個 multipart parts：

- `official_set_id`：`1..1_999_999_999` 的整數。
- `file`：有 `.osz` filename 的非空原始檔案。

成功回覆 `items`、`package`、`idempotent`。Package 公開摘要使用 UUID、
canonical `native_set_id`、`kind="osz"`、`legacy=false`、`source="official"`、
`difficulty_count`、internal `beatmap_ids` 和下載 URL；不公開 fingerprint、
檔案路徑、manifest 或 filename aliases。圖譜 detail 的 `package` 與
`download_kind="osz"` 指向這份 archive；公開 package-difficulties route
沿用 bounded pagination 和十二欄 summary。

| 狀態／code | 意義 |
|---|---|
| 400 `official_archive_invalid` | 檔案、multipart 或封包結構不合規則 |
| 413 `official_archive_too_large` | 超過檔案或 HTTP body 限制 |
| 404 `beatmapset_not_found` | 官方明確找不到 set |
| 409 `official_archive_manifest_mismatch` | 封包缺少、增加或不符合官方 difficulty 集合 |
| 409 `official_archive_conflict` | 既有內容、來源或 retained bytes 不一致 |
| 409 `official_version_conflict` | 相同官方 difficulty ID 已有不同 checksum |
| 409 `official_set_version_conflict` | 已發布 set 的封包或 member 集合不同 |
| 429 `official_rate_limited` | 官方限流；稍後重試 |
| 503 `official_api_not_configured` | 缺少營運者 API 設定 |
| 502 `official_identity_mismatch`／`official_api_failed`／`official_auth_failed` | 官方資料、服務或授權失敗 |

401／403 是本站帳號或權限問題。這些錯誤不會假造 Ranked，也不會把上游故障
當成不存在。Gateway 對 set-version 衝突回覆 `beatmap_set_version_conflict`。

原生遊戲下載、匯入、遊玩與送分仍須人工驗證；隔離 metadata／HTTP／資料庫
測試不是 native gameplay 證據。[驗證紀錄](verification.md)保留各 checkpoint。

第一手參考：官方 [API 使用與認證](https://osu.ppy.sh/docs/index.html)、
[set show／download controller](https://github.com/ppy/osu-web/blob/75829ba32e0d9e9ed199dd459bb2b123a3a5d23f/app/Http/Controllers/BeatmapsetsController.php)、
[difficulty transformer](https://github.com/ppy/osu-web/blob/75829ba32e0d9e9ed199dd459bb2b123a3a5d23f/app/Transformers/BeatmapTransformer.php)、
[stock set lookup](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Online/API/Requests/GetBeatmapSetRequest.cs)、
[stock set download](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Online/API/Requests/DownloadBeatmapSetRequest.cs)
與 [stock importer](https://github.com/ppy/osu/blob/d98369a4d17039f8abd6308f173ada1e26e22d5f/osu.Game/Beatmaps/BeatmapImporter.cs)。
伺服器、UI 與 fixture 為原創實作，沒有複製參考專案程式碼。
