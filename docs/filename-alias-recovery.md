# 舊圖譜封包的檔名修復

舊版 `.osz` 上傳可能保留了完整封包與難度檔案，卻沒有原始檔名對照。管理員可以從保留的原始封包驗證並補回對照，讓 filename-only lookup 找到既有圖譜。此操作不修改圖譜內容、checksum、難度／圖譜組 ID、owner、狀態、成績、Replay 或 package fingerprint。

## 操作

`POST /api/v1/admin/beatmap-packages/{id}/filename-aliases/backfill` 要求有效的網站 session 與 `beatmaps.moderate`。`id` 是 local package UUID，不是 difficulty ID 或 native set ID。可從圖譜 detail 的 `package.id` 取得。Cookie 請求需要設定的網站 Origin；Bearer 請求沿用 API 的驗證方式。

Body 上限 1 KiB，只接受 `apply` 布林值；未知欄位或錯誤型別回覆 422，malformed JSON 回覆 400，超出 body 上限回覆 413。`{}` 與 `{"apply":false}` 都是預覽，不寫入 alias 或 audit。先預覽，再以同一 UUID 提交 `{"apply":true}` 套用；套用會重新驗證，預覽不是繞過驗證的憑證。

成功回覆 HTTP 200，例如兩張難度都缺少對照的預覽：

```json
{
  "package_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
  "apply": false,
  "difficulty_count": 2,
  "verified_aliases": 2,
  "existing_aliases": 0,
  "missing_aliases": 2,
  "inserted_aliases": 0,
  "ambiguous_aliases": 0
}
```

`existing_aliases`／`missing_aliases` 是此次操作前的對照數量；`inserted_aliases` 是本次實際新增數量。`ambiguous_aliases` 計算會命中同名其他 difficulty 的候選對照數，包含同封包內不同難度使用相同 basename；不是 unique filename 數。回覆不包含儲存路徑、檔名或原始檔案內容。

已存在的精確對照不重複插入。只有實際新增 alias 時才在同一交易記錄一筆 `beatmaps.filename_aliases.backfill` audit，包含操作者、package UUID 與上述計數。預覽和無新增的重試沒有 audit。部分失敗不會留下部分 alias 或 audit。

## 驗證與限制

操作只接受保留原始 archive 的 local `.osz` package。原始 archive 最多 64 MiB，每個 retained `.osu` 最多 8 MiB；讀取前確認大小、普通檔案、可攜相對路徑及 maps root 邊界，拒絕 traversal、absolute／prefix 路徑與 symlink。它使用既有 raw ZIP 驗證與 UTF-8／CP932／Unicode Path 解碼，不猜測檔名、不重新打包，也不依曲名或 hashed storage filename 建對照。

解碼難度 checksum 必須無重複，完整 manifest 同時等於 immutable package manifest 與目前全部 member rows；每筆 row 的 local source、package、owner、archive 與 canonical identity 必須一致。每份 retained `.osu` bytes 必須與原始 archive member 完全相同。已建立的 archive SHA-256 與 byte length 必須吻合；未知的 legacy fingerprint 保持未知，不會因檔名修復而改成 `legacy=false`。

交易先取得現行操作者權限，再依 active set、精確 filename、checksum、map rows、package 的共同鎖順序執行；等待後及提交前重新檢查 session 到期。withdrawn set 不接受修復。缺少權限回覆 403，失效／受限／到期 session 回覆 401，共用 upload slots 忙碌時回覆 429。

| HTTP | Error code | 原因 |
| --- | --- | --- |
| 404 | `filename_backfill_package_not_found` | 找不到 local package |
| 409 | `filename_backfill_unsupported_package` | standalone `.osu` 沒有可驗證的原始 archive 檔名 |
| 409 | `filename_backfill_missing_file` | 原始 archive 或 retained member 缺少／無法讀取 |
| 409 | `filename_backfill_unsafe_file` | 不安全的路徑、非普通檔案、空檔或超出大小限制 |
| 409 | `filename_backfill_malformed_archive` | 原始 ZIP 無法依既有安全解碼規則讀取 |
| 409 | `filename_backfill_manifest_mismatch` | member 缺漏、重複、內容集合或身份不一致 |
| 409 | `filename_backfill_fingerprint_mismatch` | 原始 archive 與已建立 fingerprint 不一致 |
| 409 | `filename_backfill_member_mismatch` | retained `.osu` 與原始 member bytes 不一致 |
| 410 | `beatmap_withdrawn` | 圖譜組已撤回 |

同名的不同 difficulty 可以各自保存已驗證 alias。filename-only lookup 仍回覆 409 `beatmap_filename_ambiguous`，不任意選一張，也不因歧義查詢官方；checksum／native ID lookup 保持既有行為。

此 endpoint 不會在啟動、migration、upload retry 或玩家 lookup 時自動執行。Legacy positive embedded official ID 的原生限制、stable 混合 `Filenames`／`Ids` 的 indexing 與真人 client 驗證仍是獨立項目，檔名修復不代表它們完成。

## 驗證範圍

2026-10-09 的驗證記錄對應 Sanctuary image `sha256:6f1095b7c6f237e78ec38198e43954cc1dd853a92533b69e9f0195cd4bd7b1f5`：

- Rustfmt、Sanctuary all-target Clippy（warnings denied）、80 項非 ignored API 單元測試及 6 項 migration source checks 通過；109 項 ignored 測試並非全部重跑。
- 在新建、專屬 tmpfs PostgreSQL 17 上，9 項 backfill endpoint fixtures、10 項既有 filename、13 項 package 與 3 項 mutation fixtures 共 35 項通過。包含未知／已建立 fingerprint、正確 Unicode 配對、預覽不寫入、重複執行、壞封包／缺漏／改檔、權限撤銷／到期、同名歧義、並行套用、撤回及 commit rollback。Fixture 均完成清理，不使用保留中的資料庫。
- Linux release build 執行 2 項 reader／archive units，包含檔案及 ancestor symlink 拒絕；9 項 PostgreSQL fixtures 在此 Linux unit invocation 中維持 ignored，其通過證據是前項的獨立 PostgreSQL 執行。
- 使用上述實際 API image 與原有 Gateway image，專屬隔離 HTTP suite 通過 552 個請求、66 類檢查。新增維護流程占 15 個請求，驗證已有完整 CP932 package 的預覽／重複套用不新增 audit、權限與 body 拒絕、公開圖譜與完整 archive bytes 不變。這些 image HTTP 檢查驗證既有 aliases 的 no-op；缺少 legacy aliases 的實際新增另由 9 項 PostgreSQL fixtures 驗證。
- Guarded deployment 只 recreate 兩套 Sanctuary，十個服務均健康，其餘八個容器身分、image、health、timestamps、mounts／volumes 與原資料保持一致；52 個 client-test 與 4 個 main 原檔 SHA-256 不變，schema 22 不變。`https://illya.test` 在啟用憑證驗證下，未登入維護請求回覆 401。兩個保留資料庫目前沒有 local packages，因此本次沒有對玩家資料執行 backfill。

第一輪 withdrawal fixture 因同一 actor FK 與 set 的反向鎖順序而 deadlock；已改為不同管理員並遵循正式 authority → set 順序，handler 不變，重跑通過。原失敗記錄有保留，沒有把失敗當作通過。

可在自己的 disposable PostgreSQL 17 指定 `ILLYA_TEST_DATABASE_URL` 後重現 endpoint 與關聯 fixtures：

```powershell
cargo fmt --all --check
cargo test -p sanctuary --locked
cargo clippy -p sanctuary --all-targets --locked -- -D warnings
cargo test -p sanctuary --locked filename_backfill_tests -- --ignored --test-threads=1
cargo test -p sanctuary --locked beatmap_filename_tests -- --ignored --test-threads=1
cargo test -p sanctuary --locked map_package_tests -- --ignored --test-threads=1
cargo test -p sanctuary --locked map_mutation_tests -- --ignored --test-threads=1
```

Linux 的非 ignored reader cases 可用 `cargo test --locked --release -p sanctuary filename_backfill_tests` 執行。Final-image HTTP 使用 tracked `infra/disposable-protocol-smoke.mjs`；它建立隨機專屬 Compose project、固定當次本機 image ID，最後清理自身容器、network、volumes 與秘密環境檔。驗證不得指向玩家資料庫。

本次是 backend／隔離 HTTP 驗證，未執行真實 stable／lazer 遊戲或 Kaleidoscope 生命周期測試，不代表 native release checklist 完成。既有 native filename lookup 的契約見 [native discovery](native-beatmap-discovery.md)，ZIP 編碼與 immutable package 契約見 [local packages](../apps/docs/content/docs/operate/local-map-packages.md)。
