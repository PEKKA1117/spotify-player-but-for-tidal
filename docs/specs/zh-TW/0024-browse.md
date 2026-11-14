[English](../0024-browse.md) | 正體中文

<a id="0024--browse"></a>
# 0024 — 瀏覽

- **狀態**：implemented（2026-11-05）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#69](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/69)
- **相依於**：0006（implemented：頁面、歷史、隨捲動載入的清單、動作彈出視窗、窗格的分頁）、0008（implemented：按鍵對應、按鍵說明）、0011（implemented：mix 頁面、整份清單）、0012（implemented：篩選）、0020（implemented：選取）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Browse」一節與新按鍵；[`docs/config.md`](../../zh-TW/config.md) 新增 `BrowsePage` 並把它從略過的指令中移除，同時新增 `hide_browse_pages`；`examples/app.toml`；它們的 zh-TW 版本、`README.md` 與其 zh-TW 版本（AC15）

<a id="context"></a>
## 背景

到目前為止，播放器顯示的是使用者已經擁有的（音樂庫、mix）或自己要求的（搜尋）。Tidal 還提供它自己挑選的內容：個人首頁、編輯精選清單、新發行、曲風、心情、年代。spotify-player 在 `g b` 上有它的 `BrowsePage`，而 0011 把 Tidal 的首頁、探索與曲風頁面列為不在範圍內。

本規格新增一個以 `g b` 開啟的**瀏覽頁面**，以及它通往的頁面。凡是 0006 已有機制的地方，它都不新增機制：瀏覽頁面是歷史中的一個頁面，由播放器取得。它的區段是單一窗格的分頁（藝人頁面的機制），它的清單隨捲動載入（0006）或整份送達（0011），它的列就是 0006 的列，帶著 0006 的按鍵與動作。新的只有兩樣：一種通往另一個瀏覽頁面的列，以及一種可以容納多種類型的列的清單。

有兩個來源供應它，因為各自擁有對方缺少的東西（見「事實」）：

- **v1 `pages/*`**：編輯頁面，對每個使用者都相同，形狀穩定，有真正的分頁
- **v2 `home/feed/static`**：Tidal 自家應用程式的個人首頁（最近播放、推薦），沒有文件，也沒有分頁

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及涵蓋它的驗收條件

1. **區段很多的頁面放不下**（曲風頁面有 12 份清單，首頁有 17 份）→ 一個分頁窗格，以及隨使用中分頁捲動的分頁列（AC9、AC13）
2. **影片被當成曲目加入佇列** → `VIDEO_LIST` 模組、影片 mix 與只有影片的播放清單都不會變成列（AC3、AC6）
3. **Tidal 明天新增的模組型別讓頁面失敗** → 未知的模組與無法讀取的項目都被略過，絕不是錯誤（AC2、AC6）
4. **沒有文件的 v2 feed 改變或消失** → 失敗的首頁在它自己的頁面中說明；每個 v1 頁面都繼續可用（AC12）
5. **對不分頁的清單分頁**：`view-all` 對某些模組遵守 `limit` 與 `offset`，對其他模組則忽略（見「事實」），所以分頁會重複或遺漏列 → 只請求一次，兩者都不帶，並保持整份（AC7）
6. **一份清單中有多種類型的列**：在曲目上按 `Enter` 會把清單其餘部分加入佇列，而清單中也有專輯與 mix → 只有曲目會加入佇列（AC10）
7. **客戶端的路徑可以觸及任何端點**：路徑透過 `Command` 從頁面資料送回播放器 → 播放器只跟隨兩種已知形式的路徑（AC8）
8. **被捨棄的列讓分頁位移**：播放器略過的列不得移動下一個 `offset` → `next_offset` 照舊依所請求的頁面大小前進，且區段的第一頁把它略過的列回報為 `hidden`（AC4）

<a id="behaviour"></a>
## 行為

<a id="the-root-page"></a>
### 根頁面

`g b`（`BrowsePage`）從 `pages/explore` 開啟根頁面。它是一個有四個分頁的窗格：

| 分頁 | 列 |
|-----|------|
| *Discover* | *Home*、*Staff picks*、*For you*，接著是 explore 無標題模組的連結（*New*、*Top*、*HiRes*、*Clean Content*） |
| *Genres* | explore 的 *Genres* 模組（Hip-Hop、Pop……） |
| *Moods & Activities* | explore 中同名的模組 |
| *Decades* | explore 的 *Decades* 模組 |

*Home* 是 v2 的個人 feed，*Staff picks* 是 `pages/home`（v2 標頭給編輯內容的名稱：Tidal 自己給該頁面的標題 *Home* 會衝突，所以該頁面也標題為 *Staff picks*），*For you* 是 `pages/for_you`。*Discover* 以外的分頁標題是 Tidal 的，所以沿用它的用字。

根頁面的每一列都是**連結**：`Enter` 開啟它所指的頁面。

<a id="browse-pages"></a>
### 瀏覽頁面

一個連結開啟一個**瀏覽頁面**：它的標題是該頁面的（`Hip-Hop`、`Top`、`Staff picks`），每個區段是它唯一窗格中的一個分頁，依 Tidal 的順序。`[` 與 `]` 切換分頁；只有一個區段的頁面不顯示分頁列。

- 區段的分頁是模組的標題。無標題的模組以它所含的東西命名：`Tracks`、`Albums`、`Playlists`、`Artists`、`Mixes`，連結則是 `More`
- 列的繪製方式與其他頁面相同：曲目如播放清單中的、專輯如音樂庫中的，依此類推。播放清單沒有 `♥`，如搜尋結果中一樣：這些回應不會說明你是否追蹤它
- 有**多種類型的列**的區段（只有 v2 feed 有）在每一列前面先顯示類型：`Album`、`Artist`、`Mix`、`Playlist`、`Track`
- 分頁列顯示放得下的所有分頁，一定包含使用中的分頁，在還有更多分頁的那一側顯示 `…`
- 低於 60 欄時，分頁列依 0006 對藝人頁面的規則：使用中分頁的名稱，加上 `[ ]`

<a id="which-rows-are-kept"></a>
### 保留哪些列

區段，v1：

| 模組 | 變成 |
|--------|---------|
| `TRACK_LIST`、`ALBUM_LIST`、`PLAYLIST_LIST`、`ARTIST_LIST`、`MIX_LIST` | 這些列的區段 |
| `PAGE_LINKS`、`PAGE_LINKS_CLOUD` | 連結的區段 |
| `VIDEO_LIST`、`FEATURED_PROMOTIONS`、`MULTIPLE_TOP_PROMOTIONS`、其他任何東西 | 什麼都不變成 |

區段，v2：`items` 的每個模組都變成一個區段，不論它的 `type`；它每個項目的類型決定保留什麼（`TRACK`、`ALBUM`、`PLAYLIST`、`ARTIST`、`MIX`；其他任何東西，如 `DEEP_LINK`，都捨棄）。

兩者都略過的列：

- 沒有 `STEREO` 的曲目（0011 的規則）
- `type` 含有 `VIDEO` 的 mix（0011 的規則；v1 中是 `mixType`，v2 中是 `type`）
- `numberOfTracks` 為 0 的播放清單（影片播放清單：曲目 0，影片 59 以上）
- 標題為空的連結

沒有剩下任何列的區段會被捨棄。沒有剩下任何區段的頁面顯示 `Nothing to show here`。

<a id="hiding-pages"></a>
### 隱藏頁面

`app.toml` 中的 `hide_browse_pages` 列出 TUI 不顯示其連結的瀏覽頁面，以 `pages/` 之後的名稱表示：

```toml
hide_browse_pages = ["videos", "tidal_magazine"]   # the default
```

- 預設值包含兩個已知沒有東西可顯示的頁面：依上述規則處理後，`pages/videos` 與 `pages/tidal_magazine` 是空的（「事實」）
- `[]` 不隱藏任何東西：這些連結會回來，並開啟成 `Nothing to show here`
- 名稱與連結的頁面完全相符，含大小寫；沒有任何連結用的名稱不起作用。只有連結被隱藏：以其他方式到達的頁面仍然會開啟
- 它套用於區段的每一頁，不只第一頁，而且已載入全部列的區段，計算的是它顯示的列
- 環境變數是 `TIDAL_PLAYER_HIDE_BROWSE_PAGES`，形式同 `TIDAL_PLAYER_HIDE_VERSIONS`
- TUI 讀取它，作為顯示什麼的選擇：播放器仍然回應每個連結，每個客戶端套用自己的清單，而所有連結都被隱藏的區段會被捨棄

<a id="lists"></a>
### 清單

- **v1 區段**的前幾列放在頁面的回應中。列數少於其總數的區段，隨捲動從它的 `dataApiPath` 要求更多，與 0006 的清單完全相同。沒有 `dataApiPath` 的區段是整份的
- **v2 區段**帶有預覽（最多 15 列）。有 `viewAll` 的區段在它的分頁第一次顯示時取得它，整份清單取代預覽：游標留在原處，因為預覽就是清單的開頭。在它送達之前，分頁的標題以 `…` 結尾，而且只有那時才會：尚未顯示的分頁沒有標記，因為 `…` 也出現在被截斷的標題結尾。若失敗，預覽保留，並由訊息列說明
- 任一種區段都可以像任何清單一樣用 `/` 搜尋；v1 區段會取得更多頁，直到有足夠的列符合（0012）

<a id="keys"></a>
### 按鍵

一切都是 0006 與 0011 的：`Enter` 開啟專輯、播放清單、藝人或 mix 頁面；在曲目上按 `Enter` 會播放它，並把清單中它之後的曲目接在後面；`Z`、動作彈出視窗、選取與 `g a` 在列上的作用與在其他地方相同。

| 列 | `Enter` | `Z`、動作、選取 |
|-----|---------|-------------------------|
| 曲目、專輯、播放清單、藝人、mix | 與每個頁面相同 | 與每個頁面相同 |
| 連結 | 開啟瀏覽頁面 | 什麼都沒有：連結沒有動作，也不能被選取 |

`Backspace` 回到上一頁，分頁與游標保持原樣（0006 的歷史規則）；從另一個頁面按 `g b` 會重新取得根頁面。

在有多種類型的區段中，在曲目上按 `Enter` 只把該區段的曲目加入佇列；其他列被略過。

<a id="talking-to-the-player"></a>
### 與播放器溝通

0006 的 `Library` 請求，新增：

- `BrowseRef`：`Root`（`pages/explore`）、`Home`（v2 feed）、`Page(String)`（一個 `pages/…` 路徑）
- `PageRequest::Browse(BrowseRef)` → `PageData::Browse { title: String, sections: Vec<BrowseSection> }`
- `BrowseSection { title: String, rows: ListItems, more: BrowseMore }`：0006 的 `ListItems`，新增 `Mixes(ListPage<MixSummary>)`、`Links(ListPage<PageLink>)` 與 `Mixed(ListPage<MixedRow>)`
- `BrowseMore`：`None`（整份）、`Data(String)`（一個 `dataApiPath`：分頁）、`ViewAll(String)`（一個 `viewAll` 路徑：取得一次）
- `MixedRow`：`Track(Track)`、`Album(AlbumSummary)`、`Playlist(PlaylistSummary)`、`Artist(ArtistRef)`、`Mix(MixSummary)`：有多種類型的區段中的一列，是 0006 的 `Row` 的擁有（owned）形式（不是 `crate::Item`，它以 ID 指稱曲目、專輯或播放清單，供連結與指令使用）
- `PageLink { title: String, target: BrowseRef }`
- `ListRef::BrowseData(String)`、`ListRef::BrowseViewAll(String)`，兩者都以 `ListItems` 回應

只有一種類型的區段就是該類型的清單，只有真正含有多種類型的區段才是 `Mixed`。播放器依它保留下來的列、逐次回應來決定：`view-all` 可能比它的預覽含有更多類型（*Recently played*：`ALBUM` 與 `DEEP_LINK` 只出現在那裡），所以回應不論類型為何，都取代視窗的列。

UI 模型：`PageKind::Browse(BrowseRef)`、`WindowKind::BrowseSection`、`Rows::Links(Vec<PageLink>)`、`Rows::Mixed(Vec<MixedRow>)`、`Row::Link`。曲目的區段與任何其他曲目清單一樣是 `Rows::Tracks`，所以已經能用在那些列上的東西在這裡不用改就能用。`Rows::get` 把 `MixedRow` 轉成它那種類型的 `Row`，所以所有經過 `Row` 的東西都不需要新增什麼；直接比對 `Rows::Tracks` 而且也必須看到混合區段的曲目的部分，由 AC10 涵蓋。`Window` 的標題來自區段，而不是來自它的類型。

<a id="requests"></a>
### 請求

v1，如 0011（`countryCode`、`deviceType=BROWSER`、`locale=en_US`）：`GET /v1/pages/explore`、`/v1/pages/<name>`；帶 `limit` 與 `offset` 的 `GET /v1/<dataApiPath>`，其中 `limit` 是頁面大小，上限 50。

v2（`countryCode`、`deviceType=BROWSER`、`locale=en_US`、`platform=WEB`，標頭 `x-tidal-client-version: <the version of tidal-player-api>`）：`GET /v2/home/feed/static`，之後只要回應帶有 `cursor` 就帶著它繼續，最多 5 次請求；不帶 `limit` 或 `offset` 的 `GET /v2/<viewAll>`。`viewAll` 路徑可能帶有它自己的查詢（`?itemId=…&rankingToken=…`），原樣保留。

<a id="acceptance-criteria"></a>
## 驗收條件

1. **AC1 協定**：`BrowseRef`、`MixedRow`、`BrowseSection`、`BrowseMore`、`PageLink`、新的 `ListItems` 變體、`PageRequest::Browse`、`PageData::Browse`、`ListRef::BrowseData` 與 `ListRef::BrowseViewAll` 都能經由 serde 與 socket 的封包格式來回轉換
2. **AC2 v1 區段**：頁面的模組依「保留哪些列」之下的表格，依 Tidal 的順序變成區段；`VIDEO_LIST`、兩種宣傳模組與未知的模組型別被略過，無法讀取的項目被捨棄，且不會讓頁面失敗
3. **AC3 略過的列**：沒有 `STEREO` 的曲目、影片 mix、有 0 首曲目的播放清單與標題為空的連結被略過，v1 與 v2 皆然；沒有剩下任何列的區段被捨棄；無標題的模組採用它那種類型的名稱
4. **AC4 v1 分頁**：列數少於其總數的區段，隨捲動以 `limit`（最多 50）與 `offset` 向 `dataApiPath` 要求，路徑自己的查詢保留；略過的列不改變下一個 `offset`（第一頁把它們回報為 `hidden`，不顯示在標題中），任何類型的空回應都結束該區段的載入，而沒有 `dataApiPath` 的區段從不請求
5. **AC5 根頁面**：`g b` 請求 `BrowseRef::Root` 並顯示四個分頁，*Discover* 包含 *Home*、*Staff picks*、*For you* 與 explore 的無標題連結；explore 的宣傳不顯示
6. **AC6 v2 feed**：feed 帶著它的參數與 `x-tidal-client-version` 標頭請求，跟隨 `cursor` 直到沒有為止，最多 5 次請求；每個模組變成一個區段，它的 `TRACK`、`ALBUM`、`PLAYLIST`、`ARTIST` 與 `MIX` 項目從 `data` 讀取（mix 的標題與副標題來自 `titleTextInfo.text` 與 `subtitleTextInfo.text`），其他任何東西都捨棄
7. **AC7 view-all**：v2 區段的分頁第一次顯示時，它的 `viewAll` 只請求一次，不帶 `limit` 也不帶 `offset`；清單不論其類型都取代預覽（單一類型的預覽可能變成 `Mixed`），保留游標，且絕不再次請求；失敗的取得保留預覽，並把訊息放在訊息列
8. **AC8 路徑**：播放器只在路徑以 `pages/`（v1）或 `home/pages/`（v2）開頭且沒有 `..` 片段時才傳送；其他任何東西都回應 `Not a Tidal page: <path>`，且不送出請求
9. **AC9 導覽**：在連結上按 `Enter` 推入一個瀏覽頁面並請求它；`[` 與 `]` 在它的分頁之間移動，會循環；在專輯、播放清單、藝人或 mix 上按 `Enter` 開啟那個頁面；`Backspace` 不經取得就還原分頁與游標；從另一個頁面按 `g b` 會取得根頁面
10. **AC10 列與動作**：在曲目上按 `Enter` 播放它，並把清單中接下來的曲目加入佇列，略過其他類型的列；`Z`、動作彈出視窗與選取在列上的作用與在任何頁面相同；在連結上它們什麼都不做，而標記一個範圍時會略過連結
11. **AC11 篩選**：`/` 依每一列的欄位篩選顯示的分頁（連結則依它的標題）；篩選屬於它的分頁，而 v1 區段會載入更多頁，直到有足夠的列符合
12. **AC12 失敗**：失敗的頁面在頁面中顯示失敗（`404` 時是 `Page <path> was not found`，其他情況用 0006 的用字）；失敗的 *Home* 說 `Home is not available: Tidal answered <status>`，並讓瀏覽的其餘部分保持可用；沒有區段的頁面說 `Nothing to show here`
13. **AC13 繪製**：在 150 × 42、80 × 24 與 50 × 20：標題、在還有更多分頁的那一側帶 `…` 的分頁列且使用中的分頁始終在視野內、只有一個區段時沒有分頁列、類型欄只出現在有多種類型的區段、117 個字元的區段標題被截斷以符合寬度
14. **AC14 按鍵對應**：`BrowsePage` 是 `g b` 上的指令，列在按鍵說明與按鍵提示中；它不再屬於略過的 spotify-player 指令，所以綁定它的 `keymap.toml` 可以運作
15. **AC15 文件**：`docs/tui.md`、`docs/config.md`、`README.md` 與它們的 zh-TW 版本描述該頁面、它的按鍵、它的來源與 `hide_browse_pages`；`examples/app.toml` 展示該設定；0008 中略過指令的範例不再提到 `BrowsePage`
16. **AC16 隱藏頁面**：`hide_browse_pages` 預設為 `["videos", "tidal_magazine"]`，且 `app.toml` 與 `TIDAL_PLAYER_HIDE_BROWSE_PAGES` 可設定它（環境變數優先，不是字串陣列的值會被拒絕，如 `hide_versions` 的處理）；通往列出頁面的連結不顯示，在根頁面、每個瀏覽頁面與區段的每一頁皆然，沒有剩下任何連結的區段被捨棄，而 `[]` 顯示全部

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 會發生什麼 |
|------|--------------|
| 只有一個區段的頁面 | 沒有分頁列：頁面看起來像一份清單 |
| 只有被略過的模組的頁面 | `Nothing to show here` |
| `hide_browse_pages = []` | *Videos* 與 *TIDAL Magazine* 的連結顯示，並開啟成 `Nothing to show here` |
| TUI 執行時編輯了 `hide_browse_pages` | 與每項設定一樣在啟動時讀取：下次執行時生效 |
| `v2` 回應 `400` | *Home* 說 `Home is not available: Tidal answered 400`；*Staff picks*、*For you* 與其餘都可用 |
| 歷史中保留的頁面上，`viewAll` 的 `rankingToken` 已過期 | 取得失敗，預覽保留，訊息列說明；`g b` 會取得全新的頁面 |
| 117 個字元的區段標題 | 分頁列是視窗的標題：未顯示的分頁被截成 24 欄，使用中的分頁放得下就完整顯示，放不下就截斷 |
| 兩個區段有相同的標題（*For you* 上的 *Custom mixes*） | 兩者都保留：它們是不同的清單 |
| 一列在區段中跨頁重複 | 只保留一次（0011 對 mix 的規則，依列的鍵） |
| 無法開啟的 mix（`HISTORY_*` mix，「事實」） | 0011 的頁面失敗；瀏覽頁面不受影響 |
| 付費專輯（*Upload for purchase*） | 如 Tidal 所列；對於無法串流的曲目，播放時顯示它對任何曲目所顯示的訊息 |
| 頁面在歷史中時 Tidal 的列改變了 | 回到上一頁顯示舊的列（0006 的規則）；再次開啟才會取得 |
| `429` | 頁面顯示 `Tidal answered 429: try again in a moment`；不會重試 |
| 工作階段過期、失去連線 | 0006 的規則 |
| 篩選結果為空 | 0012 的規則 |

<a id="test-plan"></a>
## 測試計畫

每個測試以它的條件命名（`ac4_…`）。紅燈是對存根型別與函式（本體為存根，沒有 `todo!()`，沒有編譯錯誤）的斷言失敗，與 0001–0023 相同。API 測試使用 `wiremock`，其測試資料（fixture）放在 `crates/api/tests/fixtures/browse/`，依「事實」之下的形狀寫成：只有公開的目錄資料；使用者 ID、最近播放的列與圖片 URL 已被取代或捨棄。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip`（新增列）+ `crates/app/src/ipc/codec.rs` :: `ac2_framing`（新增列） | 來回轉換；解碼 | 存根 `Serialize` 寫出 `null` |
| AC2 | `crates/api/tests/browse.rs` :: `ac2_v1_sections`（表格） | 模組 → 區段、順序、被略過與未知的型別、無法讀取的項目 | 存根保留每個模組 |
| AC3 | `crates/api/tests/browse.rs` :: `ac3_rows_left_out`（表格） | 每種被略過的列，v1 與 v2；被捨棄的區段；標題的備用值 | 存根保留影片的列 |
| AC4 | `crates/api/tests/browse.rs` :: `ac4_v1_paging`（表格）+ `crates/core/src/ui/browse/tests.rs` :: `ac4_section_scroll` | `limit`、`offset`、帶查詢的路徑；列被捨棄後的 `next_offset`；沒有路徑就不請求 | 存根捨棄路徑的查詢 |
| AC5 | `crates/api/tests/browse.rs` :: `ac5_root` + `crates/core/src/ui/browse/tests.rs` :: `ac5_root_page` | 四個分頁與 *Discover* 的列；`g b` 送出請求 | 存根 `g b` 什麼都不做 |
| AC6 | `crates/api/tests/browse.rs` :: `ac6_feed`（表格） | 參數、標頭、`cursor`、5 次請求的上限、類型、mix 欄位 | 存根在第一頁就停止 |
| AC7 | `crates/api/tests/browse.rs` :: `ac7_view_all` + `crates/core/src/ui/browse/tests.rs` :: `ac7_view_all_tab` | 沒有 `limit`/`offset`；第一次顯示時請求一次；取代、保留游標；失敗時保留預覽 | 存根從不請求 |
| AC8 | `crates/api/src/library/browse.rs` :: `ac8_browse_paths`（表格）+ `crates/api/tests/browse.rs` :: `ac8_refused_paths_send_nothing` | 接受與拒絕的路徑；不送出請求 | 存根接受每個路徑 |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_navigation`（表格） | 推入、分頁、循環、開啟列、`Backspace`、重新取得的 `g b` | 存根在連結上的 `Enter` 什麼都不做 |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_rows_and_actions`（表格）+ `crates/core/src/ui/select.rs` tests :: `ac10_links_not_selected` | 加入佇列的曲目略過其他類型；連結什麼都不做 | 存根把每一列都加入佇列 |
| AC11 | `crates/core/src/ui/filter` tests :: `ac11_browse_filter`（表格） | 混合區段的欄位、連結的標題、每個分頁各有一個篩選、更多頁 | 存根不符合任何連結 |
| AC12 | `crates/api/tests/browse.rs` :: `ac12_failures`（表格）+ `crates/app/src/tui/render.rs` :: `ac12_browse_failed_80x24` | 依狀態的用字；只有 *Home* 失敗；空頁面 | 存根以原始狀態碼失敗 |
| AC13 | `crates/app/src/tui/render.rs` :: `ac13_browse_150x42`、`ac13_browse_80x24`、`ac13_browse_narrow_50x20`、`ac13_browse_mixed_80x24`、`ac13_browse_one_section_80x24` | 快照 | 沒有快照 |
| AC14 | `crates/core/src/ui/keymap/tests.rs`（更新的列）+ `crates/core/src/ui/help/tests.rs`（新增列） | 指令、它的預設按鍵、說明文字；不再被略過 | 存根讓它仍被略過 |
| AC15 | 驗收時：閱讀這些頁面。`cargo xtask wiki` 建立它們 | 沒有測試：是文字敘述 | 不適用 |
| AC16 | `crates/app/src/config.rs` :: `ac16_hide_browse_pages`（表格）+ `crates/app/src/main.rs` :: `ac16_tui_state_hide_browse_pages` + `crates/core/src/ui/browse/tests.rs` :: `ac16_hidden_links`（表格） | 預設值、檔案、環境變數、拒絕；抵達 state；被隱藏的連結、被捨棄的區段、`[]`、完全相符 | 存根顯示每個連結 |

PR 手動操作一節的手動檢查（真實的 Tidal，tmux 150 × 42）：`g b` 與每個根分頁；一個帶分頁的曲風頁面，分頁列捲過邊緣；從區段開啟的專輯、播放清單、藝人與 mix；捲動一個長的區段（*New Albums*）超過它的第一頁，這也檢查 `pages/data` 上的 `limit=50`（「未驗證」）；*Home*、它的 17 個分頁、一個混合區段（*Recently played* 含有帳號自己的歷史，所以截圖使用另一個區段）；一個 `HISTORY_*` mix。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：型別、`Rows::Links`、`Rows::Mixed`、`Row::Link`、頁面與按鍵邏輯、隱藏連結（`State::hide_browse_pages`，在頁面抵達時套用）
- `tidal-player-api`：請求、DTO、列的規則與路徑檢查，放在新的 `crates/api/src/library/browse.rs`，讓 `library.rs` 不再變大；v2 的基底 URL 與標頭留在那裡
- `tidal-player`：繪製、分頁列與讀取 `hide_browse_pages`

<a id="facts-vs-assumptions"></a>
## 事實與假設

**事實**（實際探測，2026-11-05，國家 `NG`，兩個已移除的腳本，在本規格的第一個 commit 中；走訪了 26 個 v1 頁面與 v2 feed）：

- `pages/home`、`explore`、`for_you`、`genre_page`、`moods_page`、`hires`、`explore_new_music`、`explore_top_music`、`genre_*`、`mood_*`、`m_*` 與 `single-module-page/…` 全部回應 `200`；每一列含有一個模組
- 看到的模組型別：`PLAYLIST_LIST`（42）、`ALBUM_LIST`（20）、`TRACK_LIST`（10）、`VIDEO_LIST`（6）、`PAGE_LINKS`（6）、`MULTIPLE_TOP_PROMOTIONS`（4）、`MIX_LIST`（4）、`ARTIST_LIST`（4）、`PAGE_LINKS_CLOUD`（3）、`FEATURED_PROMOTIONS`（1）
- 一個 v1 頁面最多有 12 個可列出的模組；Hip-Hop 上的 11 個標題接在一起長達 169 個字元。最長的標題是 117 個字元。11 個模組沒有標題
- `pagedList.dataApiPath` 以 `limit` 與 `offset` 分頁（`limit=5&offset=5` 對專輯、播放清單、曲目、藝人、連結都給出預期的列）。在 `explore`、`genre_page`、`moods_page` 的 `PAGE_LINKS` 上它是 null，而且可能帶有查詢（`pages/data/<uuid>?albumId=10581228`）。`MIX_LIST` 的回應回報 `offset: 0`（0011）
- 連結項目是 `{title, apiPath, icon, imageId}`。有一個標題為空（`pages/social_justice`）
- `ALBUM_LIST` 的項目沒有 `type` 鍵；`PLAYLIST_LIST` 的項目沒有 `publicPlaylist` 或 `creator` 鍵（它們有 `creators`），`type` 為 `EDITORIAL` 或 `ARTIST`；目前的 DTO 讀得了它們（`Option` 或 `serde(default)`）
- 32 個播放清單的 `numberOfTracks` 為 0，全都是影片播放清單；依規則處理後 `pages/videos` 沒有別的東西，`pages/tidal_magazine` 只有 `MULTIPLE_TOP_PROMOTIONS`
- v1 的 `mixType` 值：`ARTIST_MIX`、`DAILY_MIX`、`DISCOVERY_MIX`、`NEW_RELEASE_MIX`、`VIDEO_DAILY_MIX`
- 未知的頁面：`404`，`{"status":404,"subStatus":2001,"userMessage":"Not found"}`
- v2 `home/feed/static` 在沒有 `x-tidal-client-version` 時回應 `400`（`subStatus` 1002），試過的任何值都回應 `200`，包括 `abc` 與 `0`：只有它是否存在才算數，所以播放器送出它自己的版本。它也需要 `locale` 與 `deviceType` + `platform` 兩者
- feed 是兩頁（`page.cursor`，然後沒有），17 個 `{type, moduleId, title, viewAll, items}` 的模組，屬於 `SHORTCUT_LIST`、`HORIZONTAL_LIST`、`HORIZONTAL_LIST_WITH_CONTEXT`、`COMPACT_GRID_CARD`；項目是 `{type, data}`，帶有 `TRACK`、`ALBUM`、`PLAYLIST`、`ARTIST`、`MIX`。區段混合多種類型（*Recently played*：它的 `view-all` 中有 `MIX`、`PLAYLIST`、`TRACK`、`ALBUM`、`DEEP_LINK`）
- `view-all` 回應 `{title, subtitle, description, filters, itemLayout, items}`，沒有總數或 cursor，10 到 200 列。`limit` 與 `offset` 被某些模組遵守（`RECOMMENDED_USERS_PLAYLISTS`），被其他模組忽略（`DAILY_MIXES` 在 `limit=5` 時給出 10 列），而 `SUGGESTED_RADIOS_MIXES` 在 `offset=5` 時沒有給出任何列。在比對的 5 個模組中，feed 的預覽就是 `view-all` 的開頭
- v2 的 mix：`type` 是 mix 的型別（`ARTIST_MIX`、`TRACK_MIX`、`DAILY_MIX`、`DISCOVERY_MIX`、`NEW_RELEASE_MIX`、`HISTORY_MONTHLY_MIX`、`HISTORY_YEARLY_MIX`、`HISTORY_ALLTIME_MIX`），30 位數的 ID，標題在 `titleTextInfo.text`，副標題在 `subtitleTextInfo.text`
- v2 的未知模組：`400`，`{"error":"BAD_REQUEST","httpStatus":400,"subStatus":1002}`，不是 `404`
- 在程式碼中：`Window::next_offset` 依所請求的頁面大小前進，`Window::whole` 標記從未要求更多的清單，`Window::hidden` 計算被略過的列，窗格可以有任意數量的分頁，而 `g b` 在預設值中是空著的

**在實機檢查中確認**（2026-11-05，真實的 Tidal）：`pages/data/…` 上的 `limit=50`（超過第一頁的清單）；`TRACK_MIX` 與 `HISTORY_*` mix 經由 0011 的端點開啟；v2 基底以相對 v1 基底的 `../v2/…` 表示；在區段的曲目上按 `Enter` 會播放它，而在混合區段中（*Recently played*：曲目、播放清單、專輯、mix），佇列只含有它的曲目。

**未驗證**：

- v2 中的影片 mix：沒有出現過，所以 `type` 中的 `VIDEO` 是從 v1 的 `mixType` 沿用來的
- `pages/artist_experiences`、`pages/credits`、`pages/clean_content` 與 `pages/genre_kids` 的內容是否合適（沒有走訪）；規則會略過不合適的
- 其他國家：每次探測都在同一個帳號上執行

<a id="decisions"></a>
## 決策

1. **區段是單一窗格的分頁**（使用者，2026-11-05），而不是區段清單或兩欄：一個按鍵顯示一個區段的列，並且重用藝人頁面的分頁，連同它的分頁載入、篩選與選取
2. **v1 與 v2 兩者並用**（使用者，2026-11-05）：個人首頁用 v2，所有編輯內容用 v1
3. **標頭帶著我們自己的版本**（`tidal-player-api` 的版本，由它送出）：Tidal 只檢查它是否存在，所以沒有模仿任何東西
4. **根頁面的 *Discover* 分頁**（使用者，2026-11-05）：*Home*、*Staff picks* 與 *For you* 加在 explore 的無標題連結之上，並以 *Staff picks* 作為 `pages/home` 的名稱
5. **已知為空的頁面由設定隱藏**（使用者，2026-11-05）：`hide_browse_pages`，預設為 `videos` 與 `tidal_magazine`，而不是寫死在程式碼中的名稱；任何其他空頁面都說 `Nothing to show here`
6. **只有一種類型的區段使用該類型的 `Rows`**（使用者的審閱，2026-11-05）：曲目本來就是一種列，所以把每一列包進只有瀏覽用的列舉，會重複 `Row` 與 `ListItems`，並讓每個對 `Rows::Tracks` 的比對都多一個情況。只有含有多種類型的區段才需要 `Rows::Mixed`

<a id="out-of-scope"></a>
## 不在範圍內

- v2 feed 的其他 vibe（它標頭中的 *Staff Picks*、*Uploads*）
- `CATEGORY_PAGES` 宣傳與通往播放器之外的 `EXTURL` 連結
- 影片與影片 mix
- `HORIZONTAL_LIST_WITH_CONTEXT` 的脈絡列（*Because you listened to* 顯示時不帶它所指的專輯）
- 從 shell 跟隨 Tidal 頁面，或給瀏覽用的 `playback` 指令
- 除了作為區段之外的 Tidal *Shortcuts*
- 在多次執行之間快取頁面
