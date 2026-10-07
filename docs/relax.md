# Relax（RX）

Illyaverse 將 RX 視為獨立的成績分區。遊戲仍使用 osu!、taiko、catch、mania 的原始 ruleset 編號 `0..3`；伺服器從已驗證的 mods 判定 `score_variant=vanilla|relax`，不接受玩家自行指定提交分區。

RX 支援 osu!、taiko 與 catch。mania RX 提交會遭拒絕，網站也停用此組合。AP、AT 與 DA 維持不計入排名 pp 的政策。所有新成績仍需通過原有管理員審核，才會進入榜單。

## 計算與顯示

pp 使用鎖定的 rosu-pp 4 與完整判定資料，保留 RX、速度與其他受支援的 mod 設定。沒有採用其他私服的自製 pp 公式。catch 使用該版本函式庫提供的計算行為；這不是官方對 RX 排名資格的承諾。

一般／RX 的加權 pp、全球與國家排名、遊玩次數、準確率和等級計數依 ruleset 分開。RX 譜面榜按 pp 排序，每位玩家保留一筆最佳成績。RX 的原始分數保留；共同比較分數目前標示為不可換算，不捏造一般分數或累積分數。

網站排行榜、玩家頁與譜面頁以 `mode` 和 `variant` 網址參數保存選擇。成績及 replay 仍綁定原始 ruleset 和實際 mods；RX 標籤不會改變 replay 格式。

## API

| 用途 | 路徑／選擇 |
| --- | --- |
| 網站排名 | `GET /api/v1/leaderboard?mode=0&variant=relax` |
| 玩家統計 | `GET /api/v1/users/{id}?mode=0&variant=relax` |
| 玩家成績 | `GET /api/v1/scores?user_id={id}&mode=0&variant=relax` |
| 譜面最佳成績 | `GET /api/v1/beatmaps/{id}/scores?mode=0&variant=relax` |

省略分區的排名與個人統計使用一般模式。沒有玩家／譜面／模式篩選的全站近期成績可以包含多個分區，每筆回應都有伺服器判定的 `score_variant`。pending、failed、restricted 或不具資格的成績不會被算入 RX 榜單。

stable 依目前 action 的 RX mod 讀取對應統計；lazer API 保留原始 ruleset 路徑並接受分區查詢。原生遊戲的實際顯示、切換與遊玩仍需要玩家驗證，隔離 API 測試不能代替這項檢查。

## 原生客戶端測試

使用 [Kaleidoscope 測試包](kaleidoscope-launcher.md)與網站建立的 Illyaverse 帳號：

1. 先在 osu! 模式完成一筆 Normal 成績，再開 RX 完成同一張譜面，確認都能提交。新成績會等待管理員審核，審核前不會進榜。
2. 審核後，在網站玩家頁、排行榜與譜面榜切換 Normal／RX；核對成績、pp、準確率與遊玩次數分區，RX 譜面榜按 pp 排序。
3. 在 stable 切換 RX，或在 lazer 選擇／取消 RX，確認個人統計對應分區。lazer 已經開啟的玩家頁／排名視窗是否即時刷新，仍是需要回報的項目。
4. 開啟兩筆 replay，確認保留原來的模式與 RX mod；再分別測試 taiko、catch。mania 的 RX 應無法提交。
5. 正常關閉 lazer，確認私服資料目錄的 `rulesets` 中，本次放入的 Transfiguration DLL 已移除。若未完成，重新開啟 Kaleidoscope 檢查錯誤並重試清理。

## 舊資料與管理員重算

migration 0014 將既有 RX 成績分類到 `relax`，但不自動將原先 `pp_eligible=false` 的成績改為有資格。舊資料未記錄排除原因，無法區分舊 RX 政策和管理員的排除決定。歷史 mania RX 保留資料但不取得 pp 資格。

具有 `scores.moderate` 權限的管理員可呼叫 `POST /api/v1/admin/scores/recalculate-relax`，提供 `{"score_ids":[123,456]}`。每次限 1–16 個不重複 ID。服務會鎖定並重新驗證成績、mods、原始譜面與判定，重新計算 pp 並記錄原值、新值和計算版本。此操作不改變審核狀態、原始分數或 replay；任一成績無法安全重算時整批不提交。這不是開機時自動執行的恢復工作。
