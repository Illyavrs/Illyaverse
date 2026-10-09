# Serendipity

Illyaverse 的玩家網站，使用 Next.js、Tailwind CSS、shadcn/ui 與 next-intl。
包含公開註冊、個人頁、圖譜搜尋、排行榜、聊天、外觀設定與權限對應管理介面。
API 由 Sanctuary 提供；實作與實際客戶端驗證的範圍分別記錄。

本目錄是 Serendipity 子 repository 的來源邊界。協調開發與 Docker 建置從
Illyaverse parent 根目錄執行，使用 parent 的 npm workspace／package-lock：

```sh
npm ci
npm run dev --workspace @illyaverse/serendipity
npm run lint --workspace @illyaverse/serendipity
npm run typecheck --workspace @illyaverse/serendipity
npm run build --workspace @illyaverse/serendipity
```

Docker build context 同樣是 parent 根目錄，以取得部署文件與 workspace manifest。
單獨 clone 此子專案不包含 parent 的 lockfile、後端、部署與整合測試環境。
資料夾與建置邊界見 [repository 導覽](../../docs/repository-layout.md)，
產品設計見 [DESIGN.md](../../DESIGN.md)，驗證見 [完成度](../../docs/release-readiness.md)。

英文／數字使用 Nunito；繁中與日文使用自架 OFL 字體。`public/fonts` 是需發布的
產品資產，notice／provenance 與完整 bytes 必須保留，見 [字體文件](../../docs/fonts.md)。
預設黑底，沒有 AI 生成插畫；禁止複製參考專案程式碼。

應用程式採 MIT；字體與其他第三方資產遵循各自隨附的授權。
Commit 使用 `feat(serendipity): ...`、`fix(serendipity): ...` 等 scoped Conventional Commits。
