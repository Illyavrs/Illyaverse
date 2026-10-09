# 介面互動修正

2026-10-09 的修正以使用者提供的畫面為依據，保留原有黑色底、可調整毛玻璃、
Nunito／Noto Sans TC、語言偏好與原始首頁人物圖。

## 行為

- 連線教學的未選中 tab 不畫裝飾邊框；鍵盤 focus 使用獨立 outline。
- 個人頁原先同時使用 bottom border 與 inset shadow；選中模式現在只畫一道 3px 重點色底線。
- 首頁以獨立人物圖層跨過玻璃面板上緣。桌面標題為 64px、人物圖片寬 400px，
  手機按寬度調整。原始 PNG bytes 與 provenance 保留，面板本身仍有圓角與裁切。
- 主內容移除永久 `view-transition-name`，使用原生 root crossfade。
  非 `none` 的 transition name 會形成 backdrop root，原本隔斷了面板與全域背景；
  修正依據 [W3C rendering consolidation](https://drafts.csswg.org/css-view-transitions-1/#rendering-consolidation)。
- 頂部進度在導航快照前先畫出初始段，真實 pending 結束後才進入完成尾段。
  最短顯示時間用於可見性，不延長 pending；沒有可觀察 pending frame 的瞬間快取更新不假造載入。
- 圖譜搜尋輸入與按鈕桌面為 36px、手機為 44px，圖示垂直置中。
  卡片難度以 hover／鍵盤 focus／觸控預覽名稱與星數，項目沒有導覽連結；
  主要標題仍連到圖譜頁。完整摘要與保存數量分開，失敗、重試、撤下與取消舊請求皆有明確行為。
  詳見[官方搜尋與卡片預覽](official-map-search.md)。
- 語言選單使用共用非 modal DropdownMenu，顯示完整語言名稱與選中勾號。
  沿用 cookie／帳號偏好保存；儲存完成後恢復鍵盤焦點，若玩家已聚焦其他控制項則保留其位置。
- 個人頁及排行榜使用自託管 Twemoji 國旗 SVG；未知 country code 或圖片不可用時保留可讀文字。
  259 個原始 SVG 共 720,811 bytes，來自 [Twemoji v17.0.3](https://github.com/jdecked/twemoji/tree/b6b55fef1e8636b540a6d016a4729ca8cdf2e60b)，
  並包含 CC BY 4.0 授權、來源、逐檔 hash 與網站署名。沒有複製參考專案程式碼或新增 AI 圖像。

## 驗證範圍

前端完整 ESLint／TypeScript 與 Linux 正式建置通過。第一輪的 8 個 profile layout cases
通過；最後一輪在修正語言焦點後，以固定正式 image 通過 124 個 Chromium cases。
包含 1440px 桌面、使用者的 1280px、390px／320px 手機、深淺色、繁中／英／日文，
以及 hover、keyboard、touch、保存失敗、撤下、取消與草稿保存。
模糊與進度條另以實際 PNG 像素驗證，不只檢查 CSS。

96 張確認 PNG 的 decode、非空白、manifest bytes／SHA、sidecar 及來源 hash 檢查通過。
視覺審查與 documentation disposition 另見 `.impeccable/review/interface-corrections-verdict.md`
與 `.impeccable/review/interface-corrections-documentation.md`，不將 machine validation 當成視覺判斷。

後端有 86 個一般 unit checks、19 個獨立 PostgreSQL 17 checks、rustfmt 與 warnings-denied
Clippy 通過。API 正式 image 的隔離 HTTP gate 通過 24 個請求，確認搜尋的登入／匯入權限、
參數限制、明確設定錯誤、公開預覽只接受已保存圖譜，以及沒有隱含 publication。

這些是介面與 API 邊界驗證，不宣稱完整 `.osz` 已自動下載，也不代替 stable／lazer
的實際遊玩、Score 提交及 Replay 播放驗證。自動封包來源仍由營運者選擇與設定。
