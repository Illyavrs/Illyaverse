# 網站字體

英文與數字保留原有 Nunito Variable。繁體中文使用 TaiwanPearl 的分割網頁字體，
日文使用 Zen Maru Gothic 的分割網頁字體；兩者都從固定版本的作者來源建立，
由 Serendipity 自架提供，玩家瀏覽時不向 Google Fonts 或其他字體 CDN 請求。
Torus 沒有加入專案：使用者已選擇保留目前英文字體。

## 語言與字重

| 內容 | 字體來源 | 發布衍生字體名稱 | 已提供字重 |
| --- | --- | --- | --- |
| Latin、數字 | Nunito（既有 Fontsource 套件） | Nunito Variable | 既有可變字重 |
| 繁體中文 | TaiwanPearl | Illya Pearl | Regular 400、SemiBold 600 |
| 日文 | Zen Maru Gothic | Illya Maru | Regular 400、Bold 700 |

Nunito 在所有語系都排第一；中文介面的漢字接著使用 Illya Pearl，日文介面的
漢字使用 Illya Maru。TaiwanPearl 分割版讓 Zen 已支援的假名交給日文字體，
因此英／中文介面的日文曲名也會以 Illya Maru 顯示假名。TaiwanPearl 支援而
Zen 未涵蓋的 23 個假名／結合符號，另保留小型 fallback subset，包含分解
形式的濁音／半濁音。標有 `lang="ja"` 或 `lang="zh-TW"`
的局部文字可以自行切換漢字形態；兩個 CJK 家族與系統字體仍構成 fallback。
來源不支援的字元繼續由下一個字體處理，不把字元從玩家輸入或曲名刪除。

這次保留既有密集介面字級、行高、閱讀寬度與首頁字標動畫。指定字重之間由
瀏覽器依 CSS 字體選擇規則使用最接近的已提供字重，並非宣稱來源是可變字體。

## 授權與來源

[TaiwanPearl 原始專案](https://github.com/max32002/TaiwanPearl/tree/9ada49ab67e05e68f83b3dde46b260e2bc2a7c39)
與 [Zen Maru Gothic 原始專案](https://github.com/googlefonts/zen-marugothic/tree/553c872b216d1290e2902a466edcdc9682f0df6a)
採用 SIL Open Font License 1.1。TaiwanPearl 的內嵌授權宣告 Reserved Font Name
TaiwanPearl；分割是修改版本，因此發布的 family／full／PostScript 名稱改成
Illya Pearl。Zen Maru 分割版亦以 Illya Maru 區別。作者名稱、原始 copyright
和完整 OFL 都保留在每個字體的 name metadata，並附獨立可讀 notice：

- `apps/serendipity/public/fonts/taiwan-pearl/OFL.txt`
- `apps/serendipity/public/fonts/zen-maru-gothic/OFL.txt`
- `apps/serendipity/public/fonts/zen-maru-gothic/AUTHORS.txt`

TaiwanPearl copyright 為 Chun yu Yao（2020）；Zen Maru Gothic copyright 為
The Zen Maru Gothic Project Authors（2021）。這些字體資產仍依 OFL 散布，
不因應用程式的授權而改變字體本身的授權。

`public/fonts/provenance.json` 記錄原始下載網址、固定 commit、來源 SHA-256、
每個 WOFF2 的 SHA-256／大小／實際 Unicode 範圍，以及 stylesheet 和 notice
hash。TaiwanPearl 來源的四組 U+FE01 variation sequence 也保留於對應區塊的
cmap format 14，並在 manifest 和 checker 中獨立核對；variation selector
不是單獨可見 glyph，不算入一般 cmap 覆蓋數。原始大檔只存於忽略的
`data-fonts-20261009`，不隨網站映像或 Git 發布。

## 分割與重建

每個字重分成目前語系 UI 的 common subset，以及其餘字元的 512-codepoint
區塊。各區的 `unicode-range` 互不重疊；所有來源中符合這個 CJK 家族責任的
字元都保留，動態圖譜曲名與玩家名稱不限於目前翻譯使用的字元。Nunito 負責的
Latin 範圍，以及 TaiwanPearl 不負責的假名範圍，明列在 manifest。

瀏覽器只下載頁面實際文字需要的區塊。`font-display: swap` 允許 fallback
先顯示，沒有預載整個 CJK repertoire。部署直接使用已生成的資產，不需要
Python 或建置時連線下載字體。

要重建，安裝 `fonttools[woff]==4.63.0` 與 `brotli==1.2.0`，把 manifest 的
四個原始檔下載為 generator 中的固定檔名。generator 在任何寫入前核對四個
來源 hash；來源不同時中止，避免默默換成上游新版本。以下指令從 monorepo
執行，不會自行下載檔案：

```powershell
python apps/serendipity/scripts/build-cjk-fonts.py --source-dir data-fonts-20261009
python apps/serendipity/scripts/check-cjk-fonts.py --source-dir data-fonts-20261009
```

check 解碼每個已發布 WOFF2，核對全部實際 cmap、重疊／漏字、hash、衍生名稱、
copyright 與 OFL。Playwright 的 `tests/typography.spec.ts` 使用攔截的唯讀
首頁資料，透過 Chromium CDP 檢查實際使用的字體與 glyph 數，包含 `龘𠮷`
等動態字元、分解假名結合符號、局部語系、手機、淺／深色與 640×360 reflow
proxy（1280×720 在 200% 縮放時的等效 CSS viewport，並非實際瀏覽器縮放）；另核對字體
只從本站成功載入且沒有整套下載。這些是網站驗證，不代表原生遊戲驗證。
