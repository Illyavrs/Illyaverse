# 使用 Kaleidoscope 啟動遊戲

Kaleidoscope 是 Illyaverse 專用的 Windows 啟動器。先在網站註冊，再選擇已安裝的 stable 或 lazer，按「啟動」。遊戲已執行時，按鈕會改為「關閉並重新啟動」。沒有其他伺服器選單。

目前測試版固定連到 `https://illya.test`。lazer 使用已安裝的客戶端和現有圖譜庫，不需要另外匯入或複製圖譜。啟動器與原創 Transfiguration 外掛的測試範圍見[驗證記錄](verification.md)；真正的遊戲視窗、登入、遊玩與退出流程仍待玩家驗證。營運者尚未發布公開下載，請先取得營運者提供的完整測試包。

2026-10-09 的沿用圖譜庫版本已通過核心、橋接與介面檢查，但尚未產出完整 Windows 測試包：三次 release 建置均在執行 Rust dependency build script 時遭 Windows 拒絕存取（error 5），原因尚未確認。較早產出的 ZIP 不包含本次變更，不能用來驗證沿用圖譜庫流程；完整套件建置成功後才提供玩家測試。

## 第一次使用

1. 在 `https://illya.test/register` 建立 Illyaverse 帳號。遊戲內註冊會提供網站網址。
2. 解壓縮完整測試包，保留 `Kaleidoscope.exe` 和同一資料夾的 `osu.Game.Rulesets.Transfiguration.dll`。
3. 開啟 `Kaleidoscope.exe`，分別選擇 stable／lazer 安裝資料夾中的 `osu!.exe`。
4. 按對應的「啟動」，再使用 Illyaverse 帳號登入遊戲。

正式公開網域使用正常 DNS 與受信任 HTTPS 後，玩家不需要改 hosts。`illya.test` 是本機開發網域，仍需先完成[本機 DNS／CA 設定](local-client-testing.md)。啟動器不修改 hosts、不安裝 CA，也不跳過 TLS 驗證。

## stable

啟動器以固定 `-devserver illya.test` 參數啟動所選 stable。相同安裝已開啟時會先收到正常關閉要求；遊戲沒有結束時顯示錯誤，不強制終止或啟動另一份。

## lazer

啟動器沿用官方 lazer 的既有安裝檔，在遊戲入口執行前強制載入原創 Transfiguration 外掛。外掛設定固定網站、API 與即時服務，阻擋其他 HTTP／WebSocket 目的地，停用這次私服執行的官方更新與錯誤上報。外掛不存在、相容性不符或必要修補失敗時，遊戲入口不會執行。

不需要重新編譯整套 lazer，但啟動器與外掛仍需相容性維護。目前只允許已測試的 stock `2026.1005.1-tachyon` 檔案與執行環境。更新後不相容時，等待新版 Kaleidoscope／Transfiguration 配套測試包；啟動器會停止，沒有官方伺服器 fallback。啟動器綁定遊戲檔案與外掛的雜湊，請一起更新完整測試包，不要單獨替換 DLL。

啟動器找出這份 lazer 實際使用的資料目錄，直接沿用原有 `client.realm` 和 `files` 圖譜資源。一般安裝從 `%APPDATA%\osu` 開始查找；安裝資料夾存在 `framework.ini` 時依 portable 模式查找，再讀取 `storage.ini` 的 `FullPath` 自訂路徑。找不到或無法安全確認既有路徑時會停止，不會建立另一個空白圖譜庫。沒有複製、搬移、重新匯入或建立 junction 的步驟。這項路徑判定以固定 [osu-framework 版本](https://github.com/ppy/osu-framework/tree/ab102608f363f24e0e33ec629a9bf613e72c9950)和原創[路徑解析器](../tools/kaleidoscope/src/launcher/storage.rs)為依據。

官方的 `game.ini` 不會被讀取、複製或覆寫。私服登入資訊與遊戲設定存於同一資料目錄的 `game.illyaverse.ini`；外掛在設定載入和 API 背景執行緒啟動前確認這份獨立設定，不把官方帳號或 token 送到 Illyaverse。遊戲資料庫和圖譜資源仍是現有那份，只有這次執行的 IPC 與帳號設定分開。[設定隔離程式](../tools/transfiguration/src/AccountConfigIsolation.cs)記錄此邊界。

外掛暫放在上述現有資料目錄的 `rulesets` 子資料夾。復原紀錄與就緒訊號存於 `%LOCALAPPDATA%\Illyaverse\Kaleidoscope`，不作為圖譜庫。啟動器記錄本次圖譜目錄與目錄身分，清理時不重新依另一份 `storage.ini` 選擇路徑。此次私服執行停用自動匯入 stable 和資料目錄切換；要更改 lazer 的資料位置，先結束遊戲並完成外掛清理，再由正常 lazer 調整，下一次啟動器會重新確認既有位置。不要手動放置或替換 Transfiguration DLL。

## 關閉與清理

遊戲結束後，獨立監護程序會確認程序身分與實際退出，再刪除本次放入的外掛。只關閉啟動器視窗時，監護程序繼續等待遊戲。

強制結束監護程序或斷電時，當下無法執行刪除。下次開啟啟動器會檢查復原紀錄；有殘留時按「重試清理」。遊戲仍在執行，或檔案被更換、修改、連結到其他位置時，工具保留檔案並顯示錯誤。清理失敗不會顯示完成，也不刪除其他 ruleset。

要回到官方伺服器，先關閉此次私服遊戲並確認清理完成，再正常開啟 lazer。舊版曾在 `%LOCALAPPDATA%\Illyaverse\lazer` 留下的啟動紀錄只供復原原本自有外掛，不會被選作新版圖譜庫，也不會刪除其中的圖譜或設定。

## 營運者建置測試包

在 Illyaverse 根目錄執行：

```powershell
.\tools\kaleidoscope\scripts\package-launcher.ps1
```

需要 Windows Rust/MSVC 工具鏈、.NET 10 SDK、Node.js 22，以及已安裝且相容的 lazer。腳本只讀取安裝檔作為編譯參考，編譯自製外掛與 Tauri／Next.js 啟動器，將外掛 SHA-256 綁入啟動器；不重新編譯、修改或散布官方 client。腳本以 production 模式嵌入網站介面，並執行不開視窗的 HTML／JS／CSS 完整性檢查。必要 metadata、授權及每份 payload 的雜湊通過目錄／ZIP 一致性檢查後，才輸出測試包。預設產物是 Git 忽略的 `data-kaleidoscope-package-<時間>-<nonce>/` 目錄與同名 `.zip`；既有輸出不會被覆寫。腳本使用 repository 的 `target/` 並停用 Rust 增量編譯，不啟動遊戲。

測試包尚未經過真正原生 GUI／遊玩驗證。請依序回報登入、聊天、譜面下載、遊玩、成績／replay、多人與重連，以及退出後外掛是否移除。[驗證記錄](verification.md)區分 source、隔離整合與原生驗證。
