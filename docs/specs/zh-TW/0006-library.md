[English](../0006-library.md) | 正體中文

<a id="0006--library-favorites-playlists-album-and-artist-pages"></a>
# 0006 — 音樂庫：收藏、播放清單、專輯與藝人頁面

- **狀態**：implemented（2026-10-07；「測試計畫」中的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **修訂者**：[0028](0028-collected-tracks.md)（2026-11-12）：收藏曲目頁面是 *Collected Tracks*，固定在音樂庫的 Playlists 最前面，所有關於收藏的文字都說 *Collected Tracks* 或 *collection*
- **相依於**：0002（implemented：工作階段的 `user_id` 與 `country_code`）、0004（implemented：佇列、`LoadQueue`/`AddToQueue`、TUI）、0005（implemented：socket、客戶端／播放器的分工、`Open`）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Pages」、「The library」、「Album, playlist and artist pages」與「Actions」各節以及新的按鍵；[`docs/playback.md`](../../zh-TW/playback.md) 的「Settings」新增兩項設定（AC20）

<a id="context"></a>
## 背景

到目前為止，佇列只能從貼上的連結與 ID 填入（0004 決策 1）。本規格加入 spotify-player 所謂的**頁面**：使用者的音樂庫（他們的播放清單、收藏的專輯與收藏的藝人）、他們收藏的曲目，以及從這些地方進入的專輯、播放清單與藝人頁面，並採用 spotify-player 的導覽方式（頁面歷史、可聚焦的視窗、動作彈出視窗）。從任何曲目清單，一首曲目都可以連同**它所在清單的其餘內容**排在後面播放，或加到佇列；曲目、專輯、藝人與播放清單可以加入或移出收藏，而使用者自己的播放清單可以建立、加入曲目、刪減與刪除。它也給了佇列期待已久的移除按鍵（0004「Out of scope」）。

它解決了 0005 懸而未決的問題（0005 決策 3、「Out of scope」）：**由播放器擷取頁面**，就像它已經在展開 `Open` 一樣。客戶端仍然不持有工作階段、金鑰圈項目或 API 存取權（0005 AC14 繼續成立）。

搜尋（0007）、作為頁面的 mix 與電台（0011）不在本規格內；0009 決定不做快取。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 tidalt 的 `internal/tidal/{api,library}.go`、`internal/ui/{keys,model}.go`、`docs/layout.md` 與所列的 commit：

- **每個清單一頁，沒有分頁**：收藏的曲目以 `limit=50` 擷取（`GetFavorites(ctx, 50)`），收藏的藝人與專輯以 `limit=200`，使用者的播放清單以 `limit=50`，全都是單一請求並帶 `order=DATE&orderDirection=DESC`；專輯的曲目則完全沒有 `limit`。收藏更多的使用者只會看到前 50 個（或 200 個），沒有任何地方告訴他們。播放清單的曲目要求 `limit=1000`，`/items` 會以 `400` 拒絕（0004「Facts」；tidalt 用的端點 `/tracks` 是否也拒絕並未驗證）。只有藝人的專輯有分頁（每頁 50 個，最多 20 頁）
- **只有使用者自己的播放清單**：`/users/{id}/playlists`；使用者加入收藏的播放清單不會列出
- 播放清單是從 `/v1/playlists/{uuid}/tracks` 載入的，它把影片當成曲目回傳（0004「What went wrong」8）
- **Favorite Songs 顯示的是佇列**，而不是收藏（`65285f1`）：收藏只被保存成一組 ID，用來顯示 ♥ 標記，而頁面畫的是播放佇列
- **數量在區段被開啟之前都顯示 0**（`64d3695`），以啟動時預先擷取全部內容修正；之後重新造訪就再也不更新
- **藝人檢視中的專輯直接進入佇列**（`3b6d6ea`），沒有辦法先看它的曲目
- 從清單播放時只把選中的那首曲目放入佇列，所以佇列在它之後就結束了（0004「What went wrong」9、`59c610d`）
- 客戶端播放一份播放清單時送出 `PlayTrackID`，播放器的佇列縮成只剩一首曲目（0005「What went wrong」2、#5）
- 從隨機播放中的客戶端送出的播放清單，是以顯示順序抵達的，並被存成原始順序（0005「What went wrong」3、#21）
- 它後來的 Library 頁面（`90b5130`，spotify-player 風格）把 Playlists、Artists 與 Albums 以 40/20/40 % 並排，用 `Tab`/`Shift-Tab` 循環一個聚焦的欄，寬度低於 60 欄時只顯示聚焦的欄，`Backspace` 返回：這裡保留，並採用 spotify-player 的視窗順序

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及由哪個條件涵蓋

1. **從清單播放只播放一首曲目**（tidalt `59c610d`、#5）→ 在曲目列上按 `Enter` 會把**整份清單**（先載入其餘部分）連同選中的索引，以一個 `LoadQueue` 送出（AC12）
2. **以顯示順序送出的清單**（tidalt #21）→ 頁面的清單依 Tidal 回傳的順序送出；客戶端絕不排序或隨機排列它（AC12）
3. **影片被當成曲目加入佇列** → 收藏、播放清單與藝人的清單只保留曲目；API 混入影片項目的地方，影片項目會被丟掉（AC4）
4. **清單只取到一頁就被截斷**（tidalt：50 首收藏曲目、50 份播放清單、200 張專輯與藝人；Tidal 在數個端點的預設頁面是 10 個項目，0004「Facts」）→ 每份清單隨著捲動一路載入到最後，標題顯示第一頁就取得的 Tidal 總數（AC4、AC10）
5. **追蹤的播放清單不見了**（tidalt 只列出自己的）→ 音樂庫列出自己的與收藏的播放清單（AC4）
6. **頁面畫的是另一份清單**（tidalt `65285f1`：Favorite Songs 畫出佇列）→ 每個頁面只繪製它自己擷取的資料（AC17，每個頁面一個快照）
7. **過時或為零的數量**（tidalt `64d3695`）→ 數量隨每次擷取一起取得，而開啟頁面一律重新擷取（AC9）
8. **瀏覽讓播放停頓**：擷取數千首收藏絕不可以延遲 `Next` → 擷取在播放器的輸入迴圈旁邊執行，絕不在其中（AC8）
9. **結果落在錯誤的頁面上**：使用者在擷取進行中已經離開 → 每次擷取都帶有 ID，結果只套用到發出請求的頁面（AC9）
10. **需要自己登入的客戶端**，如 tidalt 的那樣（0005「What went wrong」9）→ 頁面透過播放器取得（AC8、AC19）
11. **從藝人開啟的專輯立刻播放**（tidalt `3b6d6ea`）→ 在專輯上按 `Enter` 開啟它的頁面；只有在曲目上按 `Enter` 才播放（AC12）
12. **把較短的頁面誤認為重疊或結尾**（探測：在 1000 個項目的頁面上有 358 首收藏曲目，總數為 362）→ `offset` 以要求的頁面大小前進，已載入的列會被略過（AC4、AC10）
13. **以過時的位置編輯播放清單**（每次移除之後位置都會位移；Tidal 會檢查 ETag）→ 移除時送出清單載入時所帶的 ETag，遇到 `412` 絕不重試（AC7、AC14）
14. **Credits 被替代版本與僅有 Atmos 的複本灌水**（貢獻者探測：僅有 Dolby Atmos 的曲目；使用者：instrumental、TV size、sped up、slowed + reverb）→ *All tracks* 以可設定的詞彙清單隱藏它們，並說明隱藏了多少（AC3、AC6）

<a id="behaviour"></a>
## 行為

<a id="pages"></a>
### 頁面

播放視窗下方的區域一次顯示一個**頁面**。佇列（0004）是歷史最底下的頁面；TUI **從音樂庫開始**（使用者，2026-10-08），它被推到佇列之上，所以 `Backspace` 會回到佇列。音樂庫在播放器的第一個 `Welcome` 抵達之前顯示 `Loading…`，然後才擷取（客戶端在連上之前不會請求任何東西）。開啟頁面會把它放到**歷史**的最上面；返回時回到它下面的頁面，已載入的列、游標與焦點都維持原狀，不會再次擷取。開啟頁面一律重新擷取（不快取任何東西；0009 決策 3）。

| 頁面 | 開啟方式 | 視窗（`Tab` 在它們之間移動） |
|---|---|---|
| Queue | `z`；在起始頁面按 `Backspace` | 佇列（0004） |
| Library | `g l`；啟動時的頁面 | **Playlists**、**Albums**、**Artists** |
| Favorite tracks | `g y` | 曲目 |
| Album | 在專輯上按 `Enter`；*Go to album* | 專輯的曲目 |
| Playlist | 在播放清單上按 `Enter` | 播放清單的曲目 |
| Artist | 在藝人上按 `Enter`；*Go to artist* | **Top tracks**、**Albums**（專輯，然後是 EP 與單曲）、**Appears on**、**All tracks** |

- 開啟已經在最上面的頁面什麼都不做。否則新頁面會被推入；歷史最多保留 **50** 個頁面（最舊的被丟掉；最底下的佇列絕不丟掉）
- `Backspace`（與 `C-q`）返回；在最底下的頁面上什麼都不做
- `z`、`g l`、`g y` 從任何地方開啟它們的頁面（和其他頁面一樣被推入）
- 每個頁面有一個**標題列**；數量是 Tidal 的總數（`totalNumberOfItems`），第一頁就已知：
  - Library：`Library`
  - Favorite tracks：`Favorite tracks · 362 tracks`
  - Album：`<title> · <artists> · <year> · 17 tracks · 1:02:15`（已知時才有年份與長度）
  - Playlist：`<title> · 39 tracks · 2:41:07`
  - Artist：`<name>`
- 正在擷取的頁面在它的視窗中顯示 `Loading…`；失敗的擷取在頁面中顯示訊息（`Could not load the library: <reason>`、`Album 1 was not found`……）；空的清單顯示 `No favorite tracks yet`、`No playlists yet`、`No favorite albums yet`、`No favorite artists yet`、`This playlist has no tracks`、`No albums`、`No top tracks`、`No credits`

<a id="lists-load-as-you-scroll"></a>
### 清單隨著捲動載入

每份清單（上面的每個視窗）一次擷取一**頁**（決策 5）：第一頁隨頁面一起，下一頁則在視窗的游標進入離最後一個已載入列一個視窗高度之內時擷取。載入期間，最後一列顯示 `Loading more…`；失敗的一頁會保留已載入的列，並把它的訊息顯示為最後一列，而下一次在接近結尾處移動游標時會再試一次。Tidal 的總數會立刻顯示；已載入的列之後的列不會繪製。

- **頁面大小**：一個設定，`TIDAL_PLAYER_PAGE_SIZE`（在 0008 把它移到 `app.toml` 之前），整數 **1–10 000**，預設 **100**；空白 = 未設定，無效 → 結束碼 2 並指出該變數（0004「Settings」的規則）。每個請求要求 `min(page size, endpoint's largest page)`（見「What the player fetches and writes」下的表格：播放清單與 credits 為 50）
- **需要整份清單**的情況有：在曲目上按 `Enter`（從這裡播放）、在清單上按 `Z`、清單的 *Add to playlist*：客戶端先擷取其餘的頁面（訊息列顯示 `Loading 300 of 1 234…`），然後送出一個帶有依頁面順序排列的每首曲目的指令。`Esc` 取消；什麼都不送出。超過 **40 000** 首曲目的清單會被拒絕，顯示 `Too many tracks to queue at once (N)`（socket 的 16 MiB 一行大約容納 50 000 首）
- `offset` 以要求的頁面大小前進，而不是以收到的項目數；已載入的列（相同的曲目 ID、專輯 ID、藝人 ID 或播放清單 UUID；對播放清單的曲目則是相同的位置，因為一份播放清單可以放同一首曲目兩次：「Bugs」）會被略過（探測到的較短頁面，見「這裡可能出什麼問題」12）

<a id="lists-and-the-cursor"></a>
### 清單與游標

每個視窗有自己的游標，以 0004 的按鍵移動（`j`/`↓`、`k`/`↑`、`g g`、`G`），另外還有 `C-f`/`PageDown` 與 `C-b`/`PageUp`（一個視窗的高度）。`G` 移到最後一個*已載入*的列（所以會載入下一頁）。聚焦視窗的游標會反白；其他的以淡色繪製。`Tab` 聚焦下一個視窗，`BackTab`（Shift-Tab）聚焦上一個，會循環。游標在瀏覽頁面上是依位置，在佇列上則依項目 ID（0004 AC22，不變）。視窗的第一頁在頁面開啟時擷取，只有 *All tracks* 例外，它在第一次被聚焦時才擷取。

列：

- **曲目列**（收藏曲目、專輯、播放清單、top tracks、all tracks、佇列）：0004 佇列的欄位（編號、標題、藝人、專輯、長度）。不可串流的曲目以淡色繪製，因為播放器會跳過它們（0004 AC7）。視窗寬度小於 100 欄時，*All tracks* 在專輯欄的位置加上藝人的角色類別（`Performer, Songwriter`）
- **專輯列**：標題、藝人、年份（EP 與單曲另加 `EP`/`Single`）
- **播放清單列**：標題、曲目數，收藏（追蹤）的播放清單另加 `♥`
- **藝人列**：名稱

<a id="the-artists-all-tracks-tidals-credits-for-artist"></a>
### 藝人的 *All tracks*（Tidal 的「Credits for <artist>」）

來自 contributor 頁面的 `ITEM_LIST_WITH_ROLES` 模組與它的 `dataApiPath`（事實），依 Tidal 的排序，最受歡迎的在前。省略下列內容，並在視窗標題中顯示隱藏列的數量（`All tracks (548 · 37 hidden)`）：

- `audioModes` 中沒有 `STEREO` 的曲目（僅有 Dolby Atmos 的複本，音質階梯不會播放它們）
- **替代版本**：曲目的 `version`，或標題中括號的部分（`(…)`、`[…]`）或 ` - …` 後綴，在兩者都轉成小寫並去掉空格、連字號、底線、句點與 `+` 之後，等於其中一個**隱藏版本詞彙**。這些詞彙是一項設定，`TIDAL_PLAYER_HIDE_VERSIONS`（以逗號分隔；空字串 = 不隱藏任何東西；在 0008 之前），預設為 `instrumental, inst, off vocal, karaoke, tv version, tv ver, tv size, tv edit, sped up, speed up, nightcore, slowed, slowed + reverb, reverb, 8d, 8d audio`。詞彙也會比對該部分的開頭（`TV Size ver.` → `tvsizever` 以 `tvsize` 開頭）。`Acoustic`、`Live`、`Remix`、`Tiësto Remix` 會保留。其他視窗與頁面顯示每個版本

**角色篩選**（如 tidal.com 的 credits 頁面，`roleCategoryFilters`）：在 *All tracks* 按 `f` 會開啟一個彈出視窗，裡面有四個角色類別（Performer、Songwriter、Producer、Engineer）的核取方塊，一開始全部勾選；`Space` 切換一個，`Enter` 套用，`Esc` 取消。當藝人在曲目上的任何一個角色屬於已勾選的類別時，該曲目就會顯示。篩選是在客戶端對已載入的列套用的（API 沒有篩選參數：事實），所以載入更多頁面照常進行；標題顯示 `All tracks (548 · Performer, Songwriter · 37 hidden)`。篩選在頁面留在歷史中的期間持續有效

<a id="playing-and-queueing-from-a-page"></a>
### 從頁面播放與加入佇列

| 按鍵 | 對象 | 作用 |
|---|---|---|
| `Enter` | 曲目（瀏覽頁面） | 把佇列取代為**該清單的每一首曲目**（先擷取其餘部分），依頁面順序，並播放選中的那首：`LoadQueue { tracks, start: index }`（what went wrong 1、2） |
| `Enter` | 曲目（佇列頁面） | 播放該項目（0004，不變） |
| `Enter` | 專輯、播放清單或藝人 | 開啟它的頁面 |
| `Z`、`C-z` | 曲目 | 把它加到佇列尾端：`Open { items: [Track(id)], at: Some(End) }`，所以沒有東西在播放時它就開始播放（0005 的規則，決策 3） |
| `Z`、`C-z` | 專輯或播放清單 | 把它的所有曲目加到尾端：`Open { items: [album / playlist], at: Some(End) }`（由播放器展開，0005） |
| `Z`、`C-z` | 藝人 | 什麼都不做 |
| `d` | 項目（佇列頁面） | 移除它：`RemoveFromQueue(entry ID)`（0004 的規則：移除目前的項目會前進到下一個） |

<a id="actions-popup"></a>
### 動作彈出視窗

spotify-player 的動作彈出視窗：對選取的列按 `g a` 或 `C-Space`，對正在播放的曲目按 `a`。頁面上方的一個小清單，標題是該項目；`j`/`k` 移動，`Enter` 執行動作並關閉彈出視窗，`Esc` 關閉它。開啟期間其他按鍵都不作用。

| 項目 | 動作，依此順序 |
|---|---|
| 曲目（瀏覽頁面） | *Go to album*、*Go to artist: <name>*（每位藝人一個）、*Add to queue*、*Play next*、*Add to favorites* 或 *Remove from favorites*、*Add to playlist…*、*Remove from this playlist*（在自己的播放清單的播放清單頁面上） |
| 項目（佇列頁面），或正在播放的曲目（`a`） | *Go to album*、*Go to artist …*、*Play next*（正在播放的曲目沒有）、*Remove from queue*、*Add to favorites*/*Remove from favorites*、*Add to playlist…* |
| 專輯 | *Open*、*Go to artist …*、*Add to queue*、*Play next*、*Add to favorites*/*Remove from favorites*、*Add to playlist…* |
| 播放清單 | *Open*、*Add to queue*、*Play next*、*Add to favorites*/*Remove from favorites*（自己的播放清單沒有）、*Delete playlist*（僅限自己的） |
| 藝人 | *Open*、*Add to favorites*/*Remove from favorites* |

- *Play next* 是帶 `at: Next` 的 *Add to queue*。沒有專輯的曲目沒有 *Go to album*；沒有東西在播放時按 `a` 什麼都不做
- **是否為收藏**：彈出視窗詢問播放器（`IsFavorite`，對已載入的收藏 ID 清單的一個請求，`GET /users/{user}/favorites/ids`，見「Not verified」；在確認之前，彈出視窗同時顯示 *Add to favorites* 與 *Remove from favorites*，多餘時兩者都無害：寫入探測顯示重複加入與移除非收藏項目都回答 `200`）
- ***Add to playlist…*** 開啟第二個彈出視窗，列出使用者自己的播放清單（`USER_CREATED`，最新的在前），第一列是 *New playlist…*；`Enter` 把該項目的曲目（清單：全部，先擷取）加到尾端。已在該播放清單中的曲目會詢問 `Already in <title>: add again? (y/n)`；`y` 以 `onDupes=ADD` 加入。*New playlist…* 開啟一列的輸入提示 `Playlist name: `，建立一個以該標題命名的私人播放清單，然後加入
- ***Delete playlist*** 詢問 `Delete <title>? (y/n)`
- 結果顯示在訊息列：`Added to favorites`、`Removed from favorites`、`Added 12 tracks to <title>`、`Removed from <title>`、`Created <title>`、`Deleted <title>`，或錯誤。顯示被改動清單的頁面（收藏、音樂庫、播放清單）在變更成功後會重新擷取

<a id="esc-and-quitting"></a>
### `Esc` 與離開

如同 spotify-player：`Esc` 關閉彈出視窗或開啟中的輸入提示（或取消整份清單的載入），其他情況什麼都不做；`q`（與 `C-c`）離開。這改變了 0004，那裡的 `Esc` 也會離開（決策 2；0004 AC20 中 `Esc` 的那一列已更新）。

<a id="talking-to-the-player"></a>
### 與播放器溝通

瀏覽與音樂庫編輯是對播放器的請求（決策 1），只回答給發出請求的客戶端：

- `ClientMessage::Library { id, request }` → `ServerMessage::LibraryReply { id, result: Ok(LibraryResponse) | Err(message) }`
- 讀取：`Page(PageRequest)`（`Library`、`FavoriteTracks`、`Album(id)`、`Playlist(uuid)`、`Artist(id)`）→ `Page(PageData)`：頁面的標題與它每份清單的第一頁（*All tracks* 除外）。`More { list: ListRef, offset, limit }` → `Items(ListPage)`。`ListRef` 指名一份清單：`FavoriteTracks`、`Playlists`、`FavoriteAlbums`、`FavoriteArtists`、`AlbumTracks(id)`、`PlaylistTracks(uuid)`、`TopTracks(id)`、`ArtistAlbums(id)`、`ArtistAppearsOn(id)`、`Credits(id)`。`IsFavorite(kind, id)` → `Favorite(bool)`
- 寫入：`AddFavorite(kind, id)`、`RemoveFavorite(kind, id)`、`AddToPlaylist { uuid, tracks, allow_duplicates }`、`RemoveFromPlaylist { uuid, index, etag }`、`CreatePlaylist { title }` → `Created(PlaylistSummary)`、`DeletePlaylist { uuid }` → `Done`（或錯誤）。`kind` 是 track、album、artist 或 playlist

規則：

- 請求以工作（job）的形式在播放器的輸入迴圈旁邊執行（如同 0004 的建議與 0005 的 `Open` 展開），所以它們絕不延遲任何指令或事件。一個客戶端的請求一次執行一個，依送出的順序；不同客戶端的則各自獨立執行
- 每個請求一個 `LibraryReply`，失敗時也一樣。它不是 `Event`：其他客戶端看不到它
- 中斷連線的客戶端會失去它待處理的回覆；進行中的寫入不論如何都會在播放器中完成或失敗
- 失敗訊息：0004 的中繼資料措辭（`Album 1 was not found`、`Playlist <uuid> was not found`、`Artist 1 was not found`、`Track 1 was not found`）、`LoginRequired` 的工作階段過期訊息、`Could not reach Tidal: …`、`Tidal answered 429: try again in a moment`；播放清單編輯遇到 `412`/`7002` 時為 `The playlist changed: nothing was changed, try again`（並重新擷取播放清單頁面）。絕不在迴圈中重試
- **播放清單編輯與 ETag**：`AddToPlaylist` 讀取播放清單目前的 ETag（`GET /playlists/{uuid}`）並送出；遇到 `412` 時再讀一次並重試一次（加到尾端重試是安全的）。`RemoveFromPlaylist` 送出客戶端已載入的清單所帶的 ETag，而且**絕不重試**：`412` 表示位置可能已經位移。`DeletePlaylist` 先讀取目前的 ETag

<a id="what-the-player-fetches-and-writes"></a>
### 播放器擷取與寫入什麼

所有請求都經過 `Authenticator`，`countryCode` 來自工作階段（0004），`{user}` 是工作階段的 `user_id`。端點、封套、頁面大小與排序是探測的結果（「事實與假設」）。

| 清單／請求 | API 呼叫 | 最大頁面 | 保留 |
|---|---|---|---|
| `Playlists` | `GET /users/{user}/playlistsAndFavoritePlaylists?order=DATE&orderDirection=DESC` | 50 | `items[].playlist`（UUID、標題、`numberOfTracks`、`duration`）；項目的 `type` 是 `USER_CREATED` 時為**自己的**，`USER_FAVORITE` 時為收藏的 |
| `FavoriteAlbums`、`FavoriteArtists` | `GET /users/{user}/favorites/{albums,artists}?order=DATE&orderDirection=DESC` | 1000 | `items[].item`：專輯（ID、標題、藝人、來自 `releaseDate` 的年份、`type`、`numberOfTracks`、`duration`）、藝人（ID、名稱） |
| `FavoriteTracks` | `GET /users/{user}/favorites/tracks?order=DATE&orderDirection=DESC` | 1000 | `items[].item`（0004 的曲目對應） |
| `AlbumTracks(id)` | `GET /albums/{id}/tracks`（0004） | 1000 | 單純的曲目 |
| `PlaylistTracks(uuid)` | `GET /playlists/{uuid}/items`（0004，僅 `type == "track"`） | 100 | `items[].item` |
| `TopTracks(id)` | `GET /artists/{id}/toptracks` | 1000 | 單純的曲目 |
| `ArtistAlbums(id)` | `GET /artists/{id}/albums`，然後（在它耗盡時）`…/albums?filter=EPSANDSINGLES` | 1000 | 專輯，然後是 EP 與單曲 |
| `ArtistAppearsOn(id)` | `GET /artists/{id}/albums?filter=COMPILATIONS` | 1000 | 專輯 |
| `Credits(id)` | 第一頁與 `dataApiPath` 用 `GET /pages/contributor?artistId={id}&deviceType=BROWSER&locale=en_US`；之後用 `GET /{dataApiPath}&limit=…&offset=…` | 50 | `items[].item` 與 `items[].roles[].category`，依「All tracks」所述篩選 |
| 頁面標題 | `GET /albums/{id}`、`GET /playlists/{uuid}`（也取它的 `ETag` 標頭）、`GET /artists/{id}` | — | 專輯（標題、藝人、年份、曲目、長度）、播放清單（標題、曲目、長度、ETag）、藝人（名稱） |
| 收藏 | `POST /users/{user}/favorites/{tracks,albums,artists,playlists}` 表單 `trackIds`/`albumIds`/`artistIds`/`uuids`；`DELETE /users/{user}/favorites/{kind}/{id}` | — | — |
| 播放清單 | `POST /users/{user}/playlists` 表單 `title`、`description=""`；`POST /playlists/{uuid}/items` 表單 `trackIds`（以逗號分隔的清單，每個請求最多 100 個：更多的依序送出，每個都帶新的 ETag）、`onDupes=ADD`、`If-None-Match`；`DELETE /playlists/{uuid}/items/{index}` 帶 `If-None-Match`；`DELETE /playlists/{uuid}` 帶 `If-None-Match` | — | — |

頁面自己的資源（`/albums/{id}`、`/playlists/{uuid}`、`/artists/{id}`、`/pages/contributor`）上的 `404`/`2001` 是 `NotFound`；未知藝人的 `/albums` 是沒有項目的 `200`，所以由標題說明它不存在。頁面的標題與第一批頁面是平行擷取的；任何一個呼叫失敗，整個頁面就失敗（沒有半個頁面）。之後的頁面各自單獨失敗（見上）。沒有 `order`/`orderDirection` 時，收藏的清單不依日期排序，所以一律送出這些參數。

<a id="types-tidal_player_core"></a>
### 型別（`tidal_player_core`）

- `Track` 新增前往它的專輯與藝人所需的 ID：`artists: Vec<ArtistRef { id, name }>`（原本只有名稱）與 `album: Option<AlbumRef { id, title }>`（原本只有標題）。對應從現有的 `artists[].id`/`album.id` 欄位填入。繪製不變
- `library` 模組：`AlbumSummary { id, title, artists, year: Option<u16>, kind: Album | Ep | Single, tracks: Option<u32>, duration: Option<Duration> }`、`PlaylistSummary { uuid, title, tracks: Option<u32>, duration: Option<Duration>, own: bool }`、`CreditedTrack { track, roles: Vec<RoleCategory> }`、`ListPage<T> { items: Vec<T>, offset, total }`、`ListRef`、`PageRequest`、`PageData`、`LibraryRequest`、`LibraryResponse`、`FavoriteKind`，以及 `hidden_version(&Track, &[String]) -> bool`（版本篩選，純函式）
- 它們全都是 `Serialize`/`Deserialize`，因為它們要通過 socket

<a id="client-model"></a>
### 客戶端 model

`tidal_player_core::ui::State` 新增歷史（`Vec<Page>`，最底下的在前），每個 `Page` 持有它的種類、標題、它的視窗（每個：已載入的列、`total`、游標、`Idle | Loading { id } | Failed(message)`）、它的焦點；彈出視窗（動作、*Add to playlist…*、一個 `y/n` 詢問、名稱輸入提示）；以及進行中的整份清單載入。`Effect` 新增 `Library { id, request }`；`Action` 新增 `LibraryReply { id, result }`。請求 ID 來自 state 中的計數器（每次客戶端執行中唯一）。

- **中斷連線**（0005）期間，已載入的頁面仍然可以瀏覽，也可以從歷史開啟，但不會請求或送出任何東西：開啟新頁面會顯示它因中斷連線的訊息而失敗，待處理的載入也以它失敗。下一個 `Welcome` 時，若最上面的頁面是這樣失敗的，就重新擷取
- 當播放器表示**工作階段已過期**時，請求以工作階段過期的訊息失敗；`LoginRestored` 之後，再次開啟頁面就能運作

<a id="rendering"></a>
### 繪製

播放視窗下方的頁面取代佇列的區域（0004「TUI」的規則仍然成立：播放視窗保持它的 4 列，沒有東西與它共用列）。有多個視窗的頁面，在內部寬度至少 60 欄時並排繪製：

- Library：Playlists 40 %、Albums 40 %、Artists 20 %（spotify-player 的預設值）
- Artist：兩個**窗格（pane）**，每個有兩個**分頁（tab）**（lazygit 的窗格與分頁）：左邊 60 % 放 *Top tracks* 與 *All tracks*，右邊 40 % 放 *Albums* 與 *Appears on*。窗格顯示它作用中的分頁；它的標題列出兩者，作用中的反白：`Top tracks (91) │ All tracks  [ ]`。`Tab`/`Shift-Tab` 在窗格之間移動焦點（保留每個窗格作用中的分頁）；`[` 與 `]` 切換聚焦窗格的分頁（上一個／下一個，會循環）。在其他頁面上，每個視窗各自是一個窗格，`[`/`]` 什麼都不做（「Bugs」）

寬度低於 60 欄時只繪製聚焦的窗格，有其他窗格時它的標題後面會接 `‹Tab›`。標題列在視窗上方佔一列。彈出視窗置中於頁面上，寬度最多 50 欄，並被裁切到頁面內。列以 `…` 截斷，如同 0004；任何大小都不會 panic（包括 0 × 0）。

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Library                                                                       │
│┌Playlists (22)──────────────┐┌Albums (14)─────────────────┐┌Artists (196)───┐│
││Running                  42 ││Collide With The Sky  Pier…  ││Pierce The Veil ││
││♥ Late night             17 ││Misadventures         Pier…  ││Sleeping With S…││
│…                                                                             │
```

<a id="acceptance-criteria"></a>
## 驗收條件

協定與型別（`tidal_player_core`，純邏輯）：

- **AC1** — `ClientMessage::Library`、`ServerMessage::LibraryReply`、每個 `LibraryRequest`、`LibraryResponse`、`PageRequest`、`PageData` 與 `ListRef` 變體、各種摘要、`CreditedTrack`、`ListPage` 與新的 `Track` 形狀，都能通過 JSON 來回轉換（0001 AC11 的測試，已擴充）；0005 的編解碼器能解碼這兩個新訊息
- **AC2** — `Track` 的新欄位：0004 的中繼資料對應從現有的測試資料（fixture）填入 `artists[].id` 與 `album.id`；`album: null` 對應為 `None`。每個原本顯示藝人名稱與專輯標題的地方，顯示的文字都與之前相同（0004/0005 的繪製快照不變）
- **AC3** — `hidden_version`（表格，預設詞彙）：隱藏 — `version: "Instrumental"`、`"TV Size"`、`"TV-size ver."`、`"Sped Up"`、`"Slowed + Reverb"`、`"Off Vocal"`、`"Nightcore"`，標題 `"X (Instrumental)"`、`"X [TV Size]"`、`"X - Sped Up Version"`（`spedupversion` 以 `spedup` 開頭）、`"X (slowed & reverb)"`（詞彙清單加上 `slowed & reverb`）；保留 — `"Acoustic"`、`"Live"`、`"Tiësto Remix"`、`"Remastered 2011"`、括號之外含有 `Instrumental` 的標題（`"Instrumentally Yours"`）、`None`；空的詞彙清單不隱藏任何東西。設定：`resolve_player_config` 中的 `TIDAL_PLAYER_HIDE_VERSIONS` 與 `TIDAL_PLAYER_PAGE_SIZE`（0004 AC25 的表格，已擴充：未設定、空白、`1`、`10000`、`0`、`10001`、`x`）

API（`tidal-player-api::library`，依探測寫成的 wiremock 測試資料）：

- **AC4** — 讀取清單：對每個 `ListRef`，每個 `More` 一個請求，帶有路徑、`countryCode`、列出時的排序參數、`limit = min(asked, largest page)`（對播放清單或 credits 要求 100 會送出 50）與給定的 `offset`；解開封套（`items[].item`、帶有來自 `USER_CREATED`/`USER_FAVORITE` 的 `own` 的 `items[].playlist`、單純的項目）；`total` 來自 `totalNumberOfItems`；非曲目的播放清單項目被丟掉。純清單耗盡後，`ArtistAlbums` 從 offset 0 起以 `filter=EPSANDSINGLES` 繼續，`total` 為兩者之和（涵蓋每個 `ListRef` 的表格）
- **AC5** — 頁面：`Library`、`FavoriteTracks`、`Album`、`Playlist`、`Artist` 從平行的請求回傳標題與每份清單的第一頁；保留播放清單的 `ETag` 標頭；標題（以及 `/pages/contributor`）上的 `404`/`2001` → 帶有該項目訊息的 `NotFound`；`/albums` 回答空的 `200` 的未知藝人，仍然從 `/artists/{id}` 失敗；任何一個呼叫失敗，整個頁面就失敗
- **AC6** — Credits：第一頁來自 `/pages/contributor`（`ITEM_LIST_WITH_ROLES` 模組的 `pagedList`），之後的頁面來自它的 `dataApiPath` 並附加 `limit`/`offset`；角色類別已對應；沒有 `STEREO` 的曲目被丟掉，替代版本依 `hidden_version` 隱藏，並回報隱藏的數量；沒有這個模組的頁面 → `No credits`
- **AC7** — 寫入：`AddFavorite`/`RemoveFavorite` 依種類送出表單欄位（`trackIds`、`albumIds`、`artistIds`、`uuids`）與 `DELETE` 路徑；`CreatePlaylist` 送出 `title`；`AddToPlaylist` 讀取 ETag，以 `If-None-Match` 送出，`trackIds` 以逗號連接（250 首曲目：3 個請求，每個都帶上一個所回答的 ETag），且只在允許時才帶 `onDupes=ADD`，遇到一次 `412` 時重新讀取並重試一次（第二次 `412` → 播放清單已改變的訊息）；`RemoveFromPlaylist` 送出給定的 ETag 與位置，遇到 `412` 時回傳播放清單已改變的訊息而不重試；`DeletePlaylist` 讀取 ETag 並送出；`LoginRequired` 與暫時性錯誤由每個呼叫原封不動地回傳（表格）

播放器（`tidal-player`，假的音樂庫來源，0005 的伺服器測試）：

- **AC8** — 每個 `Library` 請求都以恰好一個帶有相同 `id` 的 `LibraryReply` 回答，且只回給該客戶端（另一個訂閱者什麼都收不到）；錯誤變成帶有「與播放器溝通」下訊息的 `Err(message)`（表格）；當 fake 讓一個請求保持待處理時，同一個客戶端的 `Request { TogglePause }` 在 1 秒內獲得回覆，事件持續流動；一個客戶端的第二個請求在它的第一個獲得回答之後才開始，而另一個客戶端的則獨立進行；請求進行中中斷連線的客戶端不影響播放器

客戶端 model（`tidal_player_core::ui`，純邏輯）：

- **AC9** — 歷史：`z`、`g l`、`g y`，以及在專輯／播放清單／藝人列上按 `Enter`，會推入頁面並以新的 ID 發出 `Library { Page(…) }`（佇列頁面不發出任何東西）；再次開啟最上面的頁面什麼都不做；`Backspace`/`C-q` 彈出，而下面的頁面顯示它保留的列、游標與焦點，沒有任何請求；最底下的頁面絕不被彈出；第 51 次推入會丟掉佇列之上最舊的頁面。啟動：`start_on_library` 把音樂庫放在佇列之上並顯示 `Loading…`，不發出任何東西；第一個 `Welcome` 發出它的 `Page(Library)` 請求一次（第二個 `Welcome` 不會）；在它之前的 `Disconnected` 讓頁面以中斷連線的訊息失敗，下一個 `Welcome` 再擷取它。回覆只套用到帶有該 ID 的頁面／視窗，即使它不在最上面也一樣；未知 ID 的回覆會被丟掉（表格）
- **AC10** — 捲動：游標移動進入離最後一個已載入列一個視窗高度之內時，發出一次 `More { list, offset: loaded so far rounded up to the page size, limit: page size }`（offset 為目前已載入的數量向上取整到頁面大小；有一個待處理時不發出第二個）；它的列被附加，並略過已知的 ID；在總數之前的較短頁面不會停止之後的載入；失敗的 `More` 把它的訊息顯示為最後一列，下一次在接近結尾處移動時再問一次；`G` 移到最後一個已載入的列；*All tracks* 在第一次被聚焦時要求它的第一頁；角色篩選（`f`、`Space`、`Enter`、`Esc`）只顯示屬於已勾選類別的列並保留隱藏的數量，而當篩選後的列在游標附近用完時要求更多頁面（表格）
- **AC11** — 視窗與游標：`Tab`/`BackTab` 在頁面的視窗之間循環焦點，會循環；游標按鍵只移動聚焦視窗的游標，限制在已載入的列內；游標在離開再回來之後仍保留；載入中、失敗或空的視窗會忽略它們
- **AC12** — 播放與加入佇列（涵蓋「從頁面播放與加入佇列」每一列的表格）：在完整載入的 *n* 首曲目清單的第 *i* 首曲目上按 `Enter`，送出 `LoadQueue { tracks: the list in page order, start: i }`（清單依頁面順序，絕不排序或隨機排列）；在部分載入的清單上，它先發出 `More` 直到載入總數，然後送出一次；載入期間按 `Esc` 會取消且不送出任何東西；超過 40 000 首曲目時不送出任何東西，並設定太多的訊息；對曲目按 `Z`/`C-z` 送出 `Open { [Track(id)], Some(End) }`，對專輯／播放清單送出 `Open { [item], Some(End) }`，對藝人什麼都不送；在佇列上按 `d` 送出 `RemoveFromQueue(entry ID)`，在瀏覽頁面上什麼都不送；佇列的 `Enter` 是 0004 的
- **AC13** — 動作彈出視窗：`g a`/`C-Space` 對選取的列開啟它，`a` 對正在播放的曲目開啟，動作依表格中每種項目的種類與順序（表格，包括有兩位藝人的曲目、沒有專輯的曲目、自己的與追蹤的播放清單、在自己的播放清單頁面上的曲目；沒有東西在播放時按 `a` 什麼都不開啟）；`j`/`k` 移動，`Enter` 發出該動作的效果（`Go to …` 推入並擷取該頁面；*Play next* 送出 `at: Next`；收藏以正確的種類與 ID 送出 `AddFavorite`/`RemoveFavorite`）並關閉它；`Esc` 關閉它而沒有任何效果；開啟期間其他按鍵都不作用（包括 `Space`、`n`、`q`）
- **AC14** — model 中的播放清單編輯：*Add to playlist…* 只列出自己的播放清單，*New playlist…* 在最前面；選擇其中一個會送出 `AddToPlaylist`（清單：先整份擷取）；已在該播放清單已載入副本中的曲目會詢問 `y/n`，`y` 送出 `allow_duplicates: true`，`n` 什麼都不送；*New playlist…* 接受一個名稱（空名稱：什麼都不做）並送出 `CreatePlaylist`，然後對建立的 UUID 送出 `AddToPlaylist`；*Remove from this playlist* 送出該列的位置與頁面的 ETag；*Delete playlist* 詢問 `y/n`；每個成功都會設定它的訊息，並在受影響的頁面位於歷史中時重新擷取它；錯誤會設定訊息而且不改變任何東西（表格）
- **AC15** — 沒有彈出視窗、輸入提示或整份清單載入時按 `Esc` 不發出任何東西（0004 AC20 中 `Esc` 的那一列由 `Quit` 改掉）；`q` 與 `C-c` 發出 `Quit`
- **AC16** — 中斷連線與登入（表格）：中斷連線期間，開啟頁面會顯示它因中斷連線的訊息而失敗且不發出請求，待處理的載入以它失敗，歷史與游標可用，`Enter`/`Z`/`d` 與彈出視窗的動作不發出任何東西；之後的第一個 `Welcome` 會重新擷取最上面的頁面（如果它是這樣失敗的；而且只有它）；工作階段過期的錯誤顯示在頁面中

TUI 繪製與執行階段（`tidal-player`）：

- **AC17** — 繪製（`insta`，80×24，用眼睛審閱，加上 `contains` 檢查）：載入完成的音樂庫（三個視窗、數量、焦點、`♥`）；載入中的音樂庫；收藏曲目，其中一個不可串流的列以淡色顯示，最後一列是 `Loading more…`；專輯頁面標題列；藝人頁面（兩半、`‹Tab›` 標題、隱藏的數量）；失敗的頁面；每個空清單的訊息；動作彈出視窗；*Add to playlist…*；一個 `y/n` 詢問。50×20：只有聚焦的視窗，帶 `‹Tab›`。從 0×0 到 120×40，每個頁面與彈出視窗都不 panic（涵蓋大小 × 頁面的表格）
- **AC18** — 按鍵解碼：`Tab`、`BackTab`、`Backspace`、`PageUp`/`PageDown`、`C-Space`、`C-f`/`C-b`/`C-q`/`C-z`/`C-c` 解碼為 model 的按鍵（`crates/app/src/tui/input.rs`，表格）
- **AC19** — 接線：TUI 客戶端 runtime 把 `Effect::Library` 以 `ClientMessage::Library` 送出，並把 `LibraryReply` 轉成 `Action::LibraryReply`，連上的與獨立模式的都一樣（行程內連結，0005）；載入頁面的連上的客戶端仍然不開啟工作階段儲存、金鑰圈或密碼來源（0005 AC14 的測試，其中一個 `g l` 由測試播放器的 wiremock API 回答）

文件：

- **AC20** — `docs/tui.md` 記載頁面、歷史、視窗、捲動載入、從頁面播放與加入佇列、動作彈出視窗、收藏與播放清單編輯、新的按鍵與改變的 `Esc`、這兩項設定，以及空的／失敗的訊息；`docs/playback.md` 的「Settings」列出這兩項設定；`CLAUDE.md` 的「Status」提到本規格；本規格連到它們
- **AC21** — 播放清單列的位置是該項目在 Tidal 清單中的位置，被丟掉的影片也算在內：`ListPage::positions` 從 API 帶來它，`Window::positions` 保存它，而移除時送出它

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 數千首收藏曲目 | 一次載入 100 列，捲動時載入更多；播放與其他按鍵繼續運作（AC8、AC10） |
| 在 5 000 首曲目的清單的第 3 首曲目上按 `Enter` | 先載入其餘部分（`Loading 300 of 5 000…`，`Esc` 取消），然後一個 5 000 首的 `LoadQueue`（socket 上約 1.5 MB） |
| 使用者開啟頁面的速度比載入更快 | 每個請求有自己的 ID；結果落在它自己的頁面上，或被丟掉（AC9）；一個客戶端的請求在播放器中一次執行一個（AC8），所以狂按不會對 Tidal 爆發請求 |
| 載入時遇到 `429` | 頁面或 `Loading more…` 列顯示訊息；沒有任何東西在迴圈中重試（0004「What went wrong」3） |
| 收藏的專輯或播放清單已從 Tidal 移除 | 照 Tidal 回傳的列出；開啟它顯示 `Album 1 was not found` |
| 在使用者的國家無法取得的曲目 | 以淡色顯示；加入佇列後，由播放器以僅限曲目的失敗跳過（0004 AC7） |
| 只有影片的播放清單 | `This playlist has no tracks`；對它按 `Z`：`Playlist <uuid> has no tracks`（0004） |
| 沒有專輯、top tracks 或 credits 的藝人 | 該視窗會這麼說 |
| 大部分列都被隱藏的 *All tracks* | 標題說明隱藏了多少；篩選後結果為空時顯示 `No credits (37 hidden)` |
| 沒有專輯的曲目（`album: null`） | 沒有 *Go to album* 動作 |
| 在移除之前，播放清單在別處（另一個應用程式）被編輯過 | `412`：`The playlist changed: nothing was changed, try again`；頁面重新載入；沒有任何東西因過時的位置而被移除 |
| 把重複的曲目加到播放清單 | 先詢問（`y/n`）；Tidal 本身是會接受的 |
| 瀏覽或編輯時工作階段過期 | 請求以工作階段過期的訊息失敗；狀態列顯示它（0002 AC14） |
| 頁面載入時播放器關閉或連線中斷 | 頁面以中斷連線的訊息失敗，若仍在最上面，重新連線時會再次擷取（AC16） |
| 客戶端與播放器之間的版本不符 | 在問候階段被拒絕（0005），與之前相同 |
| 窄的終端機 | 一次一個視窗（AC17）；內部列數低於 6 時只有播放視窗（0004） |

<a id="test-plan"></a>
## 測試計畫

每個自動化測試都以它的條件命名（`ac10_…`）。紅燈是對存根型別與函式本體存根的斷言失敗（沒有 `todo!()`、沒有編譯錯誤），如同 0001–0005。API 測試使用 `wiremock`，測試資料（fixture）放在 `crates/api/tests/fixtures/library/` 之下，依探測記錄下來的形狀寫成（ID 與名稱換成假的；寫入探測的 ETag 與 `412` 本文照記錄的樣子）。伺服器測試重用 0005 的測試框架，搭配一個能讓請求保持待處理的假音樂庫來源。

| AC | 測試（檔案 :: 名稱） | 驗證內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (extended) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | 來回轉換；解碼 | 新型別手寫的存根 `Serialize` 寫出 `null` |
| AC2 | `crates/api/tests/metadata.rs` :: `ac2_track_refs` | 已對應的 ID；`album: null` → `None` | 存根對應讓 `id` 維持 0 |
| AC3 | `crates/core/src/library.rs` :: `ac3_hidden_version` (table) + `crates/app/src/settings.rs` :: `ac25_player_config` (new rows) | 每列隱藏或保留；設定 | 存根什麼都不隱藏 |
| AC4 | `crates/api/tests/library.rs` :: `ac4_lists` (table over `ListRef`) | 路徑、參數、被限制的 `limit`、解開封套、`total` | 存根忽略 `offset` 與限制 |
| AC5 | `crates/api/tests/library.rs` :: `ac5_pages` (table) | 標題 + 第一頁；ETag；錯誤 | 存根回傳空的標題 |
| AC6 | `crates/api/tests/library.rs` :: `ac6_credits` | 第一頁與 `dataApiPath` 頁面；篩選；隱藏的數量 | 存根保留僅有 Atmos 與 instrumental 的列 |
| AC7 | `crates/api/tests/library.rs` :: `ac7_writes` (table) | 表單、路徑、`If-None-Match`、加入時重試一次、移除時不重試 | 存根不送 ETag，所以每次編輯都是 `412` |
| AC8 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender`, `ac8_errors` (table), `ac8_does_not_block` | 一個回覆、只回給發送者、訊息、延遲、每個客戶端的順序 | 存根在播放器執行緒上執行請求，所以被扣住的請求會擋住 `TogglePause` 的回覆 |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_history` (table), `ac9_reply_by_id` (table), `ac9_start_on_library` | 堆疊、效果、保留的狀態、丟掉過時的 | 存根取代頁面而不是推入 |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_scroll_loads` (table) | `More` 的 offset、沒有重複、較短的頁面、失敗 | 存根從不要求第二頁 |
| AC11 | `crates/core/src/ui/browse/tests.rs` :: `ac11_windows_and_cursors` (table) | 焦點循環、每個視窗的游標 | 存根移動每個視窗的游標 |
| AC12 | `crates/core/src/ui/browse/tests.rs` :: `ac12_play_and_queue` (table) | 每一列的確切指令；整份清單載入 | 存根的 `Enter` 只送出已載入的列 |
| AC13 | `crates/core/src/ui/browse/tests.rs` :: `ac13_actions_popup` (table) | 每種項目的動作、效果、關閉、按鍵擷取 | 存根彈出視窗只列出 *Add to queue* |
| AC14 | `crates/core/src/ui/browse/tests.rs` :: `ac14_playlist_editing` (table) | 請求、詢問、訊息、重新擷取 | 存根加入重複的曲目而不詢問 |
| AC15 | `crates/core/src/ui/browse/tests.rs` :: `ac15_esc_and_quit` + 0004's `ac20_keys_to_commands` (row updated) | `Esc` → 無；`q`/`C-c` → `Quit` | 存根讓 `Esc` 維持 → `Quit` |
| AC16 | `crates/core/src/ui/browse/tests.rs` :: `ac16_disconnected_and_login` (table) | 中斷連線時沒有請求；一次重新擷取 | 存根在中斷連線時發出請求 |
| AC17 | `crates/app/src/tui/render.rs` :: `ac17_pages_80x24` (one snapshot per row), `ac17_narrow_50x20`, `ac17_no_panic_any_size` | `contains` 檢查 + 快照 | 存根的繪製不論哪個頁面都畫出佇列 |
| AC18 | `crates/app/src/tui/input.rs` :: `ac18_key_events` (table) | 解碼的按鍵 | 存根把新按鍵對應到無 |
| AC19 | `crates/app/src/client.rs` :: `ac19_library_round_trip` (attached and in-process) + `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (extended) | 雙向的訊息；沒有開啟任何工作階段 | 存根客戶端丟掉 `Effect::Library` |
| AC20 | — reviewed at acceptance | 文件與本規格相符，並已連結 | — |

刻意不由自動化測試涵蓋，而是在驗收時以真實帳號親手檢查（結果寫在 PR 說明中）：

- `g l` 列出與 Tidal 應用程式相同的播放清單、專輯與藝人（以及數量）；`g y` 以相同的順序列出相同的收藏曲目；捲動到 362 首收藏的結尾會把它們全部載入，沒有重複
- 一份很大的清單在曲目播放時載入，播放與按鍵都沒有停頓
- 在播放清單中按 `Enter` 會從那首曲目開始播放，其餘的排在佇列中；在音樂庫的專輯上按 `Z` 會加入它；從正在播放的曲目 *Go to artist* 會開啟該藝人；有 instrumental 與 sped-up 版本的藝人，它的 *All tracks* 會隱藏它們
- 加入與移除一個收藏、建立播放清單、加入曲目、移除一首、刪除它：每一項都會顯示在 Tidal 應用程式中

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`library`（摘要、ref、`ListPage`、請求與回應、`hidden_version`）、`track`（`ArtistRef`、`AlbumRef`）、`protocol`（`Library`、`LibraryReply`）、`ui`（頁面、歷史、視窗、捲動載入、彈出視窗、`Effect::Library`、`Action::LibraryReply`、新的按鍵）。沒有 I/O，沒有新的相依套件
- `tidal-player-api::library`：讀取與寫入，以及它們的 DTO → core 對應；`MetadataError::NotFound` 接受新的 `Artist` 種類；讀取 ETag 標頭
- `tidal-player`：`ipc::server`（`Library` → 一個工作；`LibraryReply` 送到該客戶端的發送佇列）、`player::runtime`（與 0004 的 `Metadata` 並列的 `Library` trait，由 API 客戶端實作並在測試中以 fake 取代）、`play.rs` 的設定、`client.rs`（效果與動作）、`ui.rs`（頁面、視窗、彈出視窗）、`input.rs`（新的按鍵）
- `xtask layering`：不變

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-07，從程式碼與文件）：

- 工作階段持有 `user_id` 與 `country_code`（`crates/api/src/auth.rs`，`Session`）
- 曲目 DTO 已經能從帶有 `artists[].id` 與 `album.id` 的物件反序列化（0004「Facts」；`crates/api/tests/fixtures/metadata/track.json`）
- 播放器 runtime 已經以工作的形式在它的輸入迴圈旁邊執行擷取（`crates/app/src/player/runtime.rs` 中的 `RuntimeInput::Expanded`、`RuntimeInput::Suggestions`），而伺服器能對單一客戶端送出訊息（`Hub::reply`）
- spotify-player 的預設按鍵（它的 `README.md`「Commands」表，2026-10-07 閱讀）：`g l` 音樂庫、`g y` 喜歡的曲目、`z` 佇列、`Backspace`/`C-q` 上一頁、`Tab` 下一個視窗、`g a`/`C-Space` 對選取項目的動作、`a` 對目前曲目的動作、`Z`/`C-z` 把選取的項目加到佇列、`Esc` 關閉彈出視窗、`q`/`C-c` 離開；它的音樂庫頁面依序有 Playlists、Albums 與 Artists 視窗，比例為 40/40/20 %（`spotify_player/src/ui/page.rs` `render_library_page`、`docs/config.md` `library.*_percent`）；它的藝人頁面有 top tracks、albums 與 related artists

2026-10-07 由使用者執行 `scripts/tidal-library-probe.sh`，對**真實 API** 驗證（與 0004 的探測相同的帳號與客戶端：22 份播放清單、362 首收藏曲目、14 張專輯、196 位藝人）。`crates/api/tests/fixtures/library/` 之下的測試資料（fixture）依這些形狀寫成，ID 與名稱已換掉：

- `GET /v1/users/{user}/playlistsAndFavoritePlaylists` → `{limit, offset, totalNumberOfItems, items: [{type, created, playlist}]}`，`type` 為 `USER_CREATED`（自己的，10 份）或 `USER_FAVORITE`（12 份，與 `/favorites/playlists` 的總數相符）；`playlist` 有 `uuid`、`title`、`numberOfTracks`、`numberOfVideos`、`duration`（秒）、`type`（`USER`、`EDITORIAL`）、`created`、`lastUpdated`、`creator`、`description` 與圖片欄位。預設頁面 10；**`limit=1000` → `400`/`1001` `Too big page, max page size is [50]`**；`limit=3&offset=2` 如預期分頁。帶 `order=DATE&orderDirection=DESC` 時，項目的 `created`（收藏的話是加入日期）嚴格地由新到舊。`/users/{user}/playlists`（tidalt 的）只列出 10 份自己的播放清單，是單純的形式；`/favorites/playlists` 只列出 12 份收藏，形式為 `{created, item}`
- `GET /v1/users/{user}/favorites/tracks` → `{limit, offset, totalNumberOfItems, items: [{created, item: track}]}`，`item` 是 0004 測試資料（fixture）中完整的曲目（帶有 `artists[].id`、`album.id`、`album: {id, title, cover, …}`）；沒有 `type` 欄位，也沒有影片項目（每個項目都是曲目）。預設頁面 10，而且**不依日期排序**；`order=DATE&orderDirection=DESC` 由新到舊，`ASC` 由舊到新。`limit=1000` 與 `limit=10000` 都被接受，並**在 `totalNumberOfItems: 362` 時回傳 358 個項目**
- `GET /v1/users/{user}/favorites/albums` → `{created, item: album}` 項目；專輯有 `id`、`title`、`artists[]`、`releaseDate`（`YYYY-MM-DD`）、`numberOfTracks`、`numberOfVolumes`、`duration`、`type`（見過 `ALBUM`、`SINGLE`）、`allowStreaming`/`streamReady`。預設頁面 10；`limit=1000` 被接受；DESC 排序由新到舊
- `GET /v1/users/{user}/favorites/artists` → `{created, item: {id, name, picture, artistTypes, mixes, …}}`。預設頁面 10；`limit=1000` 回傳全部 196 位；DESC 由新到舊（許多項目共用同一個 `created`，來自一次批次匯入；它們彼此之間的順序由 API 決定）
- `GET /v1/users/1/favorites/tracks`（另一位使用者的 ID）回答 `200`，內容是**這個帳號的** 362 首收藏：路徑中的使用者 ID 不會與權杖核對。送出的仍然是工作階段的 `user_id`
- `GET /v1/albums/{id}` → 上述的專輯物件；未知 → `404 {"subStatus":2001,"userMessage":"Album [1] not found"}`。`GET /v1/playlists/{uuid}` → 上述的播放清單物件；未知 → `404`/`2001` `Playlist not found`
- `GET /v1/artists/{id}` → `{id, name, picture, artistTypes, artistRoles, mixes, …}`；未知 → `404`/`2001` `Artist [1] not found`。`GET /v1/artists/1/albums`（未知藝人）→ `200`，`totalNumberOfItems: 0`
- `GET /v1/artists/{id}/toptracks` → 單純的曲目；預設頁面 10，`totalNumberOfItems: 91`；`limit=100` 回傳全部 91 首；`limit=1000` 被接受
- `GET /v1/artists/{id}/albums` → 單純的專輯，預設頁面 10，`limit=1000` 被接受；沒有 `filter` 時只有 `type: ALBUM`（12 張）。`filter=EPSANDSINGLES` → `EP` 與 `SINGLE`（66 張）。`filter=COMPILATIONS` → 「appears on」專輯（30 張，大多是 `Various Artists`，各種類型都有）。同一個標題可能出現兩次而 ID 不同（兩個 `DISASTERPIECE` 發行版，差別在 `mediaMetadata.tags`）；兩者都保留，如同 Tidal 的列法
- `GET /v1/artists/{id}/similar` → `{limit, offset, totalNumberOfItems, items: [artist + relationType: "SIMILAR_ARTIST"], source: "TiVo"}`（這位藝人有 4 位）

2026-10-07 由使用者執行 `scripts/tidal-library-write-probe.sh`，對**真實 API** 驗證（它加入的所有東西之後都已移除；探測用的播放清單已刪除）：

- **收藏**：`POST /v1/users/{user}/favorites/{tracks|albums|artists|playlists}?countryCode=…` 帶表單本文 `trackIds` / `albumIds` / `artistIds` / `uuids` → `200`、本文為空、有 `ETag` 標頭。再次加入也是 `200`（沒有重複：總數只多了一個）。逗號清單（`trackIds=a,b`）會兩個都加入。加入的項目出現在 DATE DESC 清單的最前面。`DELETE /v1/users/{user}/favorites/{kind}/{id}` → `200`，即使它不是收藏也一樣。未知的曲目 → `404`/`2001`（訊息是 `Album not found`，所以該訊息不原樣顯示）
- **建立**：`POST /v1/users/{user}/playlists?countryCode=…` 帶表單 `title`、`description` → `201` 與播放清單物件（`uuid`、`numberOfTracks: 0`、`publicPlaylist: false`）、`ETag` 標頭
- **編輯需要目前的 ETag**：`GET /v1/playlists/{uuid}` 回傳 `ETag: "<lastUpdated ms>"`；每次編輯都把它以 `If-None-Match` 送出。沒有它，或帶過時的，會得到 `412 {"subStatus":7002,"userMessage":"You must send the correct Etag value in the If-None-Match header to modify a playlist"}`。每次成功的編輯都回答新的 ETag
- **加入**：`POST /v1/playlists/{uuid}/items` 帶表單 `trackIds`（逗號清單）、`onDupes`（`FAIL`/`ADD`）、選用的 `toIndex` → `200 {"lastUpdated", "addedItemIds"}`；`toIndex=0` 插入在最上面。`onDupes=FAIL` 是否拒絕已在播放清單中的曲目並未被顯示（探測的「重複」當時還不是重複）；應用程式只在使用者確認要加入重複項目時才送出 `onDupes=ADD`，否則自己檢查已載入的清單
- **移除**：`DELETE /v1/playlists/{uuid}/items/{index}`（從 0 起算的位置）帶 ETag → `200`；用前一個 ETag 送出同一個請求 → `412`。位置在每次移除之後都會位移，所以移除是以位置定址，*並且*對照清單載入時所帶的 ETag 檢查：遇到 `412` 時，應用程式重新載入播放清單並說 `The playlist changed: removed nothing, try again`，絕不以過時的位置移除
- **刪除**：`DELETE /v1/playlists/{uuid}` 帶 ETag → `204`；然後 `GET` → `404`/`2001`
- 別人的播放清單沒有 ETag → `412`/`7002`，所以沒有測試所有權；*Add to playlist* 只列出使用者自己的播放清單（`USER_CREATED`），所以它從不嘗試

2026-10-07 由使用者從 tidal.com 擷取的內容與 `scripts/tidal-artist-tracks-probe.sh`（藝人 7367609，Dean Lewis）驗證：

- `GET /v1/pages/contributor?artistId={id}&countryCode=…&deviceType=BROWSER&locale=en_US` → 在 `api.tidal.com` 上為 `200`，有無權杖皆然：`{title: "Credits", rows: [{modules: [CONTRIBUTOR_HEADER {title: "Credits for Dean Lewis", artist}]}, {modules: [ITEM_LIST_WITH_ROLES {pagedList: {dataApiPath: "pages/data/<uuid>?artistId={id}", limit: 50, offset: 0, totalNumberOfItems: 548, items: [{item: track, type: "track", roles: [{name, category, categoryId}]}]}, roleCategories, …}]}]}`；最受歡迎的在前。未知藝人 → `404 {"subStatus":2001,"userMessage":"Not found"}`
- `GET /v1/{dataApiPath}&countryCode=…&deviceType=BROWSER&locale=en_US&limit=50&offset=50` → 接下來的 50 個（`{limit, offset, totalNumberOfItems: 548, items}`）；**`limit` 超過 50 → `400`/`1001` `Too big page, max page size is [50]`**
- `roleCategoryId=11` 被忽略（相同的總數、相同的前幾個 ID）：沒有伺服器端的角色篩選。使用者擷取 tidal.com 選擇角色類別的過程（藝人 7514330），顯示網頁播放器呼叫的是同一個未篩選的 `/pages/contributor` URL；這個選擇只存在於頁面位址（`/credits/{id}?roleCategoryFilters=2,11`），所以網頁是自己篩選清單的（或用之後一個沒有被擷取到的請求）
- 在前 100 個項目中：4 首僅有 `["DOLBY_ATMOS"]` 的曲目（例如 "Memories" 的一個 `LOW` 音質複本）、1 首 `["STEREO","DOLBY_ATMOS"]`，其餘為 `["STEREO"]`；見過的版本：`null`、`Acoustic`、`Tiësto Remix`；以及別人曲目上僅有詞曲創作與製作的 credits

未驗證：

- `GET /users/{user}/favorites/ids`（彈出視窗用來得知某個項目是否為收藏的收藏 ID 清單）：未探測。在那之前，彈出視窗同時顯示 *Add* 與 *Remove*（多餘時兩者都無害）；只有在使用者記錄下來時，才會在實作中對照測試資料（fixture）確認
- dataApiPath 的 `<uuid>` 是否跨工作階段穩定（每次開啟頁面都會從 `/pages/contributor` 重新讀取，所以無所謂）
- 較短的頁面（362 中的 358）是否是伺服器丟掉了在該國家無法取得的曲目，以及它是否發生在最後一頁以外的頁面上（這個遍歷兩者都能處理，AC4）
- 收藏的專輯／藝人與藝人專輯端點上超過 1000 的頁面大小（不需要：遍歷使用 1000）
- Tidal 應用程式自己顯示音樂庫的順序
- 0004 的專輯與播放清單遍歷（`offset` 以收到的項目數前進）在 `/albums/{id}/tracks` 或 `/playlists/{uuid}/items` 上是否會遇到相同的較短頁面。如果會，它們就會請求重疊的頁面，並把同一首曲目加入佇列兩次；本規格保持它們原樣，若看到這種情況，0004 的「Bugs」會新增一筆

從 tidalt 的程式碼與歷史驗證（2026-10-07）：「tidalt 的做法」下的清單。未驗證：tidalt 的單一請求清單在真實帳號上是否真的被截斷（從 `limit` 的值推斷；沒有 issue 回報過）

<a id="decisions-answered-by-the-user-2026-10-07-folded-into-the-body-above"></a>
## 決策（使用者於 2026-10-07 回答；已併入上面的本文）

1. **頁面在哪裡擷取**：*在播放器中*（`Fetch`/`Fetched`），照提議
2. **`Esc`**：*照提議*：只關閉彈出視窗或輸入提示；`q`/`C-c` 離開（0004 AC20 中 `Esc` 的那一列改變）
3. **沒有東西在播放時對曲目按 `Z`**：*讓它開始播放*，以 `Open { items: [Item::Track(id)], at: Some(End) }` 送出
4. **收藏與播放清單編輯**：*在本規格中*（2026-10-07：「why not」，然後對播放清單編輯回答「yes」）。動作彈出視窗：對曲目、專輯、藝人與播放清單提供 *Add to favorites*/*Remove from favorites*；*Add to playlist…*（選一份自己的播放清單，或新的）、在自己播放清單的曲目上提供 *Remove from playlist*、*New playlist*、*Delete playlist*（自己的）。形狀、ETag 與錯誤碼來自 `scripts/tidal-library-write-probe.sh`（「事實」）
5. **清單隨著捲動載入**（2026-10-07：「can do inf-fetching if they do」）：Tidal 以 `limit`/`offset`（= 頁面大小／頁面索引 × 大小）分頁並回傳 `totalNumberOfItems`（讀取探測）。所以頁面先擷取第一頁 *N* 個項目（**頁面大小**，一項設定，預設 **100**，1–10 000，依端點限制：播放清單為 50），並立刻顯示總數（`Favorite tracks · 362 tracks`）；游標進入離最後一個已載入列一個視窗高度之內時，擷取下一頁（最後一列顯示 `Loading more…`）。播放或加入整份清單（在曲目上按 `Enter`、在清單上按 `Z`/*Add to playlist*）時，先擷取其餘部分，然後送出。清單不再有 10 000 個項目的上限；對比 socket 的 16 MiB 一行所能容納的（約 50 000 首曲目）更長的清單按 `Enter` 會被拒絕並顯示訊息（本文把上限設為 40 000 首曲目）
6. **藝人頁面**：視窗 **Top tracks**、**Albums**（專輯，然後是 EP 與單曲）、**Appears on**（`filter=COMPILATIONS`）與 **All tracks** = Tidal 應用程式的 **「Credits for <artist>」**（使用者，2026-10-07）。使用者擷取了 tidal.com 的呼叫：`GET /v1/pages/contributor?artistId={id}&countryCode=…&locale=en_US&deviceType=BROWSER` → 一個頁面，它的第二列含有一個 `ITEM_LIST_WITH_ROLES` 模組，其中 `pagedList {dataApiPath: "pages/data/<uuid>?artistId={id}", limit: 50, offset: 0, totalNumberOfItems: 548, items: [{item: track, type: "track", roles: [{name, category, categoryId}]}]}`，最受歡迎的在前，以及 `roleCategories` Performer (11)、Songwriter (2)、Producer (1)、Engineer (3)。它包含藝人只被列為詞曲創作者或製作人的曲目、remix 與 acoustic 版本，以及一首曲目僅有 Dolby Atmos 的複本（`audioModes: ["DOLBY_ATMOS"]`、`audioQuality: LOW`）。*All tracks* 如決策 5 那樣透過 `dataApiPath` 分頁，顯示每首曲目的角色類別，保持 API 的順序，並丟掉：重複項目（相同的曲目 ID）、`audioModes` 中沒有 `STEREO` 的曲目，以及替代版本（見下）。`scripts/tidal-artist-tracks-probe.sh` 在 `api.tidal.com` 上確認了這些：透過 `dataApiPath` 每頁最多 50 個，而且沒有角色篩選（「事實」）

   **替代版本不放進 *All tracks***，以一份可設定的詞彙清單決定（2026-10-07：instrumental，然後是「TV Version, Speed Up, Slowed + Reverb, etc.」；一項設定，`TIDAL_PLAYER_HIDE_VERSIONS`，以逗號分隔，在 0008 把它移到 `app.toml` 之前）。當曲目的 `version`，或標題中括號的部分或 ` - ` 後綴，在兩者都轉成小寫並去掉空格、連字號、底線、句點與 `+` 之後符合其中一個詞彙時，該曲目就被省略。預設：`instrumental`、`inst`、`off vocal`、`karaoke`、`tv version`、`tv ver`、`tv size`、`tv edit`、`sped up`、`speed up`、`nightcore`、`slowed`、`slowed + reverb`、`reverb`、`8d`、`8d audio`。以表格測試（`TV-size`、`TV Size ver.`、`(Sped Up)`、`Slowed + Reverb`、`[Instrumental]`、`- Off Vocal`；而 `Acoustic`、`Tiësto Remix` 保留）。其他視窗顯示每個版本

<a id="out-of-scope"></a>
## 不在範圍內

- 搜尋（0007）
- mix、電台與「My mixes」頁面（0011）；spotify-player 的 *GoToRadio*
- 重新命名播放清單、編輯它們的描述、重新排列它們的曲目、把播放清單設為公開（0022）；播放清單資料夾（#87）
- 快取頁面或圖片、跨執行記住頁面歷史（0009 兩者都不做）
- 頁面上的封面（0004 的配置規則在它出現時適用）
- 排序與篩選清單（spotify-player 的 `s t`、頁面中的 `/`）、說明彈出視窗（`?`）、可設定的按鍵與頁面比例（0008）
- 跳到播放中曲目所在的清單（spotify-player 的 `g c`）、「currently playing context」頁面（`g space`）
- 音樂庫的一次性 `playback` 指令（例如 `playback load --favorites`）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-07"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-07）

- **型別**：ID 是單純的 `u64`（如 `Item::Album`）；`Track` 新增 `version: Option<String>` 供版本篩選使用。`ListPage` 帶有 `hidden`（頁面丟掉的列數；只有 credits 會非零），而 `LibraryResponse::Items` 為每種列型別包裝一個 `ListItems` enum。收藏請求接受 `(FavoriteKind, String)`：十進位的 ID 或播放清單 UUID。沒有 `id` 的 `artists[]` 或 `album` 物件是格式錯誤
- **篩選**：正規化後什麼都不剩的隱藏版本詞彙什麼都不符合（否則它會隱藏每一首曲目）
- **API**：`ArtistAlbums` 把兩份清單對應到一個 offset 空間：低於 `ceil(albums / limit) × limit` 的 offset 讀取純清單，其餘從 0 起讀取 `EPSANDSINGLES`；每個 `More` 先向純清單詢問它的總數（兩個請求）。Credits 快取每位藝人的 `dataApiPath`，並在 `404` 時重新讀取一次 `/pages/contributor`。`AddToPlaylist` 對每個 100 首曲目的區塊重試一次，所以中途失敗可能讓較早的區塊已經加入。播放清單標題的 `own` 是 `creator.id == user_id`（探測把 `creator` 遮掉了：要親手確認）。`IsFavorite` 以假設的形狀讀取 `GET /users/{user}/favorites/ids`；客戶端還沒有送出它（兩個收藏動作都顯示）
- **播放器**：音樂庫請求在 runtime 中依客戶端排隊；一個客戶端的下一個請求在它前一個回覆送出時開始
- **客戶端 model**：清單高度來自 `Action::Resize { list_height }`（預設 20），TUI 在版面的清單高度改變時送出；當 `cursor + list_height + 1 ≥ visible rows` 時載入下一頁。重新開啟最上面的頁面，只有在它失敗時才重新擷取。失敗文字是 `Could not load the library: …` / `Could not load the favorite tracks: …`，否則原樣使用播放器的訊息。整份清單載入期間只有 `Esc` 與 `q`/`C-c` 作用。*Add to playlist…* 重新擷取使用者的播放清單，並對照歷史中該播放清單任何已載入的副本檢查重複。沒有 ETag 的移除送出空的 ETag（由 `412` 的路徑處理）。角色篩選開啟時，`Enter` 播放可見的曲目。`C-f`/`C-b` 也移動佇列的游標；在佇列上按 `Z` 什麼都不做
- **繪製**：視窗標題太窄放不下 `‹Tab› <other>` 時，先丟掉另一個視窗的名稱。專輯列從 44 個內部欄起顯示 `year EP/Single`，從 30 欄起顯示年份，從 20 欄起顯示標題與藝人。*All tracks* 在專輯欄的位置顯示角色類別，而專輯欄只在大約 120 個終端機欄以上才存在。不論 crossterm 回報 `BackTab`，還是帶 Shift 的 `Tab`，Shift+Tab 都解碼為 `BackTab`。`TIDAL_PLAYER_PAGE_SIZE` 由每個 TUI 讀取，連上的與獨立模式的都一樣

<a id="bugs"></a>
## 錯誤

- **從含有影片的播放清單移除，會移除錯誤的項目（[#89](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/89)，在撰寫規格 0022 時發現，2026-11-05）。** 預期：*Remove from this playlist*、`d d` 與 `d s` 移除游標所在的列。實際：影片之後的列所帶的位置，會少影片之前影片的數量，所以項目為 曲目、影片、曲目 時，移除第二首曲目會送出 `DELETE /playlists/{uuid}/items/1`：影片被移除，曲目留下。根本原因：`playlist_tracks` 在客戶端看到頁面之前就丟掉影片，而 `Window::append` 以 `offset` + index 為列編號，只算剩下的；本規格在略過依位置的部分說過這件事（「the item's offset in the list」），卻從未談到被丟掉的東西。修正：API 在 `ListPage::positions` 中指出每個項目的位置（在其他每份清單上為空：`offset` + index），並由 `Window::append` 保存（AC21）。測試：`crates/api/tests/library.rs` :: `More` 表格的 *playlist tracks* 列與播放清單頁面（`[100, 102]`、`[0, 2]`）、`crates/core/src/ui/page.rs` :: `ac21_positions_count_dropped_videos`、`crates/core/src/ui/browse/tests.rs` :: `ac21_remove_after_a_video_sends_its_position`。紅燈：位置仍然是 `offset` + index。已親手檢查（2026-11-05，一份由 曲目、影片、曲目、曲目 組成的私人播放清單）：移除影片之後的那一列，拿走的是那首曲目並留下影片，所以 `DELETE /items/{index}` 有把影片算進去
- **含有同一首曲目兩次的播放清單只顯示一次（在 slice D 驗收時發現，2026-10-07）。** 預期：Tidal 列出的每個項目都有一個播放清單列，包括重複項目（寫入探測把同一首曲目加入同一份播放清單兩次，而 Tidal 兩者都保留了）；*Remove from this playlist* 以位置定址到正確的那一個。實際：附加頁面時會略過「已載入的列（相同的曲目 ID …）」，所以第二個複本永遠不會顯示。根本原因：「清單隨著捲動載入」把依 ID 略過當成每份清單防止頁面重疊的防護；對播放清單來說，ID 並不唯一。修正：播放清單的曲目改為依**位置**（該項目在清單中的 offset）略過，而不是依 ID；其他每份清單仍依 ID 略過。測試：`crates/core/src/ui/page.rs` :: `ac10_playlist_duplicates_kept`（一份播放清單的兩個頁面，同一首曲目在位置 0 與 1，以及一個重複的頁面：兩個複本各保留一次，位置為 0 與 1；收藏曲目的視窗仍然丟掉重複的 ID）。紅燈：播放清單視窗只保留一列
- **藝人頁面上的 `Tab` 跳到另一半（使用者回報，2026-10-08）。** 預期：左半的標題 `Top tracks ‹Tab› All tracks` 表示 `Tab` 會顯示 *All tracks*。實際：`Tab` 以固定順序循環四個視窗（Top tracks → Albums → Appears on → All tracks），所以從 *Top tracks* 會移到右邊的 *Albums*，而且沒有辦法在不經過另一半的情況下，在一半的兩個視窗之間切換。根本原因：「繪製」在每一半放了兩個視窗，卻讓 `Tab` 維持成一個扁平的焦點循環，而標題承諾了那個按鍵不會做的切換。修正：藝人頁面有兩個窗格、各有兩個分頁；`Tab`/`Shift-Tab` 在窗格之間循環，`[`/`]` 循環聚焦窗格的分頁（lazygit），標題列出窗格的分頁，作用中的反白並帶 `[ ]`。AC11 新增這些列。測試：`crates/core/src/ui/browse/tests.rs` :: `ac11_artist_panes_and_tabs`（從 *Top tracks* 按 Tab 會聚焦到右邊窗格作用中的分頁，再按回來仍然是 *Top tracks*；左邊窗格上的 `]` 顯示 *All tracks*（並要求它的第一頁）；`[`/`]` 會循環；在音樂庫上它們什麼都不做）以及 0006 藝人頁面的繪製快照（標題）。紅燈：從 *Top tracks* 按 `Tab` 落在 *Albums*，而 `]` 什麼都不做
