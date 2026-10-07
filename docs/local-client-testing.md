# 在這台 Windows 電腦測試 HTTPS 與遊戲客戶端

可以把測試伺服器部署在目前這台電腦。單機測試使用 `illya.test` 與 `127.0.0.1`，不需要購買網域、修改公開 DNS 或開放路由器。`localhost` 代表使用者自己的電腦；若之後要讓別台電腦連線，應另用實際主機 IP／公開部署設定。

此配置只準備本機 HTTPS、正確 Host 路由及獨立資料庫。**不代表 stable／lazer 已經通過原生客戶端驗證。** 實際測試仍需分別檢查登入、聊天、下載、遊玩、成績、replay、多人房間與重新連線。官方 lazer 發行版的任意私服切換尚未驗證；目前應使用設定 Illyaverse endpoints 的獨立 source build，詳見[連線文件](connection.md)。

## 獨立測試環境

從 Illyaverse 根資料夾執行以下命令。需要 Docker Desktop 的 Linux containers 與 Docker Compose 2.24.4 以上。

```powershell
.\infra\setup-client-test.ps1
docker compose --env-file .env.client-test.local -p illyaverse-client-test -f compose.yaml -f compose.client-test.yaml config --quiet
```

setup 會檢查 80／443 是否已有監聽服務，產生被 Git 忽略的 `.env.client-test.local`，並替資料庫、session、內部服務與測試管理員建立各自的隨機密鑰。重跑會保留既有檔案。請在本機讀取初始管理員帳號密碼，不要提交或分享該檔案。此腳本不會啟動容器、修改 hosts、安裝憑證、改防火牆或啟動遊戲。

準備好進行測試時，再啟動這組容器：

```powershell
docker compose --env-file .env.client-test.local -p illyaverse-client-test -f compose.yaml -f compose.client-test.yaml up -d --build --wait
docker compose --env-file .env.client-test.local -p illyaverse-client-test -f compose.yaml -f compose.client-test.yaml ps
```

每次命令都保留 `--env-file`、`-p` 與兩個 `-f`。這組環境使用 `illyaverse-client-test` 前綴的 PostgreSQL、media、Caddy data／config volumes，以及 `172.30.92.0/24` Docker network；與既有 `localhost:3000` 網站及 `localhost:8081` 協定預覽隔離。Caddy 是 `.10`，網站是 `.11`，後端只信任此網段的明確代理 IP。只有 `127.0.0.1:80` 與 `127.0.0.1:443` 會公開到主機；資料庫與後端沒有主機 port。

本機測試網站固定為 **`https://illya.test`**，並使用 Secure cookie。請在此根網域註冊及管理帳號；`osu.illya.test` 是遊戲客戶端規劃的 Website／API host。

## 本機 HTTPS 憑證

Caddyfile 使用 `tls internal`，替下列十個明確的 hostname 簽發本機憑證；`skip_install_trust` 關閉自動安裝信任。Caddy 的本機 CA 不使用 ACME／公開 DNS 驗證，客戶端必須另外信任該 CA。[Caddy Local HTTPS](https://caddyserver.com/docs/automatic-https#local-https)、[tls internal](https://caddyserver.com/docs/caddyfile/directives/tls#internal)。

```text
illya.test
osu.illya.test
c.illya.test
ce.illya.test
c4.illya.test
api.illya.test
spectator.illya.test
bss.illya.test
a.illya.test
b.illya.test
```

容器啟動後，只提取 CA 的**公開憑證**到 Git 忽略的資料夾。不要提取 `root.key`／`intermediate.key` 或分享整個 Caddy volume。官方 Caddy image 使用 `/data` 作為 XDG data root，因此公開 CA 路徑是 `/data/caddy/pki/authorities/local/root.crt`。[官方 image 定義](https://github.com/caddyserver/caddy-docker/blob/master/2.11/alpine/Dockerfile)。

```powershell
New-Item -ItemType Directory -Path .\data-client-test -Force | Out-Null
docker compose --env-file .env.client-test.local -p illyaverse-client-test -f compose.yaml -f compose.client-test.yaml cp caddy:/data/caddy/pki/authorities/local/root.crt .\data-client-test\caddy-root.crt
```

先檢查 subject、有效日期、SHA-256 憑證指紋與用於移除信任的 thumbprint。此步只讀取檔案：

```powershell
$taskCertificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 -ArgumentList (Resolve-Path .\data-client-test\caddy-root.crt).Path
$taskHasher = [System.Security.Cryptography.SHA256]::Create()
try { $taskFingerprint = [BitConverter]::ToString($taskHasher.ComputeHash($taskCertificate.RawData)).Replace('-', ':') } finally { $taskHasher.Dispose() }
$taskCertificate | Select-Object Subject, Issuer, NotBefore, NotAfter, Thumbprint
$taskFingerprint
```

## 一個命令預覽本機連線

建置 Kaleidoscope 後，執行 Windows helper：

```powershell
cargo build --release -p kaleidoscope
.\infra\connect-client-test.ps1
```

這個命令預設只預覽：顯示 CA subject、有效日期、SHA-256、thumbprint、既有信任狀態、十個 hosts 記錄、endpoint 與客戶端限制。它不會新增 journal、備份、hosts 記錄或憑證信任。`-Mode Apply` 與 `-Mode Restore` 不加 `-Write` 也只預覽。

若明確決定套用，請先核對預覽的指紋，再自行於提高權限的終端執行；範例的 placeholder 必須換成這次**實際核對過的 CA SHA-256**：

```powershell
.\infra\connect-client-test.ps1 -Mode Apply -Write -ExpectedSha256 'REPLACE_WITH_INSPECTED_CA_SHA256'
```

Apply 同時需要明確的 `-Mode Apply`、`-Write` 與相符的 `-ExpectedSha256`；也可再加 `-ExpectedThumbprint` 核對 SHA-1 thumbprint。它只接受有效、自簽發的公開 CA 憑證，拒絕 private key／bundle、不相符指紋與 reparse-point 路徑。憑證信任預設限定 `CurrentUser\Root`；信任此 CA 會讓其簽出的憑證受此使用者信任。

修改 Windows 系統 hosts 需要提高權限，helper 不會自己要求提權。請使用**與執行遊戲相同的 Windows 帳號**；若用另一個管理員帳號啟動終端，`CurrentUser` 是那個管理員而非原玩家。多使用者測試才明確指定 `-TrustScope LocalMachine`，並接受該電腦所有使用者的信任範圍。

helper 使用 Kaleidoscope 建立原始位元組備份、保留原有 BOM／換行、檢查衝突，再比較預覽 snapshot 才寫入。既有、不屬於這次 helper journal 的 Kaleidoscope 區塊會拒絕取代。操作紀錄保存於被 Git 忽略的 `data-client-test\connection-state.json`；不要在連線仍套用時刪掉它。

還原先預覽，再明確寫入：

```powershell
.\infra\connect-client-test.ps1 -Mode Restore
.\infra\connect-client-test.ps1 -Mode Restore -Write
```

Restore 只移除這次紀錄的 managed block，保留外部後來新增的 hosts 記錄；如果該 block 被改過，會停止並要求人工檢查。只有 helper 新增的 CA 才會移除；原先已存在的信任會保留，且移除前再核對 SHA-256／thumbprint。若重建了 Caddy CA，Restore 仍使用原操作紀錄核對舊 CA，不會移除新 CA。重跑已完成的 Restore 不再寫入。

若途中 hosts 或 trust 操作失敗，journal 會保留為 `incomplete`，可先 `-Mode Restore` 檢查，再明確還原；它不會用舊備份覆蓋整個 hosts。helper 的 lock 若因強制中止留在原處，確認已無執行中的 helper 後，再自行刪除 `connection-state.json.lock`。套用前可用[隔離測試腳本](../infra/tests/client-test-connection.ps1)驗證：

```powershell
.\infra\tests\client-test-connection.ps1
```

測試只使用臨時 hosts 與記憶體 trust provider，不修改真實 OS trust／hosts。`-HostsPath`／`-CertificatePath`／`-BinaryPath` 可指定測試用檔案；正式 wrapper 不提供任意 trust provider 注入。

若指定客戶端有自己的 trust store，需按它的設定另行信任同一 CA；不應關閉 TLS 憑證驗證。保留 Caddy data volume，重新啟動後才能繼續使用同一個 CA。

## 規劃 hosts 與驗證 HTTPS

先建置 Kaleidoscope，然後以十個明確的 `illya.test` 域名預覽。這組命令不包含官方 `ppy.sh` 域名，也不會寫入 hosts：

```powershell
$taskDomainArguments = @(
    '--ip', '127.0.0.1',
    '--domain', 'illya.test',
    '--domain', 'osu.illya.test',
    '--domain', 'c.illya.test',
    '--domain', 'ce.illya.test',
    '--domain', 'c4.illya.test',
    '--domain', 'api.illya.test',
    '--domain', 'spectator.illya.test',
    '--domain', 'bss.illya.test',
    '--domain', 'a.illya.test',
    '--domain', 'b.illya.test'
)
.\target\release\kaleidoscope.exe plan @taskDomainArguments
.\target\release\kaleidoscope.exe apply @taskDomainArguments --dry-run
```

以上是低階 Kaleidoscope 預覽。需要可一起還原 hosts 與 CA 的本機流程時，使用上一節的 helper；不要在它的操作紀錄仍有效時混用其他 `apply --write`／`restore --write`。完整低階操作與權限要求見[連線文件](connection.md)。

即使尚未套用 hosts，也可以用 curl 的 `--resolve` 做一次指定 URL 的連線測試；它不改 DNS 或系統 hosts。以下使用 CA 公開憑證驗證 TLS，不使用 `--insecure`：

```powershell
curl.exe --fail --silent --show-error --ssl-revoke-best-effort --cacert .\data-client-test\caddy-root.crt --resolve illya.test:443:127.0.0.1 https://illya.test/ --output NUL
curl.exe --fail --silent --show-error --ssl-revoke-best-effort --cacert .\data-client-test\caddy-root.crt --resolve c.illya.test:443:127.0.0.1 https://c.illya.test/health
```

Windows 內建 curl 使用 Schannel；此本機 CA 沒有公開 CRL distribution point，嚴格撤銷查詢可能回報 `revocation status is unknown`。上面的 `--ssl-revoke-best-effort` 只容許缺少／離線的撤銷資料，仍驗證憑證鏈、有效期與 hostname；此選項限於這個明確指定 CA 的本機測試命令。其他 TLS backend 不需要該選項。[curl 官方選項說明](https://curl.se/docs/manpage.html#--ssl-revoke-best-effort)。

Website 的 `/api/v1/*` 與 `/media/*` 送到 Sanctuary；legacy／OAuth／API v2 送到 Resonance。`/spectator`、`/multiplayer`、`/metadata` 及各自的 `/negotiate` 和 `/signalr/*` 都送到 Resonance。Caddy 的 reverse proxy 支援 WebSocket，SignalR 的 JSON SSE 也經同一條代理路徑；服務的 real-time 實作狀態見[協定相容性文件](protocol-compatibility.md)。

## 原生客戶端測試入口

Stable 的根網域規劃與啟動參數：

```powershell
.\target\release\kaleidoscope.exe profile --client stable --domain illya.test
# Launch only after closing existing osu! and preparing a separate test account.
& 'C:\Games\osu!\osu!.exe' '-devserver' 'illya.test'
```

路徑只是範例，請用實際安裝位置與獨立測試設定。此文件沒有啟動客戶端，也沒有修改官方帳號的連線設定。

Lazer 需獨立 source build 與資料目錄，不能以 stock client 的 hosts 切換當作完成。Kaleidoscope 以下只輸出規劃 manifest，並不是官方客戶端可直接匯入的設定：

```powershell
.\target\release\kaleidoscope.exe profile --client lazer --domain illya.test
```

對應 source build 的 Website／API 是 `https://osu.illya.test`，三個 hub 是 `https://spectator.illya.test/spectator`、`/multiplayer`、`/metadata`。BSS hostname 預留路由，但目前不宣稱支援原生 editor 投稿。OAuth、score／replay 與指定版本的實際測試仍需依[連線文件](connection.md)逐項完成。

## 停止測試

```powershell
docker compose --env-file .env.client-test.local -p illyaverse-client-test -f compose.yaml -f compose.client-test.yaml down
```

`down` 保留這組測試資料與 CA。不要隨意加 `--volumes`；刪掉 Caddy data 後重建會產生另一個 CA。系統 hosts 與已匯入的憑證信任需各自明確還原，不會隨容器停止自動變更。

若本次測試曾由你明確允許暫停 WSL Ubuntu 的 nginx，先停止這組容器釋放 port 80，再恢复原服務；這不會修改 nginx 的開機設定：

```powershell
wsl --distribution Ubuntu --user root --exec systemctl start nginx
```
