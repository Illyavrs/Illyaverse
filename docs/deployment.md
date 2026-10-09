# Docker 部署

Illyaverse 的預設 Compose 提供本機網站與 API，正式配置另加自己的 DNS／HTTPS。`setup` 先產生獨立密鑰，`docker compose up -d --build --wait` 建置並依健康狀態啟動全部服務。需要啟動中的 Docker daemon 與 Compose 2.24.4 以上；production override 使用 [官方支援的 `!override`／`!reset` merge 行為](https://docs.docker.com/reference/compose-file/merge/)。

## 本機啟動

Windows：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File infra/setup.ps1
docker compose config --quiet
docker compose up -d --build --wait
docker compose ps
```

Linux／macOS：

```sh
sh infra/setup.sh
docker compose config --quiet
docker compose up -d --build --wait
docker compose ps
```

從 [http://localhost:3000](http://localhost:3000) 存取網站。`PUBLIC_ORIGIN` 的 host、scheme、port 必須和瀏覽器實際來源一致；如果改用 `http://127.0.0.1:3000`，也要同步修改設定並重建 Sanctuary 容器。

`.env.example` 的密鑰故意留白，Compose 不提供共用弱密碼。初始化腳本對 PostgreSQL、session signing、內部服務與初始管理員各使用 32 個隨機 bytes，並輸出成 64 位 hex；它不會列印密鑰，也不會覆蓋現有 `.env`。Linux／macOS 建立的檔案權限是 `0600`。請在本機讀取 `BOOTSTRAP_ADMIN_USERNAME`／`BOOTSTRAP_ADMIN_PASSWORD`，使用後清空三項 `BOOTSTRAP_ADMIN_*` 並重新建立 Sanctuary，讓管理員密碼改動不再和 bootstrap 設定衝突。

新安裝的資料庫與媒體會建立在 named volumes 中：

| Volume | 內容 |
| --- | --- |
| `postgres_data` | PostgreSQL 資料，掛載 `/var/lib/postgresql/data` |
| `shared_data` | `/data/media`、`/data/maps`、`/data/replays` |
| `caddy_data` | TLS 憑證與 ACME 狀態 |
| `caddy_config` | Caddy runtime 設定 |

採用受支援的 PostgreSQL 17 系列，[官方版本政策](https://www.postgresql.org/support/versioning/) 說明其支援期限與 major 升級需求。對 17 image，volume 必須掛在 `/var/lib/postgresql/data`；[官方 image 文件](https://hub.docker.com/_/postgres) 說明資料目錄與初始化行為。不要只改 image major tag 就把既有資料當成已升級。

## 正式網域與 HTTPS

以下設定屬於伺服器部署。目前 Kaleidoscope 測試包的固定端點是 `illya.test`；正式網域上線還需要開發者更新並驗證啟動器與 Transfiguration 的固定端點，再發布對應套件。更改 `.env` 不會同步改寫玩家已安裝的啟動器，也不會提供玩家選服功能。

在 `.env` 設定真實 `ILLYA_DOMAIN` 與 `ACME_EMAIL`。營運者應把根網域與 `osu`、`c`、`ce`、`c4`、`api`、`spectator`、`bss`、`a`、`b` 子域名的 A／AAAA 指向部署主機；只發布確實能到達主機的 IPv6。防火牆允許 TCP 80／443，HTTP/3 可用 UDP 443。

```sh
docker compose -f compose.yaml -f compose.production.yaml config --quiet
docker compose -f compose.yaml -f compose.production.yaml up -d --build --wait
```

網站位於 `https://<ILLYA_DOMAIN>`。正式 override 會移除本機 3000／8081 的映射，僅發布 Caddy 的 80／443，並強制 Sanctuary `PUBLIC_ORIGIN=https://<ILLYA_DOMAIN>`／`COOKIE_SECURE=true`。Caddy 取得及續訂 HTTPS 憑證；這依賴域名驗證成功與持久化的 Caddy volume，詳見 [Caddy automatic HTTPS](https://caddyserver.com/docs/automatic-https)。

`osu.<ILLYA_DOMAIN>` 也提供遊戲 legacy paths 與網站；瀏覽器操作請使用根網域的 canonical origin。協定 Host 路由已配置，接口的功能程度仍由 Resonance 決定：增加一個 HTTPS Host 不代表全部遊戲流程都已通過驗證。正式接納玩家前請按 [相容性文件](protocol-compatibility.md) 與 [連線教學](connection.md) 測試選定的客戶端版本。

## 服務設定

| 變數 | 用途 |
| --- | --- |
| `POSTGRES_PASSWORD` | DB 密碼；setup 產生的 hex 適合目前 DATABASE_URL |
| `SESSION_SECRET` | 至少 32 bytes，簽署／驗證網站 session |
| `INTERNAL_SERVICE_KEY` | Resonance 與 Sanctuary 之間的受信任請求 |
| `PUBLIC_ORIGIN` | 本機瀏覽器 mutating request 來源；production 由 override 決定 |
| `COOKIE_SECURE` | 本機 HTTP 為 false；production 強制 true |
| `ILLYA_DOMAIN`／`ACME_EMAIL` | 正式域名與 ACME contact |
| `BOOTSTRAP_ADMIN_*` | 初始管理員建立；用過後清空 |
| `OSU_CLIENT_ID`／`OSU_CLIENT_SECRET` | 可選的官方譜面 metadata 匯入 |
| `OSU_API_INTERVAL_MS` | 官方請求節流間隔，預設及最小值 1000ms；difficulty 與整包 metadata 共用節流 |
| `RUST_LOG` | 服務 log filter |
| `SMTP_HOST`／`SMTP_FROM` | 可選的帳號復原與信箱驗證寄信服務 |
| `SMTP_USERNAME`／`SMTP_PASSWORD` | SMTP 認證，必須成對設定 |
| `SMTP_TLS`／`SMTP_PORT` | 預設 STARTTLS／587，可改 implicit／465 |
| `MAIL_ENCRYPTION_KEY` | 獨立 64 位隨機 hex，加密待寄連結；需私有備份 |
| `MAIL_ALLOW_LOCAL_DEV` | 預設 false；只供隔離 loopback 郵件測試 |

不要把 `POSTGRES_PASSWORD` 換成帶 `@`、`:` 等符號的未編碼值而直接放進目前的 URL；保留 setup 的隨機 hex，或自行同步調整正確 URL escaping。變更 PostgreSQL 的 `.env` 密碼不會替既有資料庫執行 `ALTER ROLE`；需由管理員同步資料庫認證後重建服務。變更 session secret 會讓既有簽名 session 失效。

公開註冊可搭配 SMTP 啟用郵件驗證與忘記密碼。未設定時，能力 API 與網站會顯示未開放；不會假稱郵件已寄出。正式網站 origin 必須使用 HTTPS，佇列會加密保管一次性連結並重試短暫失敗。設定和保留密鑰的方式見 [帳號復原](account-recovery.md)。

Bridge subnet 預設是 `172.30.91.0/24`。Caddy 固定 `.10`，Serendipity 固定 `.11`，Sanctuary 只信任這兩個地址的 forwarded headers，Resonance 只信任 Caddy。如果和主機現有網路衝突，請同步修改 Compose 的 subnet、兩個 `ipv4_address` 與兩個 Rust 服務的 `TRUSTED_PROXY_CIDRS`。不應把所有外部來源都列為 trusted proxy。

## 開發與健康檢查

需要從主機執行 Rust／整合測試時：

```sh
docker compose -f compose.yaml -f compose.dev.yaml up -d postgres
```

PostgreSQL 只綁定 localhost `15432`。主機的 `DATABASE_URL` 使用 `postgres://illya:<POSTGRES_PASSWORD>@127.0.0.1:15432/illyaverse`。Sanctuary 的 `BIND_ADDRESS=127.0.0.1:8080`、Resonance 的 `RESONANCE_BIND=127.0.0.1:8081`；把主機端 `MEDIA_ROOT`／`MAPS_ROOT`／`REPLAY_ROOT` 與 `STORAGE_PATH` 配到同一資料目錄。`npm run dev` 預設 rewrite 到 `http://127.0.0.1:8080`。

```sh
docker compose logs --tail 100 sanctuary resonance serendipity caddy
docker compose exec sanctuary curl --fail http://127.0.0.1:8080/health/ready
docker compose exec resonance curl --fail http://127.0.0.1:8081/health
```

Compose [以 `service_healthy` 等待依賴](https://docs.docker.com/compose/how-tos/startup-order/)；它不會把容器剛啟動視為 DB 或 migration 已完成。Serendipity 使用 Next.js [standalone output](https://nextjs.org/docs/app/api-reference/config/next-config-js/output)，連同 public／static 資產一起封裝；內部 API rewrite 在 image build 時設定為 Sanctuary 的 Docker DNS 地址。

## 停止、更新與備份

`docker compose down` 停止並移除容器，但保留 named volumes。`docker compose down -v` 會刪掉資料 volumes，請只在明確要重置資料時使用。更新來源與 lockfiles 後，以相同 Compose 配置 `up -d --build --wait`；對 production 每次都帶上兩個 `-f`，避免退回本機設定。

### 建立可還原的備份

升級前先備份資料庫、整個 `shared_data` volume 與部署密鑰，並在隔離環境驗證可還原。`/data/media`、`/data/maps`、`/data/replays` 必須屬於同一個停止寫入的時間點；只保存資料庫或只保存上傳圖片不足以還原部署。

1. 先切到維護狀態，阻止新的網站、遊戲與背景工作請求。使用來源部署原有的 project name、`.env` 與 Compose files，停止 Sanctuary／Resonance；production 每次都帶上兩個 `-f`。其他直接連到資料庫或 `/data` 的自訂 writer 也必須停止。保留 PostgreSQL 啟動，讓 `pg_dump` 可以讀取它。
2. 記錄來源 commit、image IDs、PostgreSQL major、成功套用的 migration versions／checksums、各資料表 row counts，以及 `/data` 檔案的相對路徑、SHA-256 與 UID／GID。這些私有基準必須在停止寫入後取得，供還原時比對。目前來源 migration 到 `0022_rank_history.sql`；應有 versions 1–22 且 `success=true`，不可因為新資料庫能啟動就省略核對。
3. 執行 backup helper，指定正確來源 project 與一個**尚不存在**的私有輸出目錄。以下 project name 是範例，必須改成來源部署的實際名稱；每次備份使用新的目錄。

本機 Compose：

```sh
docker compose -p illyaverse -f compose.yaml stop sanctuary resonance
node infra/recovery-backup.mjs --project illyaverse --output /private-backups/illyaverse-20261009-01 --writers-stopped
```

production：

```sh
docker compose -p illyaverse -f compose.yaml -f compose.production.yaml stop sanctuary resonance
node infra/recovery-backup.mjs --project illyaverse --output /private-backups/illyaverse-20261009-01 --writers-stopped
```

Windows 的 `--output` 可換成例如 `D:\PrivateBackups\illyaverse-20261009-01`。父目錄也要是只有備份營運者可讀取的目錄；Linux／macOS 使用 `0700` 目錄與 `0600` 檔案，Windows 用受限 ACL。備份包含帳號、郵件佇列及玩家檔案，請加密保存並限制存取。

`--writers-stopped` 是營運者對維護狀態的確認。helper 依 Docker Compose labels 找到選定 project 的來源容器及預設 named volumes，不依賴目前 checkout 的 `.env` 或 Compose 配置，也沒有 `--production` 選項。它還會檢查來源 Sanctuary／Resonance 已停止，並在備份過程重新確認；它不會自動停止、啟動或刪除來源服務與 volumes。helper 在 PostgreSQL 容器的 `/tmp` 寫出 custom-format dump，再透過 `docker cp` 保存；shared data 由獨立 helper 以唯讀掛載封裝，使用來源 Sanctuary 的本機 immutable image ID，保留整個 `/data` 和數字 ownership。這避免 shell 重新導向二進位 stdout 的相容性問題，也不會覆寫上一份備份。

成功備份包含 `illyaverse.dump`、`shared-data.tar.gz` 和 `manifest.json`。manifest 的 `format` 必須為 `illyaverse-recovery-v1`、`complete` 必須為 `true`，兩份 artifacts 的 `bytes`／`sha256` 必須通過 helper 的驗證。它另記錄來源 project、PostgreSQL major 17、`sanctuaryImage` 和備份包含／排除項目，供還原檢查。若 dump、複製、tar 或最後驗證失敗，保留當次目錄與 partial artifacts 供診斷；不要把部分檔案當成成功備份，也不要在同一目錄重跑來掩蓋失敗。改用新目錄重試，確認完整 manifest 和所有 artifact 雜湊後，才依來源原配置恢復 writer 服務。來源 migration／rows／檔案清單等基準需另行私有保存，manifest 不會自動替營運者建立這些內容。

另外將來源 `.env` 與其他部署 secrets 以私有方式保存在同一個備份組中；helper 的核心 dump／tar 不代替密鑰備份。至少保留 `POSTGRES_PASSWORD`、`SESSION_SECRET`、`INTERNAL_SERVICE_KEY`、`MAIL_ENCRYPTION_KEY` 和已使用的 SMTP／OAuth credentials。還原郵件佇列必須使用原 `MAIL_ENCRYPTION_KEY`，重新執行 setup 產生新密鑰無法解密既有佇列。不要把 secrets 放進 Git、公開 log 或演練報告。

Caddy 的 `caddy_data`（憑證、私鑰、ACME 狀態）及 `caddy_config` 可另做受限的主機／volume snapshot：先停止 Caddy，完成這兩個 volume 的一致快照再啟動它。保存 `/etc/caddy` 的來源設定和 `.env`，並按原 domain／Caddy 配置還原。這屬於獨立的 TLS 狀態備份，不包含在核心 recovery helper 或 disposable drill；不要以執行 drill 代表憑證已備份或還原。

### 只還原到全新 project 與 volumes

還原先在隔離 Docker 主機或獨立 daemon 執行，使用備份對應的來源 commit／images。預設 Compose 使用固定 subnet 與 proxy IP；同一 daemon 上直接啟動第二組原配置可能和來源網路衝突。保持 DNS、公開 ingress 和 Caddy 關閉，先完成資料驗證。還原目的地必須是全新 project、空白 PostgreSQL 17 與全新 `shared_data` volume；若發現已有容器、volume 或業務資料，立即停止，不使用 `--clean`、`down -v` 或覆寫檔案來強行還原。

以下 PowerShell 範例在隔離主機、備份對應的 checkout 內執行。先私有還原 `.env`，清空 `BOOTSTRAP_ADMIN_*`，並讓 SMTP 暫時停用以避免驗證期間寄出舊佇列；保留原 `MAIL_ENCRYPTION_KEY`。`$backupPath` 必須是完整備份的絕對路徑，`$restoreProject` 每次更換。先核對 manifest 的完成狀態、兩份 artifact 的 SHA-256 及來源基準。

```powershell
$restoreProject = 'illyaverse-restore-20261009-01'
$backupPath = (Resolve-Path -LiteralPath 'D:\PrivateBackups\illyaverse-20261009-01').Path
node --input-type=module -e "import { verifyBackup } from './infra/recovery-lib.mjs'; await verifyBackup(process.argv[1]);" $backupPath
if ($LASTEXITCODE -ne 0) { throw 'Incomplete backup or artifact hash mismatch.' }
$manifest = Get-Content -LiteralPath (Join-Path $backupPath 'manifest.json') -Raw | ConvertFrom-Json
$restoreTarImage = $manifest.sanctuaryImage
if ($restoreTarImage -notmatch '^sha256:[a-f0-9]{64}$') { throw 'Manifest must identify the exact source Sanctuary image.' }
$existingContainers = @(docker ps -a --filter "label=com.docker.compose.project=$restoreProject" --format '{{.ID}}')
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Docker; stop recovery.' }
$existingVolumes = @(docker volume ls --filter "label=com.docker.compose.project=$restoreProject" --format '{{.Name}}')
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Docker volumes; stop recovery.' }
$allVolumeNames = @(docker volume ls --format '{{.Name}}')
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect all volume names; stop recovery.' }
$plannedVolumes = @('postgres_data', 'shared_data', 'caddy_data', 'caddy_config') | ForEach-Object { "${restoreProject}_$_" }
$nameConflicts = @($plannedVolumes | Where-Object { $allVolumeNames -contains $_ })
$existingNetworks = @(docker network ls --filter "label=com.docker.compose.project=$restoreProject" --format '{{.ID}}')
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Docker networks; stop recovery.' }
if ($existingContainers.Count -ne 0 -or $existingVolumes.Count -ne 0 -or $nameConflicts.Count -ne 0 -or $existingNetworks.Count -ne 0) {
    throw 'Restore project already exists; choose a new project and empty volumes.'
}
docker compose -p $restoreProject -f compose.yaml config --quiet
if ($LASTEXITCODE -ne 0) { throw 'Invalid restore configuration.' }
$sharedVolume = "${restoreProject}_shared_data"
docker volume create --label "com.docker.compose.project=$restoreProject" --label 'com.docker.compose.volume=shared_data' $sharedVolume
if ($LASTEXITCODE -ne 0) { throw 'Could not create a fresh shared_data volume.' }
docker compose -p $restoreProject -f compose.yaml up -d --no-deps --no-build --pull never --wait postgres
if ($LASTEXITCODE -ne 0) { throw 'Fresh PostgreSQL failed to start.' }
```

啟動 PostgreSQL 後、啟動任何 Sanctuary 容器前，檢查 `illyaverse` 的 public schema 沒有業務表；此時 PostgreSQL 的 image 已建立空白 DB，不應有 `_sqlx_migrations`。若有表，停止並調查目的地，另選全新 project。再將 dump 複製到這個 PostgreSQL 容器：

```powershell
$publicTableCount = docker compose -p $restoreProject -f compose.yaml exec -T postgres psql -U illya -d illyaverse --no-psqlrc --set ON_ERROR_STOP=1 -At -c "SELECT count(*) FROM pg_tables WHERE schemaname = 'public';"
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect the fresh database.' }
if ($publicTableCount.Trim() -ne '0') { throw 'Database already contains tables; refuse restoration.' }
$postgresContainer = docker compose -p $restoreProject -f compose.yaml ps -q postgres
if ($LASTEXITCODE -ne 0 -or -not $postgresContainer) { throw 'Fresh PostgreSQL container is missing.' }
docker cp (Join-Path $backupPath 'illyaverse.dump') "${postgresContainer}:/tmp/illyaverse-recovery.dump"
if ($LASTEXITCODE -ne 0) { throw 'Dump copy failed.' }
docker compose -p $restoreProject -f compose.yaml exec -T postgres pg_restore --list /tmp/illyaverse-recovery.dump
if ($LASTEXITCODE -ne 0) { throw 'Dump archive cannot be read.' }
docker compose -p $restoreProject -f compose.yaml exec -T postgres pg_restore -U illya -d illyaverse --exit-on-error --no-owner --no-acl /tmp/illyaverse-recovery.dump
if ($LASTEXITCODE -ne 0) { throw 'Restore failed; retain this project for diagnosis and retry in a new one.' }
```

先讀取 `pg_restore --list` 的 archive 目錄，確認它包含來源 schema／table data／migration 記錄，再進行還原。[PostgreSQL 17 的 `pg_restore` 文件](https://www.postgresql.org/docs/17/app-pgrestore.html) 說明這些選項；`--exit-on-error` 遇錯即停止，`--no-owner --no-acl` 讓新 DB 使用目的地 `illya` 角色管理物件。這個流程刻意不使用 `--clean`，也不在 Sanctuary 已 migration 的 DB 上灌入 dump。

接著使用上面明確建立的**新** `shared_data` volume，先列出 archive 再解開。將 manifest 記錄的來源 Sanctuary image 以相同 immutable ID 預先載入目的地；`--pull never` 不會改用較新 tag，`--entrypoint tar` 不會啟動 Sanctuary／migration。這個 helper 以 root 保留數字 UID／GID、mode 與目錄結構，備份目錄唯讀，只有新 volume 可寫：

```powershell
docker image inspect $restoreTarImage --format '{{.Id}}'
if ($LASTEXITCODE -ne 0) { throw 'Load the exact source Sanctuary image before restoring files.' }
docker run --rm --pull never --network none --read-only --user 0:0 --security-opt no-new-privileges --mount "type=bind,source=$backupPath,target=/backup,readonly" --entrypoint tar $restoreTarImage -tzf /backup/shared-data.tar.gz
if ($LASTEXITCODE -ne 0) { throw 'Shared data archive cannot be read.' }
docker run --rm --pull never --network none --read-only --user 0:0 --security-opt no-new-privileges --mount "type=bind,source=$backupPath,target=/backup,readonly" --mount "type=volume,source=$sharedVolume,target=/data" --entrypoint tar $restoreTarImage --numeric-owner -xzf /backup/shared-data.tar.gz -C /data
if ($LASTEXITCODE -ne 0) { throw 'Shared data restore failed; retain this project and retry in a new one.' }
```

Sanctuary／Resonance 使用 `10001:10001`，必須確認 `/data/media`、`/data/maps`、`/data/replays` 和其中的檔案具有來源 ownership，且服務使用者能讀寫它們；不要只把三個子目錄的檔案重新上傳。

還原後、Sanctuary 啟動前，比對來源成功 migration versions／checksums、全部資料表的 row counts 與代表資料內容，並核對 `/data` 的完整檔案列表、SHA-256 與 ownership。這些結果應和停止 writer 後保存的來源基準一致。若來源是目前版本，`_sqlx_migrations` 應保留 versions 1–22 全部成功；不能把空白 DB 上重新跑完 22 個 migration 當成資料還原通過。

只有上述比對通過後才啟動 Sanctuary，讓 SQLx 正常驗證已還原的 migration。先將 Compose 的三個 app image 指向來源記錄的 immutable images，或確認本機 tags 的 image IDs 完全相符，再用 `--no-build --pull never` 啟動備份對應版本；要升級到較新 commit，另依正常 migration 流程在這份可丟棄的還原副本驗證。接著啟動 Resonance／Serendipity，檢查 readiness 與實際資料存取：

```powershell
docker compose -p $restoreProject -f compose.yaml up -d --no-build --pull never --wait sanctuary resonance serendipity
if ($LASTEXITCODE -ne 0) { throw 'Restored services did not become healthy.' }
docker compose -p $restoreProject -f compose.yaml exec -T sanctuary curl --fail http://127.0.0.1:8080/health/ready
if ($LASTEXITCODE -ne 0) { throw 'Sanctuary is not ready.' }
docker compose -p $restoreProject -f compose.yaml exec -T resonance curl --fail http://127.0.0.1:8081/health
if ($LASTEXITCODE -ne 0) { throw 'Resonance is not healthy.' }
```

確認使用原帳號登入、權限、譜面檔案、媒體與 replay 讀取，再恢復 SMTP 和對外流量。正式目的地仍需使用 production override，確認其 domain／origin／TLS 配置及 Caddy 憑證狀態，全部通過後才切換 ingress／DNS。readiness 成功是服務已就緒的證據，不能代替 DB row counts、檔案雜湊或真實客戶端相容性驗證。

### 可丟棄的 recovery 演練

`node infra/disposable-recovery-drill.mjs --dry-run` 可先檢視隔離演練計畫，不啟動 Docker。live 演練需明確設定允許旗標：

```sh
ILLYA_ALLOW_DISPOSABLE_TEST=1 node infra/disposable-recovery-drill.mjs
```

PowerShell：

```powershell
$env:ILLYA_ALLOW_DISPOSABLE_TEST = '1'
node infra/disposable-recovery-drill.mjs
Remove-Item Env:ILLYA_ALLOW_DISPOSABLE_TEST
```

演練使用獨立可丟棄的來源／還原 projects，在停止自建 writer 後呼叫同一個 `backupDeployment` 營運 helper，實際驗證來源 guard、唯讀封裝、binary-safe `docker cp`、完成 manifest，再還原 dump 與完整 `/data`，比對 migration／全部 rows／sequences／schema／檔案 hashes／ownership 和 readiness；它不會使用營運 project 的資料。成功結果只證明當次程式版本與演練資料的流程，正式備份仍需私有密鑰、來源基準及實際還原驗收。

Schema 比對先在演練來源 PostgreSQL 內建立新的 schema-only reference DB，透過 PostgreSQL 17 parser 正規化來源定義，再比對還原後的完整 schema。這涵蓋 `pg_restore` 對 CHECK 內相同邏輯 AND 括號的正規化；原始 schema dump 雜湊也會保留。reference DB 隨演練來源 volume 清除，migration checksums 仍直接比對全部 22 份來源 SQL bytes。

現在的媒體 volume 是單機儲存。如果之後要水平擴充，需先加入共用 object storage 與多實例一致的節流／chat event transport；不能只增加 replica 而沿用每個容器各自的本機檔案或記憶體狀態。

### Migration 換行與 checksum

`migrations/*.sql` 必須保持 UTF-8、無 BOM、LF 換行。此目錄的 `.gitattributes` 規則明確設定 `text eol=lf`，讓 Windows、Linux／macOS 的 Git checkout 和來源 archive 保持相同 SQL bytes，無須修改全域 `core.autocrlf`；[Git 官方文件](https://git-scm.com/docs/gitattributes/2.43.0#_eol) 說明明確的 `eol` 優先於預設換行設定。建置前執行 `node --test infra/tests/migration-source.test.mjs`，會驗證目前所有 migration 的 bytes／Git blob，以及 `core.autocrlf=true`、`false`、`input` 的獨立 checkout 與 archive。

已套用的 migration 內容不可修改；schema 變更請新增版本。鎖定的 [SQLx 0.8.6 使用 SQL bytes 計算 SHA-384](https://github.com/launchbadge/sqlx/blob/v0.8.6/sqlx-core/src/migrate/migration.rs#L25)，因此只把 LF 換成 CRLF 也會改變 checksum，並在啟動時產生 `VersionMismatch`。曾用 CRLF migration 建立的資料庫，不會因為加入 LF checkout 規則自動相容。

若既有部署出現 mismatch，停止更新並保留資料庫／媒體備份、原部署來源與 image；不要修改歷史 SQL 或 `_sqlx_migrations` checksum 來繞過驗證。先在隔離環境比對原 SQL bytes 與目前 migration、驗證備份可還原，再由營運者明確制定並驗證遷移方案。尚未完成方案前，保持 migration 驗證失敗時停止啟動。

Docker 建置保留依賴快取，但每次執行 Rust 建置步驟前，以 [package-scoped `cargo clean`](https://doc.rust-lang.org/cargo/commands/cargo-clean.html) 刷新該服務的 release 產物，重新嵌入目前 migration。這避免從較舊 commit／archive 還原來源時，Cargo 的 [mtime 變更檢查](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed) 沿用快取中的舊 migration 定義。更新後先對新 image 執行 disposable HTTP／瀏覽器測試，通過後再替換既有服務；不需要清空資料庫或整份依賴快取。
