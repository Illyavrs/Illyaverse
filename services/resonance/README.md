# Resonance

Illyaverse 的 Rust stable／lazer 協定服務，負責 legacy web／stable 封包、lazer OAuth
與 API v2 相容接口、聊天、房間及原生成績流程。實作完成與隔離 fixture 通過，
不代表所有 stock client 功能已通過真人驗證。

本目錄是 Resonance 子 repository 的來源邊界。從 Illyaverse parent 根目錄使用
共用 Cargo workspace／lockfile；`../../crates`、`../../migrations` 與 Sanctuary
由 parent 的協調環境提供：

```sh
cargo test -p resonance --locked
cargo clippy -p resonance --all-targets --locked -- -D warnings
```

資料庫／即時協定整合案例需另外指定隔離 fixture。Docker build context 是 parent
根目錄；單獨 clone 本專案不包含共用 crates、schema 或完整部署環境。
協定狀態見 [相容性文件](../../docs/protocol-compatibility.md)，路由見
[架構文件](../../docs/architecture.md)，原生成績見
[提交文件](../../docs/native-score-submission.md)。

程式碼採 MIT 且為原創；參考專案僅用於接口行為研究。
Commit 使用 `feat(resonance): ...`、`fix(resonance): ...` 等 scoped Conventional Commits。
