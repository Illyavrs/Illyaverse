# 本站有效狀態目錄

這是有限的本站目錄：保存的圖譜、社群上傳，以及有本站 difficulty 決定且已發布完整公開 metadata 的官方 set。官方／網站／lazer 的預設搜尋仍使用官方來源。官方 Ranked 篩選維持上游成員；本站核准官方 Pending 或 Graveyard 後，使用本站目錄才能按本站有效狀態發現它。

網站使用 `GET /api/v1/server/beatmapsets`。Lazer `GET /api/v2/beatmapsets/search` 使用 `source=local`，或未指定 source 且 `q` 以保留的 `server:` 開頭。前綴後的文字是搜尋詞，例如 `server: 星`；`server:` 空詞可列出目錄。`source=official` 明確保留官方搜尋詞。這個選擇使用 stock 原有的 `q`，不需要修改客戶端；實際遊戲畫面仍須真人驗證。

## 成員與狀態

文字、模式、狀態與 uploader 條件必須由同一張原始 difficulty 同時符合，才令其 set 入選；回覆包含全部 siblings，不把其他 difficulty 一併改 Ranked。狀態按目前 override、已保存狀態、驗證的官方狀態順序決定。撤下 set 全部省略。保存內容與 metadata 使用 canonical native ID 去重，只有 checksum 相同的實際本站內容才附本站 ID。版本衝突回覆 409。

`ranked` 只匹配有效 Ranked，`leaderboard` 匹配 Ranked／Loved。原生 `pending` 包含 Pending／WIP／本站 unranked，`qualified` 可搜尋但目前 Qualified 映射 unranked，因此不會宣稱為 Qualified。原生 JSON 的 unranked 使用 Pending／rank 0。網站保留 unranked；全組同狀態用共用摘要，混合組摘要用 Pending，並保留逐張有效狀態及 `official_status`。

網站回覆既有 `{items,total,page,page_size}`，頁數由實際 total／page_size 計算；每項為 `{kind:"stored",stored:DiscoverySet}` 或 `{kind:"official_metadata",official:OfficialBeatmapset}`。後者有必要的 `metadata_only`、原始 `official_status`、所有 difficulties 的有效狀態；保存部分內容時仍使用這個完整分支。`local_id` 只在真正保存時出現。Metadata-only 的 `local.stored_count` 為 0 且本站整包不可下載，不生成虛構的本機 leaderboard 或 ID。

`stored` 分支包含全部實際保存的 siblings；較舊的部分官方匯入若尚未發布完整 snapshot，不會假造未知 originals。一般官方搜尋／完整 set lookup 仍可取得上游完整清單，staff 也可對已知保存的官方 set 明確 refresh。所有新官方 difficulty 決定會發布完整 snapshot，因此不會產生這種新的部分排名投影。

## 篩選與分頁

網站支援 `q`、`mode=0..3`、`status`、`source=official|local`、`uploaded_by`、`uploaded_only`、`page`、`page_size`（最多 50）。`uploaded_only` 必須是真正有 uploader 的本站高區段社群套件；metadata-only 官方結果不會冒充玩家上傳。排序為 `recent`、`title`、`stars`、`length`，並接受下列原生排序。`recent` 是本站加入日期；`updated` 使用驗證的原始更新日期或本站內容建立日期。

原生支援 `q`、`m`、`s`、`sort`、`nsfw`、`cursor_string` 或單一 `cursor[illya]`。排序支援 `ranked`、`updated`／`date`、`title`、`artist`、`difficulty`、`relevance` 的 `_asc`／`_desc`。Relevance 是原創、公開的文字權重：標題完全符合 8／包含 4，artist 完全符合 4／包含 2，version 包含 1，取符合 difficulty 的最高分。全部排序用 canonical set ID 決定同分順序。

目前非零 `g`／`l`、非空 `c`／`e`、個人化篩選、plays／favourites／rating／nominations 排序回覆 400 `unsupported_server_search_filter`。Stock 初始 UI 常帶 Featured Artists，使用 `server:` 時須清除該條件；支援清單沒有宣稱这些資料已建索引。

`total` 是同一 SQL snapshot 的實際 distinct set 數，不先抓一頁官方 `s=any` 再篩，也不合成上游 totals。原生每頁最多 50，回覆原有 `beatmapsets`／`total`／`cursor`／`cursor_string` 加內部 `catalogue_revision`。Cursor 綁定完整條件及 catalogue epoch；metadata、內容、override、撤下、package 或 archive 改變會使 continuation 回覆 409 `server_catalogue_changed`，重新從第一頁搜尋。Gateway 在回覆前用同一 SQL snapshot 再驗 epoch、有效狀態與撤下，避免已變動的結果維持舊 total。網站每頁是新的查詢，因此跨頁期間目錄改變仍可能移動頁界。

## 發布與舊資料修復

Migration 0027 只新增表及失效 epoch，不連網、不猜測舊 metadata。官方核准與直接 difficulty 決定取得既有受限快取中的完整原始 metadata，核對完整欄位、checksum 與 canonical IDs，在同一 authority／set-fence transaction 發布 snapshot、override、request decision 與 audit。Metadata 驗證／保存／commit 失敗時整筆決定回滾；不只發布角色或 audit。普通搜尋保持唯讀。

舊 override 可能只有 private nomination manifest；它不足以生成完整公開 metadata。具有 `beatmaps.moderate` 權限的 staff 可讀 `GET /api/v1/admin/beatmapsets/catalogue-metadata-pending?page=1&page_size=50`，逐組執行 `POST /api/v1/admin/beatmapsets/{id}/catalogue-metadata`，body 為 `{"reason":"確認完整公開 metadata"}`。清單只含 public set ID／override count，省略撤下 set。Refresh 使用現有 OAuth／shared cache，重新驗 staff 權限及 active set，逐組 audit；沒有背景無界掃描。既有內容／已核准 manifest／先前公開 snapshot 的 checksum 衝突不能靠普通 refresh 靜默重綁。舊手動 metadata-only override 若沒有任何歷史 checksum 證據，普通 refresh 明確回覆 409。

Staff 查看目前完整 metadata 及 difficulty revisions 後，可在同一 refresh body 提供明確的 `reaffirm_difficulties`，例如 `[{"native_id":123,"checksum":"0123456789abcdef0123456789abcdef","expected_revision":0,"expected_status_revision":null}]`。最多 512 個不同 difficulty；checksum 必須符合目前完整 snapshot，override revision 與已保存 status revision 必須一致（未保存時明確 `null`）。這是以新 audit 重新確認既有狀態；沒有宣稱找回舊版證據。全部未證實 siblings 可同一交易重新確認，任一 stale revision／checksum／persistence failure 都回滾整組。已保存的官方完整 archive 若與新 metadata 的完整 checksum manifest 不同，也拒絕發布，避免舊封包冒充新增 siblings。

搜尋共四個工作槽，公開最多兩個；公開 IP 每分鐘 30 次，主要工作六秒，Gateway 八秒，每頁輸出最多 4 MiB。搜尋不連官方、不匯入檔案、不建立 score／alias／archive。Metadata 發布／repair 共用既有官方 request pacing、cache、429 backoff 與大小上限；私人 nomination 不會被傳給玩家。現有讀取快取不保存有效狀態，狀態與撤下每次讀取資料庫。

## 驗證界線

契約採用 pinned stock 原有 [q/filter 請求](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/API/Requests/SearchBeatmapSetsRequest.cs)、[extension-data cursor](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Extensions/WebRequestExtensions.cs) 與 [cursor query 序列化](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Extensions/WebRequestExtensions.cs)，實作與 fixture 均為原創。隔離 PostgreSQL／HTTP fixture 分別驗證 membership、同張 difficulty 條件、完整 siblings、部分匯入、精確分頁、withdrawal／revocation、跨服務 epoch 與 bearer 撤銷。這個來源分支基於 1..25 加 catalogue migration 0027，尚未合併 PM migration 0026；其 fixture 不是完整連續 ledger 的 25→27 升級證據。Source／隔離測試／部署／真人 stock client 是不同狀態；schema25 的舊 passed 收據不能證明新實作已部署。
