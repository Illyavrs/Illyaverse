# Sanctuary

Illyaverse 的 Rust 社群 API，負責帳號、權限、偏好、媒體、圖譜、成績、pp、Replay、
聊天與稽核。Sanctuary 會在服務邊界驗證登入、帳號狀態及每項寫入所需權限。

本目錄是 Sanctuary 子 repository 的來源邊界。從 Illyaverse parent 根目錄使用
共用 Cargo workspace／lockfile；`../../crates` 與 `../../migrations` 由 parent 提供：

```sh
cargo test -p sanctuary --locked
cargo clippy -p sanctuary --all-targets --locked -- -D warnings
```

資料庫整合案例需另外指定隔離 PostgreSQL fixture，不使用營運資料庫。
Docker build context 是 parent 根目錄；單獨 clone 本專案不包含共用 crates、schema
或完整部署環境。API 合約見 [API 文件](../../docs/api.md)，部署見
[部署文件](../../docs/deployment.md)，實作與真人測試範圍見
[完成度](../../docs/release-readiness.md)。

程式碼採 MIT 且為原創；參考專案僅用於架構／協定研究。
Commit 使用 `feat(sanctuary): ...`、`fix(sanctuary): ...` 等 scoped Conventional Commits。
