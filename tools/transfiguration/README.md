# Transfiguration

原創的 Illyaverse lazer 連線橋接 DLL。啟動器重用玩家已安裝的官方
Windows x64 lazer 與 .NET runtime，以必要的 startup hook 在 `Main` 前安裝
endpoint、storage、HTTP 與 WebSocket guards，無須重新建置整個 lazer。

目前支援實測官方 `2026.1005.1.0`。未知版本、assembly hash 不符、任何必要
patch 失敗都拒絕啟動；相容性更新只發布這個小 DLL。此方式需要啟動器的
native hosting，單純把 DLL 放入 rulesets 不能提供必要 bootstrap 的保證。

## 建置與測試

使用 .NET 10 SDK 和符合 `compatibility.json` 的現有官方安裝：

```powershell
pwsh -File tools/transfiguration/build.ps1
pwsh -File tools/transfiguration/test.ps1 -NativeHost target/debug/examples/runtime_probe.exe
```

可用 `-NativeRoot` 指定安裝目錄。腳本讀取安裝 metadata/DLL 並驗證 hashes，
不修改安裝、hosts、CA 或任何玩家資料。package 在 `artifacts/`，正式暫存
安裝檔只有 `osu.Game.Rulesets.Transfiguration.dll`。隨啟動器發送 `artifact.json`、
`compatibility.json` 和第三方授權 notices；`proof-output/`、fixture DLL、osu!
assemblies 與 proof runtime 不可放進正式 package。

完整啟動器的本機測試 package 由
`tools/kaleidoscope/scripts/package-launcher.ps1` 組裝，必要 bridge notices、五份
授權與 `artifact.json` 都必須存在。組裝目錄與 ZIP 的 payload hashes 通過驗證
後才輸出完成 artifact；詳見 [package 指南](../kaleidoscope/README.md#本機測試-package)。
合成檔案的 package integrity 測試不執行 bridge，也不代表真實原生介面或登入
已驗證。

測試使用獨立的 compile-time loopback fixture（127.0.0.1:18376）與自己的測試
資料夾，沒有執行 osu! `Main`、GameHost、UI 或 Realm database，也沒有讀取玩家的
帳號快取。它驗證 stock assemblies 的實際 endpoint/storage 方法、同版
.NET 10.0.12 transport、foreign redirect 阻擋、官方 HTTP/WS 在連線前拒絕、
六種啟動參數拒絕，以及必要 native bootstrap 在測試 `Main` 前完成。正式 DLL
另有沒有網路操作的固定 policy proof。完整 stdout 留在 `proof-output/contract-proof.txt`。
遊戲 UI、登入與完整協定仍需玩家實機驗證。

帳號隔離 proof 使用真正的 stock `NativeStorage`、`OsuConfigManager` 和
`APIAccess` 建構子，資料僅有合成的有效／過期官方 token 與圖譜資源／Realm
標記檔。它驗證私服設定在 Load 前選用另一個檔名、官方 `game.ini` 保持
byte-identical，以及私服設定儲存、重載、記住密碼切換與 stock Logout handler。
proof 專用的 `APIAccess.run` prefix 讓 worker 在入口立即結束，避免登入或
refresh 網路工作；此 patch 不存在正式 DLL。這些測試不是實際登入／refresh
或共享 Realm schema 相容性驗證。

## RX 讀取 context

橋接在 `APIAccess` 建構子開始時取得 stock `SelectedMods` bindable。每個指定
API request 在建立時固定 vanilla/relax snapshot；只有 `GetMeRequest`、
`GetUserRequest`、`GetScoresRequest`、performance `GetUserRankingsRequest`、
best/recent `GetUserScoresRequest` 的 URI 加上 `variant=vanilla|relax`。
osu/taiko/catch 選到 RX 時讀取 relax，mania 永遠 vanilla。wire ruleset ID 仍為
0/1/2/3，沒有新增可玩的 ruleset 或 server selector。OAuth、聊天、提交、replay、
hub 與外部網址不加這個 query。Mapboard 的 RX/NM 讀取 filter 會配合所選
partition 正規化，request equality 也區分 partition；提交成績的 mods 不受影響。

RX 切換立即令 `LocalUserStatisticsProvider` 舊 cache 不可見，在 API update
scheduler 上清除、通知空的新 context 並 refetch 已使用的 rulesets。重新取得
資料前 PP 為未提供狀態；舊 generation、舊帳號或較早 refetch 的 response
不會覆寫新 statistics。測試只使用 fake API provider、記憶體中的 native DTO /
request / component 和未建構的 Game shell，沒有建立 `APIAccess` 或開啟 Realm
database。測試 double 的 internal-interface access metadata 只存在測試程式，
不在正式 bridge DLL。

目前 headless 已驗證 URI、filter、cache 與 scheduler 行為。切換 RX 後，已開啟
的其他玩家 profile／ranking overlay 是否由 stock UI 重新建立 request 仍待實機
確認；開啟新頁面時會使用當前 context。此 bridge 不重播登入 GetMe 或接管所有
API success callbacks。

## 啟動器契約

啟動器先解析玩家既有的 lazer 資料目錄，包括預設、portable 和 `storage.ini`
的自訂路徑。原本的 `client.realm` 與 `files/` 直接沿用，沒有獨立圖譜庫、複製
或再次匯入。啟動器驗證目錄和 `rulesets/` 不是 reparse points，再暫存正式 DLL
至該資料目錄的 `rulesets/`。guardian／journal 和 readiness 使用啟動器既有的
`%LOCALAPPDATA%\Illyaverse\Kaleidoscope` 控制目錄。

私服的帳號與遊戲設定存於同一資料目錄的 `game.illyaverse.ini`。橋接在
`IniConfigManager<OsuSetting>.Filename` 加上僅適用於 exact `OsuConfigManager`
instance 的 patch，確保基底建構子的 Load 已選用私服檔名。`APIAccess` 的
constructor prefix 在讀取 username/token 或啟動 worker 前再次驗證該 config
確實經過隔離建構子，並且 endpoints 是固定私服設定。官方 `game.ini` 不讀取、
不複製、不覆寫；私服第一次使用的遊戲偏好會從預設值開始。

遊戲名稱與 portable host option 保留；已解析的資料路徑由 bridge 固定，IPC
使用獨立的 `illyaverse-lazer`。橋接模式下不再套用第二次 custom-storage
redirect，遊戲內更換或遷移資料目錄也停用，避免改寫共用的 `storage.ini`。
stable 自動匯入停用。若要更換資料路徑，先退出私服啟動器，透過 stock lazer
設定後再從啟動器重開。

| Child process environment | 值 |
| --- | --- |
| `ILLYAVERSE_LAZER_INSTALL_ROOT` | 所選官方安裝的絕對本機路徑 |
| `ILLYAVERSE_LAZER_DATA_ROOT` | 玩家既有且已解析的 lazer 資料目錄，絕對本機路徑 |
| `ILLYAVERSE_LAZER_CONTROL_ROOT` | `%LOCALAPPDATA%\Illyaverse\Kaleidoscope` |
| `ILLYAVERSE_LAZER_SESSION` | 啟動器以 CSPRNG 產生的 64 hex nonce |
| `ILLYAVERSE_LAZER_READY_FILE` | control root 下 `session-ready-<nonce>.json`，必須不存在 |

native host 初始化自己的第一個 runtime context，強制
`System.StartupHookProvider.IsSupported=true` 與 `STARTUP_HOOKS=<DLL absolute path>`
後才 `hostfxr_run_app`。正式官方 Release 關閉了 startup hooks；單純設定
`DOTNET_STARTUP_HOOKS` 無效。失敗時不可啟動原始 `osu!.exe` 作為 fallback。
啟動器只在驗證 matching nonce 的這份精確 acknowledgment 後顯示 ready：

```json
{"protocol":1,"session":"<64 hex nonce>","status":"ready","endpoint":"https://illya.test","fixture":false}
```

橋接先驗證 versions、hashes 與 method contracts，安裝並確認所有 Harmony patch
owners 後才以 `CreateNew` 寫入 acknowledgment。它沒有 endpoint 選單或讀取 server
環境變數，API 固定 `https://osu.illya.test`，hubs 固定 `https://spectator.illya.test`；
OAuth client ID 與公開測試 client secret 固定為 `illyaverse-local-client` /
`illyaverse-public-test-client`。所有 transport 僅接受精確列出的 Illya HTTPS/WSS
hostnames 和 443，並禁用自動 redirects。BSS 和官方 liveness probe 為 null。
使用正常 TLS 驗證；`illya.test` 的本機 DNS 與受信任 Caddy CA 仍是本機測試前提。

正式啟動器負責 process identity、已執行 client 的關閉/重啟、guardian、journal
與遊戲退出後的 DLL 清理。bridge 本身不刪除已載入檔案。sentinel 不加入可玩的
ruleset；若相容 DLL 被手動放入官方 profile 後直接啟動，sentinel 拒絕執行。
官方 ruleset loader 可以忽略不相容 DLL，因此手動安裝、被修改的 DLL 或繞過
啟動器不是 no-fallback 保證的範圍。

## 設計依據

- [.NET native hosting API](https://github.com/dotnet/runtime/blob/main/docs/design/features/native-hosting.md)
- [osu! desktop Release startup hook 設定](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Desktop/osu.Desktop.csproj)
- [osu! endpoint、storage 與 API 建立順序](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/OsuGameBase.cs)
- [osu! custom storage 切換](https://github.com/ppy/osu/blob/6cbcae7fbb764839a66c1adf564c2e1679242072/osu.Game/IO/OsuStorage.cs)
- [osu-framework config Load/Save 與 Filename](https://github.com/ppy/osu-framework/blob/ab102608f363f24e0e33ec629a9bf613e72c9950/osu.Framework/Configuration/IniConfigManager.cs)
- [.NET SocketsHttpHandler transport](https://github.com/dotnet/runtime/blob/v10.0.12/src/libraries/System.Net.Http/src/System/Net/Http/SocketsHttpHandler/SocketsHttpHandler.cs)
- [Harmony 2.4.2](https://github.com/pardeike/Harmony/releases/tag/v2.4.2.0)

僅閱讀 LazerAuthlibInjection 理解技術可行性；此 implementation、guard 與 proof
皆原創。第三方 dependency 授權見 `THIRD-PARTY-NOTICES.md`。
