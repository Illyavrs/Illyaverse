# 排名歷史

個人頁的排名歷史記錄全站排名的實際觀測。可切換全球／國家排名與 30、90、180 天，並沿用個人頁選擇的遊戲模式和 Vanilla／RX。圖表以 UTC 日期排列，較好的名次在上方；每日紀錄表列出觀測時間、pp、全球排名、國家排名和當時的國家。

Sanctuary 啟動時與每分鐘檢查一次是否需要記錄，同一天的成功觀測至少相隔 30 分鐘。正常情況約每 30–31 分鐘完成一次，新的 UTC 日期可立即記錄。每次涵蓋所有未受限制玩家，以及 Vanilla 四個模式和 RX 三個模式；osu!mania 不支援 RX。排名使用網站目前的同一份查詢：已驗證、通關、可計入 pp 且圖譜仍為 Ranked／可用的成績，每張圖取最高 pp，再依 0.95 遞減加權。相同 pp 依玩家 ID 排序。pp 為零時，公開名次為空值。

每個 UTC 日期只保留當天最後一次成功觀測；過去日期不重算。不會把舊成績時間當成過去排名，也不會補造停機期間的資料。缺漏日期、沒有名次的日期，以及國家排名中途更換國家，都會中斷連線。剛開始使用的伺服器需要時間累積資料。當天的最新紀錄與即時排名可能相差約一個正常收集週期；收集失敗、停機或大量資料處理時可能更久。

排名與歷史採公開個人頁的存取規則：受限制或不存在的帳號回傳 404，包括管理員讀取。歷史保留當時的國家資訊；介面不會把它當成現在的國家。切換模式或 RX 後不顯示上一個分區的資料；載入失敗可重試。圖表支援滑鼠／觸控，以及方向鍵、Home、End 選取觀測，也提供可展開的資料表。

`GET /api/v1/users/{id}/rank-history?mode=0&variant=vanilla&days=90` 無須登入。`mode`、`variant` 遵循現有分區規則；`days` 接受 1–180，預設 90。回傳：

```json
{
  "mode": 0,
  "variant": "vanilla",
  "days": 90,
  "start_date": "YYYY-MM-DD",
  "end_date": "YYYY-MM-DD",
  "time_zone": "UTC",
  "collection_interval_seconds": 1800,
  "last_captured_at": null,
  "points": []
}
```

`points` 只包含實際紀錄，依日期升序，欄位為 `date`、`captured_at`、`pp`、`rank`、`country_rank`、`country`。空陣列表示該範圍尚無這位玩家的紀錄。資料庫以交易及跨副本鎖保證七個分區一同完成；失敗時不留下部分觀測。重啟或多副本在同一收集週期內不會重複寫入。

遷移 `0022_rank_history.sql` 只建立儲存結構，不回填歷史。升級時需要一起更新 Sanctuary 和 Resonance，因為兩者都內嵌完整遷移。原有帳號、圖譜、成績與公告保留。此功能不代表 native stable／lazer gameplay 或送分已通過實際客戶端驗證；進度見[驗證說明](../apps/docs/content/docs/reference/verification.md)。
