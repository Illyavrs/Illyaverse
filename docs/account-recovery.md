# 帳號復原與郵件驗證

Illyaverse 開放公開註冊。郵件驗證是帳號設定中的狀態，不會阻擋現有玩家登入；忘記密碼透過原帳號的電子郵件復原。營運者未設定 SMTP 時，這兩項功能會顯示尚未開放，帳號註冊與既有登入仍可使用。

## 玩家流程

在登入頁選擇「忘記密碼」，輸入註冊信箱。對存在、不存在、受限制或仍在冷卻中的地址，服務皆回覆相同訊息；此回覆表示要求已受理，不能證明郵件已送達。收到郵件後，開啟同一個網站的連結，再主動提交新密碼。

重設連結有效 15 分鐘，只能使用一次。成功後需要重新登入：網站 session、stable session、lazer access／refresh token 都會撤銷。舊密碼與舊的復原連結失效，已保存的聊天、分數、replay 與個人資料保留。既有即時連線會依各自的 session 檢查關閉。

登入後可在帳號設定要求驗證郵件。驗證連結有效 24 小時，按下確認後才消耗連結；單純開啟頁面不會改動帳號。驗證狀態與電子郵件只會出現在自己的私有帳號 API。

連結使用 `/reset-password#token=…` 或 `/verify-email#token=…`。瀏覽器讀取 fragment 後立即清除網址中的 token，只在提交時放入 POST body；它不會成為 HTTP query、瀏覽器持久化設定或網站日誌。請保留信箱安全，並確認郵件指向營運者的正確網站。

## 營運者設定

正式部署使用能送信的 SMTP 帳號和 HTTPS `PUBLIC_ORIGIN`。在私有 `.env` 設定：

```dotenv
SMTP_HOST=smtp.example.com
SMTP_FROM=accounts@example.com
SMTP_USERNAME=your-smtp-username
SMTP_PASSWORD=your-private-smtp-password
SMTP_TLS=starttls
SMTP_PORT=587
MAIL_ENCRYPTION_KEY=your-independent-64-character-random-hex-key
MAIL_ALLOW_LOCAL_DEV=false
```

以上為占位值。`MAIL_ENCRYPTION_KEY` 必須是獨立的 32 隨機 bytes，以 64 位 hex 表示；不可使用全零或共用範例密鑰。新安裝的 `infra/setup.ps1`／`setup.sh` 會產生此密鑰，並保留既有 `.env`。SMTP 尚未設定時，單獨保留密鑰不會啟用寄信。

預設 STARTTLS 使用 587；`SMTP_TLS=implicit` 預設 465。可依寄信服務的文件明確指定 port。username／password 必須同時提供或同時留白；服務也支援由可信 SMTP relay 提供認證。TLS 憑證驗證維持啟用，不接受對外的明文 SMTP。

`MAIL_ALLOW_LOCAL_DEV=true` 只供隔離測試：可允許 loopback SMTP 搭配 `SMTP_TLS=none`，以及 loopback／`.test` 的 HTTP 網站來源。Compose 裡的 `127.0.0.1` 是該容器本身；主機的郵件工具並不會因為設定 loopback 就自動能被容器存取。

設定完成後執行 `docker compose up -d --force-recreate sanctuary`。不完整或不安全的 SMTP 設定會使 Sanctuary 啟動失敗，並回報不含密鑰的固定錯誤代碼。公開的 `/api/v1/auth/capabilities` 只提供兩個啟用狀態，不公開 SMTP 設定或佇列內容。

正式寄送仍需營運者完成寄信服務的域名驗證、SPF／DKIM 等設定，並使用自己的信箱檢查收件和垃圾郵件。程式測試中的 loopback SMTP 接受訊息，不等於外部信箱已能收到郵件。

## 儲存與重試

一次性 token 使用 256-bit 隨機值，資料庫只存 SHA-256 hash，綁定帳號、用途、簽發時的信箱與到期時間。簽發 token 與建立寄信工作在同一個 PostgreSQL transaction 完成；原始連結只存在以 XChaCha20-Poly1305 加密的郵件 payload 中。

加密資料另綁定工作 UUID、帳號、用途與網站 origin，不能把密文挪到另一個工作或不同網站。寄送工作使用 row lock／`SKIP LOCKED` 與 lease，服務重啟後可續寄。最多嘗試 8 次，短暫 SMTP 失敗會延後重試，永久拒絕不再重試；過期、已消耗、受限制或信箱變更的連結會取消寄送。完成或終止後清除加密 payload。

寄送採至少一次語意：SMTP 接受郵件後、資料庫確認前若服務中斷，重試可能重複送出同一封信。相同工作保留相同 Message-ID，連結仍只能成功使用一次。

服務以每批至多 32 筆的方式清理資料：消耗或過期超過 7 天的 token 與其郵件工作會刪除，地址限額紀錄保留至 24 小時。停用 SMTP 後，這些清理與過期 lease 的處理仍會繼續。

每個地址限制 60 秒一次、每小時 5 次，限額在 PostgreSQL 中跨 API 實例生效；公開復原 API 另有每來源 IP 每分鐘 5 次限制。未知地址的限額以 hash 儲存，不另外保存原信箱。簽入後更改密碼也會撤銷尚未使用的復原連結。

備份資料庫時一併保管私有 `MAIL_ENCRYPTION_KEY`，避免服務重啟後無法解密待寄工作。更換密鑰或 `PUBLIC_ORIGIN` 會讓舊的加密 payload 無法驗證；請先處理既有佇列，再更換設定。SMTP password 可獨立更換，不影響 payload 加密。

目前本機預覽未設定外部 SMTP。外部投遞、正式域名郵件設定與真實玩家復原流程仍需營運者實測；實作驗證紀錄見 [測試紀錄](verification.md)。

設計參照 [OWASP 帳號復原指引](https://cheatsheetseries.owasp.org/cheatsheets/Forgot_Password_Cheat_Sheet.html) 的一致回覆、隨機一次性 token 與到期原則；SMTP TLS 使用 [lettre 官方 API](https://docs.rs/lettre/0.11.23/lettre/transport/smtp/struct.AsyncSmtpTransport.html) 的必要 STARTTLS 或 implicit TLS 設定。復原與佇列的服務邏輯為 Illyaverse 原創實作。
