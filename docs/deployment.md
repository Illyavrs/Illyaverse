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

升級前先備份 DB 與 shared data，並在隔離環境驗證可還原。以下 Linux／macOS 例子使用 PostgreSQL custom-format dump 與 tar，backup 目錄應私有：

```sh
mkdir -p backups
chmod 700 backups
docker compose exec -T postgres pg_dump -U illya -d illyaverse -Fc > backups/illyaverse.dump
docker compose run --rm --no-deps --user root --entrypoint tar sanctuary -czf - -C /data . > backups/shared-data.tar.gz
```

需要 DB／檔案時間點一致時，先進入維護狀態、停止會寫入的 Sanctuary／Resonance，再完成備份，最後啟動服務。PowerShell 7.4 以上可使用 native stdout binary redirection；舊版 Windows PowerShell 可能轉換二進位輸出，請改用容器內檔案加 `docker cp` 保存 dump。Caddy 的 volume 也應納入主機備份，避免不必要的重複發證。

現在的媒體 volume 是單機儲存。如果之後要水平擴充，需先加入共用 object storage 與多實例一致的節流／chat event transport；不能只增加 replica 而沿用每個容器各自的本機檔案或記憶體狀態。

### Migration 換行與 checksum

`migrations/*.sql` 必須保持 UTF-8、無 BOM、LF 換行。此目錄的 `.gitattributes` 規則明確設定 `text eol=lf`，讓 Windows、Linux／macOS 的 Git checkout 和來源 archive 保持相同 SQL bytes，無須修改全域 `core.autocrlf`；[Git 官方文件](https://git-scm.com/docs/gitattributes/2.43.0#_eol) 說明明確的 `eol` 優先於預設換行設定。建置前執行 `node --test infra/tests/migration-source.test.mjs`，會驗證目前所有 migration 的 bytes／Git blob，以及 `core.autocrlf=true`、`false`、`input` 的獨立 checkout 與 archive。

已套用的 migration 內容不可修改；schema 變更請新增版本。鎖定的 [SQLx 0.8.6 使用 SQL bytes 計算 SHA-384](https://github.com/launchbadge/sqlx/blob/v0.8.6/sqlx-core/src/migrate/migration.rs#L25)，因此只把 LF 換成 CRLF 也會改變 checksum，並在啟動時產生 `VersionMismatch`。曾用 CRLF migration 建立的資料庫，不會因為加入 LF checkout 規則自動相容。

若既有部署出現 mismatch，停止更新並保留資料庫／媒體備份、原部署來源與 image；不要修改歷史 SQL 或 `_sqlx_migrations` checksum 來繞過驗證。先在隔離環境比對原 SQL bytes 與目前 migration、驗證備份可還原，再由營運者明確制定並驗證遷移方案。尚未完成方案前，保持 migration 驗證失敗時停止啟動。

Docker 建置保留依賴快取，但每次執行 Rust 建置步驟前，以 [package-scoped `cargo clean`](https://doc.rust-lang.org/cargo/commands/cargo-clean.html) 刷新該服務的 release 產物，重新嵌入目前 migration。這避免從較舊 commit／archive 還原來源時，Cargo 的 [mtime 變更檢查](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed) 沿用快取中的舊 migration 定義。更新後先對新 image 執行 disposable HTTP／瀏覽器測試，通過後再替換既有服務；不需要清空資料庫或整份依賴快取。
