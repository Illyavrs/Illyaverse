# 完成度與原生驗證

這份表依原始需求核對 Illyaverse 的實作與證據。網站與隔離整合測試通過，不等於 stable／lazer 已可正常遊玩；原始目標目前仍未完成。

## 原始需求

| 需求 | 目前狀態與證據 |
| --- | --- |
| Illyaverse monorepo、原創程式碼 | Serendipity、Sanctuary、Resonance、Kaleidoscope／Transfiguration 各有明確責任；來源規範見 [AGENTS.md](../AGENTS.md) 與 [PRODUCT.md](../PRODUCT.md)。參考專案不作為複製來源。 |
| Next.js、Tailwind、shadcn/ui、Rust | 已實作；來源與依賴位於 `apps/serendipity`、`services`。最新網站 lint、typecheck、正式 Docker build 及操作證據見 [圖譜與共用層次檢查](map-discovery-review.md)。 |
| Docker 部署 | 已有初始化、Compose、持久化、健康檢查、備份及正式 HTTPS 配置；兩套本機部署共十個健康容器。見 [部署](deployment.md)。正式 DNS／ACME 仍由營運者配置，本機測試不證明公開域名部署成功。 |
| 公開註冊、多語系 | 已有網站註冊、登入、帳號設定及 cookie／帳號語系偏好；遊戲註冊導向網站。郵件復原／驗證取決於 SMTP 配置。見 [帳號復原](account-recovery.md) 與 [網站功能](website-features.md)。 |
| 深淺色、純黑毛玻璃與自訂外觀 | 已有背景上傳、背景 blur／dim、獨立面板 blur／opacity、重點色與圓角。預設 8px／55%／14px，無 AI 圖。見 [外觀狀態](appearance-state.md) 與 [設計系統](../DESIGN.md)。 |
| 個人頁自訂 | 已有頭像、Banner、裁切位置、公開重點色、介紹、統計、成績、上傳圖譜與實際排名歷史。見 [個人頁](profile-presentation.md)、[排名歷史](rank-history.md)。沒有補造早期記錄或活動。 |
| 權限與不同角色區域 | 伺服器檢查權限，網站呈現相應管理區；有角色、帳號限制、圖譜／成績管理與稽核。見 [網站功能的權限驗證](website-features.md)。隱藏入口本身不作為授權證據。 |
| 聊天系統 | 已有網站公開／私人對話、好友／封鎖及跨協定隔離整合證據。見 [聊天](website-chat.md)。原生遊戲的聊天畫面與重連仍待實機驗證。 |
| 官方與社群圖譜 | 已有共享官方節流、metadata／整包匯入、本地 `.osu`／`.osz` 上傳、下載、難度切換及下架。見 [圖譜搜尋](map-discovery.md)、[官方圖譜包](official-beatmap-archives.md)、[本地圖譜包](local-map-packages.md)。 |
| Score、Replay、rosu-pp、RX | 已有持久化、驗證／私密政策、Replay 綁定、PP 計算與獨立 Normal／RX 分區。隔離協定與資料庫證據見 [驗證紀錄](verification.md)。最新五次真人 stable 提交帶 19 欄，被原先精確 18 欄檢查以 400 拒絕；已按參考協定修正有界前段解析，真人成績／Replay 保存仍待確認。 |
| stable 與 lazer 連線 | 已有 stable `-devserver` 協定及固定端點的 stock lazer 原創橋接，並有 HTTP／DTO／即時協定測試。見 [協定相容性](protocol-compatibility.md)。實機登入、遊玩、送分、Replay 播放及重連尚未通過完整清單。 |
| 專用啟動器、禁止官方 fallback、退出移除 DLL | 已有必須先載入的外掛、固定服務、相容性拒絕、監護程序與復原清理。見 [Kaleidoscope](kaleidoscope-launcher.md)。真實遊戲的啟動／重新啟動／退出清理仍待玩家驗證；外掛只允許已驗證的 client 版本。 |
| Landing 與連線教學 | 已有實際伺服器資料、功能入口、註冊／連線步驟及各客戶端操作指南。見 [網站功能](website-features.md) 與 [連線](connection.md)。 |

## 目前版本

網站／圖譜檢查對應 `1e49045`。最後一輪有 75 項圖譜／首頁／個人頁操作及 17 項外觀／排名回歸通過；十張桌面、手機、三語系截圖經獨立審查。兩套部署另有 38 項唯讀 HTTP 檢查通過，schema 22 與原有資料保持一致。精確 image、來源與截圖 hash、範圍和跳過項目記錄於 [證據](../.impeccable/review/map-discovery-evidence.json)。

圖譜改版部署時，兩套保留資料庫各有兩個帳號、十四筆當日排名觀察；主站有零張圖譜，客戶端測試站有四十四個難度。兩者當時都沒有實際保存的成績或 Replay。這些數量是該 checkpoint 的資料，不是演示活動。

圖譜改版當時的 Sanctuary／Resonance 映像另完成一次全新的隔離協定回歸：458 次 HTTP 請求、55 類檢查通過，schema 22；涵蓋加密 stable 送分、lazer、RX、聊天、Replay、圖譜包與下架／復原。55 是涵蓋類別數，不是 assertion 數。這輪只執行 PostgreSQL、Sanctuary 與 Resonance；網站和 Caddy 的 image 僅核對 pin，網站操作證據仍來自前述檢查。

三個測試容器、兩個 volume、一個 network 與產生的環境檔已按擁有者身分核對後清除。協定與清理通過後，外層 runner 的環境復原檢查因未設定值被還原成空字串而失敗；修正 helper 後，另以未設定／空字串／有值三種情況確認精確復原，沒有重跑協定。影響範圍僅限已結束的子 PowerShell process；保留的失敗紀錄與各檢查範圍見 [完成度證據](release-readiness-evidence.json)。

其後 stable 附加欄位排空修正的 Resonance 映像 `sha256:d2d4252932a7833ebb8ce1e3baa283c961ee13ae78c0f42015b0d5d793ebb8c9` 通過 25 個 multipart tests、Clippy 及一輪全新的 461 HTTP／56 類隔離加密協定回歸；runner、環境還原及擁有者清理均 exit 0。兩套本機服務已於 2026-10-08 09:36 UTC（台灣 17:36）更新 Resonance，十個容器健康、其餘八個容器與原有資料 ID／schema 保留。更新時 client 有四十五個難度，兩套都仍為零成績／Replay；實際送分成功待新重試確認。確切來源、備份範圍與實機狀態另記在 [附加欄位修正證據](stable-ancillary-evidence.json)，不覆寫早期 checkpoint 的結果。

其後的版本欄位 ASCII 空格旗標修正完成 7 個核心 tests、診斷隱私測試與 Clippy，Resonance 映像 `sha256:bcd541bbdd0a154ae7082136b042d851f69713218b2a46937eb6bf0fe27345cd` 通過一次 465 HTTP／57 類隔離協定 gate。兩套本機服務在 2026-10-08 10:00 UTC（台灣 18:00）完成更新，仍有十個健康容器；只重新建立兩個 Resonance，schema 22、其餘容器與原有資料 ID 保留。部署時兩套都仍為零成績／Replay。此 checkpoint 的 source 與備份範圍另記在 [版本旗標證據](stable-version-flags-evidence.json)。

最新的有界前段解析修正映像 `sha256:4e298cd79897886bf08e29c867737aa7b92779c564390e1a4d1362a628bcae9d` 完成 10 個核心 tests、更新後的隱私測試、Clippy，以及一次 472 HTTP／58 類隔離協定 gate；其中首次加密送分就是已觀察到的 19 欄結構，以原創附加資料驗證保存及 Replay 回傳。兩套本機 Resonance 在 2026-10-08 14:07 UTC（台灣 22:07）更新，schema 22、其餘八個容器與原有資料 ID 保留，十個容器健康。Client 當時有 46 個難度，兩套仍為零成績／Replay；真人保存另待對應證據。見 [有界前段證據](stable-prefix-evidence.json)。

## 尚需完成

1. 確認真人 stable 能成功保存成績與 Replay；2026-10-08 13:53–13:55 UTC 的五次重試確定因 19 欄／原先精確 18 欄限制而收到 400。已參照 Sunrise／bancho.py 的 canonical 前段契約修正，附加內容有界且不參與任何成績驗證；完整實機成功仍待對應請求與持久化證據。見 [原生成績提交](native-score-submission.md)。
2. 由玩家執行 [原生 release checklist](protocol-compatibility.md#native-client-release-checklist)，記錄 client 版本、時間與結果，涵蓋 stable／lazer 登入、聊天、官方／本地圖譜、各模式、RX、成績、Replay 與重連。
3. 由玩家確認 Kaleidoscope 的真實啟動／關閉並重新啟動，以及遊戲結束後外掛清理；異常中斷後依復原流程檢查。

2026-10-08 09:15:29 UTC 的更新核對已有四十一次 stable 送分 POST；其中十四次新請求在 Resonance 均因同一普通附加欄位超過 32 KiB 而拒絕，最新為 09:15:16 UTC、HTTP 413。先前 04:23 UTC 的二十七次／沒有新請求記錄只描述當時的保留範圍，見完成度證據 checkpoint。自動測試及本機服務健康不能取代上述玩家操作。
