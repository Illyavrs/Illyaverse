# Kaleidoscope

Illyaverse 的原創 Rust 連線助手。玩家使用 `apps/kaleidoscope` 的 Tauri／Next.js 視窗，固定連到 Illyaverse；stable 使用 `-devserver`，lazer 沿用已安裝的客戶端和現有圖譜庫，在啟動前載入原創 Transfiguration 外掛。沒有玩家伺服器選單。

操作與退出清理見[啟動器指南](../../docs/kaleidoscope-launcher.md)。公開 DNS／HTTPS 環境不需要 hosts。現在的本機 `illya.test` 測試仍需開發 DNS／CA，真實原生遊玩尚待驗證。

Windows storage resolver 從 `%APPDATA%\osu` 或含 `framework.ini` 的 portable
安裝目錄開始，再依 `storage.ini` 的 `FullPath` 找出既有有效路徑。直接沿用
`client.realm`／`files`，不建立空白替代庫、不複製或匯入、不建立 junction。
Transfiguration 只將 `OsuConfigManager` 導向同一資料目錄的
`game.illyaverse.ini`，於設定載入及 API 背景執行緒啟動前確認私服設定；不讀取、
複製或覆寫官方 `game.ini`。私服執行不切換這次已確認的圖譜資料位置。
`%LOCALAPPDATA%\Illyaverse\Kaleidoscope` 保存控制、ready 和 session v2 復原紀錄，
session 記錄原本 library 的路徑及目錄身分，並按已安裝 DLL 的雜湊／檔案身分清理。
舊 session v1 只用於清理原 `%LOCALAPPDATA%\Illyaverse\lazer` 的自有外掛；此路徑
不再是玩家圖譜庫。原生現有圖譜、私服登入、官方設定保留及退出清理仍需實測。

以下 CLI hosts／profile 命令保留給開發者部署規劃，不是玩家 launcher 的伺服器設定。

```powershell
cargo run -p kaleidoscope -- --help
cargo run -p kaleidoscope -- profile --client stable --domain illya.example.com
cargo run -p kaleidoscope -- profile --client lazer --domain illya.example.com
```

`inspect`、`plan`、`apply` 與 `restore` 全部預設只讀／預覽。只有 `apply --write` 或 `restore --write` 會修改指定 hosts 檔。每次修改先產生原始位元組備份，並驗證結果。這些命令不啟動遊戲、不安裝憑證、不修改 DNS 快取。新 `launcher status/set-client/start/stop/recover` 使用同一個固定連線核心。

完整操作方式、TLS 要求與 lazer 相容性限制見 [連線文件](../../docs/connection.md)。

## 本機測試 package

`pwsh -File tools/kaleidoscope/scripts/package-launcher.ps1` 會驗證所選官方安裝、
建置正式 bridge／介面／原生啟動器，再執行 `--verify-embedded-interface`。
Rust 使用現有 repository `target/`，停用 incremental，完成後還原 process
environment。這個指令會執行原生啟動器的無視窗介面檢查，不啟動遊戲。

預設輸出是新的 `data-kaleidoscope-package-<時間>-<nonce>/` 與同名 `.zip`。
`-OutputDirectory` 必須在 repository 內、父目錄已存在，目錄和 ZIP 都必須尚未
存在。必要 payload 先複製到唯一 sibling staging 目錄；所有內容完成後最後才
寫入 `package.json`，驗證組裝目錄與 ZIP 一致後才移至指定輸出。既有 artifacts
不會覆寫或刪除；失敗的本次 staging 可保留供檢查，不能當成完成 package。

完成 package 包含 `Kaleidoscope.exe`、正式 bridge、`artifact.json`、
`compatibility.json`、`LICENSE`、`THIRD-PARTY-NOTICES.md`、五份 `licenses/`
授權文字、`README.txt` 與 `Transfiguration-README.md`。`package.json` 記錄每份
payload 的 SHA256、固定 `https://illya.test`、`fixture:false`、完成狀態及介面檢查
類型。verifier 拒絕缺檔、額外檔案、reparse points、hash 不符、不完整／重複
JSON fields、fixture bridge、其他 endpoint 及無效相容性 metadata；ZIP 不需解壓
即可檢查每個 entry 與內容 hashes。

```powershell
pwsh -File tools/kaleidoscope/scripts/verify-launcher-package.ps1 -Directory <package-directory>
pwsh -File tools/kaleidoscope/scripts/verify-launcher-package.ps1 -ArchivePath <package.zip>
pwsh -NoProfile -File tools/kaleidoscope/tests/package-integrity.ps1
```

最後一個指令只在自己的 identity-marked fixture 目錄使用文字 EXE／DLL bytes，
不執行任何 EXE／DLL。它只證明 package 組裝及拒絕條件，metadata 明確為
`synthetic_package_integrity`、`embedded_interface_verified:false`；production
verifier 預設拒絕這類 fixture。package hashes 及 packager 所記錄的介面檢查
不能證明實際遊戲登入或原生 UI。這是需要本機 DNS／CA 的 `illya.test` 測試
package，公開下載尚未承諾，原生 GUI／登入／遊玩仍待使用者實機驗證。
