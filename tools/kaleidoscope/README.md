# Kaleidoscope

Illyaverse 的原創 Rust 連線助手。玩家使用 `apps/kaleidoscope` 的 Tauri／Next.js 視窗，固定連到 Illyaverse；stable 使用 `-devserver`，lazer 使用原創 Transfiguration 強制啟動外掛。沒有玩家伺服器選單。

操作與退出清理見[啟動器指南](../../docs/kaleidoscope-launcher.md)。公開 DNS／HTTPS 環境不需要 hosts。現在的本機 `illya.test` 測試仍需開發 DNS／CA，真實原生遊玩尚待驗證。

以下 CLI hosts／profile 命令保留給開發者部署規劃，不是玩家 launcher 的伺服器設定。

```powershell
cargo run -p kaleidoscope -- --help
cargo run -p kaleidoscope -- profile --client stable --domain illya.example.com
cargo run -p kaleidoscope -- profile --client lazer --domain illya.example.com
```

`inspect`、`plan`、`apply` 與 `restore` 全部預設只讀／預覽。只有 `apply --write` 或 `restore --write` 會修改指定 hosts 檔。每次修改先產生原始位元組備份，並驗證結果。這些命令不啟動遊戲、不安裝憑證、不修改 DNS 快取。新 `launcher status/set-client/start/stop/recover` 使用同一個固定連線核心。

完整操作方式、TLS 要求與 lazer 相容性限制見 [連線文件](../../docs/connection.md)。
