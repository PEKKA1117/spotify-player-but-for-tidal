[English](../0007-search.md) | 正體中文

<a id="0007--search"></a>
# 0007 — 搜尋

- **狀態**：implemented（2026-10-08；「測試計畫」下的手動檢查在使用者的機器上執行）；API 的資料形狀已由即時探測驗證（2026-10-08，「事實」）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0006（implemented：頁面、歷史紀錄、隨捲動載入的視窗、列上的 `Enter`/`Z`/動作、`Library`/`LibraryReply`）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Search」一節與新的按鍵；[`docs/playback.md`](../../zh-TW/playback.md) 的「Settings」新增搜尋頁面大小（AC14）

<a id="context"></a>
## 背景

0006 讓使用者瀏覽已經屬於自己的內容。要播放其他任何東西，他們仍得貼上連結或 ID（0004 的 `o`/`O`）。本規格加入 spotify-player 的**搜尋頁面**：輸入查詢字串，在四個視窗中得到 Tidal 的最佳結果（top hit）及其相符的曲目、專輯、藝人與播放清單，並且完全像使用任何其他頁面的列一樣使用它們（播放、加入佇列、開啟、動作彈出視窗）。

凡是 0006 已有的機制，它都不再新增：搜尋頁面只是歷史紀錄中的另一個頁面，透過播放器取得（`PageRequest::Search`），它的四個清單隨捲動載入（`ListRef::Search*`），而它的列就是 0006 的曲目、專輯、藝人與播放清單列，使用 0006 的按鍵與動作。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 tidalt 的 `internal/tidal/library.go`（`SearchAll`）、`internal/tidal/api.go`（`Search`）與 `internal/ui/search.go`（`6876b42`、`1dc0f4b`）：

- **一個請求，每種類型 20 筆，不分頁**：`GET /v1/search?query=…&types=TRACKS,ARTISTS,ALBUMS,PLAYLISTS&limit=20&countryCode=…`。第 20 筆之後的結果都拿不到，也沒有任何東西提示這一點
- **播放清單取回了卻從未顯示**：回應中的播放清單被解碼了，但攤平後的列清單（`searchRows`）只放了曲目、藝人與專輯
- **一個攤平的清單**（先曲目，再藝人，再專輯）位於輸入框之下，游標用 `↓` 從輸入框走進清單
- **曲目上的 `Enter` 只播放那一首曲目**（`playTrackCmd`），因此播放在它之後就停了（0004「What went wrong」9）
- **專輯上的 `Enter` 把它載入佇列**（`openAlbum`），無法先看它的曲目（0006「What tidalt did」，`3b6d6ea`）
- 每次送出時結果都被取代，且沒有請求 ID：較慢的舊搜尋可能在較新的搜尋之後才送達

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及由哪個條件涵蓋

1. **結果在一頁處被截斷**（tidalt：每種類型 20 筆）→ 每個視窗隨捲動載入更多，且標題顯示 Tidal 的總數（AC3、AC8）
2. **某種結果類型取回了卻沒有顯示**（tidalt：播放清單）→ 四個視窗，各有一個繪製快照（AC12）
3. **舊的搜尋覆蓋了較新的搜尋** → 每次搜尋都是帶有自己 ID 的請求；只套用該頁面最新的一個（AC7）
4. **輸入會觸發播放按鍵**：在查詢中輸入的 `q`、`n`、`Space` 不可以結束程式、跳過曲目或暫停 → 輸入框有焦點時，每個可列印的按鍵都是輸入文字（AC6）
5. **每按一個鍵就搜尋一次**：請求會達到 Tidal 的速率限制（0004「What went wrong」3）→ 只在 `Enter` 時送出搜尋（決策 2）（AC7）
6. **`Enter` 在曲目上只播放一首曲目**（tidalt）→ 曲目播放，並把曲目結果排在它之後（決策 4）（AC9）
7. **`Enter` 在專輯上把它排入佇列**（tidalt）→ 它開啟專輯頁面，與其他各處相同（AC9）
8. **無法播放或非音樂的結果**：除非送出 `types`，否則會回傳影片，而曲目結果包含僅有杜比全景聲（Dolby Atmos）的版本（探測：一個 `LOW`、`DOLBY_ATMOS` 的「Hell Above」）→ 一律送出 `types`，且 `audioModes` 中沒有 `STEREO` 的曲目會被捨棄，如同 0006 的 *All tracks*（AC2、AC3）

<a id="behaviour"></a>
## 行為

<a id="the-search-page"></a>
### 搜尋頁面

`g s` 開啟**搜尋頁面**（spotify-player 的 `SearchPage`），像任何 0006 的頁面一樣推入歷史紀錄；若最上層的頁面已經是搜尋頁面，`g s` 則讓它的輸入框取得焦點。頁面有一列的**輸入框**、一列的 **Top hit**，以及它們下方的四個視窗：

| 視窗 | 列（0006 的） | 標題 |
|---|---|---|
| **Top hit** | 任何類型的一列，附上它的類型（`Pierce The Veil · artist`）；Tidal 沒有指名時不出現（決策 6） | — |
| **Tracks** | 曲目列 | `Tracks (1 234)` |
| **Albums** | 專輯列 | `Albums (87)` |
| **Artists** | 藝人列 | `Artists (12)` |
| **Playlists** | 播放清單列 | `Playlists (300)` |

- 頁面開啟時輸入框是空的且有**焦點**，視窗中沒有任何東西。標題列顯示 `Search`；搜尋之後顯示 `Search · "<query>"`
- **輸入按鍵**（輸入框有焦點時）：可列印的字元（包括 `Space`）與貼上的文字會被附加；`Backspace` 刪除最後一個字元；`C-u` 清空輸入；`Enter` 送出搜尋；`Tab`/`BackTab` 把焦點移到視窗；`Esc` 在有結果時，把焦點交給搜尋會聚焦的視窗（見下），否則不做任何事。其他每個按鍵都被忽略，因此 `q`、`n`、`Space`、`g …`，以及在空輸入上的 `Backspace` 都不會有作用（`C-c` 仍然離開，`C-q` 仍然返回）
- **送出**：輸入經修剪空白後為空時，`Enter` 不做任何事。否則修剪後的查詢（最多 200 個字元，更長的輸入不會被接受：輸入到 200 個字元就停止）會以 `Library { Page(Search(query)) }` 送出；四個視窗都顯示 `Loading…`，然後是各清單的第一頁，焦點在有最佳結果時移到 *Top hit*，否則移到 *Tracks*、*Albums*、*Artists*、*Playlists* 中第一個非空的視窗，再否則維持在輸入框。同一個頁面上的新搜尋會取代結果；游標回到最上方
- **視窗**：`Tab`/`BackTab` 讓焦點依輸入框 → *Top hit*（顯示時）→ *Tracks* → *Albums* → *Artists* → *Playlists* → 輸入框的順序循環，首尾相接（spotify-player 的順序，而最佳結果放第一個，如 Tidal 的應用程式）。焦點在視窗上時，每個 0006 的按鍵都如在任何頁面上一樣有作用：游標按鍵、`Enter`、`Z`、`g a`/`C-Space`、`a`、`Backspace`（返回）、`g l`、`z` 等等。`/` 讓輸入框再次取得焦點（spotify-player 以 `/` 開啟它的頁面內搜尋彈出視窗，0008；在這個頁面上它的意思是「再搜尋一次」）。*由 [0012](0012-filter.md)（決策 1）變更：`/` 現在如同在每個頁面一樣，篩選取得焦點的結果視窗；`g s` 與 `Tab` 則回到輸入框*
- **歷史紀錄**：返回到搜尋頁面時，會顯示它原本的查詢、結果、游標與焦點，而不會再搜尋一次。在另一個頁面按 `g s` 會推入新的空搜尋頁面（較早的那個留在下方的歷史紀錄中）
- **空的結果**：沒有結果的視窗會顯示 `No tracks found`、`No albums found`、`No artists found`、`No playlists found`；失敗的搜尋會在頁面中顯示它的訊息（`Could not search: <reason>`），如同失敗的 0006 頁面

<a id="lists-load-as-you-scroll"></a>
### 清單隨捲動載入

每個視窗都是一個 0006 的清單，隨捲動**載入到 Tidal 結果的盡頭**（使用者，2026-10-08）：第一頁隨搜尋一起來，下一頁在游標接近最後一個已載入的列時載入，顯示 `Loading more…`，失敗顯示為最後一列，已載入的列依 ID 略過，直到視窗持有 Tidal 的總數。應用程式本身沒有設定任何上限。

- **搜尋頁面大小**：一項設定，`TIDAL_PLAYER_SEARCH_PAGE_SIZE`（在 0008 把它移到 `app.toml` 之前），整數 **1–1000**（端點最大的一頁），預設 **20**（使用者，2026-10-08）；空值 = 未設定，無效 → 以 2 結束並指出該變數（0004「Settings」的規則）。它決定每個視窗的第一頁與之後每一頁的大小；`TIDAL_PLAYER_PAGE_SIZE`（0006）仍然只決定音樂庫清單的大小
- Tidal 自己的總數很小：探測從未見過單一清單超過 300 筆結果（「love」：295 首曲目），而超過總數的 `offset` 會回答 `200` 且沒有項目。這是 Tidal 對一個查詢所擁有的量，而不是應用程式的限制

<a id="playing-and-queueing-from-results"></a>
### 從結果播放與加入佇列

0006 的表格依然成立，只有曲目上的 `Enter` 有一點不同（決策 4），而最佳結果的作用就像它所屬類型的一列：

| 按鍵 | 在 | 作用 |
|---|---|---|
| `Enter` | 曲目 | 以 *Tracks* 中**已載入**的列依結果順序取代佇列，並播放選取的那一首：`LoadQueue { tracks, start: index }`。前後都不會再取得更多（決策 4）：預設為 20 首曲目，若視窗已捲動過則更多 |
| `Enter` | 最佳結果，一首曲目 | 當它在已載入的列中時，如同在 *Tracks* 中對那首曲目按 `Enter`；否則為 `LoadQueue { [top hit] + the loaded rows of Tracks, start: 0 }` |
| `Enter` | 專輯、播放清單或藝人（最佳結果或列） | 開啟它的頁面（0006） |
| `Z`、`C-z` | 曲目、專輯或播放清單 | 把它加到佇列尾端（0006） |
| `g a`、`C-Space` | 任何列 | 0006 針對該種列的動作彈出視窗（搜尋到的播放清單絕不是*自己的*：沒有 *Delete playlist*，且 *Add to favorites*/*Remove from favorites* 兩者都會顯示） |

<a id="talking-to-the-player"></a>
### 與播放器溝通

0006 的 `Library` 請求，新增兩項：

- `PageRequest::Search(String)` → `PageData::Search { top_hit: Option<TopHit>, tracks: ListPage<Track>, albums: ListPage<AlbumSummary>, artists: ListPage<ArtistRef>, playlists: ListPage<PlaylistSummary> }`
- `TopHit` = `Track(Track) | Album(AlbumSummary) | Artist(ArtistRef) | Playlist(PlaylistSummary)`
- `ListRef::SearchTracks(String)`、`SearchAlbums(String)`、`SearchArtists(String)`、`SearchPlaylists(String)`，供 `More` 使用

錯誤沿用 0006 的（`Could not reach Tidal: …`、`Tidal answered 429: try again in a moment`、工作階段過期的訊息）；Tidal 以無效為由拒絕的請求（`400`）會顯示 `Tidal refused the search: <userMessage>`。

<a id="what-the-player-fetches"></a>
### 播放器取得什麼

資料形狀來自探測（「事實」）：

| 請求 | API 呼叫 | 保留 |
|---|---|---|
| `Page(Search(q))` | `GET /search?query={q}&types=TRACKS,ALBUMS,ARTISTS,PLAYLISTS&limit={n}&offset=0` | `tracks`、`albums`、`artists`、`playlists`：各為 `{limit, offset, totalNumberOfItems, items}`；`topHit` 的 `{type, value}` 依 `type` 對應（`TRACKS`、`ALBUMS`、`ARTISTS`、`PLAYLISTS`；任何其他類型、`topHit: null` 或缺少該鍵 → `None`）；`videos` 忽略 |
| `More { SearchTracks(q) … }` | `GET /search/{tracks,albums,artists,playlists}?query={q}&limit=…&offset=…` | `{limit, offset, totalNumberOfItems, items}`，項目是裸的 |

項目使用 0006 的對應規則（曲目：0004 的，專輯與藝人：0006 的摘要，播放清單：`own: false`）。`audioModes` 中沒有 `STEREO` 的曲目會從 *Tracks* 以及最佳結果中捨棄（此時最佳結果為 `None`）；被捨棄的列仍然計入 `offset`，如同 0006 較短的頁面。兩個端點最大的一頁都是 **1000**（探測的 `limit=1000` 被接受），這就是 `TIDAL_PLAYER_SEARCH_PAGE_SIZE` 的上限。各類型的端點多了 `artist` 欄位，並省略曲目的 `album.releaseDate`；對應時兩者都忽略。查詢由 HTTP 客戶端做 URL 編碼（空格、`&`、`/`、非 ASCII）。`countryCode` 與其他各處相同。

<a id="rendering"></a>
### 繪製

標題列之下：輸入列（`Search: <query>▏`，有焦點時帶游標方塊，沒有時為淡色）、最佳結果列（`Top hit: <row as in its window> · <kind>`，取得焦點時反白；沒有最佳結果時就沒有這一列），然後是內寬至少 60 欄時以 2 × 2 格狀排列的四個視窗：*Tracks* | *Albums* 在上，*Artists* | *Playlists* 在下，各占一半寬度與一半高度（spotify-player 的水平版面，去掉它的 Shows/Episodes 那一列）。少於 60 欄時，輸入框與最佳結果之下只繪製取得焦點的視窗，並附上 `‹Tab›`，如 0006。在任何大小下都不會 panic。

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Search · "pierce the veil"                                                    │
│Search: pierce the veil                                                       │
│Top hit: Pierce The Veil · artist                                             │
│┌Tracks (300)──────────────────────┐┌Albums (41)──────────────────────────────┐│
││Hell Above        Pierce The Veil ││Collide With The Sky  Pierce The Veil 2012││
││King for a Day    Pierce The Veil ││Misadventures         Pierce The Veil 2016││
│┌Artists (6)───────────────────────┐┌Playlists (300)──────────────────────────┐│
││Pierce The Veil                   ││This Is Pierce The Veil              50  ││
│…                                                                             │
```

<a id="acceptance-criteria"></a>
## 驗收條件

協定與型別（`tidal_player_core`，純邏輯）：

- **AC1** — `PageRequest::Search`、`PageData::Search`、每個 `TopHit` 變體與四個 `ListRef::Search*` 變體都能通過 JSON 往返與 0005 的編解碼器（0006 AC1 的測試，新增列）。設定：`resolve_player_config` 中的 `TIDAL_PLAYER_SEARCH_PAGE_SIZE`（0006 AC3 的表格，新增列：未設定 → 20、空值 → 20、`1`、`1000`、`0`、`1001`、`x`）

API（`tidal-player-api::library`，來自探測的 wiremock 測試資料）：

- **AC2** — `Page(Search(q))` 送出一個 `GET /search`，其中 `query` 經 URL 編碼（列：`pierce the veil`、`AC/DC`、`Sigur Rós`、`a&b`）、`types`、`countryCode`、`limit` = 搜尋頁面大小、`offset=0`；回傳四個清單及其總數與最佳結果（每種 `type` 一列，未知的 `type`、`topHit: null` 與缺少該鍵 → `None`）；僅有杜比全景聲的曲目從 *Tracks* 與最佳結果中捨棄；空的結果（`topHit: null`，總數 0）是四個空清單；`400` → `Tidal refused the search: <userMessage>`；`LoginRequired` 與暫時性錯誤維持不變（表格）
- **AC3** — 對每個 `ListRef::Search*` 的 `More` 送出 `GET /search/{type}`，帶有 `query`、`limit`（搜尋頁面大小）、`offset`；解開裸的項目（各類型的曲目形狀有 `artist` 而沒有 `album.releaseDate`，仍能對應）；捨棄僅有杜比全景聲的曲目；超過總數的 `offset` 回傳沒有項目（表格，涵蓋四個清單）

播放器（`tidal-player`）：

- **AC4** — `Library { Page(Search(…)) }` 與搜尋清單上的 `More`，如任何 0006 的請求一樣得到回答：一個 `LibraryReply` 給發送者，與輸入迴圈並行（0006 AC8 的測試架構，新增列）

客戶端模型（`tidal_player_core::ui`，純邏輯）：

- **AC5** — `g s` 推入輸入框取得焦點的空搜尋頁面且不發出任何東西；最上層是搜尋頁面時的 `g s` 讓它的輸入框取得焦點且不推入任何東西；在歷史紀錄中來回，會保留查詢、結果、游標與焦點，且不發出請求
- **AC6** — 輸入編輯（表格）：可列印的按鍵、`Space`、`q`、`n`、`g` 都是附加；`Backspace` 刪除一個字元，在空輸入上不做任何事（不彈出頁面）；`C-u` 清空；第 201 個字元不被附加；`C-c` 離開；`C-q` 返回；除了 `Enter` 之外沒有任何按鍵會發出請求，也沒有任何按鍵會發出播放器指令
- **AC7** — 送出：空的或全為空白的輸入上的 `Enter` 不發出任何東西；否則發出一個帶有新 ID 的 `Library { Page(Search(trimmed)) }`，視窗顯示 `Loading…`；回覆填入最佳結果與四個視窗，並聚焦最佳結果，否則聚焦第一個非空視窗，再否則聚焦輸入框；第一個回覆之前的第二次 `Enter` 會讓第一個回覆被丟棄；對歷史紀錄中較下層頁面上較早搜尋的回覆，仍會落在該頁面上
- **AC8** — 視窗：`Tab`/`BackTab` 依輸入框 → Top hit（不存在時略過）→ Tracks → Albums → Artists → Playlists → 輸入框的順序循環；視窗上的 `/` 聚焦輸入框（*自 [0012](0012-filter.md) 起：`g s` 聚焦輸入框，`/` 篩選視窗*）；有結果時，輸入框上的 `Esc` 聚焦回覆會聚焦的位置；捲動視窗時依 0006 AC10 的規則發出 `More { SearchTracks(q) … }`（以及另外三個）
- **AC9** — 列的作用如同 0006 的頁面（表格）：曲目 *i* 上的 `Enter` 送出 `LoadQueue { the loaded track rows, start: i }`，先前沒有 `More`，且在回覆之後不會為它送出 `More`；已載入列中的最佳結果曲目上的 `Enter` 做相同的事，不在其中的則送出 `LoadQueue { [top hit] + loaded rows, start: 0 }`；最佳結果的專輯、播放清單或藝人上的 `Enter` 推入它的頁面；專輯、播放清單、藝人上的 `Enter` 推入它的頁面；`Z` 送出 0006 的 `Open`；動作彈出視窗列出 0006 的動作，搜尋到的播放清單視為非自己的
- **AC10** — 中斷連線與工作階段過期：中斷連線時的 `Enter` 讓頁面以中斷連線的訊息失敗，且不送出任何東西；若最上層的頁面是以這種方式失敗的，下一個 `Welcome` 會重新送出搜尋（0006 AC16 的規則）

TUI 繪製與執行期（`tidal-player`）：

- **AC11** — 按鍵解碼：`C-u` 能解碼；貼上的文字（括號式貼上）會以字元的形式到達輸入框（`crates/app/src/tui/input.rs`，表格）
- **AC12** — 繪製（`insta`，80×24，由人工審閱，加上 `contains` 檢查）：輸入框取得焦點的空搜尋頁面；載入中；有最佳結果與四個附總數視窗的結果；沒有最佳結果的結果；每個 `No … found`；失敗的搜尋；50×20：輸入框加上取得焦點且附有 `‹Tab›` 的視窗。從 0×0 到 120×40 都不 panic
- **AC13** — 接線：連上的客戶端的搜尋由測試播放器的 wiremock API 回答，且不開啟工作階段儲存庫（0006 AC19 的測試，加上一個 `g s`、輸入的查詢、`Enter`）

文件：

- **AC14** — `docs/tui.md` 記載搜尋頁面、它的按鍵（`g s`、輸入編輯、`/`、`Tab`）、曲目上的 `Enter` 把已載入的結果排入佇列、捲動到結果的盡頭，以及訊息；`docs/playback.md` 的「Settings」列出 `TIDAL_PLAYER_SEARCH_PAGE_SIZE`；`CLAUDE.md` 的「Status」指名這份規格；這份規格連結到它們

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 沒有最佳結果，或是未列出的類型（影片） | 沒有 *Top hit* 列；`Tab` 略過它 |
| 沒有任何相符的查詢 | 每個視窗顯示 `No … found`；輸入框保有焦點 |
| 含有 `&`、`/`、`#`、`?`、引號、表情符號或 CJK 的查詢 | 以 URL 編碼送出；結果依 Tidal 回傳的為準（AC2） |
| 貼上 Tidal 連結作為查詢 | 當作文字搜尋（開啟連結仍屬於 `o`/`O`；見「不在範圍內」） |
| 重複按 `Enter` | 每次都送出一次搜尋；只套用最新的結果（AC7）；播放器一次執行一個客戶端的請求（0006 AC8） |
| `429` | 頁面顯示 `Tidal answered 429: try again in a moment`；不會重試 |
| 結果是僅有杜比全景聲的曲目 | 不列出（也不是最佳結果）；標題中的總數是 Tidal 的，因此可能比列數多幾筆（如 0006 的收藏曲目） |
| 200 個字元的查詢 | Tidal 回答沒有結果：`No … found` |
| 剛搜尋完就對曲目按 `Enter` | 已載入的 20 首曲目被排入佇列（若有全景聲版本被捨棄則較少），該佇列不會再取得更多（決策 4）；要排入更多，請先捲動 |
| 捲動到大量結果的盡頭 | 每頁 20 筆一直載入到 Tidal 的總數（295 首曲目約需 15 個請求，游標接近尾端時一次一個）；之後不再詢問任何東西 |
| 結果曲目無法串流 | 淡色顯示，可加入佇列，由播放器跳過（0004 AC7） |
| 窄的終端機 | 輸入框加一個視窗（AC12）；內部列數少於 6 時只顯示播放視窗（0004） |
| 搜尋時中斷連線 | 頁面以中斷連線的訊息失敗；若在最上層，重新連線時會重新送出（AC10） |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名（`ac7_…`）。紅燈是針對存根型別與函式本體為存根的函式（沒有 `todo!()`，沒有編譯錯誤）的斷言失敗，如 0001–0006。API 測試使用 `wiremock`，測試資料放在 `crates/api/tests/fixtures/search/` 之下，依探測所記錄的資料形狀寫成。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) + `crates/app/src/settings.rs` :: `ac25_player_config` (new rows) | 往返；解碼；設定 | 存根 `Serialize` 寫出 `null`；存根設定維持 100 |
| AC2 | `crates/api/tests/search.rs` :: `ac2_search_page` (table) | 路徑、編碼後的查詢、參數、四個清單、錯誤 | 存根送出未編碼的 `query` 並回傳空清單 |
| AC3 | `crates/api/tests/search.rs` :: `ac3_search_more` (table) | 各類型的路徑、限制、offset、解包、捨棄全景聲 | 存根忽略 `offset` |
| AC4 | `crates/app/src/ipc/server/tests.rs` :: `ac4_search_reply_to_sender` + `crates/app/src/player/runtime.rs` :: `ac4_tokio_jobs_search_page_size` | 給發送者的一個回覆 | 存根以錯誤回答 `Search` |
| AC5 | `crates/core/src/ui/search/tests.rs` :: `ac5_search_page_history` (table) | 推入、重新聚焦、保留的狀態 | 存根 `g s` 不做任何事 |
| AC6 | `crates/core/src/ui/search/tests.rs` :: `ac6_input_editing` (table) | 輸入的文字，沒有指令 | 存根讓 `q` 離開 |
| AC7 | `crates/core/src/ui/search/tests.rs` :: `ac7_send_and_reply` (table) | 一個請求、已修剪、丟棄過時的回覆、焦點 | 存根套用每個回覆 |
| AC8 | `crates/core/src/ui/search/tests.rs` :: `ac8_windows_and_scroll` (table) | 焦點循環、`/`、`Esc`、`More` | 存根的 `Tab` 略過輸入框 |
| AC9 | `crates/core/src/ui/search/tests.rs` :: `ac9_rows_act` (table) | 每列確切的指令或頁面 | 存根的 `Enter` 取得整個曲目清單 |
| AC10 | `crates/core/src/ui/search/tests.rs` :: `ac10_disconnected` (table) | 沒有送出任何東西；一次重新送出 | 存根在中斷連線時仍然送出 |
| AC11 | `crates/app/src/tui/input.rs` :: `ac18_key_events` (new `C-u` row, 綠燈 at once: Ctrl+letter was already decoded) + `ac11_paste_reaches_the_search_input` | 解碼後的按鍵、貼上 | 存根丟掉 `C-u` 與貼上的文字 |
| AC12 | `crates/app/src/tui/render.rs` :: `ac12_search_80x24` (one snapshot per row), `ac12_search_narrow_50x20`, `ac17_no_panic_any_size` (search rows) | `contains` 加快照 | 存根的繪製不畫輸入列 |
| AC13 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (search step) + `crates/app/src/main.rs` :: `ac13_tui_state_page_sizes` | 已回答；沒有開啟工作階段 | 存根客戶端丟掉搜尋 |
| AC14 | —（驗收時審閱） | 文件與本規格相符，已連結 | — |

驗收時以真實帳號手動檢查（結果寫在 PR 說明中）：搜尋一位藝人、一個專輯標題與一個片語，得到的前幾筆結果與 Tidal 應用程式相同；把 *Tracks* 捲動到大量結果的盡頭時乾淨地停下；結果曲目上的 `Enter` 會播放它，並把其餘已載入的結果接在它之後；非 ASCII 的查詢（`Sigur Rós`、`米津玄師`）能找到它們的藝人。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`library`（`PageRequest::Search`、`PageData::Search`、`ListRef::Search*`）、`ui`（`PageKind::Search`、輸入框與它的焦點、`WindowKind::{SearchTracks, SearchAlbums, SearchArtists, SearchPlaylists}`；一個附有自己測試的 `ui/search` 模組）。沒有 I/O，沒有新的相依套件
- `tidal-player-api::library`（或它旁邊的 `search` 模組）：兩個呼叫與它們的 DTO 對應
- `tidal-player`：`player::runtime`（`Library` trait 回答新的請求）、`ui.rs`/`ui/pages.rs`（搜尋頁面）、`input.rs`（`C-u`、貼上）
- `xtask layering`：沒有變動

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-08，取自程式碼與文件）：

- spotify-player（`README.md`、`spotify_player/src/ui/page.rs` 的 `render_search_page`，2026-10-08 讀取）：`SearchPage` 是 `g s`；`/` 是 `Search`，「a popup for searching in the current page」；頁面開啟時焦點在輸入框，`Enter` 搜尋，`FocusNextWindow`/`FocusPreviousWindow` 從輸入框移到結果視窗；視窗順序為 Tracks、Albums、Artists、Playlists、Shows、Episodes，在它的水平版面中分成兩欄；載入中或失敗時顯示 `Loading...` / `Search failed` 且沒有視窗。`C-s` 是它的隨機播放，與我們的相同（0004）
- tidalt 的搜尋（見上）以 `query`、`limit=20`、`countryCode`、`types=TRACKS,ARTISTS,ALBUMS,PLAYLISTS` 呼叫 `GET /v1/search`，並解碼 `tracks.items`、`artists.items`、`albums.items`、`playlists.items`；它對 tidalt 的使用者在即時 API 上是可行的，使用與我們相同的客戶端 ID
- 0006 的機制涵蓋其餘部分：在播放器中以工作執行的 `Library` 請求、`ListPage` 分頁、請求 ID、動作彈出視窗

2026-10-08 以 `scripts/tidal-search-probe.sh` 對**即時 API** 驗證，由使用者執行（國家 NG；這個指令碼在這次更新之後已刪除）。`crates/api/tests/fixtures/search/` 之下的測試資料是依這些資料形狀寫成的（目錄資料是公開的；使用者 ID 已替換）：

- `GET /v1/search?query=…&types=…&limit=…&offset=…&countryCode=…` → `200 {artists, albums, playlists, tracks, videos, topHit}`；每個清單是 `{limit, offset, totalNumberOfItems, items}`，項目是裸的：藝人如 0006 的（`id`、`name`、…），專輯如 0006 的（`type` 為 `ALBUM`/`EP`/`SINGLE`、`releaseDate`、`numberOfTracks`、`duration`、`artists[]`），曲目如 0004 的（`artists[].id`、`album {id, title, releaseDate}`、`audioModes`、`version`），播放清單（`uuid`、`title`、`numberOfTracks`、`duration`、`type` 為 `EDITORIAL`/`ARTIST`/`PODCAST`、`creator: {}`）。預設頁面大小 10
- **`types` 會限制清單**：`types=TRACKS` 讓其他清單以總數 0 回答為空；不帶 `types` 時 `videos` 清單會被填滿（「pierce the veil」有 14 支音樂影片）
- **`topHit`**：`{value, type}`，`value` 是該類型的完整項目；`type` 為 `ARTISTS`（「pierce the veil」、「love」、「Sigur Rós」、「米津玄師」）、`ALBUMS`（「collide with the sky」、「a」、「AC/DC」）、`TRACKS`（「hell above」、「this is pierce the veil」，以及帶 `types=TRACKS` 時）；沒有相符項目時為 **`null`**（空查詢、沒有相符結果與 200 個字元的查詢）。沒有看到 `PLAYLISTS`（沒有任何查詢讓播放清單成為最佳結果）；它依同樣的規則對應
- **頁面大小與深度**：`/v1/search` 與 `/v1/search/tracks` 都接受 `limit` 50、100、300 與 1000；每個查詢的總數都維持在 300 以內（「love」：295 首曲目、189 張專輯、125 位藝人、114 個播放清單；「a」與「collide with the sky」：300 首曲目）；`/v1/search/tracks?offset=250&limit=50` → 45 個項目，`offset=300`、`1000`、`10000` → `200` 且沒有項目、總數相同
- **各類型的端點** `GET /v1/search/{tracks,albums,artists,playlists}` 存在，並以 `{limit, offset, totalNumberOfItems, items}` 回答，順序與總數都與合併的端點相同；它們的曲目與專輯多了 `artist`（主要藝人），曲目省略 `album.releaseDate`，編輯精選的播放清單帶有 `creator: {id: 0}`
- **曲目結果包含僅有杜比全景聲的版本**（「hell above」：位置 2 有一個 `LOW`、`["DOLBY_ATMOS"]` 的版本）以及標示為 AI 的曲目（`ai: true`，保留）
- **查詢**：空 → `200`，全部為空，`topHit: null`（客戶端從不送出空查詢）；以 URL 編碼的 `AC/DC`、`Sigur Rós` 與 `米津玄師` 分別最先找到 AC/DC、Sigur Rós 與米津玄師（Kenshi Yonezu）；200 個字元的查詢 → `200` 且沒有結果。沒有看到 `400`；`400` 訊息的規則為保險起見仍保留

未驗證：

- `PLAYLISTS` 的最佳結果（與其他的一樣對應；只在測試資料上測試過）
- 結果中使用者建立的播放清單是否帶有擁有者的 `creator.id`（因此決策 5 維持：絕不視為自己的）
- Tidal 對一個查詢是否曾回報超過 300 筆結果（不論如何，客戶端都依 `totalNumberOfItems` 進行）

<a id="decisions-answered-by-the-user-2026-10-08-folded-into-the-body-above"></a>
## 決策（使用者已回覆，2026-10-08；已併入上方正文）

1. **開啟搜尋的按鍵**：*`g s`*，照提案；`/` 保留給 0008 的頁面內篩選
2. **何時搜尋**：*在 `Enter` 時*，照提案
3. **版面**：*2 × 2 格狀*，Tracks | Albums 在 Artists | Playlists 之上
4. **結果曲目上的 `Enter`**：*只把目前的結果排入佇列*（「just add current search results, do not fetch more after enqueuing」）：*Tracks* 中已載入的列，前後都不取得任何東西
5. **結果中自己的播放清單**：未回答；正文保留提案（絕不標示為自己的）：探測只顯示了編輯精選的播放清單，帶有 `creator: {}` 或 `{id: 0}`
6. **Tidal 的最佳結果**：*顯示*：輸入框與視窗之間有一列 *Top hit*，在 `Tab` 循環中排第一，並在搜尋之後取得焦點，其作用就像它所屬類型的一列
7. **頁面大小與載入**：*如 0006 一樣載入到盡頭，一次 20 筆*（「we do the inf fetching as spec 6 get the first X (for search i think a default of 20 is fair enough), then pages it」）：一項設定，`TIDAL_PLAYER_SEARCH_PAGE_SIZE`，預設 20

<a id="out-of-scope"></a>
## 不在範圍內

- 頁面內篩選（其他頁面上的 `/`，spotify-player 的 `Search` 彈出視窗）與可設定的按鍵（0008；篩選是 [0012](0012-filter.md)，它也賦予這個頁面上的 `/` 那個意義）
- 搜尋歷史與跨執行記住上一次的查詢（0009 略過了它：客戶端不儲存任何東西）
- 從搜尋輸入框開啟貼上的 Tidal 連結（仍屬於 `o`/`O`）
- 結果中的影片、mix 與電台（0011）
- 從命令列進行一次性搜尋（`tidal-player playback search …`）
- 輸入時的搜尋建議／自動完成

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-08"></a>
## 實作備註（規格沒有說明的地方所做的選擇，2026-10-08）

- **型別**：`PageData::Search` 持有 `top_hit: Option<Box<TopHit>>`（裝箱：曲目很大，而 clippy 的 `large_enum_variant` 會對未裝箱的列舉提出警告）
- **API**：`/search` 或 `/search/{type}` 上帶有非空 `userMessage` 的 `400` 是 `LibraryError::SearchRefused`（`Tidal refused the search: …`）；沒有的話就與其他任何狀態一樣處理（`Tidal answered 400`）。`value` 無法讀取的最佳結果視為沒有最佳結果（清單仍會顯示）；缺少某個清單的內容視為格式錯誤。被捨棄的僅有全景聲的曲目不計入 `ListPage::hidden`（標題顯示 Tidal 的總數）。所有搜尋程式碼都放在 `tidal-player-api::library` 中，與它共用的輔助函式放在一起
- **播放器**：`LibrarySettings::page_size_for(&LibraryRequest)` 對 `Page(Search(_))` 給出 `search_page_size`，其他情況給出 `page_size`；`More` 帶著自己的 `limit`
- **客戶端模型**：`PageKind::Search(String)` 持有最後送出的查詢（第一次搜尋之前為空）；`Page::search: Option<Search { input, top_hit, focus: Input | TopHit | Windows }>`。輸入框保留輸入時的文字；只有送出的查詢會被修剪。搜尋載入期間焦點留在輸入框；同一個頁面上的新搜尋會重建它（結果清空，游標歸 0）。從最佳結果按 `Tab` 會到 *Tracks*，從 *Tracks* 按 `BackTab` 在有最佳結果時會到最佳結果。中斷連線時的 `Enter` 以單純的中斷連線訊息讓頁面失敗（因此 0006 在 `Welcome` 時的重新送出能與之對上）；來自播放器的失敗顯示 `Could not search: <message>`。貼上的內容會進入取得焦點的搜尋輸入框（丟掉控制字元，上限 200）。不論資料怎麼說，客戶端都把搜尋到的播放清單視為絕不是自己的
- **TUI**：貼上的對應移到 `input::event_to_action`；`main.rs` 在 `tui_state(&PlayerSettings)` 中建立模型，它為連上的與獨立模式的 TUI 設定兩種頁面大小。最佳結果列顯示 `Top hit: <title or name> · <kind>`（取得焦點時反白，無法串流的曲目為淡色），載入中不繪製；輸入框的游標是繪製出來的 `▏`，取得焦點時整列粗體，否則為淡色，過長的查詢顯示它的尾端（`…tail`）。第一次搜尋之前，四個視窗是空的外框；失敗的搜尋保留輸入列，並以訊息取代視窗。格狀版面平均分配列數（上排多拿到餘下的一列）。搜尋到的播放清單不顯示 `♥`。`Action::Resize` 的清單高度仍然是整個頁面的高度，因此搜尋視窗（約一半高）會比 0006 的規則稍早要求它的下一頁；無害
