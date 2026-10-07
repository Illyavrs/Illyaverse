# 可重現的 Illyaverse lazer 測試客戶端

本文件保留先前的 source build 作為開發比較工具。玩家新流程是 [Kaleidoscope／Transfiguration 啟動器](kaleidoscope-launcher.md)，沿用官方安裝檔，不必重編整套 lazer。下文的 JSON profile 仍只是開發 endpoint 規劃。

以下獨立編譯、固定版本的 lazer 測試客戶端供開發者比較協定行為，玩家不需依本文件編譯。Kaleidoscope 的 JSON profile 仍只是開發 endpoint 規劃；目前啟動器透過 Transfiguration 在遊戲啟動前設定固定端點。直接啟動官方 client 並沒有經驗證的任意 `--server` 切換功能，修改 hosts 也不會改寫 OAuth／API 設定。啟動器的原生遊戲流程仍待玩家實測。

本流程固定官方 [2026.1005.1-tachyon](https://github.com/ppy/osu/tree/6cbcae7fbb764839a66c1adf564c2e1679242072)，commit `6cbcae7fbb764839a66c1adf564c2e1679242072`，與本機已安裝 DLL 的 informational version 一致。它需要 .NET SDK 10.0.100 以上且符合上游 [global.json](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/global.json) 的 SDK roll-forward 規則、Git 與 NuGet 網路存取。

## 為什麼這個方法可用

上游的 [OsuGameBase.CreateEndpoints](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/OsuGameBase.cs) 是 public virtual；[EndpointConfiguration](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/EndpointConfiguration.cs) 可明確設定 Website、API 與三個 SignalR hub；[APIAccess](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/Online/API/APIAccess.cs) 接收這份設定並交給 OAuth、API requests 和 hub connector。離線 reflection 已確認本機已安裝 DLL 的上述公開合約。

`OsuGameDesktop` 是 internal，因此原創 adapter 加入獨立 checkout 的 Desktop assembly，再由該 checkout 的啟動程式建立它。Illyaverse 不修改已安裝 DLL，也不提交／散布上游 source、resources 或官方 client secret；Git 下載的上游保留其原有授權，位於忽略的 `data-lazer-client/upstream`。

## 預覽、準備、建置

在 Illyaverse 根資料夾執行：

```powershell
.\tools\kaleidoscope\scripts\prepare-lazer-client.ps1
.\tools\kaleidoscope\scripts\prepare-lazer-client.ps1 -Prepare
.\tools\kaleidoscope\scripts\prepare-lazer-client.ps1 -Build
```

第一個命令只預覽。`-Prepare` 下載指定 commit，加入原創 `IllyaverseTestGame.cs`，並核對六個唯一的 bootstrap anchor 後修改該獨立 checkout。既有 checkout 會保留並拒絕重新初始化，不會 reset 其他修改。`-Build` 核對 HEAD 與 adapter 內容，再建置 Debug；兩個模式都不會啟動遊戲、寫入系統 hosts／憑證信任、修改安裝資料夾或讀取玩家帳號設定。

產物為：

```text
data-lazer-client/upstream/osu.Desktop/bin/Debug/net10.0/osu!.exe
```

原創 adapter 與 bootstrap 使用獨立 game name `illyaverse-lazer-test`、獨立 IPC pipe、停用 legacy IPC port、使用不檢查更新的 UpdateManager，並關閉自動查找 stable 安裝資料。上游依獨立 host 的 storage 載入設定；首次啟動仍需確認測試資料目錄，勿把它改成官方 client 的資料目錄。不要使用此 adapter 的 tournament 模式。

## 連到本機測試環境

先按[本機 HTTPS 文件](local-client-testing.md)準備獨立 Docker stack、公開 CA 指紋及 hosts 預覽。只有明確決定套用後，才用該文件的 `connect-client-test.ps1 -Mode Apply -Write -ExpectedSha256 ...` 寫入；client build 不會代做這一步。

| 項目                       | 設定                                                                             |
| -------------------------- | -------------------------------------------------------------------------------- |
| 玩家網站                   | `https://illya.test`                                                             |
| API／OAuth root            | `https://osu.illya.test`；API requests 自行加 `/api/v2`，OAuth 加 `/oauth/token` |
| Spectator                  | `https://spectator.illya.test/spectator`                                         |
| Multiplayer                | `https://spectator.illya.test/multiplayer`                                       |
| Metadata                   | `https://spectator.illya.test/metadata`                                          |
| Client identity            | `illyaverse-local-client`／公開的測試 client value；不是官方 OAuth secret        |
| Editor BSS／liveness probe | 未設定；不宣稱支援原生 editor 投稿                                               |

**只使用獨立的 Illyaverse 測試帳號。** 準備完成後由使用者手動開啟上面的測試執行檔，依序測試登入、好友／聊天、下載與匯入譜面、實際遊玩、成績／replay、觀戰／多人及重連。本流程至今只有建置與離線合約驗證，沒有啟動這個 GUI 或聲稱原生遊玩通過。

## 可進一步驗證的原生 API harness

公開 `APIAccess` constructor 接收獨立 `OsuGameBase`、指向臨時 NativeStorage 的 `OsuConfigManager`、自有 endpoints 與 version hash。測試程式可使用隔離伺服器的測試帳密呼叫 `Login`，以官方 `GetMeRequest`／好友／譜面／成績 request 類別經 `Perform`／`PerformAsync` 驗證真正的 OAuth、header、URL 與 DTO 解析。其 callback 經 framework scheduler 排程，不能把尚未處理 scheduler 的 callback 當作登入成功；最後必須 Dispose APIAccess 以結束 background thread，並清除測試資料。

這層 harness 可測 API requests，仍無法證明完整 GUI、渲染、實際 gameplay、TLS 信任與 SignalR 重連。它也不可讀取已安裝 client 的 `game.ini`、token 或官方帳號。

## 已有證據

2026-10-07：固定 commit 的原創 adapter 完整 Debug build 成功，**0 warnings／0 errors**。獨立離線 probe 載入新 assembly，在不執行 game constructor 或 host 的情況下呼叫 adapter 的 `CreateEndpoints`，確認 API root 與三個 hub 指向 `illya.test`，BSS／liveness 為 null。此結果證明原創連線 adapter 已編譯並提供正確設定；[協定相容性](protocol-compatibility.md)仍依各項實際測試記錄判定。

同日，原創 CLI harness 使用這份固定版本的 `OsuJsonWebRequest<T>`／JSON serializer 與 DTO，通過 **16 項 loopback fixture 和 20 項真 HTTP 測試**。後者只連到獨立新建 PostgreSQL／Docker project，驗證 multipart OAuth、空 ACK、好友、PM／公頻聊天、玩家資料、整數等級進度、譜面／checksum、score token、原生 `SoloScoreInfo` JSON 提交及兩種原生成績 DTO。測試之後已移除該 project 與臨時帳密；沒有使用主站或 `illya.test` 玩家資料。它沒有執行 `APIAccess` scheduler、GUI 或遊玩，因此手動 client release checklist 仍待完成。[完整驗證紀錄](verification.md)。
