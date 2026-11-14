[English](../0028-collected-tracks.md) | 正體中文

<a id="0028--collected-tracks-favorite-tracks-as-a-pinned-playlist"></a>
# 0028 — Collected Tracks：以置頂播放清單呈現收藏的曲目

- **狀態**：implemented（2026-11-12；approved 2026-11-12）
- **負責人**：tech-lead（主要 session）
- **Issue**：[#103](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/103)
- **相依於**：0006（implemented：音樂庫頁面、收藏曲目頁面、整份清單載入、動作彈出視窗、收藏）、0012（implemented：`/` 與它的標題）、0019（implemented：移除詢問、`d d`）、0020（implemented：選取、`d s`、`s a`）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md)（頁面、音樂庫、動作彈出視窗、確認移除、選取列）、[`docs/config.md`](../../zh-TW/config.md)（指令表）、[`docs/playback.md`](../../zh-TW/playback.md)（`confirm_removals`）、`examples/keymap.toml` 與 `examples/app.toml` 的註解、`README.md`，以及它們的 zh-TW 版本（AC10）

<a id="context"></a>
## 背景

Tidal 的 *My Collection* 把 Tracks 放在 Playlists、Albums 與 Artists 旁邊。音樂庫頁面（`g l`，0006）顯示 Playlists、Albums 與 Artists；收藏的曲目是自成一頁，只能用 `g y` 到達，如同 spotify-player 的 Liked Songs。看著音樂庫的人看不到曲目，也不知道這個頁面存在。

Spotify 的應用程式用把 *Liked Songs* 置頂在播放清單最上方來回應這個問題。本規格做同樣的事：收藏曲目頁面變成 **Collected Tracks**，從音樂庫 Playlists 視窗置頂的第一列開啟（或如前以 `g y`）。而且既然 Tidal 把收藏稱為你的**收藏（collection）**，從不叫 *favorites* 或 *liked*，TUI 也這樣做：曲目的收藏動作與訊息說 *Collected Tracks*，專輯、藝人或播放清單的則說 *collection*。

本規格取代 0006 中的：收藏曲目頁面的名稱、標題、空白與失敗訊息（「Pages」、「Rows」）；音樂庫頁面的清單（它現在也會取得收藏曲目的總數）；以及在 0006/0008/0019/0020 中，TUI 為加入或移除收藏所顯示的每一段文字、音樂庫 Albums 與 Artists 的空白訊息，以及 `AddToLiked`、`DeleteFromLiked` 與 `LikedTrackPage` 的按鍵說明文字。

<a id="behaviour"></a>
## 行為

<a id="the-pinned-row"></a>
### 置頂列

音樂庫的 Playlists 視窗以一個不是播放清單的列開頭：**Collected Tracks** 與收藏曲目的數量，然後才是原本的播放清單。

```
┌Playlists (22)───────────────┐
│  Collected Tracks        362│
│  Running                  42│
│♥ Late night               17│
```

- 它畫得像你自己的播放清單的一列（沒有 `♥`：那標記的是你追蹤的播放清單），永遠在第一個，不論播放清單的順序如何
- 數量是 Tidal 的收藏曲目總數，隨音樂庫頁面一起取得。沒有收藏曲目時顯示 `0`；不送出總數的播放器（較舊的常駐程式）則讓數量留空
- 視窗標題的數量仍是 Tidal 的播放清單數（`Playlists (22)`）；分頁、`Loading more…` 與下一頁的 offset 都只計算播放清單
- 游標從它開始，如同任何第一列

| 按鍵／動作 | 在置頂列上 |
|---|---|
| `Enter` | 開啟 Collected Tracks 頁面（如同 `g y`） |
| `Z`、`C-z`、*Add to queue* | 把每一首收藏的曲目加到佇列尾端 |
| *Play next* | 把每一首收藏的曲目加到正在播放的曲目之後 |
| *Add to playlist…* | 把每一首收藏的曲目加到所選的播放清單，如同對專輯 |
| `g a`、`C-space`（動作彈出視窗） | *Open*、*Add to queue*、*Play next*、*Add to playlist…*：沒有收藏、重新命名、描述、公開或刪除的動作 |
| `/` | 以它的名稱 `Collected Tracks` 比對，如同播放清單的標題；算進符合數 |
| `s c`、`s <N> j`/`k`、`s m` | 無法被選取：它上面的 `s c` 什麼都不做，涵蓋它的計數或動作模式只會選取播放清單 |
| `d d`、`d s` | 對它什麼都不做（它上面的 `d d` 不送出任何東西，也不開啟任何東西） |
| `r`、*Go to radio* | 什麼都不做（清單沒有電台） |

*Add to queue*、*Play next*、`Z` 與 *Add to playlist…* 需要每一首曲目：它們會先取得剩下的頁面，如 0006 的「需要整份清單」（`Loading 300 of 1 234…`，`Esc` 取消，什麼都不送出；超過 40 000 首曲目時以同樣方式拒絕），然後依清單順序送出一個帶有每一首曲目的指令。

<a id="the-collected-tracks-page"></a>
### Collected Tracks 頁面

原本的收藏曲目頁面，改了名稱；它的列、按鍵、載入、動作與移除都不變。

- 標題列：`Collected Tracks · 362 tracks`
- 視窗名稱（視窗標題、按鍵說明標題、篩選標題）：`Collected Tracks`（`Collected Tracks (548 · /love · 12 matches)`）
- 空白：`No collected tracks yet`
- 失敗：`Could not load Collected Tracks: <reason>`
- `g y` 仍從任何地方開啟它；它的說明文字與按鍵提示寫作 `Collected Tracks`

<a id="collection-not-favorites"></a>
### Collection，而不是 favorites

Tidal 把你收藏的東西稱為你的收藏（collection）；TUI 不再在任何地方說 *favorite* 或 *favorites*。**曲目**進入 *Collected Tracks*；**專輯、藝人或播放清單**進入你的 *collection*。

| 位置 | 原本 | 現在：曲目 | 現在：專輯、藝人或播放清單 |
|---|---|---|---|
| 動作彈出視窗 | *Add to favorites*、*Remove from favorites* | *Add to Collected Tracks*、*Remove from Collected Tracks* | *Add to collection*、*Remove from collection* |
| 訊息列 | `Added to favorites`、`Removed from favorites` | `Added to Collected Tracks`、`Removed from Collected Tracks` | `Added to collection`、`Removed from collection` |
| 詢問（0019），一個 | `Remove <title> from favorites? (y/n)` | `Remove <title> from Collected Tracks? (y/n)` | `Remove <title> from your collection? (y/n)` |
| 詢問（0019），多個 | `Remove 3 tracks from favorites? (y/n)` | `Remove 3 tracks from Collected Tracks? (y/n)` | `Remove 3 albums from your collection? (y/n)` |
| 選取彈出視窗（0020） | *Add to favorites*、*Remove from favorites* | *Add to Collected Tracks*、*Remove from Collected Tracks*（每個選取的列都是曲目） | *Add to collection*、*Remove from collection*（任何其他選取） |
| 選取清單詢問（0020） | `Remove from favorites:` | `Remove from Collected Tracks:` | `Remove from your collection:` |
| 選取結果（0020） | `Added 3 tracks to favorites`、`Removed 3 albums from favorites` | `Added 3 tracks to Collected Tracks`、`Removed 3 tracks from Collected Tracks` | `Added 3 albums to collection`、`Removed 3 albums from collection` |
| 自己的與追蹤的播放清單（0020） | `Delete 2 playlists and remove 1 from favorites? (y/n)` | | `Delete 2 playlists and remove 1 from your collection? (y/n)` |
| 音樂庫的空白清單（0006） | `No favorite albums yet`、`No favorite artists yet` | | `No albums in your collection yet`、`No artists in your collection yet` |

一個選取中有多種類型（含多種類型的瀏覽區段）時，每種類型的子句用它自己的詞：`Added 2 tracks to Collected Tracks, 1 album to collection`。

按鍵說明文字（0008）：`AddToLiked` → `add to collection`、`DeleteFromLiked` → `remove from collection (asks y/n first)`、`LikedTrackPage` → `Collected Tracks`。指令保留它們來自 spotify-player 的名稱。

<a id="keeping-the-count-current"></a>
### 讓數量保持最新

曲目收藏的成功變更（加入或移除，一個或一個選取）會在音樂庫頁面位於歷史中時重新取得它，如同專輯或藝人現在已做的，所以置頂列的數量會跟著變；Collected Tracks 頁面則如先前一樣重新取得。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 取得音樂庫：`Page(Library)` 也會在三份清單之外，以 `limit=1` 請求 `users/{user}/favorites/tracks`，而 `PageData::Library` 以 `collected_tracks: Option<u32>` 攜帶它的 `totalNumberOfItems`（沒有時為 `Some(0)`）；該請求失敗會使頁面失敗，如同其他三個請求之一失敗。沒有這個欄位的 `PageData::Library`（較舊的播放器）反序列化為 `collected_tracks: None`
- **AC2** — 置頂列（表格測試）：套用 `PageData::Library` 會讓 Playlists 視窗有第一列 *Collected Tracks* 與數量（`Some(n)`、`Some(0)`、`None` → 沒有數量），然後才是播放清單；視窗的 `total` 是播放清單的總數；下一個 `More` 從 offset = 頁面大小請求 `Playlists`，而已載入的播放清單 < total 時顯示 `Loading more…`；重新載入讓它保持在第一個。其他視窗與頁面沒有這樣的列
- **AC3** — 置頂列上的 `Enter` 開啟 Collected Tracks 頁面（`Page(FavoriteTracks)`，如同 `g y` 那樣推入）；那個頁面在最上面時按 `g y` 什麼都不做
- **AC4** — 置頂列上的 `Z`/`C-z` 與彈出視窗的 *Add to queue*、*Play next* 和 *Add to playlist…* 會載入完整的收藏曲目（`More { FavoriteTracks, … }` 直到總數，`Loading N of M…` 訊息，`Esc` 取消且什麼都不送出，超過 40 000 就拒絕），然後送出一個指令：`AddToQueue { tracks: <the tracks>, at: End }`、`AddToQueue { tracks: <the tracks>, at: Next }`（曲目就是載入的樣子，所以播放器不去查詢其中任何一首），或 `AddToPlaylist`；沒有收藏曲目時什麼都不送出，訊息列說 `Collected Tracks is empty`，並帶有 0006 的重複檢查（針對這四者的表格測試）
- **AC5** — 置頂列上的動作彈出視窗恰好列出 *Open*、*Add to queue*、*Play next*、*Add to playlist…*；`r` 與 *Go to radio* 對它什麼都不做；它上面的 `d d` 不送出也不開啟任何東西
- **AC6** — 選取（0020）：置頂列上的 `s c` 什麼都不選取；從它開始的 `s 3 j` 在它涵蓋的三列中選取兩個播放清單；涵蓋它的動作模式只選取播放清單；`s a` 與 `d s` 從不對它動作（表格測試）
- **AC7** — 篩選（0012）：音樂庫 Playlists 中的 `/coll` 保留置頂列（以及任何標題含有 `coll` 的播放清單）並把它算進符合數；`/run` 把它隱藏；`Enter` 保留篩選，而 `Esc` 清除它，置頂列回到第一個
- **AC8** — 文字（表格測試）：Collected Tracks 頁面的標題列、視窗名稱、空白與失敗訊息；`g y`、`AddToLiked` 與 `DeleteFromLiked` 的按鍵說明文字與按鍵提示；對曲目，以及對專輯、藝人與播放清單：動作彈出視窗的標籤、訊息列文字、一個與三個項目的詢問；只有曲目的選取，以及曲目加專輯的選取的選取彈出視窗標籤、選取清單詢問、選取結果子句（一種類型與多種類型）、自己的與追蹤的播放清單詢問；音樂庫空白的 Albums 與 Artists 訊息，全部如「Collection，而不是 favorites」。TUI 畫出的任何文字都不含 `favorite`（針對這些表格產生的每一段文字的檢查）
- **AC9** — 讓數量保持最新：曲目的 `AddFavorite`/`RemoveFavorite`（一個，或一個選取的）成功時，會在音樂庫頁面位於歷史中的任何位置時重新取得它，並如先前一樣重新取得 Collected Tracks 頁面；歷史中沒有音樂庫頁面時，不再多取得任何東西
- **AC10** — 繪製（`insta`，80 × 24，人工檢視）：帶有置頂列的音樂庫（數量、沒有 `♥`、游標在它上面）、帶 `0` 與沒有數量的；Collected Tracks 頁面；置頂列上的動作彈出視窗；50 × 20 的窄音樂庫；從 0 × 0 到 120 × 40 都不 panic（0006 AC17 的表格）。文件：`docs/tui.md`、`docs/config.md`、`docs/playback.md`（`confirm_removals`）、`examples/keymap.toml` 與 `examples/app.toml` 的註解、`README.md` 以及它們的 zh-TW 版本都指出 Collected Tracks、置頂列與收藏，而且只在說明 Tidal 的 favorites 就是 collection 的地方才說 *favorite*；`CLAUDE.md` 的 Library 項目已更新

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- **沒有收藏的曲目**：置頂列顯示 `0`；`Enter` 開啟頁面並顯示 `No collected tracks yet`；`Z` 與彈出視窗的清單動作什麼都不送出並說 `Collected Tracks is empty`
- **總數請求失敗**（網路、429、工作階段過期）：音樂庫頁面以 0006 的訊息失敗，如同它的任何清單失敗時；`Backspace`，然後 `g l` 再試一次
- **較舊的常駐程式**（沒有 `collected_tracks`）：置頂列在，但沒有數量；其他一切正常
- **已中斷連線**：`Enter` 開啟頁面，頁面像任何頁面一樣以中斷連線的訊息失敗；清單動作會如 0006 的整份清單載入那樣說明
- **篩選到只剩置頂列**：`Enter` 開啟頁面；符合數說 `1 match`
- **很大的收藏**（超過 40 000）：`Z` 與清單動作以 0006 的訊息拒絕；頁面仍會開啟並捲動
- **很小的終端機**：置頂列像播放清單的列一樣縮短（0006 的 `playlist_row` 規則）

<a id="test-plan"></a>
## 測試計畫

每個測試以其條件命名。紅燈是對存根的失敗斷言（音樂庫頁面的 `collected_tracks` 永遠是 `None` 且從不顯示、沒有置頂列、舊的文字），沒有 `todo!()`，沒有編譯錯誤。

| AC | 測試（檔案 :: 名稱） | 驗證內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/api/tests/library.rs` :: `ac1_library_page_counts_collected_tracks`（錄製的測試資料：一個總數、`0`、一個失敗的請求）；`crates/core/src/protocol/tests.rs` :: `ac1_library_page_without_collected_tracks` | 額外的請求與它的 `limit=1`、攜帶的總數、失敗的頁面、被讀成 `None` 的缺少欄位 | 存根不送出請求並攜帶 `None` |
| AC2 | `crates/core/src/ui/browse/tests.rs` :: `ac2_collected_tracks_row`（表格：`Some(362)`、`Some(0)`、`None`、重新載入、分頁） | 第一列與數量、視窗 total、下一個 offset、`Loading more…`、沒有它的其他視窗 | 沒有置頂列：第一列是第一個播放清單 |
| AC3 | `crates/core/src/ui/browse/tests.rs` :: `ac3_collected_tracks_row_opens_page` | 請求與推入的頁面；在最上面時按 `g y` 什麼都不做 | 第一列上的 `Enter` 開啟一個播放清單 |
| AC4 | `crates/core/src/ui/browse/tests.rs` :: `ac4_collected_tracks_row_whole_list`（表格：`Z`、*Add to queue*、*Play next*、*Add to playlist…*；`Esc`；超過 40 000） | `More` 請求、訊息、送出的那一個指令、取消與拒絕 | 什麼都沒載入也沒送出 |
| AC5 | `crates/core/src/ui/browse/tests.rs` :: `ac5_collected_tracks_row_actions` | 彈出視窗的四個動作；`r`、*Go to radio* 與 `d d` 什麼都不做 | 彈出視窗持有第一個播放清單的動作 || AC6 | `crates/core/src/ui/select/tests.rs` :: `ac6_collected_tracks_row_not_selectable`（表格：`s c`、`s 3 j`、動作模式、`s a`、`d s`） | 選取從不持有它 | `s c` 選取第一個播放清單 |
| AC7 | `crates/core/src/ui/filter/tests.rs` :: `ac7_filter_collected_tracks_row` | 被 `/coll` 保留、被 `/run` 隱藏、被計入、`Esc` 之後回到第一個 | 那一列從不在那裡 |
| AC8 | `crates/core/src/ui/popup.rs` :: `ac8_collection_texts`（表格：類型 × 標籤、N = 1 與 3 的詢問、選取標籤、選取清單詢問、播放清單詢問）；`crates/core/src/ui/browse/tests.rs` :: `ac8_collected_tracks_texts`（表格：標題、視窗名稱、空白與失敗訊息、音樂庫空白的 Albums 與 Artists、每種類型的訊息列文字）；`crates/core/src/ui/select/tests.rs` :: `ac8_selection_collection_clauses`（一種類型、多種類型）；`crates/core/src/ui/help/tests.rs` :: `ac8_collection_help_texts`（`g y`、`AddToLiked`、`DeleteFromLiked`、`g` 之後的按鍵提示） | 每種類型的每一段文字；沒有任何一段含 `favorite` | 存根回傳舊的 *favorites* 文字 |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_track_favorite_refetches_library`（表格：加入、移除、選取；音樂庫在歷史中或不在） | 音樂庫頁面有沒有被重新取得 | 只有收藏曲目頁面被重新取得 |
| AC10 | `crates/app/src/tui/render/tests.rs` :: `ac10_library_collected_tracks`（快照：數量、`0`、沒有；窄的音樂庫）、`ac10_collected_tracks_page`、`ac10_collected_tracks_row_actions`、`ac17_no_panic_any_size`（既有的，新的列）；`crates/app/tests/docs.rs` :: `ac10_collected_tracks_documented`；zh-TW 版本、README、`examples/keymap.toml` 與 `CLAUDE.md` 於驗收時檢視 | 畫出的列與頁面；文件指出 *Collected Tracks* 與置頂列 | 沒有畫出列；文件缺少這些詞 |

預期文字改變的既有測試（*Favorite tracks*、曲目的 *favorites* 文字、音樂庫快照）在同一次變更中依本規格更新，絕不削弱。

**實機檢查**（真實 Tidal，tmux 150 × 42，如 `CLAUDE.md`）：音樂庫第一個顯示 *Collected Tracks*，數量與 Tidal 應用程式相同；`Enter` 開啟標題為 `Collected Tracks · N tracks` 的頁面；`g y` 開啟同一個頁面；`/coll` 保留該列；在別處對一首曲目執行 *Add to Collected Tracks*，然後 `g l`：數量多一（之後復原）；該列上的 `Z` 把每一首曲目排入佇列。憑耳朵：沒有新的東西要檢查。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::library`：`PageData::Library` 新增帶有 `#[serde(default)]` 的 `collected_tracks: Option<u32>`（AC1）
- `tidal-player-api::library`：`PageRequest::Library` 分支把 `favorite_tracks(0, 1)` 的總數與三份清單一起 join（AC1）
- `tidal-player-core::ui::page`：音樂庫 Playlists 視窗中的置頂列：它的 `Row`、在 `rows`、`positions`、`total` 與 `next_offset` 仍是播放清單的之下把它放在第一個的索引對應；`WindowKind::FavoriteTracks` 與它的頁面改名後的文字（AC2、AC8）
- `tidal-player-core::ui::browse`：來自置頂列的 `Enter`、`Z`、動作彈出視窗與整份清單載入（一個脫離的 `FavoriteTracks` 視窗，以及一個與 `Play` 和 `AddToPlaylist` 並列的佇列用途）；曲目收藏變更之後重新取得音樂庫；訊息（AC3–AC5、AC8、AC9）
- `tidal-player-core::ui::popup`、`ui::select`、`ui::filter`、`ui::help`：該列的文字、選取與篩選規則（AC6–AC8）
- `tidal-player`（`tui/render`）：繪製該列（AC10）
- 內部名稱（`PageKind::FavoriteTracks`、`ListRef::FavoriteTracks`、`LikedTrackPage`、`FavoriteKind`）維持不變：只有使用者讀到的東西改變

<a id="facts-vs-assumptions"></a>
## 事實與假設

事實（來自程式碼，2026-11-12）：

- 音樂庫頁面以 `tokio::try_join!` 取得 `playlists`、`favorite_albums` 與 `favorite_artists`（`crates/api/src/library.rs`），所以一個失敗就使頁面失敗
- `favorite_tracks(offset, limit)` 已經以 `ListPage::total` 回傳 Tidal 的 `totalNumberOfItems`；0006 的探測在維護者的帳號上找到 362，與 Tidal 應用程式相符
- 0006 的整份清單載入可以在沒有頁面顯示的清單上執行（`WholeListSource::Detached`，用於專輯列的 *Add to playlist…*），用途有 `Play` 與 `AddToPlaylist`
- 曲目的收藏變更已經會重新取得收藏曲目頁面；專輯或藝人的則重新取得音樂庫（`browse.rs`，`AddFavorite`/`RemoveFavorite` 的回應處理）

假設，於驗收時檢查：

- 在 `users/{user}/favorites/tracks` 上 `limit=1` 會回傳完整的 `totalNumberOfItems`（來自真實 API 的錄製測試資料，在實作 AC1 時製作）

<a id="decisions-answered-by-the-user-2026-11-12"></a>
## 決策（由使用者回答，2026-11-12）

1. **位置**：*已回答*：收藏的曲目變成置頂的播放清單 *Collected Tracks*，在音樂庫 Playlists 視窗的最上面，而不是第四個音樂庫視窗（那會改變版面與 `[layout] library`）
2. **`g y`**：*已回答*：仍從任何地方開啟該頁面；該列與 `g y` 通往同一個頁面
3. **規格**：*已回答*：新的規格，就是這一份，指出它取代 0006 中的什麼，而不是改寫 0006
4. **篩選**：*已回答*：該列像播放清單一樣以它的名稱比對，而不是永遠顯示
5. **用詞**：*已回答*：凡是指稱曲目的收藏之處都用 *Collected Tracks*，不只是頁面；而且既然 Tidal 說 *collection* 而不是 *favorites* 或 *liked*，專輯、藝人與播放清單說 *collection*，`AddToLiked`/`DeleteFromLiked` 的按鍵說明文字也一樣。落選的另一種：只改說明文字，而在彈出視窗與訊息中保留 *favorites*
6. **沒有收藏的曲目**：*已回答*：該列保留並顯示 `0`，如同 Spotify 的 Liked Songs，而不是隱藏
7. **數量的來源**：Tidal 的總數，來自與音樂庫的請求 join 在一起的 `limit=1` 請求，一起失敗，而不是另一個在失敗時讓該列沒有數量的獨立請求（tech-lead 的選擇，2026-11-12）
8. **該列的外觀**：畫得像你自己的播放清單（沒有 `♥`），只靠位在第一個來區分，而不是新的佈景主題元件（tech-lead 的選擇，2026-11-12）

<a id="out-of-scope"></a>
## 不在範圍內

- 音樂庫或它的清單的排序（#52）；排序出現時，置頂列會維持在第一個
- Tidal 的 *My Collection* 中的影片與個人檔案
- 藝人頁面上你收藏的曲目（#63）
- 在 `keymap.toml` 中重新命名指令（`LikedTrackPage`、`AddToLiked`、`DeleteFromLiked`）：它們是 spotify-player 的名稱

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-11-12"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-11-12）

- **置頂列**：一個替身的 `PlaylistSummary`（uuid `collected-tracks`，沒有 Tidal 播放清單有這個 uuid；`own: false`，所以 *Add to playlist…* 清單永遠不會提供它；`tracks` 持有數量），位於音樂庫 Playlists 的 `rows` 與 `positions` 的索引 0，由 `Window::pin_collected` 在音樂庫頁面每次填入時插入；`Rows::get` 把它讀成 `Row::Collected(Option<u32>)`。`total`、`next_offset` 與 `complete()` 仍是播放清單的，所以游標、篩選與選取不需要索引對應
- **來自該列的整份清單載入**：與 `Play` 和 `AddToPlaylist` 並列的 `Purpose::Queue { at }`、`MenuAction::QueueCollected(InsertAt)` 與 `TrackSource::Collected`，全部透過一個 `load_detached` 輔助函式，專輯列的 *Add to playlist…* 現在也使用它。第一頁給出總數時，就立即拒絕超過 40 000 的情況；已中斷連線時什麼都不做，如同專輯的路徑。`AddToQueue` 與 `PlayNext` 的 `[[actions]]` 綁定透過 `QueueCollected` 作用於該列
- **選取**：每個選取的列都是曲目（`tracks_only`）時為 *Add to/Remove from Collected Tracks*，否則為 *collection*。`AddFavorites` 現在攜帶每一列自己的類型（它原本以第一列的類型送出每一列，對含多種類型的瀏覽區段是錯的）。清單詢問的標題依類型而定（`Remove from Collected Tracks:`、`Remove from your collection:`）；自己的與追蹤的播放清單放在一起時，以 `Deleted 2 playlists and removed 1 from collection` 結尾
- **重新取得的順序**：曲目的收藏變更先重新取得 Collected Tracks 頁面，再取得音樂庫頁面
- **空的音樂庫**：Playlists 視窗永遠持有置頂列，所以它不再說 `No playlists yet`（0006 的空清單訊息保留給沒有該列的音樂庫，那現在不可能發生）
- **API 標籤**：會到達訊息列的 `Malformed` 錯誤標籤（`malformed … response from Tidal`）寫作 `collected tracks`、`collection albums`、`collection artists` 與 `collection`
- **AC1 的協定測試**不可能是紅燈：serde 不論存根怎麼做都把缺少的 `Option` 讀成 `None`；它從現在起守護該欄位的 `#[serde(default)]`
- **`crates/app/tests/daemon.rs`**（`ac15_daemon_reads_app_toml`）：在檢查音樂庫請求的頁面大小時，省略 `limit=1` 的請求

<a id="bugs"></a>
## 錯誤

<a id="z-on-the-pinned-row-queues-nothing-when-one-collected-track-is-gone-found-at-the-hands-on-check-2026-11-12"></a>
### 當有一首收藏的曲目已不存在時，置頂列上的 `Z` 什麼都沒排入佇列（於實機檢查時發現，2026-11-12）

- **預期**：*Collected Tracks* 上的 `Z`（以及 *Add to queue*、*Play next*）把每一首收藏的曲目加到佇列，如 AC4 所說
- **實際**：有 373 首收藏的曲目、其中一首已不在 Tidal 上時，什麼都沒加入，訊息列說 `Track 256746446 was not found`
- **根本原因**：AC4 讓客戶端送出 `Open { items: [Item::Track(id), …] }`，播放器會一次一個 ID 地解析它，並在第一首不見的曲目就整個失敗。曲目其實已經帶著佇列需要的一切載入了，所以 `AddToQueue { tracks, at }`（0004）原樣攜帶它們，如同 0006 的 `LoadQueue` 用於播放一份清單。是規格錯了，不是程式碼
- **新的條件**：AC4 現在指名 `AddToQueue { tracks, at }`；它的測試斷言那個指令，並且清單中有一首 Tidal 已經沒有的曲目的列，仍然送出每一首曲目。不可串流的曲目會被排入佇列，並由播放器跳過，如同任何清單（0004 AC7）
