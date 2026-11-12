[English](../0012-filter.md) | 正體中文

<a id="0012--filter-the-focused-window"></a>
# 0012 — 篩選取得焦點的視窗

- **狀態**：implemented（2026-10-10）；2026-10-10 核准（使用者回答了下方每一項決策並要求實作）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#18](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/18)
- **相依於**：0004（implemented：佇列頁面）、0006（implemented：頁面、隨捲動載入的視窗、整份清單載入、角色篩選）、0007（implemented：搜尋頁面與它的 `/`）、0008（implemented：按鍵對應、`Search` 指令、文字輸入框的固定按鍵、按鍵說明與它的篩選）、0013（implemented：按鍵提示）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Filtering a list」，且 `/` 那一列有變動；[`docs/config.md`](../../zh-TW/config.md) 的指令表更動 `Search` 的文字（AC10）

<a id="context"></a>
## 背景

收藏的曲目、很長的播放清單、藝人的 *All tracks* 或佇列，可能有數百或數千列。目前要到達某一列的唯一辦法是捲動（`j`、`C-f`、`G`），而在隨捲動載入的清單上，那一列甚至可能還沒載入。spotify-player 把 `/` 綁定到 **`Search`**，「open a popup for searching in the current page」：一個單行的輸入框，隨著輸入把取得焦點的清單縮減為相符的列。

在這裡，`Search`（`/`）做的是別的事：在搜尋頁面上它讓搜尋輸入框重新取得焦點（0007「Windows」、0008 的指令表），在其他每個頁面上則不做任何事。0007 與 0008 都把頁面內篩選排除在範圍之外，並指向這份規格（0013 稱它為「0012，issue #18」）。

應用程式中已有兩處行為像篩選，並確立了本規格所依循的先例：

- 按鍵說明的篩選（0008「The keys help」）：`/` 開始輸入，輸入的字元會縮減列（不分大小寫），`Backspace` 編輯，`Enter` 保留篩選並停止輸入，`Esc` 清除它，沒有任何相符時顯示 `No keys match "xyz"`
- *All tracks* 的角色篩選（0006）：視窗只顯示它已載入的列中的一部分（`Window::visible`），標題會這麼說，而曲目上的 `Enter` 會把**顯示出來的列**排入佇列，而不是被隱藏的

tidalt 沒有篩選，所以沒有東西需要沿用。

<a id="behaviour"></a>
## 行為

<a id="where-it-acts"></a>
### 作用的範圍

`/`（`Search`）會對最上層頁面中取得焦點的**視窗**開始一個**篩選**：

- 每個清單視窗：音樂庫的 *Playlists*、*Albums*、*Artists*；*Favorite tracks*；專輯與播放清單的曲目；藝人頁面的四個視窗（*Top tracks*、*Albums*、*Appears on*、*All tracks*）；搜尋頁面的四個結果視窗；mix 頁面；mix 與電台的曲目
- **佇列**頁面（它唯一的清單，從播放器的快照繪製）

在載入中或失敗的頁面上、在搜尋頁面的輸入框或它的 *Top hit* 列取得焦點時（輸入框會輸入 `/`）、在彈出視窗、開啟輸入提示或整份清單載入之上，它都不做任何事。每個視窗各有自己的篩選。

<a id="typing"></a>
### 輸入

`/` 開啟篩選進入**輸入**狀態：視窗的標題會顯示它（見下），而每個可列印的按鍵與貼上的文字都會先於按鍵對應進入它（固定按鍵，如其他文字輸入框，0008 決策 6）。輸入期間：

| 按鍵 | 作用 |
|---|---|
| 可列印的字元（包括 `space`）、貼上 | 附加到篩選（丟掉控制字元；最多 100 個字元） |
| `backspace` | 刪除最後一個字元（空篩選時不做任何事） |
| `C-u` | 清空篩選文字（繼續輸入） |
| `enter` | 停止輸入；篩選保留 |
| `esc` | 停止輸入並清除篩選 |
| `up`、`down`、`page_up`、`page_down` | 在相符的列上移動游標（一次一列、一次一個視窗的高度），因此不必離開輸入框就能選取一列 |
| `C-c` | 離開（如同在搜尋輸入框） |

輸入期間，其他每個按鍵都被忽略（`q`、`n`、`g …`、`?`、`tab` 或是輸入，或是不做任何事；在 `keymap.toml` 中重新綁定它們不會改變這一點）。已有篩選的視窗上的 `/` 會帶著它的文字重新開始輸入，游標在文字尾端。

列**隨著輸入**縮減：每次編輯都立即套用（篩選在本機進行；每按一個鍵都不會送出任何東西）。

<a id="a-kept-filter"></a>
### 保留的篩選

`enter` 之後，視窗保留它的篩選，每個按鍵都照常作用於顯示出來的列：游標按鍵、`enter`、`Z`、`g a`、`r`、佇列上的 `d`、*All tracks* 中的 `f`。此外：

- `esc`（`ClosePopup`）清除取得焦點視窗的篩選（目前它在頁面上不做任何事）
- `/` 再次編輯它
- `tab`/`backtab`、`[`/`]` 移到另一個視窗，它顯示自己的篩選或沒有篩選；回來時顯示的是這個視窗原本的樣子
- 篩選屬於頁面：在歷史紀錄中來回會保留它，重新取得的頁面（重新連線、播放清單編輯）保留它，新開啟的頁面一開始沒有篩選，搜尋頁面上的新搜尋會清除它四個視窗的篩選（它們持有新的結果）

<a id="what-matches"></a>
### 什麼算相符

當篩選的文字，依輸入的樣子（包括空格，視為一個字串：按鍵說明的規則，決策 4），不分大小寫地包含於該列至少一個欄位中，這一列就**相符**。空的或全為空白的篩選等於沒有篩選：每一列都顯示。欄位如下：

| 列 | 欄位 |
|---|---|
| 曲目（以及佇列項目、製作名單） | 標題、版本、每位藝人的名稱、專輯標題 |
| 專輯 | 標題、每位藝人的名稱、年份 |
| 播放清單 | 標題 |
| 藝人 | 名稱 |
| mix | 標題、副標題 |

大小寫以 Unicode 小寫化折疊（`RÓS` 相符於 `rós`）；重音符號不折疊（`ros` 不相符於 `Rós`，見「不在範圍內」）。因此 `hell ab` 相符於 *Hell Above*，`the veil` 相符於每一首 *Pierce The Veil* 的曲目，但 `pierce hell` 什麼都不相符（沒有單一欄位包含它），專輯的年份則以 `2012` 相符。在 *All tracks* 中，角色篩選與文字篩選都會套用：一列在它有已勾選的角色**而且**相符時才顯示。

<a id="rows-cursor-and-actions"></a>
### 列、游標與動作

- 視窗只顯示相符的列，依清單的順序。佇列頁面顯示相符的項目與它們的**佇列位置**（`▶ 2`、`7`、…），而它的 `Suggested` 列只在有建議的項目相符時才顯示
- 編輯之後，若游標所在的列仍然相符，**游標**就留在那一列；否則移到第一個相符的列。在佇列上，游標仍然是項目 ID
- **動作**：`Z`、`g a`、`r`、動作彈出視窗與 `d` 對選取的相符列的作用，與未篩選時相同（從播放清單移除，仍然是移除那一列在播放清單中的位置）
- **曲目上的 `enter`** 把**顯示出來的**清單排入佇列（決策 2）：`LoadQueue { the matching tracks, start: the selected one }`。當清單不完整時，先執行整份清單載入（0006），再把整份清單中相符的曲目排入佇列；在佇列頁面上，`enter` 仍然播放選取的項目
- **沒有相符**：視窗顯示 `No rows match "xyz"`；沒有選取的列，因此 `enter`、`Z`、`g a`、`r`、`d` 不做任何事

<a id="lists-that-load-as-you-scroll"></a>
### 隨捲動載入的清單

篩選縮減的是**已載入**的列，並且會替它載入更多（決策 3）：當視窗有篩選且它的清單不完整時，只要游標之下相符的列少於一個視窗的高度，就會要求下一頁，規則與捲動相同（0006「Lists load as you scroll」），只是以相符的列來衡量。它在每次篩選編輯、游標移動與頁面抵達之後檢查，一次一個請求，因此相符很少的篩選會一頁一頁地載入清單，直到有足夠的列相符或清單完整，期間相符的列之下會顯示 `Loading more…`。`esc`（清除篩選）會讓它停止：要求的不會比未篩選的捲動更多。整份抵達的清單（mix、一個 mix、一個電台，0011）與佇列絕不載入更多。

<a id="drawing"></a>
### 繪製

- **標題**：視窗的標題加上 ` · /<filter>` 與相符的數量：`Favorite tracks (548 · /love · 12 matches)`、`All tracks (548 · Performer · 37 hidden · /live · 1 match)`、`Queue (40 · /veil · 9 matches)`。在總數已知之前：`Tracks (/love · 3 matches)`。輸入期間，文字後面跟著游標方塊（`/love▏`），標題為粗體；輸入期間篩選為空時：`Favorite tracks (548 · /▏)`，每一列都顯示。對視窗而言太長的標題會從尾端以 `…` 截斷，與現在相同
- **沒有相符**：視窗的第一列顯示 `No rows match "xyz"`（篩選的文字以 `…` 截斷以符合寬度），清單仍在載入時，它之下顯示 `Loading more…`
- 其他東西都不動：頁面版面、清單高度（`Resize`）與其他視窗都不變

<a id="the-search-page"></a>
### 搜尋頁面

搜尋頁面上的 `/` 現在像其他各處一樣，篩選取得焦點的結果視窗（決策 1），因此 `Search` 的意思就是它在 spotify-player 中的意思。回到搜尋輸入框則維持用 `g s`（搜尋頁面上的 `SearchPage` 本來就會聚焦它的輸入框，0007）與 `tab`/`backtab`。搜尋輸入框仍然輸入 `/`。

<a id="keys-help-and-key-hints"></a>
### 按鍵說明與按鍵提示

- 按鍵說明在每個視窗區段（以及 `Queue`）中把 `/` 列為 **filter the rows**，在它不會有作用的地方（失敗或載入中的頁面）以淡色顯示。輸入期間，視窗的區段改為列出篩選的固定按鍵，如同搜尋輸入框所做的（0008）
- 0008 指令表的 `Search` 那一列變成：作用於「lists」，文字為「filter the rows」
- 按鍵提示（0013）不需要變動：沒有任何預設的按鍵序列以 `/` 開頭

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 比對（`filter::matches(row, filter) -> bool`，表格）：上述每種列類型的每個欄位；大小寫折疊（`LOVE` ~ `love`、`RÓS` ~ `rós`）；篩選是單一欄位的一個子字串（`hell ab` 相符於標題 *Hell Above*，`the veil` 相符於藝人，跨兩個欄位的 `pierce hell` 不相符）；空的與全為空白的篩選相符於所有列；不折疊重音符號
- **AC2** — 輸入按鍵（表格）：`/` 對每種視窗與佇列開始輸入，並在「作用的範圍」所說的地方不做任何事（載入中／失敗的頁面、搜尋輸入框、*Top hit*、彈出視窗、輸入提示、整份清單載入）；可列印的字元、貼上、`backspace`、`C-u`、100 個字元的上限；`enter` 保留，`esc` 清除，兩者都停止輸入；`up`/`down`/`page_up`/`page_down` 在相符的列上移動；在 `q`→`NextTrack`、`j`→`Quit` 與 `?` 都已綁定的情況下，輸入 `qj?` 會把它輸入進去且不發出任何東西；`C-c` 離開；保留的篩選上的 `/` 帶著它的文字繼續
- **AC3** — 保留的篩選（表格）：按鍵作用於顯示出來的列；`esc` 清除它（且不做其他事）；`/` 編輯它；`tab`/`backtab`/`[`/`]` 保留每個視窗的篩選；在歷史紀錄中來回會保留它；頁面重新取得會保留它；新頁面沒有篩選；新的搜尋會清除搜尋視窗的篩選
- **AC4** — 列與游標：顯示出來的列是依清單順序的相符列（有角色篩選時，兩者都套用）；編輯之後，游標留在仍然相符的列上，否則移到第一個相符的列；沒有相符 → 沒有選取的列（`enter`、`Z`、`g a`、`r`、`d` 不發出任何東西）；在佇列上，游標仍是項目 ID，且只在相符的項目上移動
- **AC5** — 對篩選後的列執行動作（表格）：相符列中第 *i* 首曲目上的 `enter` 送出 `LoadQueue { matches, start: i }`；在不完整的清單上，它執行整份清單載入，然後送出整份清單的相符列，並以選取的曲目作為 `start`；`Z`、動作彈出視窗與 `r` 作用於選取的相符列；*Remove from this playlist* 送出選取的相符列的位置；在佇列上，`enter` 送出 `PlayEntry`，`d` 送出選取的相符列的項目 ID 的 `RemoveFromQueue`
- **AC6** — 為篩選而載入（表格）：不完整的清單上的篩選，若游標之下相符的列少於一個視窗的高度，就送出一個 `More`；之後每次回覆再送一個，直到有足夠的列相符或清單完整；絕不同時有兩個；整份清單與佇列沒有；`esc` 之後，不會有超出未篩選規則的 `More`
- **AC7** — 搜尋頁面：結果視窗上的 `/` 篩選它，而不把焦點移到輸入框；`g s` 與 `tab` 仍能到達輸入框；輸入框會輸入 `/`；*Top hit* 上的 `/` 不做任何事
- **AC8** — 按鍵說明：`/` 在每個視窗區段與 `Queue` 中列為「filter the rows」（一個情況是 `Search` 被重新綁定：顯示移動後的按鍵）；在載入中或失敗的頁面上為淡色；輸入期間，區段列出篩選的固定按鍵
- **AC9** — 繪製（`insta`，由人工審閱，加上 `contains`）：80 × 24 時，帶有保留篩選的收藏曲目、輸入期間的同一個視窗、沒有相符、篩選後的佇列（位置與 `Suggested`）、帶有角色篩選與文字篩選的 *All tracks*、篩選後的搜尋視窗；40 × 12 時篩選後的視窗；上述的標題文字；在設有篩選的情況下，從 1 × 1 到 200 × 60 都不 panic（0004 AC17 的測試，新增列）
- **AC10** — 文件：`docs/tui.md` 有「Filtering a list」與新的 `/` 列，`docs/config.md` 的指令表有 `Search` 的新文字，它們的 zh-TW 副本與之相符，`README.md` 與 `README.zh-TW.md` 提到篩選；0007 與 0008 關於 `/` 的文字更新為指向這裡

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 在已載入 40 000 列的清單上輸入篩選 | 比對在每次編輯或頁面抵達時對已載入的列執行一次，而不是每繪製一列就執行一次；輸入保持流暢（驗收時手動檢查） |
| 篩選在很長的不完整清單上沒有任何相符 | 頁面一次一個地載入，直到清單完整（`Loading more…`）；`429` 或其他失敗顯示為最後一列（0006），載入就停在那裡，直到游標移動；`esc` 讓它停止 |
| 中斷連線 | 篩選已載入的列仍可運作；不送出 `More`（0005）；重新連線時，載入像捲動一樣繼續 |
| 篩選時佇列有變動（一首曲目結束、自動播放加入） | 顯示出來的項目依新的快照重新計算；游標若仍相符就保留它的項目，否則移到第一個相符的 |
| 篩選時某一列有變動（播放清單編輯重新取得頁面） | 篩選保留並套用到新的列；游標依 AC4 的規則 |
| 含有換行的貼上 | 控制字元被丟掉（如輸入提示與搜尋輸入框所做的） |
| 篩選文字比標題能顯示的更長 | 標題以 `…` 截斷；篩選仍完整套用 |
| 空視窗上的 `/` | 開始輸入；一旦輸入了東西就顯示 `No rows match "x"`（篩選為空時，顯示空清單本身的訊息） |
| 終端機少於 8 列 | 不繪製任何頁面（0004）；篩選保留在模型中，輸入期間按鍵仍會編輯它 |
| 連上的 TUI | 篩選是客戶端自己的：除了一般的 `More` 與指令之外，沒有任何東西經過 socket |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名。紅燈是針對函式本體為存根的存根函式的斷言失敗（`matches` 回傳 `true`、`/` 維持目前的行為、標題忽略篩選），沒有 `todo!()`，也沒有編譯錯誤。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/filter/tests.rs` :: `ac1_matches` (table: row × filter → bool) | 欄位、大小寫、單一子字串、空值、重音符號 | 存根 `matches` 對每一列都回傳 `true` |
| AC2 | `crates/core/src/ui/filter/tests.rs` :: `ac2_typing_keys` (table), `ac2_typing_ignores_keymap`, `ac2_slash_where_it_does_nothing` | 輸入、編輯、enter/esc、移動、重新綁定、繼續 | 存根的 `/` 做它目前做的事（在搜尋頁面之外什麼都不做） |
| AC3 | `crates/core/src/ui/filter/tests.rs` :: `ac3_kept_filter` (table) | esc 清除、`/` 編輯、每個視窗、歷史紀錄、重新取得、新頁面、新搜尋 | 存根：從不保留任何篩選 |
| AC4 | `crates/core/src/ui/filter/tests.rs` :: `ac4_rows_and_cursor` (table, a browse window and the queue) | 顯示的列、角色加文字篩選、游標規則、沒有相符 | 存根 `matches` 回傳 `true` |
| AC5 | `crates/core/src/ui/filter/tests.rs` :: `ac5_acting_on_matches` (table) | 相符列的 `LoadQueue`（完整與整份清單）、`Z`、動作、電台、播放清單移除的位置、佇列的 `PlayEntry`/`RemoveFromQueue` | 存根 `matches` 回傳 `true`：整份清單被排入佇列 |
| AC6 | `crates/core/src/ui/filter/tests.rs` :: `ac6_loads_for_filter` (table) | 依規則送出的 `More`；沒有兩個同時進行；整份清單／佇列沒有；`esc` 之後沒有 | 存根：篩選編輯時不送出 `More` |
| AC7 | `crates/core/src/ui/search/tests.rs` :: `ac8_windows_and_scroll` (0007's, `/` row changed to this spec) + `crates/core/src/ui/filter/tests.rs` :: `ac7_search_page` | `/` 篩選，焦點不動；`g s`/`tab` 到達輸入框 | 存根 `/` 聚焦輸入框（目前的行為） |
| AC8 | `crates/core/src/ui/help/tests.rs` :: `ac7_help_keys_and_texts`, `ac7_help_dim_rows` (0008's, rows changed) + `ac8_filter_in_help` | 文字、淡色、重新綁定、輸入區段 | 存根的說明文字為「back to the search input」 |
| AC9 | `crates/app/src/tui/render.rs` :: `ac9_filter_*` (snapshots), `ac9_filter_titles` (table), `ac17_no_panic_any_size` / `ac23_no_panic_any_size` (new rows) | 標題、列、沒有相符的文字、各種大小 | 存根的繪製忽略篩選 |
| AC10 | `crates/app/tests/docs.rs` :: `ac10_filter_documented` + zh-TW copies and README reviewed at acceptance | `tui.md`、`config.md`、兩份 zh-TW 副本都提到 `/` 與「Filtering a list」 | 文件尚未變動 |

驗收時手動檢查：在真實的終端機中，對有幾千列的收藏曲目輸入篩選，清單縮減時不卡頓；什麼都不相符的篩選會把清單載入到盡頭並顯示 `No rows match`；篩選後的曲目上的 `enter` 會播放相符的曲目。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::ui`：新的 `filter` 模組：`Filter { text: String, typing: bool }`、`matches(Row, &str)`（佇列頁面使用項目的曲目列）、輸入按鍵（由 `dispatch::fixed_key` 呼叫的固定按鍵處理常式）、`Search` 與 `ClosePopup` 頁面指令的新意義，以及對相符列的接近尾端規則（在 `browse` 中）。`Window` 新增 `filter: Filter` 與它的 `visible()`（顯示的列）併入兩種篩選；相符的索引會被快取，只在列或篩選改變時重新計算。佇列頁面的篩選放在它的 `Page` 上（它沒有視窗）。`help` 的文字與輸入區段。沒有 I/O，沒有新的相依套件
- `tidal-player`：`ui.rs` 與 `ui/pages.rs` 中的標題、沒有相符的列，以及佇列頁面篩選後的列
- `xtask layering`：沒有變動

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-10，取自程式碼）：`Search` 綁定在 `/`，只呼叫 `search::focus_input`（`crates/core/src/ui/dispatch.rs` 的 `page_command`；`keymap::defaults`）；`ClosePopup` 在頁面上不做任何事；角色篩選縮減 `Window::visible`，而 `Window::row`、`selected`、`tracks` 與 `len` 都讀它，`play_from` 從 `window.cursor` 起把 `window.tracks()` 排入佇列，兩者都是針對顯示出來的列；`near_end` 把游標與 `Window::len`（顯示出來的列）比較，且只在游標移動與角色篩選之後執行；佇列頁面沒有 `Window`（`Page::new(Queue)` 沒有，`render_queue` 繪製快照）；按鍵說明的篩選按鍵與文字（`help::key`）；搜尋輸入框的固定按鍵（`search::input_key`）。

已驗證（2026-10-10，spotify-player 的 README「Commands」）：`Search` 是「open a popup for searching in the current page」，預設為 `/`；`SearchPage` 是「go to the search page」，`g s`；它的提示說 `Search` 在「shortcut help page and other pages」中有效。排序是獨立的指令（`SortTrackByTitle` `s t`、…）。

假設：沒有任何關於外部系統的假設（沒有 API 變動：篩選在本機進行，`More` 是 0006 的請求）。

<a id="decisions-answered-by-the-user-2026-10-10"></a>
## 決策（使用者已回覆，2026-10-10）

1. **搜尋頁面上的 `/` 做什麼**：*已回答：照提案*：像其他各處一樣篩選取得焦點的結果視窗（spotify-player 中 `Search` 的意思）；`g s` 與 `tab` 回到輸入框。替代方案：在搜尋頁面上保留「回到搜尋輸入框」，只在其他地方篩選
2. **篩選後的曲目上的 `enter` 排入佇列什麼**：*已回答：照提案*：相符的曲目，從選取的那一首開始（所見即所播；角色篩選的先例）。替代方案：整份清單，從選取的曲目開始（spotify-player 播放整個 context）
3. **隨捲動載入的清單**：*已回答：照提案*：篩選涵蓋已載入的列，並在相符的列太少時一頁一頁地載入更多頁面，直到有足夠的列相符或清單完整。替代方案：篩選只涵蓋已載入的列，自己不載入任何東西（捲動到相符列的盡頭才像現在一樣載入）
4. **比對**：*已回答：替代方案*：整個篩選是單一欄位的一個子字串，不分大小寫（按鍵說明的規則）；不折疊重音符號。（原提案：詞之間為 AND，每個詞可在任何欄位）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-10"></a>
## 實作備註（規格沒有說明的地方所做的選擇，2026-10-10）

- **顯示的列被快取**在 `Window` 上（私有的 `shown`，兩種篩選都已套用），在文字篩選被編輯時（`set_filter`，它也套用 AC4 的游標規則）、套用角色篩選時（`set_roles`），以及列抵達或被遺忘時（`append`、`append_mixes`、`reset`）由 `Window::refilter` 重新計算。沒有啟用的文字篩選時不快取任何東西，`visible()` 仍是角色篩選的結果。`row`、`len`、`index` 與標題以 O(1) 讀取快取，因此繪製時不會逐列比對。`Window` 仍然衍生 `Clone`/`PartialEq`（快取是其他欄位的函式）
- **佇列的篩選**放在佇列的 `Page` 上（`Page::filter`，其他頁面不使用）。它的相符結果不快取（每次呼叫 `filter::queue_shown`：每個按鍵一次、每個畫面一次）；游標仍是項目 ID，`filter::queue_selected` 只在游標的項目相符時才是游標（`enter`、`d`、動作彈出視窗與按鍵說明的淡色顯示都讀它），而佇列在篩選時，新的快照會執行 AC4 的游標規則
- **它在按鍵中的位置**：`dispatch::fixed_key` 依序檢查彈出視窗、輸入提示、搜尋輸入框，然後是正在輸入的篩選（整份清單載入期間絕不）。輸入狀態是取得焦點視窗的 `filter.typing`（或佇列的），因此輸入期間 `?` 不能開啟說明；`help::help` 對這樣的狀態仍然顯示輸入區段（標題：視窗的區段標題，列：`enter`、`esc`、`backspace`、`C-u`、`up  down  page_up  page_down`、`C-c`）
- **按鍵說明**：`/`（`Search`）位於每個視窗區段中 `RoleFilter` 之後（指令表的順序）以及 `Queue` 中；*Top hit* 不再列出它。`esc` 在頁面上的新意義記載於 `docs/tui.md` 與 `docs/config.md`，但 `ClosePopup` 不會加到視窗區段中（它的說明文字維持「close / cancel」）
- **全為空白的篩選上的 `enter`** 會停止輸入並清除它（它不算篩選，所以標題中不保留任何東西）
- **列號**：篩選後的瀏覽視窗從 1 開始為顯示出來的列編號，如同角色篩選本來就這麼做；佇列保留佇列位置（規格）
- **為篩選而載入**：篩選編輯不會重試最後一個 `More` 失敗的視窗（邊界情況的「直到游標移動」；游標移動會像之前一樣重試）。頁面抵達時，會對歷史紀錄中任何頁面上有篩選的視窗檢查規則；完成整份清單載入的那次抵達則不檢查（由載入本身處理）。`esc` 讓進行中的請求仍然抵達，而它的抵達不會再要求更多
- **搜尋頁面**：*Top hit* 曲目上的 `Enter` 會把 *Tracks* 顯示的列排入佇列，因此 *Tracks* 上保留的篩選也套用在那裡（所見即所播）
- **繪製**：沒有相符的列是 `No rows match "<text>"`，文字以 `…` 截斷以符合該列，然後整列再依寬度截斷；其下是 `Loading more…` 或視窗的失敗訊息。佇列的標題同樣以 `…` 截斷，輸入期間為粗體；瀏覽視窗的標題在取得焦點時本來就是粗體
- **測試**：計畫中的 0007 測試是 `ac8_windows_and_scroll`，0008 的是 `ac7_help_sections`（它們在程式碼中的名稱）；`ac9_titles_in_model`（在 `filter/tests.rs` 中）在模型中檢查標題文字，與應用程式的 `ac9_filter_titles` 並行。由規格驅動的測試變更：0007 的 `ac7_*` 新搜尋案例與 0008 的 `ac6_text_inputs_first` 改用 `g s` 而不是 `/` 來到達搜尋輸入框；0008 的 `ac8_help_moves` 計入佇列區段的新列。快照有變動：`ac14_help_queue`、`ac14_help_library_albums`、`ac14_help_custom_keymap`、`ac14_help_40x12`（新的 `/  filter the rows` 列）
- **紅燈存根**：一個存根 `filter.rs` 服務每個測試：`matches` 保留每一列，`title_parts` 不回傳任何東西，`/` 仍呼叫舊的 `search::focus_input`，而輸入按鍵沒有接線

<a id="out-of-scope"></a>
## 不在範圍內

- 排序視窗（spotify-player 的 `Sort*` 指令）
- 不分重音符號的比對（需要 Unicode 正規化，一個新的相依套件）
- 從篩選搜尋 Tidal（搜尋頁面做這件事）、跨欄位逐詞比對、正規表示式、模糊比對
- 篩選彈出視窗（動作彈出視窗、播放清單選擇器）與播放視窗
- 跨執行記住篩選（頁面不會被持久化，0009）
