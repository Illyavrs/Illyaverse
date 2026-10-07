# 連線到 Illyaverse

玩家使用專用 [Kaleidoscope 啟動器](kaleidoscope-launcher.md)：先在網站註冊，選擇已安裝的 stable／lazer，再按「啟動」。stable 使用固定 `-devserver`，lazer 沿用官方安裝檔並強制載入原創 Transfiguration 外掛。原生登入、遊玩與退出清理仍待玩家驗證；隔離測試不代表全功能相容。

本文範例使用 `illya.example.com` 作為營運者擁有的根網域；請換成自己的實際網域。`illya.test` 適合本機開發，沒有公開 DNS 或公開 CA 憑證。公開玩家應優先使用正常 DNS 與受信任的 HTTPS，通常不需要修改 hosts。

## 使用 stable

先在網站建立 Illyaverse 帳號，關閉已啟動的 osu!，再用營運者公布的根網域啟動 stable。Kaleidoscope 可以產生啟動參數與域名清單：

Illyaverse 統一在網站註冊。stable／lazer 遊戲內的註冊要求會回覆網站註冊網址，並且不建立帳號；完成網站註冊後再回遊戲登入。

```powershell
cargo run -p kaleidoscope -- profile --client stable --domain illya.example.com
```

Windows PowerShell 的啟動範例：

```powershell
& 'C:\Games\osu!\osu!.exe' '-devserver' 'illya.example.com'
```

`-devserver` 接受根網域，請不要加入 `https://`、路徑或 `:port`。這個方式由 [bancho.py 維護者的連線說明](https://github.com/osuAkatsuki/bancho.py/discussions/254) 與 [部署討論](https://github.com/osuAkatsuki/bancho.py/discussions/403) 支持；這些來源用於核對協定與操作方式，Kaleidoscope 沒有複製其程式碼。不同 stable 版本的實際請求仍必須在測試環境確認。

營運者需要替 `osu.illya.example.com`、`c.illya.example.com`、`ce.illya.example.com`、`c4.illya.example.com`、`a.illya.example.com` 等實際使用的域名建立 DNS、TLS 與反向代理路由。Kaleidoscope 另列出根域名、`b`、`api` 作為部署規劃清單；不是對每個客戶端版本的完整域名保證。反向代理應根據 Host 與 path 路由至正確服務，不能讓所有 legacy 請求都落到網站的 HTML 頁面。

首次測試依序檢查登入、聊天、譜面查詢／下載、實際遊玩後提交成績、榜單，以及 replay 下載／播放。連上聊天室不代表 score submission 或 replay 格式已相容。

## 使用 lazer

新的 [Kaleidoscope／Transfiguration 流程](kaleidoscope-launcher.md)使用已安裝的官方 lazer，不需要重編整套 client。Rust 啟動橋接器強制在遊戲 Main 之前載入外掛，確認固定 endpoints、私有資料目錄及 HTTP／WebSocket guard。必要修補、相容性或檔案驗證失敗時不執行遊戲入口；更新到未測版本後需新版小型外掛，沒有官方 fallback。

獨立監護程序等待遊戲實際結束後清理自己的 DLL；GUI 關閉不會讓它提前刪除。中斷時使用持久復原紀錄，不刪其他 ruleset。私服 profile 不讀取或複製官方 token。公開 DNS／HTTPS 不需要 hosts；目前 `illya.test` 仍需[本機 DNS／CA](local-client-testing.md)。

### 開發用 source build 與協定規劃

先前的[獨立 lazer 測試客戶端](lazer-client-build.md)保留作為比較工具，不是玩家預設流程。以下 manifest 與 endpoint 表是開發規劃。

**官方發行版 lazer 直接切換任意私服目前未驗證。** 2026-10-07 核對官方 `ppy/osu` master 的 `6359741babba4ced42da1fcda738c35370fdf3f4` 後，release 的 endpoint 設定仍指向官方 HTTPS API 與 spectator；沒有找到可以拿來保證使用的任意伺服器 `--server` 開關。[官方 production endpoint 設定](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/Online/ProductionEndpointConfiguration.cs)、[endpoint 選擇方式](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Game/OsuGameBase.cs)、[桌面啟動參數](https://github.com/ppy/osu/blob/6359741babba4ced42da1fcda738c35370fdf3f4/osu.Desktop/Program.cs)。

先前的開發比較使用獨立 lazer source build、自有 API／OAuth／SignalR endpoints、獨立資料目錄與 Illyaverse 帳號。官方 [完整伺服器堆疊測試教學](https://github.com/ppy/osu/wiki/Testing-web-server-full-stack-with-osu!) 提供這種協定測試的背景。

```powershell
cargo run -p kaleidoscope -- profile --client lazer --domain illya.example.com
```

此指令輸出 JSON **規劃 manifest**。它不會被官方 lazer 自動讀取，也不是可以直接匯入的連線設定。source build 必須由營運者把自己的設定接到客戶端 endpoint 選擇處。需設定的項目包括：

| 項目 | 規劃 endpoint | 驗證內容 |
| --- | --- | --- |
| Website／API | `https://osu.illya.example.com` | API v2 格式、版本、帳號與 OAuth 流程 |
| Spectator | `https://spectator.illya.example.com/spectator` | SignalR negotiation、認證、遊玩 session 與 replay frames |
| Multiplayer | `https://spectator.illya.example.com/multiplayer` | SignalR 方法、房間生命週期、同步與斷線重連 |
| Metadata | `https://spectator.illya.example.com/metadata` | 事件與譜面資料更新 |
| Beatmap submission | `https://bss.illya.example.com` | 投稿協定；尚未實作時不要宣稱可使用 |
| OAuth client | 營運者產生的 client ID／設定 | audience、grant、refresh、撤銷與 token 驗證 |

單獨支援 `/api/v2` REST 回應仍不足以完成 lazer：它的 replay 與 online gameplay 牽涉 spectator 服務，正式提交成績需要相容的 score token／session 流程。[官方 score submission 流程](https://github.com/ppy/osu-infrastructure/blob/master/score-submission.md) 可作為互通性研究來源。

玩家用的 lazer 發行流程還需要固定上游版本、完整連線測試、更新渠道與清楚的客戶端來源；完成之前網站應顯示「開發測試中」，不要把 hosts 切換工具當成 stock lazer 相容性保證。

## 本機開發：Kaleidoscope hosts 管理

從 Illyaverse 根資料夾建置：

```powershell
cargo build --release -p kaleidoscope
cargo test -p kaleidoscope
```

開發 CLI 的 Windows 執行檔位於 `target\release\kaleidoscope.exe`，Linux／macOS 位於 `target/release/kaleidoscope`。玩家圖形啟動器目前僅支援 Windows。Windows 預設 hosts 路徑為 `%SystemRoot%\System32\drivers\etc\hosts`；Unix 預設為 `/etc/hosts`。以下命令可用 `--hosts PATH` 指向測試檔。

先讀取現況，再檢查明確指定域名的修改計畫：

```powershell
.\target\release\kaleidoscope.exe inspect
.\target\release\kaleidoscope.exe plan --ip 127.0.0.1 --domain osu.illya.test --domain c.illya.test
.\target\release\kaleidoscope.exe apply --ip 127.0.0.1 --domain osu.illya.test --domain c.illya.test --dry-run
```

以上只讀取與預覽，不會寫入任何檔案。`apply` 不加 `--write` 也維持預覽模式。確認內容後在有足夠權限的終端執行同一組參數：

```powershell
.\target\release\kaleidoscope.exe apply --ip 127.0.0.1 --domain osu.illya.test --domain c.illya.test --write
```

Windows 系統 hosts 需要「以系統管理員身分執行」的終端；Linux／macOS 則需要對檔案及其所在目錄的寫入權限。工具不會自動要求提權，也不會為了預覽要求管理員權限。

Kaleidoscope 只建立或取代以下標記內的區塊：

```text
# BEGIN ILLYAVERSE KALEIDOSCOPE
# Managed by Kaleidoscope. TLS and client compatibility must be configured separately.
127.0.0.1    osu.illya.test
127.0.0.1    c.illya.test
# END ILLYAVERSE KALEIDOSCOPE
```

未管理的行、註解、UTF-8 BOM 與原有換行會保留。若原檔末尾沒有換行，區塊內會有 separator metadata，以便還原時取回原本格式。不要手動刪除這項 metadata。重複／巢狀／缺少結尾的區塊會拒絕處理；指定域名若已存在於其他位置也會拒絕，避免不確定的 hosts 優先順序。

IPv4／IPv6 都可用。domain 必須是明確的 ASCII DNS／punycode hostname；不接受 wildcard、IP、URL、port、path 或換行注入。Kaleidoscope 拒絕導流 `ppy.sh` 及其子域名：請使用營運者擁有的域名與自己的帳號，避免把官方登入流量送往別的伺服器。

每次實際修改先在 hosts 同一目錄建立 `hosts.kaleidoscope-<nonce>-<pid>.bak` 原始位元組備份，再建立同目錄暫存檔、檢查檔案是否被修改、替換並核對寫入結果。Windows 使用 [ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew) 保留原有 ACL 與屬性。備份不會自動覆蓋現有檔案。相鄰的 `.kaleidoscope.lock` 協調同時執行的 Kaleidoscope；請在寫入時關閉其他 hosts 編輯器，因為這個鎖不會限制其他程式。

## 還原連線

```powershell
.\target\release\kaleidoscope.exe restore
.\target\release\kaleidoscope.exe restore --write
```

`restore` 預覽移除 Kaleidoscope 區塊的結果；`restore --write` 才實際移除。這個方式會保留在套用後新增的其他 hosts 記錄。工具不會用舊備份覆蓋整個 hosts，以免抹掉其他軟體或營運者後來的變更。

如果一次系統寫入失敗，錯誤會顯示備份與 staging 路徑。先檢查實際 hosts 狀態，再由管理員決定是否用位元組備份救回完整檔案。若程序被強制中止，鎖可能留在原處；確認已沒有 Kaleidoscope 程序執行後，才手動移除該檔案並重試。日常還原仍優先使用區塊移除方式。

Windows 若有 DNS 快取，可以在系統管理員終端自行執行 `ipconfig /flushdns`，再重啟遊戲。Kaleidoscope 不會自動執行這一步。

## TLS 與 Docker 邊界

Docker Compose 的 HTTP 埠只適合網站／API 本機開發。公開遊戲連線需要反向代理的 HTTPS 443 與正確的 Host 路由；lazer 的 SignalR 也需要 WebSocket／long-polling 支援與合適的 timeout。使用公有域名時，TLS 憑證的 SAN 必須涵蓋實際 domain，且完整 chain 必須受客戶端信任。

hosts 只改 DNS 結果，**不會改變 HTTPS 的 hostname 或憑證驗證**。所以把官方域名指到自己的 IP，不能憑空得到有效的官方域名憑證；官方 lazer 的硬編碼 OAuth／endpoint 設定也不會因此變成 Illyaverse 設定。

本機 `.test` 開發若需要 HTTPS，請使用只涵蓋自己開發域名的測試 CA，並在隔離的測試環境明確安裝／移除；Kaleidoscope 不會安裝根憑證或關閉 TLS 驗證。lazer 官方教學中允許 insecure HTTP 的選項是開發用途，公開部署仍應使用有效 TLS。

## 上線前的互通性驗證

| 客戶端 | 必須以真實客戶端驗證 |
| --- | --- |
| Stable | 指定版本 `-devserver` 登入、封包／token、頻道聊天、下載、自訂譜面、score submit、replay、登出／重連 |
| Lazer source build | 自有 endpoints、OAuth refresh／撤銷、API v2、SignalR negotiation／方法、score token、replay frames、multiplayer／metadata |
| 官方 lazer release | 尚未驗證；沒有測試證據前維持不支援標示 |

Kaleidoscope 的單元測試驗證輸入安全、區塊處理、換行／BOM、衝突拒絕、位元組備份、檔案變更檢查、鎖，以及「移除區塊仍保留其他修改」。測試只使用臨時 fixture，從未操作玩家電腦的系統 hosts。這些測試不等於 osu! 客戶端端到端相容性測試。
