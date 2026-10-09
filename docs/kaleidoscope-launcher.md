# 使用 Kaleidoscope 啟動遊戲

Kaleidoscope 是 Illyaverse 的 Windows 圖形啟動器，負責啟動 stable／lazer、顯示狀態及清理暫存外掛。Transfiguration 是實際修改 lazer 連線與帳號載入方式的原創橋接外掛，會由啟動器管理。

## 目前測試包狀態

新版 [GUI 測試 ZIP](../.artifacts/packages/kaleidoscope-gui-test-20261009-141323-4ae3b865e72a4f75a7d1010d10e73040.zip) 已建置，含伺服器網址設定、沿用遊戲偏好及私服帳號隔離。SHA-256：`4dc11a2f7a87fddcc0b50d945b2610504d5643d422a6637f588845f60881bdec`。此包是 Debug 原生建置，內嵌 production 網頁介面；完整目錄、ZIP 及內嵌介面檢查通過。先前使用整份獨立遊戲設定的 CLI 包只保留為[歷史整合證據](current-integration-20261009.md)，不適用下面的新流程。玩家已回報使用此 GUI 能開啟 lazer 並登入 Illyaverse；這項回報尚不包含遊玩、送分、Replay、聊天、多人、重連與退出清理的完整原生驗證。

## 開始使用

1. 在營運者提供的 Illyaverse 網站註冊帳號；預設本機測試網站是 `https://illya.test/register`。遊戲內註冊會引導至網站。
2. 完整解壓營運者提供的新版 GUI 包，保留整個資料夾及其中的 EXE、DLL、相容性資料與授權文件。
3. 開啟 `Kaleidoscope.exe`，選擇已安裝的 stable／lazer 的 `osu!.exe`。
4. 如需指定部署網址，在「設定」填入伺服器網址並按「儲存」。確認網址後按對應遊戲的「啟動」，再用該伺服器的網站帳號登入。

遊戲已執行時，按鈕會改為「關閉並重新啟動」。工具先請求所選安裝正常關閉；沒有結束時會顯示錯誤，不強制終止。

## 伺服器網址

預設為 `https://illya.test`。請填 HTTPS 基底網址，例如 `https://your-illyaverse.example`，不要加登入資訊、路徑、查詢參數或 `#`。只接受完整 ASCII DNS 名稱，不接受 IP、單段主機名稱、尾端句點或官方 `ppy.sh` 及其子網域。lazer 可使用自訂 HTTPS 連接埠；stable 使用 443，自訂其他連接埠時介面會停用 stable 啟動並說明原因。

儲存只影響下一次啟動。目前執行的遊戲保留啟動時的網址，設定面板會分別顯示已保存網址與本次連線網址。連線失敗、外掛缺少或相容性不符時會停止，不會切回官方伺服器。

公開網域需要營運者設定 DNS 與受信任 HTTPS。`illya.test` 是本機開發網域，仍需先完成[本機 DNS／CA 設定](local-client-testing.md)。啟動器不修改 hosts、不安裝 CA，也不略過 TLS 驗證。stable 由啟動器加上 `-devserver <設定的網域>`；lazer 由 Transfiguration 套用本次網址。

## lazer 的圖譜、偏好與帳號

lazer 使用現有安裝、`client.realm` 和 `files` 圖譜資源，不需要重新匯入。啟動器辨識一般、portable 或 `storage.ini` 指定的既有資料位置；不能安全確認時會停止，不建立另一個空白圖譜庫。音訊、輸入、顯示、遊戲與介面偏好沿用原本的 `game.ini`，修改後正常保存至同一份檔案。

只有登入與伺服器相關帳號快取另外保存到同一資料目錄的 `auth.illyaverse.json`，按正規化後的 HTTPS 網址分開：`Username`、`Token`、`SaveUsername`、`SavePassword`、`WasSupporter`、`UserOnlineStatus`、`PMFriendsOnly`。切換網域或連接埠不會沿用另一部署的 token。官方帳號值不會載入私服 API，保存遊戲偏好時會保留原有官方帳號行。舊的 `game.illyaverse.ini` 不會載入或遷移。

同一份 lazer 資料目錄同時只支援一個遊戲。啟動器會關閉所選安裝的既有程序，並拒絕已偵測到的其他標準名稱 lazer 程序；這不能阻止之後從外部手動開啟遊戲。不要同時用其他程式替換設定檔；保存只提供樂觀衝突檢查，無法保證比較後才發生的外部寫入。更換圖譜資料位置前，先退出私服遊戲並完成清理，再由正常 lazer 調整。

目前相容性限定為包內 `compatibility.json` 所列的精確 stock 檔案與執行環境（目前為 `2026.1005.1-tachyon`、runtime `10.0.12`）。版本字串相同仍可能因檔案雜湊不同而拒絕啟動。lazer 更新後可能需要新版 Kaleidoscope／Transfiguration 配套；不必重新編譯整套 lazer。請更新完整包，不要單獨替換 DLL。

## 關閉、清理與 lock 檔

正常關閉遊戲後，獨立監護程序確認退出，再刪除本次放入且未被更換的 Transfiguration DLL。只關閉啟動器視窗時，監護程序仍會等待遊戲。遊戲與監護程序執行期間請保留整個測試包資料夾。

若斷電或監護程序被結束，下一次開啟啟動器會檢查復原紀錄；有殘留時按「重試清理」。遊戲仍在執行、檔案被修改或目錄身分異常時會保留檔案並報錯，不刪除其他 ruleset。回到官方 lazer 前，先關閉此次私服遊戲並確認清理完成。

`rulesets` 中的 `.kaleidoscope.lock` 是 Kaleidoscope 的互斥鎖；舊的 `.illyaverse-launcher.lock` 也會一起取得，以避免新舊啟動器同時安裝外掛。這兩個空檔案會刻意保留，存在本身不表示遊戲正在執行，也不是另一個 ruleset。檔案名稱來自啟動器，實際連線外掛仍叫 Transfiguration。

## CLI 與營運者建置

另行提供的新版 CLI 測試包也可讀取、保存下次啟動網址與查看狀態。以下指令適用 CLI 包的 EXE；GUI 包使用前述介面操作：

```powershell
.\Kaleidoscope.exe launcher settings
.\Kaleidoscope.exe launcher set-endpoint "https://your-illyaverse.example"
.\Kaleidoscope.exe launcher status
.\Kaleidoscope.exe launcher stop
.\Kaleidoscope.exe launcher recover
```

如使用新版 CLI 包，可執行 `launcher start lazer <osu!.exe 路徑>`。CLI 最多等待 40 秒；`running` 需確認首幀完成及程序仍存活，不能單靠它證明 OS 視窗可見或登入成功。逾時後用 `launcher status` 查詢，工具不因逾時自動結束遊戲。

營運者在 Illyaverse 根目錄建置 GUI 測試包：

```powershell
.\tools\kaleidoscope\scripts\package-launcher.ps1 -BuildProfile Debug
```

Debug 使用現有 `target/debug`，仍以 production 模式嵌入 Next.js 介面並啟用 Tauri `custom-protocol`。腳本驗證內嵌介面、配套外掛與目錄／ZIP 雜湊，不開啟遊戲；需要 Windows Rust/MSVC、.NET 10 SDK、Node.js 22 以上及相容 lazer。省略 `-BuildProfile` 時使用 Release；先前三次 Release 建置遇到 Windows error 5，原因尚未確認。建置選項或隔離測試通過都不能替代真正的 GUI、登入、Score／Replay、聊天、多人、重連與退出清理驗證，詳見[驗證記錄](verification.md)。

建置快取預設使用來源工作區的 `target`。若已設定 `CARGO_TARGET_DIR`，腳本沿用該路徑；`-TargetDirectory` 可明確指定並優先於環境變數，相對路徑以來源工作區為基準解析。共用快取位於工作區外時，腳本只把成功建置後的指定 profile `kaleidoscope-desktop.exe` 複製到工作區內新的 `.artifacts/launcher-builds/` 目錄，記錄來源、複本的 SHA-256 與路徑，然後以複本驗證內嵌介面及組裝 ZIP。其餘封裝來源仍須位於工作區內；拒絕含 reparse point 的建置路徑，並保留既有快取與封裝。這項共用快取支援不代表先前的 Windows error 5 已修復。
