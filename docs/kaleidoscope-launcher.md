# 使用 Kaleidoscope 啟動遊戲

Kaleidoscope 是 Illyaverse 專用的 Windows 啟動器。先在網站註冊，再選擇已安裝的 stable 或 lazer，按「啟動」。遊戲已執行時，按鈕會改為「關閉並重新啟動」。沒有其他伺服器選單。

目前測試版固定連到 `https://illya.test`。啟動器與原創 Transfiguration 外掛已通過隔離測試；真正的遊戲視窗、登入、遊玩與退出流程仍待玩家驗證。營運者尚未發布公開下載，請先取得營運者提供的測試包。

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

不需要重新編譯整套 lazer，但外掛仍需相容性維護。目前只允許已測試的 stock `2026.1005.1-tachyon` 檔案與執行環境。更新後不相容時，等待新版小型連線外掛；啟動器會停止，沒有官方伺服器 fallback。

私服資料固定在 `%LOCALAPPDATA%\Illyaverse\lazer`，與官方帳號設定、token、譜面庫和 IPC 分開。外掛暫放在這個私服資料目錄的 `rulesets` 子資料夾，不複製官方登入資料，也不自動匯入 stable 資料。遊戲的資料目錄切換維持此隔離範圍。不要手動把外掛複製到官方 rulesets。

## 關閉與清理

遊戲結束後，獨立監護程序會確認程序身分與實際退出，再刪除本次放入的外掛。只關閉啟動器視窗時，監護程序繼續等待遊戲。

強制結束監護程序或斷電時，當下無法執行刪除。下次開啟啟動器會檢查復原紀錄；有殘留時按「重試清理」。遊戲仍在執行，或檔案被更換、修改、連結到其他位置時，工具保留檔案並顯示錯誤。清理失敗不會顯示完成，也不刪除其他 ruleset。

## 營運者建置測試包

在 Illyaverse 根目錄執行：

```powershell
.\tools\kaleidoscope\scripts\package-launcher.ps1
```

需要 Windows Rust/MSVC 工具鏈、.NET 10 SDK、Node.js 22，以及已安裝且相容的 lazer。腳本只讀取安裝檔作為編譯參考，編譯自製外掛與 Tauri／Next.js 啟動器，將外掛 SHA-256 綁入啟動器；不重新編譯、修改或散布官方 client。產物位於 Git 忽略的 `data-kaleidoscope-package`，腳本不啟動遊戲。

測試包尚未經過真正原生 GUI／遊玩驗證。請依序回報登入、聊天、譜面下載、遊玩、成績／replay、多人與重連，以及退出後外掛是否移除。[驗證記錄](verification.md)區分 source、隔離整合與原生驗證。
