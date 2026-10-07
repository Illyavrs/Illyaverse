# Kaleidoscope

Illyaverse 的原創 Rust 連線助手。名稱來自 kaleidoscope，負責在客戶端與不同世界之間提供清楚、可還原的連線設定。

```powershell
cargo run -p kaleidoscope -- --help
cargo run -p kaleidoscope -- profile --client stable --domain illya.example.com
cargo run -p kaleidoscope -- profile --client lazer --domain illya.example.com
```

`inspect`、`plan`、`apply` 與 `restore` 全部預設只讀／預覽。只有 `apply --write` 或 `restore --write` 會修改指定 hosts 檔。每次修改先產生原始位元組備份，並驗證結果。Kaleidoscope 不會啟動遊戲、安裝憑證或修改 DNS 快取。

完整操作方式、TLS 要求與 lazer 相容性限制見 [連線文件](../../docs/connection.md)。
