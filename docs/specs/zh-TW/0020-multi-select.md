[English](../0020-multi-select.md) | 正體中文

<a id="0020--selecting-several-rows"></a>
# 0020 — 選取多列

- **狀態**：implemented（2026-11-02；approved 2026-10-26，2026-11-02 修訂：`Esc` 先清除保留的篩選，再清除選取）
- **負責人**：tech-lead（主要 session）
- **Issue**：[#71](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/71)
- **相依於**：0004（implemented：佇列）、0006（implemented：清單、動作彈出視窗、收藏與播放清單的寫入）、0008（implemented：按鍵對應、按鍵序列）、0012（implemented：篩選）、0013（implemented：按鍵提示）、0017（implemented：佈景主題）、0019（implemented：移除詢問、`d d`）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md)（新的「Selecting rows」一節、按鍵表）與 [`docs/config.md`](../../zh-TW/config.md)（新的指令、`selected_row`），以及它們的 zh-TW 版本（AC13）

<a id="context"></a>
## 背景

目前每個動作都只作用於一列：游標所在的列（`Enter`、`a`、`d d`、動作彈出視窗）或正在播放的曲目。從專輯加五首曲目到佇列，或從播放清單移除十首，要重複五次或十次「游標、按鍵、詢問」。spotify-player 沒有多選功能；tidalt 也沒有。沒有任何東西是承襲過來的。

本規格加入**選取**：在視窗中標記的列，由一組以 `s` 開頭的按鍵對它們動作。目前作用於游標所在列的按鍵仍然如此，不論有沒有選取。0019 讓單獨的 `d` 保持空著以供此用；使用者改選 `d s`（決策 4），所以 `d` 維持未綁定。

<a id="behaviour"></a>
## 行為

<a id="the-selection"></a>
### 選取

每個列出列的視窗（每個頁面的每個清單視窗、搜尋頁面的四個視窗、佇列）都有自己的**選取**：它的列的一個集合，一開始是空的。選取不需要模式：下列按鍵把列加進去或拿出來。把焦點移到另一個視窗會保留每個視窗的選取；按鍵作用於焦點視窗的選取。

列只要還在清單中就維持被選取，不論游標是否在它上面，也不論篩選（0012）是否把它隱藏（決策 3）。選取在以下情況會被**清除**：

- 在沒有彈出視窗、輸入提示或篩選輸入開啟，且焦點視窗沒有保留篩選（0012）時，按 `Esc`（`ClosePopup`）：清除焦點視窗的選取（以及動作模式，見下）。有保留的篩選時，那次 `Esc` 只清除篩選，選取保留，所以篩選、選取，再按 `Esc` 看整份清單，會保留已選取的內容；第二次 `Esc` 才清除它（決策 10）
- 換頁（任何頁面變更：`g …`、在專輯上按 `Enter`、歷史中的上一頁與下一頁）：離開的頁面上的每個選取都清除。透過歷史回到該頁不會把它帶回來
- 成功的選取動作（見下）：該視窗的選取。失敗的動作（被拒絕的寫入、已變更的播放清單）會讓它維持原樣

列是以它是什麼來持有，不是以游標在哪裡：佇列項目以它的項目 ID，清單列以它在視窗已載入清單中的位置（位置只會隨頁面載入而增加，0006）。離開佇列的佇列項目（它播完了而自動播放丟掉它、另一個客戶端移除了它）會連同選取一起離開。清單被重新載入的視窗（寫入之後的重新整理，0006）會失去它的選取，但失敗動作的列除外（見下）。

<a id="keys"></a>
### 按鍵

所有新的按鍵都是以 `s` 開頭的序列（決策 1）。單獨的 `s` 是前綴，什麼都不做；0013 的提示會列出它之後可接的按鍵。

| 按鍵 | 指令 | 作用 |
|---|---|---|
| `s c` | `SelectCurrent`* | 選取游標所在的列，已選取則取消選取 |
| `s j`、`s <N> j` | `SelectDown`* | 從游標往下選取 `N` 列（預設 1），包含游標所在的列；游標停在最後一列 |
| `s k`、`s <N> k` | `SelectUp`* | 同上，往上 |
| `s m` | `SelectMotion`* | 開啟或關閉動作模式（見下） |
| `s a` | `SelectionActions`* | 開啟選取動作彈出視窗（見下） |
| `d s` | `RemoveSelection`* | 從列所在的清單移除選取的列，先詢問（見下） |

`*`：我們自己的（0008 的標記）。說明文字：`select / unselect the row`、`select rows down`、`select rows up`、`motion mode on / off`、`actions on the selection`、`remove the selection`。

**計數。**`<N>` 是在 `s` 之後、`j` 或 `k` 之前輸入的一個或多個數字鍵（`0`–`9`）：`s 5 j`、`s 1 2 k`。計數是放在序列中取代一個按鍵：`SelectDown` 或 `SelectUp` 綁定到序列 `s j`（預設如此，或 `keymap.toml` 如何綁定）時，也接受在它最後一個按鍵之前的數字。只有這兩個指令接受計數（決策 2）。計數為 0，或數字之後接任何其他按鍵，都會取消該序列（0008 的不相符：什麼都不發生）。`s 1 j` 就是不會取消選取的 `s c`。選取已被選取的列會讓它維持選取；`SelectDown`/`SelectUp` 絕不取消選取。超過清單結尾的列不存在：`s 9 j` 在倒數第三列上會選取三列。在隨捲動載入的清單中，尚未載入的列不會被選取；游標的移動會像任何移動一樣載入下一頁（0006）。

**動作模式。**`s m` 以游標所在的列作為**錨點**開始它。開啟期間，從錨點到游標之間的每一列（兩個方向都算，包含游標所在的列）都顯示為已選取，不論游標怎麼移動（`j`、`k`、`G`、`g g`、`C-f`……）。再按一次 `s m` 結束它，並把那些列加進選取。在它開始之前已選取的列維持選取。開啟期間：

- 視窗的標題顯示 `· motion`（見下）
- `s a` 與 `d s` 作用於含有動作範圍的選取，並結束動作模式
- `s c` 什麼都不做：游標永遠是範圍移動的那一端，所以它的列永遠在範圍內；按鍵說明會把 `s c` 變淡（決策 5）。要切換單一列，先以 `s m` 結束動作模式
- `Esc` 結束它並清除該視窗的選取（如上），範圍也包含在內；清除保留篩選的 `Esc` 會讓模式維持開啟，它的範圍現在套用在顯示的列上
- 換頁或換焦點會結束它：換頁如上所述清除；把焦點移到另一個視窗會把範圍加進選取，如同 `s m`
- 設定了篩選時，範圍是錨點與游標之間看得見的列

**游標所在列的按鍵。**`Enter`、`a`、`d d` 與其他列按鍵，不論選取了什麼，都像現在一樣作用於游標所在的列（決策 6）。

<a id="the-selection-actions-popup"></a>
### 選取動作彈出視窗

有選取時按 `s a` 會開啟標題為 `N selected` 的彈出視窗，列出適用於每一個選取列的動作（決策 7）。沒有選取時它什麼都不做（按鍵說明也把它變淡，0008 的規則）。動作依序如下，每個都有固定的按鍵：

| 動作 | 按鍵 | 列出的條件 | 送出 |
|---|---|---|---|
| *Play* | `p` | 一律 | `Open { items, at: None }`：選取的列取代佇列，並播放第一個 |
| *Play next* | `N` | 一律 | `Open { items, at: Some(Next) }` |
| *Add to queue* | `a` | 一律 | `Open { items, at: Some(End) }` |
| *Add to playlist…* | | 這些列是曲目（或佇列項目） | 為這些曲目依序開啟 0006 的播放清單選擇器 |
| *Add to favorites* | | 就清單所知（0006），某個選取的列不是收藏 | 對每個這樣的列送出一個 `AddFavorite` |
| *Remove from favorites* | | 在 `d d` 會從收藏移除的視窗中（0019 的表格） | 如該處的 `d s` |
| *Remove from this playlist* | | 自己的播放清單的曲目 | 如該處的 `d s` |
| *Remove from queue* | | 佇列 | 如該處的 `d s` |

`items` 是依清單順序的選取列（佇列用佇列的順序；其他用已載入清單的順序，包含被隱藏的列），每個都是它那一列的 `Enter` 或 `a` 目前送出的 `Item`（曲目、專輯或播放清單）。藝人不是 `Item`，所以在藝人視窗中不列出 *Play*、*Play next* 與 *Add to queue*。導覽動作（*Open*、*Go to album*、*Go to artist*、*Go to radio*）永遠不列出：它們只作用於一列。

彈出視窗接受 0006 的彈出視窗按鍵（`j`/`k`、游標所在列的 `Enter`、`Esc`）以及表格的固定按鍵：緊接在 `s a` 之後輸入（`s a p`），固定按鍵會執行它的動作，沒有可見的停頓，然後彈出視窗關閉。彈出視窗中的 `p`、`N` 與 `a` 是彈出視窗自己的按鍵（像詢問的 `y`/`n`），不是按鍵對應的綁定，所以不會與 0008 前綴規則下的 `s a` 衝突（決策 8）。

成功的動作會清除該視窗的選取；*Add to playlist…* 在曲目加入之後才清除它（以 `Esc` 取消選擇器則保留它）。

<a id="removing-the-selection-d-s"></a>
### 移除選取：`d s`

`d s` 從列所在的清單移除選取的列，如同 0019 的 `d d` 對單一列所做的，透過 0019 的詢問，在同樣的視窗中（0019 的表格：佇列、自己的播放清單的曲目、收藏的曲目、音樂庫的 Albums、Artists 與 Playlists）。在其他地方，以及沒有選取時，它什麼都不做。在音樂庫的 Playlists 中，選取裡自己的播放清單會被刪除，追蹤的則從收藏移除（決策 9）。

詢問會指出它的目標。0019 對多個項目的文字保留為第一行，而彈出視窗在其下方**列出每個目標**，每行一個標題，依清單順序，包含被隱藏的列（決策 3）：

```
Remove 3 tracks from Road trip? (y/n)
  Collide With The Sky
  Bloom
  Holocene
```

比彈出視窗更長的清單可用 `j`/`k` 捲動（詢問的 `y`/`n`/`Esc` 作用如前）。音樂庫的選取中有自己的播放清單時，詢問是 `Delete 2 playlists and remove 1 from favorites? (y/n)`，被刪除的列在 `Delete:` 之下先列出，其他的列在 `Remove from favorites:` 之下；不論 `confirm_removals` 怎麼設定都會詢問，如同 *Delete playlist*（0019）。否則 `confirm_removals = false` 會立刻移除，如同對單一列一樣。只有一個目標時，詢問是 0019 的單一項目文字，清單只有一行。

`y` 送出什麼：

| 來自 | 送出 |
|---|---|
| 佇列 | 每個選取的項目一個 `RemoveFromQueue(entry)` |
| 收藏 | 每列一個 `RemoveFavorite(kind, id)` |
| 自己的播放清單 | 一個帶有每個選取位置的 `RemoveFromPlaylist { uuid, indices, etag }`：API 在一次寫入中把它們全部移除（假設 A1）；頁面之後的重新載入如同一次移除之後那樣 |
| 音樂庫的 Playlists | 每個自己的播放清單一個 `DeletePlaylist`，每個追蹤的播放清單一個 `RemoveFavorite(Playlist, …)` |

之後的訊息會指出數量：`Removed 3 tracks from Road trip`、`Removed 3 tracks from the queue`、`Removed 2 albums from favorites`。若多個寫入中有些失敗，訊息會說有多少個沒成功（`Removed 2 albums from favorites; 1 failed`），選取則保留失敗的列。當清單再次被取得（在成功的寫入之後，或在已變更播放清單的 `412` 之後）時，它們仍維持選取，並依它們是什麼（曲目、專輯、藝人或播放清單）重新找到，每一列在它再次載入時就立即找到。

<a id="how-it-looks"></a>
### 外觀

- 被選取的列在它的第一欄之前畫上 `+ ` 標記（沒有被選取的列得到兩個空格，這樣一旦有任何東西被選取，欄位就不會移動；選取為空的視窗完全不畫標記欄），並套用新的佈景主題元件 **`selected_row`**（我們自己的；預設 `Bold`），疊在列的樣式之上、游標的 `selection` 之前（0017 的順序：游標所在的列仍明顯是游標）
- 視窗標題在 0012 的符合數之後加上 `· N selected`（篩選隱藏了一些時為 ` · N selected, M hidden`），動作模式開啟時加上 ` · motion`
- 佇列頁面的列、搜尋視窗與每個清單視窗都以同樣方式繪製；彈出視窗沒有選取

<a id="keymap"></a>
### 按鍵對應

預設值新增上面的六個綁定。沒有任何預設綁定是 `s` 或以 `s` 開頭（0008 的表格），所以 0008 的前綴規則成立；綁定了 `s`（或 `d`）的 `keymap.toml` 會丟掉衝突的預設值，並附上 0019 的通知。`examples/keymap.toml` 列出它們。按鍵說明在它們作用的地方（有列的視窗；沒有選取時 `s a` 與 `d s` 變淡）把它們列在 `Lists` 之下，而 0013 在 `s` 之後的提示也列出它們。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — `SelectCurrent`（針對清單視窗、佇列、搜尋視窗的表格測試）：未選取的游標列變成已選取，已選取的變成未選取；游標不移動；在空視窗上什麼都不改變；有篩選時，游標下的列是篩選後的那一列
- **AC2** — 帶計數的 `SelectDown`/`SelectUp`（表格測試）：`s j` 只選取游標所在的列；`s 3 j` 選取游標所在的列與其下 2 列，游標在最後一列；`s 3 k` 往上；`s 1 2 j` 讀作 12；超過結尾（或開頭）時停在最後（第一）個已載入的列；已選取的列維持選取；`s 0 j`、`s 3 x`、`s 3 esc` 什麼都不選取，也不留下任何等待中的東西；游標到達結尾時，隨捲動載入的清單會請求下一頁，如同 `j`
- **AC3** — 按鍵對應中的計數：`build` 只接受 `SelectDown`/`SelectUp` 綁定最後一個按鍵之前的數字：`SelectDown` 重新綁定到 `x y` 時，`x 4 y` 選取 4 列，`s 4 j` 什麼都不選；任何其他指令的序列中的數字不是計數（`g 4 g` 不相符）；序列是一個數字的綁定（`5` → `NextTrack`）在等待計數的序列之外仍會觸發
- **AC4** — 動作模式（表格測試）：`s m` 設定錨點；`j j` 之後三列讀作已選取；`s m` 把它們加進去並結束模式；先前已選取的列維持不變；往回移動超過錨點會往另一個方向選取列；`G` 與 `g g` 延伸它；`Esc` 結束它並清除選取；換焦點會加入範圍，而沒有移動任何東西的 `Tab` 讓模式維持開啟；換頁會清除；`s a` 與 `d s` 作用於選取加上範圍並結束模式；`s c` 什麼都不改變，且按鍵說明把它變淡
- **AC5** — 清除（表格測試）：失敗的寫入的列在清單重新載入之後仍維持選取，不論重新載入與最後一個回應哪個先到，並且在其他寫入要求的每一次之後的重新載入中也是如此，依它們是什麼來找到（之後因捲動才載入的列也是），而成功的列不會回來；選取一旦被手動改變，重新載入就會失去它；沒有彈出視窗或輸入開啟時的 `Esc` 只清除焦點視窗的選取；有保留的篩選時，它清除篩選並保留選取（以及動作模式），下一次 `Esc` 才清除選取；關閉彈出視窗或篩選輸入的 `Esc` 什麼都不清除；換頁會清除離開的頁面的每個視窗，回到該頁不會還原它；在其頁面被離開之後才到達的寫入回應不會改變任何選取，現在取代該位置的頁面也一樣；成功的動作會清除；失敗的會保留選取；重新載入的視窗會失去它；離開佇列的佇列項目會離開選取；換焦點會保留每個視窗的選取
- **AC6** — 篩選：被篩選隱藏的已選取列維持選取並被動作作用；清除篩選會顯示它們仍然被選取；透過篩選選取只會選取看得見的列
- **AC7** — 選取動作彈出視窗：沒有選取時 `s a` 不開啟任何東西；有選取時，它以標題 `N selected` 開啟，並依視窗列出表格中的動作（針對各視窗的表格測試：專輯的曲目、自己的播放清單、佇列、收藏的曲目、音樂庫的 Albums、Artists、Playlists、搜尋視窗、mix）；絕不列出任何導覽動作
- **AC8** — 選取動作送出（表格測試）：*Play*（`s a p`）、*Play next*（`s a N`）、*Add to queue*（`s a a`）送出 `Open`，帶有依清單順序的選取項目（包含被隱藏的列）與表格中的 `at`，然後清除選取；彈出視窗列上的 `Enter` 做與它的按鍵相同的事；*Add to playlist…* 以依序的曲目開啟選擇器，選取在加入後清除，在 `Esc` 時保留；*Add to favorites* 對每個不確定為收藏的列送出一個 `AddFavorite`
- **AC9** — 游標列的按鍵忽略選取：有列被選取時，`Enter`、`a`、`d d` 與 `[[actions]]` 綁定作用於游標所在的列，與沒有選取時完全相同，並讓選取維持原樣
- **AC10** — 各視窗的 `d s`（0019 AC10 的表格，選取 3 個）：佇列、自己的播放清單的曲目、收藏的曲目、Albums、Artists、追蹤的播放清單 → 詢問帶有 0019 對多個項目的第一行，並依清單順序列出三個標題，包含被隱藏的列；`y` 送出表格中的寫入（一個帶有三個位置，並在被要求時帶有頁面 ETag 的 `RemoveFromPlaylist`）；`n`/`Esc` 什麼都不送並保留選取；其他視窗與空的選取什麼都不做；一個目標給出 0019 的單一項目文字；`confirm_removals = false` 立即送出
- **AC11** — 音樂庫 Playlists 中同時有自己的與追蹤的播放清單時的 `d s`：帶有兩份清單的 `Delete N playlists and remove M from favorites? (y/n)` 詢問，即使 `confirm_removals = false` 也會詢問；`y` 對每個自己的播放清單送出一個 `DeletePlaylist`，對每個追蹤的播放清單送出一個 `RemoveFavorite`；全部成功與部分失敗（`…; 1 failed`）的訊息，失敗的列維持選取
- **AC12** — 繪製（`insta`，80 × 24）：有 2 個已選取列的清單（標記、`selected_row` 樣式、`· 2 selected`）、游標在已選取的列上、動作模式（`· motion`）、篩選隱藏了一個已選取的列（`· 2 selected, 1 hidden`）、篩選隱藏了已選取列上方的列（標記在已選取的列上，絕不在取代它位置而顯示的列上）、有選取的佇列、選取動作彈出視窗、帶有三個標題與標題多到放不下（已捲動）的 `d s` 詢問；設定了 `selected_row` 的佈景主題會為標記的列上色；在任何大小下都不 panic（0006 AC17 的表格，新增的列）。儲存格斷言（0017 AC10 的風格）：在游標所在的列上，`selection` 疊在 `selected_row` 之上
- **AC13** — 按鍵對應與文件：預設值綁定表格中的六個指令，除了 `d d` 之外沒有其他預設值以 `s` 或 `d` 開頭；`examples/keymap.toml` 相符；按鍵說明在它們作用的地方列出它們，並在選取為空時把 `s a`/`d s` 變淡，以及在沒有任何動作適用於選取時（mix 清單）把 `s a` 變淡；0013 在 `s` 之後的提示列出 `c`、`j`、`k`、`m`、`a`；`theme.toml` 的 `component_style` 接受 `selected_row`，且 `examples/theme.toml` 仍能載入而沒有通知；`docs/tui.md`、`docs/config.md` 與它們的 zh-TW 版本描述選取、按鍵與 `selected_row`；`README.md` 與 `CLAUDE.md` 的「Features」已更新
- **AC14** — 一次寫入多個播放清單位置：`LibraryRequest::RemoveFromPlaylist` 攜帶 `indices: Vec<u32>`；API 帶著 ETag 送出 `DELETE playlists/{uuid}/items/{i,j,k}`（錄製的測試資料），而一個位置送出的是目前原封不動的請求；`412` 會保留選取並給出 0006 的「playlist changed」訊息

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- **0 個可見列的選取**（每個已選取的列都被篩選隱藏）：`s a` 與 `d s` 仍然對它們動作；標題顯示 `· 3 selected, 3 hidden`
- **佇列在底下改變**（自動前進、另一個客戶端）：離開的項目會從選取中掉出；對在此期間已不在的項目執行 *Remove from queue*，播放器會忽略它，如現在一樣（0019）
- **很長的選取**（數百列）：`Open` 以一個指令接受它們；從收藏與佇列移除時每個送出一個請求，如 0019 的多個項目說明；詢問的清單會捲動
- **已中斷連線**（連上的 TUI，常駐程式不見了）：選取可以運作（它是 UI 狀態）；動作做的是單列動作在中斷連線時目前所做的事，而選取保留，因為沒有任何東西成功
- **進行中的整份清單載入**（0006）：只有 `Esc`/`q` 有作用，所以沒有任何選取按鍵會作用
- **佇列中正在播放的列**：可以被選取與移除，如同 `d d` 可以
- **很小的終端機**：視窗寬度低於 10 欄時捨棄標記欄（只靠樣式顯示選取）；不會 panic
- **在文字輸入框中輸入的按鍵**（搜尋、篩選、輸入提示）：`s` 在那裡是文字，如同每個按鍵一樣（0008）
- **無法播放的列**（變淡，0006）：與其他列一樣被選取與移除；*Play* 會送出它們，播放器像現在一樣跳過它們

<a id="test-plan"></a>
## 測試計畫

每個測試以其條件命名。紅燈是對存根的失敗斷言（指令存在但什麼都不做，彈出視窗什麼都不列，繪製不畫標記），沒有 `todo!()`，沒有編譯錯誤。

| AC | 測試（檔案 :: 名稱） | 驗證內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/select/tests.rs` :: `ac1_select_current`（表格） | 切換、游標保留、空視窗、篩選後的列 | 選取維持為空 |
| AC2 | `crates/core/src/ui/select/tests.rs` :: `ac2_select_rows_with_count`（表格） | 每個計數與方向的列和游標、結尾、取消、延遲載入的請求 | 選取維持為空 |
| AC3 | `crates/core/src/ui/keymap/tests.rs` :: `ac3_count_in_sequence`（表格） | 重新綁定的計數綁定、其他指令、數字綁定 | `s 4` 不相符 |
| AC4 | `crates/core/src/ui/select/tests.rs` :: `ac4_motion_mode`（表格） | 範圍、結束、兩個方向、`G`、`Esc`、焦點、頁面、動作 | 沒有範圍 |
| AC5 | `crates/core/src/ui/select/tests.rs` :: `ac5_clearing`（表格） | 每種清除與保留的情形 | 存根的選取從未設定，所以「保留」的列失敗 |
| AC6 | `crates/core/src/ui/filter/tests.rs` :: `ac6_selection_through_filter` | 被隱藏的列保留並被動作作用 | 選取為空 |
| AC7 | `crates/core/src/ui/select/tests.rs` :: `ac7_selection_actions_listed`（針對各視窗的表格） | 彈出視窗標題與動作 | 彈出視窗沒有開啟 |
| AC8 | `crates/core/src/ui/select/tests.rs` :: `ac8_selection_actions_send`（表格） | `Open` 指令、選擇器、收藏、清除 | 什麼都沒送出 |
| AC9 | `crates/core/src/ui/select/tests.rs` :: `ac9_cursor_keys_ignore_selection`（表格） | `Enter`、`a`、`d d`、一個綁定作用於游標所在的列 | 存根的選取為空，所以「選取保留」的斷言失敗 |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_remove_selection`（針對各視窗的表格） | 詢問文字與清單、`y` 送出什麼、`n`/`Esc`、關閉的開關 | 沒有詢問 |
| AC11 | `crates/core/src/ui/browse/tests.rs` :: `ac11_remove_selected_playlists` | 混合的詢問、一律詢問、寫入、訊息 | 沒有詢問 |
| AC12 | `crates/app/src/tui/render.rs` :: `ac12_selection_*`（快照）、`ac10_theme_cues`（新的儲存格列）、`ac17_no_panic_any_size`（新的列） | 畫出的標記、標題、彈出視窗、詢問 | 沒有標記或標題文字 |
| AC13 | `crates/core/src/ui/keymap/tests.rs` :: `ac13_select_defaults`；`crates/core/src/ui/help/tests.rs` :: `ac13_select_keys_listed`；`crates/core/src/ui/theme/tests.rs` :: `ac13_selected_row_component`；`crates/app/tests/examples.rs`（既有的範例測試）；`crates/app/tests/docs.rs` :: `ac13_selection_documented`；zh-TW 版本、README 與 `CLAUDE.md` 於驗收時檢視 | 預設值、說明、提示、佈景主題名稱、範例、文件 | 預設值缺少這些綁定 |
| AC14 | `crates/api/src/library.rs` 的測試 :: `ac14_remove_several_positions`（錄製的測試資料）；`crates/core/src/library.rs` :: 協定往返的列 | 請求路徑與標頭；一個位置不變；412 | 存根只送出第一個索引 |

驗收時在真實終端機、針對真實 API 手動檢查：`s 3 j` 然後 `s a a` 把三首曲目排入佇列；`s m`、`j j`、`s m`、`s a p` 播放它們；自己的播放清單的三首曲目上的 `d s` 列出它們，`y` 以一次寫入移除它們（A1）；`Esc` 清除。

2026-11-02 在 tmux（150 × 42）中針對真實 API 執行，全部如上：`s 3 j`、`s a a` 把三首曲目排入佇列；`s m j j s m`、`s a p` 以它們取代佇列並播放；在有六首曲目的新播放清單上，選取第 1、3、5 列，`d s` 依序列出這三首，`y` 以一次寫入恰好移除它們（`Removed 3 tracks from …`，其他三首留下），所以 A1 成立；有保留的篩選時，`Esc` 清除篩選並保留選取，下一次 `Esc` 清除它。這次執行發現了「錯誤」之下的標記錯誤。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::ui`：新的 `select` 模組：`Selection`（每個視窗一個，列鍵的集合：佇列用 `EntryId`，其他地方用已載入清單的位置；動作錨點）、純粹的轉換（切換、選取 N 個、動作開始／結束、清除、佇列變動時修剪）；`State` 為目前頁面的每個視窗與佇列各持有一個；`UiCommand` 新增六個指令；`keymap` 新增計數槽（`SelectDown`/`SelectUp` 綁定接受它們最後一個按鍵之前的數字）與預設值；`Popup::SelectionActions`；0019 的 `Removal` 接受多個項目（它已持有 `items: Vec<Removed>`），而它的詢問新增目標清單；`Component::SelectedRow`。沒有 I/O，沒有新的相依套件
- `tidal-player-core::library`：`LibraryRequest::RemoveFromPlaylist { indices: Vec<u32>, .. }`（原本是單一的 `index`）：協定變更，所以常駐程式與客戶端一起移動（0005：同一個二進位檔）
- `tidal-player-api`：多個位置的 `DELETE`
- `tidal-player`：繪製（標記欄、標題、彈出視窗、會捲動的詢問）、文件測試
- `xtask layering`：沒有變更

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-26，來自程式碼）：`Command::Open { items: Vec<Item>, at: Option<InsertAt> }` 把任何項目排入佇列（`None` 取代佇列，`Next`/`End` 插入）（`crates/core/src/protocol.rs`）；動作彈出視窗的項目是 `MenuAction`（`crates/core/src/ui/popup.rs`），其中 `AddToPlaylist` 接受 `TrackSource::Tracks(Vec<Track>)`；0019 的 `Removal { items: Vec<Removed> }` 與 `question` 已經給出多個項目的第一行；API 以每個 `DELETE playlists/{uuid}/items/{index}` 加上 `if-none-match` 移除一個播放清單位置（`crates/api/src/library.rs`）；按鍵對應只比對固定的序列，在檔案項目之間拒絕作為較長序列前綴的已綁定前綴（`check_prefixes`），並以通知丟掉衝突的預設值（0019）；沒有預設綁定是 `s` 或以 `s` 開頭，而 `d` 只開頭 `d d`；佈景主題的 `Selection` 元件就是游標所在的列（0017）。

假設：

- **A1** — `DELETE playlists/{uuid}/items/{i,j,k}`（位置以逗號連接）在一次寫入中移除多個位置。python-tidal 的 `UserPlaylist.remove_by_indices` 就是送出這個。**已在驗收時針對真實 API 驗證**（2026-11-02，六首中的位置 0、2 與 4）。若它被拒絕，API 會改為從最高位置往下，每個位置送出一個 `DELETE`，並取每個回應的新 ETag，而 AC14 就會改成那樣（core 的請求兩種情況都一樣）

<a id="decisions"></a>
## 決策

由使用者回答（2026-10-25 與 2026-10-26）：

1. **按鍵**：*已回答*：沒有獨立的選取模式；`s` 之下的一組按鍵：`s c` 切換游標所在的列，`s <N> j`/`s <N> k` 選取 N 列，`s m` 切換動作模式（像 Vim 的 visual 模式，再按一次 `s m` 結束），`s a` 是對選取的動作
2. **計數**：*已回答*：只在 `s` 之內（`s <N> j/k`），包含游標所在的列，計數 1 就是 `s c`；`s c` 不帶計數。其他按鍵的計數（`5j`）是另一個 issue
3. **篩選**：*已回答*：被篩選隱藏的已選取列維持選取；移除詢問列出每個目標
4. **移除**：*已回答*：`d d` 像現在一樣移除游標所在的列，`d s` 移除選取；`d` 維持未綁定
5. **動作模式中的 `s c`**：*已回答*：什麼都不做（在按鍵說明中變淡）：游標所在的列永遠在範圍內
6. **游標列的按鍵**：*已回答*：`Enter` 像現在一樣連同它的清單播放游標所在的列；`s a p` 播放選取
7. **動作彈出視窗**：*已回答*：作用於每個已選取的列；只列出適用於多個列的動作（沒有導覽）
8. **`s a` 與它的按鍵**：*已回答*：`s a` 開啟彈出視窗；`p` *Play*（取代佇列）、`N` *Play next*、`a` *Add to queue* 是彈出視窗自己的按鍵，所以 `s a p` 很快就能輸入，而且 0008 的前綴規則成立
9. **音樂庫中自己的播放清單上的 `d s`**：*已回答*：在一個詢問中刪除它們並取消追蹤追蹤中的播放清單，一律詢問。另一種選擇：把自己的播放清單排除在外（只有 `d d` 會刪除）
10. **清除**：*已回答*：`Esc`、換頁與成功的動作會清除選取；在視窗之間移動焦點會保留每個視窗的選取。`Esc` 是分層的（第一次建置之後回答）：先清除保留的篩選，下一次 `Esc` 才清除選取
11. **外觀**：*已回答*：`+ ` 標記、`selected_row` 元件（預設 `Bold`）、標題中的 `· N selected`
12. **全選**：*已回答*：不在範圍內（隨捲動載入的清單必須載入每一頁）
13. **發行**：*已回答*：重大：`tidal-player` 二進位檔的次版本號升級，`1.2.1` → `1.3.0`

<a id="out-of-scope"></a>
## 不在範圍內

- 其他按鍵的計數（`5j`、`5 d d`）
- 全選、反向選取、以篩選的符合項目選取
- 能撐過換頁或重新載入的選取
- 以移動選取來重新排序佇列或播放清單
- 對多個列的導覽動作

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-26"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-26）

模型（`tidal-player-core::ui`）：

- **選取放在哪裡**：視窗的選取放在它的 `Window` 上（`Window::selection`，已載入列索引的 `select::Selection<usize>`，以及動作錨點），所以 `Window::reset`（重新載入）會連同列一起丟掉它（但不包括等待再次被選取的識別資料，見下）；佇列的放在歷史最底層的佇列頁面上（`Page::selection`，一個 `Selection<EntryId>`），在佇列的篩選旁邊。每個 `Queue` 頁面都顯示那一個
- **計數**：`Keymap::lookup(keys) -> Lookup::{Bound(binding, count), Starts, Cancel, Unbound}`。計數放在 `SelectDown`/`SelectUp` 綁定的非空起頭之後（它們的單一按鍵綁定不接受計數，所以單獨輸入的數字永遠不是計數）；已綁定的序列或其起頭，優先於計數。該起頭之後的數字是等待中的；綁定的最後一個按鍵帶著計數執行它；任何其他按鍵，或計數為 0，都會取消：什麼都不執行，連那個按鍵也不執行（0008 的不相符會單獨執行它；這裡規格說什麼都不發生）。計數會飽和。計數不會顯示在 0013 的提示中（`s 4` 之後沒有任何東西可列）
- **頁面上的 `Esc`**：有保留的篩選（0012）時清除焦點視窗的篩選，不做其他事；沒有保留篩選時，清除該視窗的選取與動作模式
- **換頁**是推入（`g …`、`Enter`、`z`）或彈出（上一頁）：離開的頁面失去每個選取（是佇列頁面時為佇列的）；新的搜尋取代搜尋頁面及其視窗。開啟已在最上面的頁面不算換頁
- **焦點變更**（`Tab`/`BackTab`、`[`/`]`、搜尋頁面的循環）會結束該視窗的動作模式，並加入它的範圍；沒有移動任何東西的 `Tab` 或 `[`/`]`（只有一個窗格的頁面、只有一個分頁的窗格）不算焦點變更，會讓模式維持開啟
- **動作模式**：錨點是一列（佇列上則是一個項目）；範圍是錨點與游標所在列之間顯示的列。離開佇列的錨點項目會結束動作模式，不加入任何東西
- **彈出視窗**：`Popup::SelectionActions { title, actions: Vec<SelectionAction>, cursor, origin }`；`p`、`N`、`a` 是 `popup_fixed_key` 中的固定按鍵，按鍵說明把它們列為彈出視窗的按鍵。沒有任何動作適用時（mix 清單：mix 不是項目也不是收藏），`s a` 什麼都不開啟
- **什麼算已知的收藏**（*Add to favorites*）：收藏曲目的列、音樂庫的 Albums 與 Artists，以及它的 Playlists 中追蹤的播放清單；自己的播放清單（搜尋之外）不可能是收藏，所以含有一個的選取不列出 *Add to favorites*。*Remove from favorites* 只有在音樂庫的 Playlists 中、每個選取的播放清單都是追蹤的時候才列出：選取了自己的播放清單時，`d s` 會刪除它們，而那個標籤並沒有這樣說
- **詢問**：`Confirmed::RemoveSelection(SelectionRemoval { origin, parts })`，每份清單一個 `SelectionPart { removal, held }`（0019 的 `Removal` 加上每個項目原本所在的列）；帶有 `RemovedTarget::Playlist(uuid)` 的 `RemoveFrom::OwnPlaylists` 會刪除。`popup::selection_question` 給出文字（單數時為 `Delete 1 playlist and remove …`），`popup::target_lines(&Confirmed)` 給出它下方的清單（標題 `Delete:` 與 `Remove from favorites:` 只有在有兩份清單時才出現；0019 自己的詢問則什麼都沒有）。`Popup::Confirm` 新增 `scroll`，也就是顯示的第一個目標行：清單指令移動它，最多到最後一行（繪製器顯示從那裡開始放得下的部分）；按鍵說明不為詢問列出它們
- **成功與失敗**：指令（`Open`、`RemoveFromQueue`）在送出時就算成功，所以選取在那時清除，且只在已連線時。選取的音樂庫寫入會被記為 `select::Batch`（`State::batches`），直到全部都有回應：然後成功的列離開選取（全部：它被清除）。在那之前不顯示這些寫入自己的訊息；之後訊息會計數它們。一列對應一次寫入時保留 0019 的訊息（`Removed from favorites`）；每個寫入都失敗時保留最後一個的錯誤；*Add to playlist…* 保留它自己的（`Added 2 tracks to Nine`）；兩份清單都有時為 `Deleted 2 playlists and removed 1 from favorites`，沒有任何東西被移除的清單會省略（`Deleted 2 playlists; 1 failed`）；多個列的 *Add to favorites* 說 `Added 2 tracks to favorites`。佇列中單一項目的 `d s` 不給訊息，如同 `d d`；多個項目則在送出時給出 `Removed 3 tracks from the queue`。中斷連線會丟掉批次並保留選取
- **失敗的列與重新載入**（2026-11-02 修訂，見「錯誤」）：成功的寫入會重新取得頁面（0006），而 0006 在 `412` 之後會重新取得播放清單。一個批次的每個寫入都保有它的列從送出時起的識別資料（`Row::key`：曲目、專輯或藝人 ID，播放清單或 mix UUID）。批次在有失敗的情況下得到回應時，視窗的選取會變成在現在已載入的列中依識別資料找到的失敗列，而識別資料留在視窗上（`Window::reselect`）：每次重新載入都保留它們（多個成功的寫入會要求多次），而每個載入的列（重新載入的第一頁，或之後的頁面），只要其識別資料已選取的列數少於失敗的列數，就會被選取。一份播放清單中出現兩次的曲目，會在它第一個已載入的副本上找到。當選取被清除（`Esc`、成功的動作、換頁）或被手動改變（`s c`、`s j`、`s k`、`s m`）時，它們就被丟掉：選取這時又屬於使用者，重新載入就像對任何其他選取一樣會失去它
- 選取的 ***Add to playlist…***：`State::selection_add` 保有它的來源與列，直到播放清單的寫入送出（它成為一個批次）；選擇器或名稱輸入提示上的 `Esc`、重複詢問上的 `n`/`Esc`、失敗的建立、某一列自己的 *Add to playlist…* 與中斷連線都會忘掉它
- **預設值**：`d s` 緊接在 `d d` 之後列出，所以綁定了 `d` 的 `keymap.toml` 會得到 `keymap.toml: "d" replaces the defaults "d d" (RemoveSelectedItem), "d s" (RemoveSelection)`；0019 的測試現在預期該文字。新的指令接在 `COMMANDS` 中的 `SelectLastOrScrollToBottom` 之後，所以按鍵說明在 `Lists` 中把它們列在六個移動之後
- **按鍵說明**：`s c`、`s j`、`s k`、`s m`、`s a` 在每個清單視窗與佇列的 `Lists` 中（不包括最佳結果、搜尋輸入框或彈出視窗），`d s` 在 `d d` 作用的地方。變淡：沒有列或在動作模式時的 `s c`、沒有列時的 `s j`/`s k`/`s m`、沒有選取或沒有適用於選取的動作（mix）時的 `s a`、沒有選取或已中斷連線時的 `d s`（如同 `d d`）
- **給 TUI**（繪製是下一步）：`select::window_rows(&Window)` 與 `select::queue_entries(&State)`（顯示為已選取的列，包含動作範圍）、`select::window_marks` / `select::queue_marks`（`Marks { count, hidden, motion }`，`Marks::title_parts()` 給出 `2 selected, 1 hidden` 與 `motion`，接在篩選的部分之後出現在標題中；`Window::title` 不變）、`Popup::SelectionActions`、`popup::target_lines` 與 `Popup::Confirm::scroll`
- `WholeListSource::Detached` 把它的視窗裝箱（視窗大到超過 clippy 的 enum 大小限制）
- 測試：0013 的 `ac2_hints_nested_prefix`（core）把它自訂的前綴從 `s` 移到 `x`，因為 `s` 現在開頭了預設綁定；app 的 `ac6_key_hints_nested` 快照（同一個自訂的 `s …` 按鍵對應）現在也列出預設的 `c`、`j`、`k`、`m`、`a`

<a id="bugs"></a>
## 錯誤

- **標記在篩選下落在錯誤的列上（於實機檢查時發現，2026-11-02）。**預期：有保留的篩選時，`+ ` 標記與 `selected_row` 樣式在顯示出來的已選取列上，而且不在其他任何列上。實際：第 1 與第 2 列已選取，而 `/ado` 把兩者都隱藏時，標題說 `2 selected, 2 hidden`，但篩選顯示的前兩列卻被畫成已選取。根本原因：清單繪製是以列在顯示的列中的位置去查詢選取（已載入清單中的位置）；AC12 快照的篩選只隱藏了選取下方的一列，在那裡兩者相同。AC12 新增案例
- **失敗的列因重新載入而失去選取（於 PR 審查時發現，2026-11-02）。**預期：選取的某些寫入失敗時，失敗的列維持選取（「移除選取」）。實際：每個成功的寫入都會重新取得頁面，而重新載入重設了視窗的選取，所以只要有任何寫入成功，失敗的列最後都是未選取，而以位置保留的列會指向重新載入後清單中的其他列。根本原因：選取持有的是已載入清單中的位置，而「失敗的列與重新載入」把撐過重新載入排除在範圍之外；測試回應了寫入，卻從未回應重新載入。現在改以識別資料撐過重新載入（修訂後的備註）。AC5 新增案例
- **沒有移動任何東西的 `Tab` 結束了動作模式（於 PR 審查時發現，2026-11-02）。**預期：只有焦點變更會結束動作模式。實際：在只有一個窗格的頁面上，`Tab` 結束了它，並加入了範圍。根本原因：動作範圍在循環焦點之前就結束了，不論焦點是否移動。AC4 新增案例
- **遲到的寫入回應作用於另一個頁面的選取（於 PR 審查時發現，2026-11-02）。**預期：在選取所屬的頁面被離開之後才到達的回應，不改變任何選取（該頁面的選取在離開時已被清除）。實際：批次以它在歷史中的索引來指名它的頁面，所以回到上一頁再開啟另一個頁面（它佔用同一個索引）之後，或在已滿的歷史中最舊的頁面被丟掉之後，回應會清除或取消選取現在位於該索引的頁面的列。根本原因：歷史索引會被重複使用。離開頁面現在會讓它的批次與等待中的 *Add to playlist…* 脫離：它們的回應仍然給出自己的訊息，但不碰任何選取。AC5 新增案例
- **按鍵說明在 `s a` 什麼都不做的地方提供它（於 PR 審查時發現，2026-11-02）。**預期：按鍵說明把什麼都不做的按鍵變淡。實際：選取了 mix 時，`s a` 什麼都不開啟（沒有任何動作適用於 mix），但說明沒有把它變淡。AC13 新增案例
