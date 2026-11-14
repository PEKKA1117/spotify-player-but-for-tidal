[English](../0019-confirm-removals.md) | 正體中文

<a id="0019--confirming-removals"></a>
# 0019 — 確認移除

- **狀態**：implemented（2026-10-25；同日修訂：`d d` 可從你的任何清單移除該列，AC10；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#61](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/61)
- **相依於**：0004（implemented：佇列、`RemoveFromQueue`）、0006（implemented：動作彈出視窗、收藏與播放清單的寫入、`y/n` 詢問）、0008（implemented：按鍵對應、`[[actions]]`、`app.toml`）、0012（implemented：佇列的篩選）
- **後續**：多選（[#71](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/71)），它會把本規格的詢問重複用於多個列
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md)（「Favorites and playlists」、「The queue」、按鍵表）與 [`docs/config.md`](../../zh-TW/config.md)（`confirm_removals`、`RemoveSelectedItem` 與 `RemoveFromQueue`、`[[actions]]` 表格），以及它們的 zh-TW 版本（AC8）

<a id="context"></a>
## 背景

有三個動作只按一個鍵就移除東西，除了再用手動方式做一次之外沒有任何挽回的辦法：

- ***Remove from favorites***（你追蹤的曲目、專輯、藝人或播放清單）：再加入時它會放在 *Date added* 的最上面，而不是原本的位置
- ***Remove from this playlist***：再加入時它會放在最後面，而不是原本的位置
- ***Remove from queue***（佇列頁上的 `d`，或彈出視窗的項目）：該項目就消失了，正在播放的會跳到下一首

不小心按到 `d`，或在動作彈出視窗中選錯列，就會立刻執行。目前只有 *Delete playlist*（以及把重複的曲目加入播放清單）會詢問，用的是 0006 的 `y/n` 詢問（`Popup::Confirm`）。spotify-player 在這類動作之前也會詢問（issue #61）。

本規格讓這三個移除動作以同樣的詢問來確認，讓使用者可以關閉它，並給你的每個清單同一個移除按鍵 `d d`（佇列原本的是 `d`）：它把游標所在的列從它所在的清單（佇列、你的播放清單、你的收藏）移除，所以單獨的 `d` 可以留給以後移除選取的列（#71）。tidalt 沒有確認；沒有任何東西沿用。

<a id="behaviour"></a>
## 行為

<a id="what-asks"></a>
### 什麼會詢問

| 動作 | 來自 | 詢問 |
|---|---|---|
| *Remove from favorites* | 動作彈出視窗；`DeleteFromLiked` 的 `[[actions]]` 綁定；你的收藏上的 `RemoveSelectedItem`（`d d`）（見下） | `Remove <title> from favorites? (y/n)` |
| *Remove from this playlist* | 自己的播放清單頁上的動作彈出視窗；`DeleteFromPlaylist`；那裡的 `d d` | `Remove <title> from <playlist>? (y/n)` |
| *Remove from queue* | 佇列頁上的 `d d` 與 `RemoveFromQueue` 指令；動作彈出視窗；`RemoveFromQueue` 動作 | `Remove <title> from the queue? (y/n)` |
| *Delete playlist*（照舊詢問） | 動作彈出視窗；`DeletePlaylist`；音樂庫中自己的播放清單上的 `d d` | `Delete <playlist>? (y/n)` |

`<title>` 是該列在清單中顯示的標題（曲目的標題、專輯的標題、藝人的名稱、播放清單的標題）；`<playlist>` 是播放清單頁的標題。

詢問就是 0006 的彈出視窗：置中畫在頁面之上，依頁面的寬度換行。`y` 完全照現在的方式執行該動作（相同的請求或指令，之後相同的訊息：`Removed from favorites`、`Removed from <playlist>`……）；`n` 或 `Esc`（`ClosePopup`）關閉它，什麼也不送出。開啟期間沒有其他按鍵有作用（0006）。被詢問的就是被問到的那一列，即使清單在那段時間移動了（佇列前進了、載入了一頁）：詢問持有的是要移除什麼（一個項目 ID、一個收藏的 ID、一個播放清單的位置與 ETag），不是游標。

把已在播放清單中的曲目再加入，仍保有它的詢問（`Already in <playlist>: add again? (y/n)`）；那不是移除。

<a id="several-items"></a>
### 多個項目

詢問以項目清單的形式列出它要移除的東西，每個項目附上標題。本規格只會詢問一個項目。多選（#71）會詢問多個，這裡先固定它的文字，讓兩者讀起來一致：

| 項目數 | 詢問 |
|---|---|
| 1 | `Remove <title> from favorites? (y/n)`（如上） |
| N > 1，收藏 | `Remove N <kind>s from favorites? (y/n)`：`tracks`、`albums`、`artists`、`playlists` |
| N > 1，播放清單 | `Remove N tracks from <playlist>? (y/n)` |
| N > 1，佇列 | `Remove N tracks from the queue? (y/n)` |

`y` 對多個項目送出什麼（每個一個請求，或對多個播放清單位置一個請求）由 #71 來規定。播放清單的位置在每次移除之後都會位移，所以不是「每個項目一個 `RemoveFromPlaylist`」。

<a id="d-d-remove-the-row-from-its-list"></a>
### `d d`：從清單中移除該列

一個新的指令 `RemoveSelectedItem`*（我們自己的；說明文字 `remove from the list`），透過同樣的詢問，把游標所在的列從它所在的清單移除。它的預設綁定是 `d d`。它做什麼取決於視窗（設定了篩選時，作用在篩選後的列上，0012）：

| 視窗 | `d d` |
|---|---|
| 佇列 | *Remove from queue* |
| 自己的播放清單的曲目 | *Remove from this playlist* |
| 收藏的曲目（`g y`） | *Remove from favorites*（該曲目） |
| 音樂庫的 Albums、Artists | *Remove from favorites*（該專輯、該藝人） |
| 音樂庫的 Playlists：你追蹤的 | *Remove from favorites*（該播放清單） |
| 音樂庫的 Playlists：你自己的 | *Delete playlist*（`Delete <playlist>? (y/n)`，不論 `confirm_removals` 怎麼設定都會詢問） |
| 任何其他清單：專輯或已追蹤播放清單的曲目、藝人頁、搜尋結果、mix 清單、一個 mix、一個電台 | 無作用（該清單不是你的） |

在空的視窗中，游標下沒有列時，什麼也不會發生。按鍵說明只在 `d d` 有作用的地方列出它（0008 的規則），所以 0013 對 `d` 的提示也只在那裡顯示。

`RemoveFromQueue` 仍是一個指令（0004/0006），只作用於佇列，現在**沒有預設按鍵**：`d d` 在佇列上做同樣的事。除了 `d d` 之外，沒有預設綁定以 `d` 開頭，所以 0008 的前綴規則仍然成立。單獨的 `d` 預設什麼也不做；#71 會讓它移除選取的列。使用者的 `keymap.toml` 可以像對任何指令一樣，把任一個指令綁定到 `d`。

<a id="keymaps-written-for-d"></a>
### 為 `d` 寫的按鍵對應

從較早的 `examples/keymap.toml` 複製來的 `keymap.toml` 把 `RemoveFromQueue` 綁定到 `d`。依照 0008 原本的前綴規則，`d` 與新的預設 `d d` 並存是錯誤，播放器不會啟動。0008 的決策 4 因此修訂：當**檔案**綁定的序列與**預設**綁定衝突（其中一個是另一個的開頭）時，檔案勝出，衝突的預設被丟棄。TUI 的訊息列在啟動時說一次，與 0008 的按鍵對應通知一起（以 ` · ` 連接）：

- 一個：`keymap.toml: "d" replaces the default "d d" (RemoveSelectedItem)`
- 多個：`keymap.toml: "g" replaces the defaults "g g" (SelectFirstOrScrollToTop), "g l" (LibraryPage), …`（依預設值的順序）

檔案綁定的兩個序列之間的衝突仍是錯誤，如 0008 所說。檔案自己的項目會先套用，所以把某個預設序列（`d d`）重新綁定到別的東西、同時又綁定 `d` 的檔案，是以它自己的綁定來判斷。

<a id="turning-it-off"></a>
### 關閉它

`app.toml` 新增下列項目，只由 TUI 讀取（常駐程式接受並忽略它，如同 `key_hints`）：

| 鍵 | 可接受的值 | 預設值 | 可被覆寫於 |
|---|---|---|---|
| `confirm_removals` | `true`、`false` | `true` | `TIDAL_PLAYER_CONFIRM_REMOVALS`（`on`、`off`） |

`confirm_removals = false` 時，這三個移除動作會像本規格之前一樣立刻執行。*Delete playlist* 與重複曲目的詢問仍會詢問：刪除播放清單完全無法復原。`confirm_removals` 不改變按鍵對應：`RemoveSelectedItem` 不論如何都在 `d d` 上。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 從動作彈出視窗或 `DeleteFromLiked` 綁定執行的 *Remove from favorites*，對曲目、專輯、藝人與已追蹤的播放清單，會開啟詢問 `Remove <title> from favorites? (y/n)`，且不送出任何東西；`y` 接著送出 `RemoveFavorite(kind, id)`，並像以前一樣記住這次寫入（成功時相同的訊息）；`n` 與 `Esc` 關閉它且不送出任何東西（表格）
- **AC2** — 從彈出視窗或 `DeleteFromPlaylist` 綁定執行的 *Remove from this playlist*，會開啟 `Remove <title> from <playlist>? (y/n)`；`y` 送出 `RemoveFromPlaylist`，帶有該列的位置與頁面**在詢問當時**的 ETag（期間的頁面重新載入不會改變它們）；`n`/`Esc` 不送出任何東西
- **AC3** — 佇列頁上的 `RemoveSelectedItem` 與 `RemoveFromQueue`、彈出視窗的 *Remove from queue*，以及 `RemoveFromQueue` 動作綁定，會開啟 `Remove <title> from the queue? (y/n)`；`y` 送出 `RemoveFromQueue`，帶有被詢問那一列的項目 ID，即使之後游標移動或佇列改變了（另一個項目播放完畢、一份快照抵達）；`n`/`Esc` 不送出任何東西。佇列為空時什麼也不開啟。佇列被篩選時（0012），詢問針對的是游標所在的篩選後的列
- **AC4** — `confirm_removals` 關閉時，AC1–AC3 與 AC10 的移除動作立刻送出，沒有詢問（現在的行為）；*Delete playlist* 與重複曲目的詢問仍會詢問（表格，涵蓋設定 × 五種詢問）
- **AC5** — 一個與多個項目時詢問的文字：`question(&Removal) -> String` 對每種類別的收藏、一個播放清單與佇列，給出「多個項目」下的表格，N = 1 與 N = 3（表格；N = 3 的列之後只能透過 #71 到達）
- **AC6** — 預設按鍵對應把 `RemoveSelectedItem` 綁定到 `d d`，`d` 沒有綁定任何東西，`RemoveFromQueue` 也沒有綁定任何東西（在 `keymap.toml` 中仍是有效的指令名稱）；佇列頁上單獨的 `d` 不送出任何東西，並讓 `d` 維持待處理；`d d` 開啟 AC3 的詢問；`examples/keymap.toml` 與預設值相符（`RemoveSelectedItem` 在 `d d`，沒有 `RemoveFromQueue` 項目，因為它只列出已綁定的預設值），且它的標頭說明 AC9 的前綴規則（檔案的兩個序列不可互為開頭；開頭是某個預設的序列會取代它）；按鍵說明在 `d d` 有作用的地方列出它，其他地方不列（AC10 的表格），按鍵提示也隨之；`docs/config.md` 的指令表有 `RemoveSelectedItem`*
- **AC7** — `app.toml` 的 `confirm_removals` 與 `TIDAL_PLAYER_CONFIRM_REMOVALS` 以 0008 的優先順序與錯誤設定 TUI 的設定（非布林值的值或變數：指出鍵名的錯誤；結束碼 2）；連上的 TUI 與獨立模式的 TUI 都把它複製進模型；`examples/app.toml` 列出它與它的預設值
- **AC8** — 繪製：頁面上方的詢問（`insta`，80 × 24），針對從收藏移除；任何大小都不會 panic（0006 AC17 的表格，新增列）。`docs/tui.md`、`docs/config.md` 與它們的 zh-TW 版本描述這些詢問、`confirm_removals` 與 `d d`；`README.md` 與 `CLAUDE.md` 的「Features」已更新
- **AC9** — 建立按鍵對應（0008 AC2 的表格，新增列）：把 `RemoveFromQueue` 綁定到 `d` 的檔案能建立，`d` 從佇列移除（附 AC3 的詢問），`d d` 未綁定，並附上通知 `keymap.toml: "d" replaces the default "d d" (RemoveSelectedItem)`；綁定 `g` 的檔案會丟棄每個預設的 `g …`，附一則點名它們的通知；同時綁定 `s` 與 `s l a` 的檔案仍是錯誤；這則通知與不支援名稱的通知以 ` · ` 連接。`docs/config.md` 與它的 zh-TW 版本描述這條規則。以這樣的檔案啟動執行檔（`-c` 是存放它的暫存目錄）會正常執行，不是結束碼 2
- **AC10** — 各視窗的 `RemoveSelectedItem`（`d d`），如「`d d`：從清單中移除該列」下的表格（涵蓋各視窗的表格）：佇列 → AC3 的詢問；自己的播放清單的曲目 → AC2 的詢問，帶有該列的位置與頁面的 ETag；收藏的曲目、音樂庫的 Albums 與 Artists、音樂庫 Playlists 中已追蹤的播放清單 → AC1 的詢問，帶有該類別與 ID；音樂庫 Playlists 中自己的播放清單 → `Delete <playlist>? (y/n)`，`y` 送出 `DeletePlaylist`，`confirm_removals` 關閉時也是；已追蹤播放清單的曲目、專輯的曲目、藝人頁的每個視窗、每個搜尋視窗、mix 清單、一個 mix、曲目電台與藝人電台 → 不送出任何東西、沒有彈出視窗、狀態不變；空的視窗 → 無作用；有篩選時（0012）→ 游標所在的篩選後的列

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- **詢問期間項目改變了**：詢問持有的是 ID 與位置，不是游標。已經不在的佇列項目（它播放到結尾而自動播放把它丟掉了、另一個客戶端移除了它）：播放器對未知項目的 `RemoveFromQueue` 會忽略，如同現在。在別處被改變的播放清單：ETag 不再相符，0006 的訊息會說明（`The playlist changed: nothing was changed, try again`）
- **中斷連線**（連上的 TUI，常駐程式已消失）：詢問仍會開啟；`y` 接著做的是該動作在中斷連線時現在會做的事（音樂庫的寫入不會送出）
- **很長的標題**：詢問依頁面的寬度換行（0006 的彈出視窗）；極小的頁面區域會畫出放得下的部分，絕不 panic
- **整份清單載入中**：只有 `Esc`/`q` 有作用（0006），所以不會開啟詢問
- **`[[actions]]` 綁定**：與彈出視窗完全一樣地詢問（使用者的選擇：綁定了按鍵不是略過詢問的理由）
- **使用者的按鍵對應綁定 `d`**（綁到 `RemoveFromQueue` 或任何其他東西）：預設的 `d d` 會被丟棄並附上通知（AC9）；播放器會啟動。舊的 `d` → `RemoveFromQueue` 時，`d` 仍從佇列移除（現在會詢問），而音樂庫在使用者綁定 `RemoveSelectedItem` 之前沒有移除按鍵
- **收藏狀態未知的列**（0006：彈出視窗無法判斷某個項目是否是收藏）：只有列出收藏的視窗才會作用，所以 `d d` 從收藏移除的每一列都是收藏
- **無法串流的列**（淡色，0006）：仍然可以移除
- **音樂庫的 Playlists 在頁面載入哪些播放清單是你的之前**：擁有者來自已載入的列，如同彈出視窗的 *Delete playlist*

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名。紅燈是針對存根、其本體為存根的斷言失敗（移除動作像現在一樣立刻送出、`question` 回傳空字串、設定忽略該鍵、預設按鍵對應仍保留 `d`），沒有 `todo!()`、沒有編譯錯誤。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/browse/tests.rs` :: `ac1_confirm_remove_favorite` (table: kind × popup/binding × `y`/`n`/`Esc`) | 詢問的文字、`y` 之前沒有送出任何東西、`y` 之後的寫入、`n`/`Esc` 之後沒有任何東西 | 寫入立刻送出，沒有彈出視窗 |
| AC2 | `crates/core/src/ui/browse/tests.rs` :: `ac2_confirm_remove_from_playlist` | 詢問、在頁面重新載入之間保持的位置與 ETag | 寫入立刻送出 |
| AC3 | `crates/core/src/ui/browse/tests.rs` :: `ac3_confirm_remove_from_queue`; `crates/core/src/ui/filter/tests.rs` :: `ac3_confirm_remove_filtered_queue` | 詢問、游標與快照改變之後被詢問的項目、空的佇列、被篩選的佇列 | 指令立刻送出 |
| AC4 | `crates/core/src/ui/browse/tests.rs` :: `ac4_confirm_removals_off` (table) | 關閉時立刻送出；刪除與重複仍會詢問 | 存根的 `State::confirm_removals` 沒有任何地方讀取，移除動作從不詢問：「開啟」的列與刪除的列會通過，測試在設定開啟時預期的缺少的詢問上失敗 |
| AC5 | `crates/core/src/ui/popup.rs` (a new `#[cfg(test)] mod tests`) :: `ac5_removal_question` (table) | N = 1 與 N = 3 的文字 | 存根回傳 `""` |
| AC6 | `crates/core/src/ui/keymap/tests.rs` :: `ac6_remove_selected_item_is_d_d` (renamed from `ac6_remove_from_queue_is_d_d`); `crates/core/src/ui/help/tests.rs` :: `ac6_d_d_listed_where_it_acts`; `crates/core/src/ui/browse/tests.rs` :: `ac6_d_alone_does_nothing`; `crates/app/tests/examples.rs` :: `ac18_example_keymap_is_the_defaults` (existing, fails until the example says `d d`) | 預設綁定、`d` 待處理、`d d` 詢問、範例 | 預設值保留 `d` |
| AC7 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/src/main.rs` :: `ac7_tui_state_confirm_removals`; `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` (existing) | 各來源的值、錯誤、模型的複製、範例 | 存根設定忽略該鍵 |
| AC8 | `crates/app/src/tui/render.rs` :: `ac8_confirm_removal` (snapshot), `ac17_no_panic_any_size` (new row); `crates/app/tests/docs.rs` :: `ac8_confirm_removals_documented`; zh-TW copies, README and `CLAUDE.md` reviewed at acceptance | 畫出的詢問；文件提到 `confirm_removals` 與 `d d` | 沒有畫出詢問；文件缺少這些詞 |
| AC9 | `crates/core/src/ui/keymap/tests.rs` :: `ac9_file_beats_conflicting_defaults` (table), `ac2_build_keymap` (its "`g` bound" row now builds; the `s`/`s l a` row still errors); `crates/app/tests/config.rs` :: `ac9_old_keymap_starts` (`-c` a temp dir with `d` → `RemoveFromQueue`) ; `crates/app/tests/docs.rs` :: `ac8_confirm_removals_documented` (new terms) | 被丟棄的預設值、通知、只在檔案內的衝突、執行檔能啟動 | 存根的 `build` 仍拒絕該衝突（`PrefixConflict`） |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_remove_selected_item` (table: window × own/followed → question or nothing, then `y`); `crates/core/src/ui/filter/tests.rs` :: `ac10_remove_selected_item_filtered` | 每個視窗：詢問與 `y` 送出什麼、其他視窗什麼也沒有、篩選後的列 | 存根的 `RemoveSelectedItem` 只作用於佇列（如同 `RemoveFromQueue`）：佇列的列通過，其他每個視窗都因缺少詢問而失敗 |

既有的、在佇列上按 `d` 並預期立刻得到 `RemoveFromQueue` 的測試（`crates/core/src/ui/browse/tests.rs`、`filter/tests.rs`、`search/tests.rs`、`help/tests.rs`），在同一個變更中依本規格更新（`d d`，然後 `y`），絕不削弱。

驗收時手動檢查：在真實終端機中，佇列上的 `d d` 會詢問、`y` 移除；`d` 然後 `Esc` 什麼也不做；彈出視窗中的 *Remove from favorites* 會詢問；`confirm_removals = false` 時立刻移除。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::ui`：`Confirmed` 新增 `Remove(Removal)`，`Removal { from: RemoveFrom, items: Vec<Removed> }` 搭配 `RemoveFrom::{Favorites(FavoriteKind), Playlist { uuid, title, etag }, Queue}` 與 `Removed { title, target }`（`target`：收藏的 ID、播放清單的位置或 `EntryId`），以及純函式 `question(&Removal) -> String`；`State` 新增 `confirm_removals: bool`（預設 `true`）；`UiCommand` 新增 `RemoveSelectedItem`（名稱、說明文字、按鍵說明中它作用的地方）；`keymap::defaults` 把它綁定到 `d d`，並讓 `RemoveFromQueue` 不綁定。移除動作透過一個函式開啟詢問，彈出視窗、綁定、`RemoveFromQueue` 與 `RemoveSelectedItem` 共用它（`RemoveSelectedItem` 依視窗，從彈出視窗會為該列列出的動作中挑出該列的移除動作），所以 #71 新增的是項目清單，不是新的路徑。沒有 I/O，沒有新的相依套件
- `tidal-player`：設定（`confirm_removals`、變數）、`tui_state` 複製它、文件測試。沒有協定變更：`y` 送出的是現在的 `Command`/`LibraryRequest`
- `xtask layering`：沒有變更

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-25，來自程式碼）：詢問是 `Popup::Confirm { question, on_yes: Confirmed }`（`crates/core/src/ui/popup.rs`），由 `popup_fixed_key` 與 `confirmed`（`crates/core/src/ui/browse.rs`）回答；`Esc` 關閉它（`ClosePopup`，`help.rs`）；移除動作經由 `browse::run` 執行（`MenuAction::RemoveFavorite`、`RemoveFromPlaylist`、`RemoveFromQueue`），`[[actions]]` 綁定經由 `run_action` → `run`，佇列的按鍵經由 `dispatch::queue_command`；預設的 `RemoveFromQueue` 綁定是 `d`，沒有其他預設以 `d` 開頭（`keymap::defaults`）；0008 拒絕較長序列的已綁定前綴（`check_prefixes`）；`logout` 與清空佇列無法從 TUI 到達（前者是 CLI 子指令；`Command::ClearQueue` 沒有按鍵）。

假設：沒有關於外部系統的假設（沒有新的 API 呼叫）。

<a id="decisions-answered-by-the-user-2026-10-25"></a>
## 決策（使用者已回答，2026-10-25）

1. **什麼會詢問**：*已回答*：從收藏移除、從播放清單移除與從佇列移除；刪除播放清單照舊。登出（一個 CLI 指令）與清空佇列（沒有按鍵）不在內
2. **關閉的開關**：*已回答*：要，`confirm_removals`，預設開啟；不論它怎麼設定，*Delete playlist* 都會詢問
3. **`[[actions]]` 綁定**：*已回答*：與彈出視窗一樣地詢問
4. **按鍵**：*已回答*：`d d` 移除游標所在的列；`d` 保留給移除選取的列（#71）
5. **綁定 `d` 的舊按鍵對應**（驗收時發現：播放器拒絕以從先前範例複製的 `keymap.toml` 啟動）：*已回答*：檔案的綁定勝過衝突的預設值，並附通知；0008 決策 4 隨之修訂
6. **你的每個清單上的 `d d`**（第一次實作之後詢問，2026-10-25）：*已回答*：要，透過同樣的詢問；在音樂庫中自己的播放清單上，它刪除該播放清單（*Delete playlist* 的詢問，一律詢問）；在不是你的清單（專輯、藝人、搜尋、mix、電台）上無作用。已併入本規格與 PR
7. **指令**：新增一個綁定到 `d d` 的 `RemoveSelectedItem`*，而不是擴大名稱指明佇列的 `RemoveFromQueue`；`RemoveFromQueue` 在使用者的按鍵對應中仍可運作（tech-lead 的選擇，2026-10-25）

<a id="out-of-scope"></a>
## 不在範圍內

- 多選以及 `d` 對選取內容做什麼（#71）；一次送出多個移除
- 復原（把移除的東西再加回去）
- 在 `tidal-player logout` 或清空佇列的按鍵之前詢問
- 不是你的清單上的 `d d`（決策 6）；從專輯頁或搜尋結果把曲目從收藏移除，仍是彈出視窗的 *Remove from favorites* 或 `DeleteFromLiked` 綁定

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-25"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-25）

- **型別**：`Removed::target` 是 `RemovedTarget::{Favorite(String), Position(u32), Entry(EntryId)}`；`RemoveFrom::Playlist` 持有播放清單的 `uuid` 與 `title`，以及彈出視窗的那一列所帶的頁面 ETag（`Option<String>`，沒有時以 `""` 送出，如同以前）。這些型別從 `tidal_player_core::ui` 匯出；`question` 留在 `ui::popup`
- **一條路徑**：`browse::ask_removal(state, Removal)` 開啟詢問，或在 `confirm_removals` 關閉時立刻移除。`browse::run`（彈出視窗的 `Enter` 與 `[[actions]]` 綁定）與 `dispatch::queue_command`（佇列頁上的 `RemoveFromQueue`）呼叫它；`run` 的列標題取自彈出視窗的標題（`selected_actions`/`playing_actions`），佇列按鍵的則取自該項目的曲目標題
- **多個項目**：對多個收藏或佇列項目的移除按 `y`，會為每個項目各送出一個請求或指令。對多個播放清單位置的移除目前什麼也不送出：位置在每次移除之後都會位移，而它們如何送出由 #71 規定
- **按鍵說明開啟時**：按鍵說明開啟時，`d` 與任何其他第一個按鍵一樣被收集進待處理的序列（如同 `g`）；`ac8_help_no_other_key_acts` 現在像檢查 `g l` 一樣檢查 `d d`
- **按鍵說明與提示**：按鍵說明（`?`）在佇列頁上列出 `d d  remove from the queue`，如既有的快照 `ac14_help_queue` 與 `ac9_theme_help_80x24` 所示；按住 `d` 等待延遲後，會顯示 0013 的提示框，標題為 `d …`，項目為 `d  remove from the queue`（0013 只列出下一個按鍵）
- **換行**：詢問依彈出視窗的寬度換行，所以 80 × 24 的快照把 `Remove Collide With The Sky from favorites?` 與 `(y/n)` 顯示在兩行
- **使用者舊的 `keymap.toml`**：由 AC9 處理（決策 5），在第一次驗收執行之後加入
- **測試隔離**：`crates/app/tests/cli.rs` 的 `bin_in` 沒有設定設定目錄，所以它的測試讀取了開發者自己的 `~/.config/tidal-player`，並因舊的 `keymap.toml` 而在那裡失敗；它現在把 `TIDAL_PLAYER_CONFIG_DIR` 指向該測試的暫存目錄（0008「錯誤」）
- **AC9 的通知**：取代預設值的子句排在不支援名稱的子句之前；每個都以 `keymap.toml: ` 開頭，以 ` · ` 連接。檔案中有多個序列各自丟棄預設值時，每個給一個子句，在單一個 `keymap.toml: ` 前綴內以 `; ` 連接。兩個檔案序列都衝突的預設值只由第一個點名。之後被 `None` 解除綁定的檔案序列不取代任何東西
- **AC9 的規則**：「預設值」是檔案沒有綁定其序列的任何綁定；重新綁定某個預設值自己序列的檔案序列不算衝突。Quit 的檢查仍在丟棄之後執行，所以丟棄每個 `Quit` 按鍵仍像以前一樣被拒絕
- **AC10 移除動作的選擇**：`browse::removal_action` 在彈出視窗會為該列列出的動作（`selected_actions`）中，依視窗挑選：音樂庫的 Playlists 中是 *Delete playlist*，否則是 *Remove from favorites*，自己的播放清單的曲目中是 *Remove from this playlist*，收藏的曲目、Albums 與 Artists 中是 *Remove from favorites*；`browse::removable(page, kind)` 是視窗的規則，並與按鍵說明（`in_window`）共用，所以按鍵說明與提示恰好列在 `d d` 有作用的地方。佇列的 `RemoveSelectedItem` 與 `RemoveFromQueue` 都走分派器的路徑（`queue_command`）
- **AC10 與中斷連線的播放器**：收藏列上的 `d d` 會詢問，`y` 什麼也不送出，如同彈出視窗的動作（0006 AC16 的測試已更新：它原本預期 `d` 什麼也不做）
- **AC10 的說明文字**：`RemoveSelectedItem` 的說明文字在每個視窗（包括佇列）都是 `remove from the list`，所以在 `d` 之後的提示在那裡也是 `d  remove from the list`
- **快照**：列出 `d d` 的按鍵說明快照現在寫的是 `remove from the list`，而 Library · Albums 與 40 × 12 的按鍵說明快照新增了 `d d` 列（該視窗現在會列出它）
