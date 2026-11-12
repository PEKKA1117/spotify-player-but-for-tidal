[English](../0022-playlist-editing.md) | 正體中文

<a id="0022--editing-your-playlists"></a>
# 0022 — 編輯你的播放清單

- **狀態**：implemented（2026-11-05）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#66](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/66)（資料夾移到 [#87](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/87)，CLI 移到 [#88](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/88)）
- **相依於**：0006（implemented：頁面、動作彈出視窗、播放清單寫入與 ETag 規則）、0008（implemented：按鍵對應、略過清單）、0012（implemented：篩選）、0020（implemented：選取）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md)（「Favorites and playlists」、新的「Editing your playlists」一節、按鍵表）與 [`docs/config.md`](../../zh-TW/config.md)（新指令），以及它們的 zh-TW 版本（AC13）

<a id="context"></a>
## 背景

0006 只能在為了加入曲目的途中建立播放清單（*Add to playlist… → New playlist…*），而且描述永遠是空的，並把重新命名、描述、設為公開與重新排序列為不在範圍內。要維持播放清單的整齊，仍然得去開另一個 Tidal 應用程式。spotify-player 有 `CreatePlaylist`（`N`）與 `MovePlaylistItemUp`/`Down`（`C-k`/`C-j`）；在這裡這些名稱放在按鍵對應的略過清單中，所以從 spotify-player 複製來的按鍵對應會忽略它們。

tidalt 完全沒有這些。

<a id="behaviour"></a>
## 行為

<a id="creating-a-playlist"></a>
### 建立播放清單

在音樂庫頁面（它的任何視窗）按 `N`，會在 *New playlist…* 的單列提示中先詢問名稱，再詢問描述：

1. `Playlist name: `：`Enter` 繼續，`Esc` 取消。修剪後為空白的名稱會關閉提示且不送出任何東西，與現在相同
2. `Description (optional): `：`Enter` 建立播放清單（描述可以是空的），`Esc` 取消，且不建立任何東西

播放清單是私人的。訊息說 `Created <title>`，並重新取得音樂庫。

*Add to playlist… → New playlist…* 採用相同的兩個步驟，然後如 0006 一樣加入曲目。

在其他任何地方按 `N` 什麼都不做。

<a id="editing-a-playlist"></a>
### 編輯播放清單

你自己的播放清單（音樂庫的 **Playlists** 視窗中的一列）的動作彈出視窗，在 *Play next* 之後、*Delete playlist* 之前新增：

| 動作 | 作用 |
|---|---|
| *Rename…* | 提示 `Playlist name: `，內含目前的標題。`Enter` 儲存，`Esc` 取消。修剪後為空白或沒有改變的名稱會關閉提示且不送出任何東西 |
| *Edit description…* | 提示 `Description: `，內含目前的描述。`Enter` 儲存，空的文字會清除描述，`Esc` 取消。沒有改變的文字不送出任何東西 |
| *Make public* / *Make private* | 適用的那一個：立即變更，不詢問 |

訊息：`Renamed <new title>`、`Saved the description of <title>`、`<title> is now public`、`<title> is now private`。音樂庫與該播放清單已開啟的頁面會重新取得。

收藏（追蹤）的播放清單，以及搜尋結果中的播放清單列，都不提供這些（0006、0007）。

<a id="moving-a-track"></a>
### 移動曲目

在你自己的播放清單頁面的曲目視窗中：

- `C-k` 把游標所在的曲目上移一列，`C-j` 下移一列。游標留在該曲目上
- 列立即移動。播放器在背景被告知（見「按住按鍵」）
- 在清單第一列按 `C-k`、最後一列按 `C-j` 什麼都不做
- 在還有更多要載入的清單的最後一個已載入的列按 `C-j` 不會移動它：它會載入下一頁，與那裡的 `j` 相同。再按一次才會移動該列
- 選取的標記留在它們的曲目上：被標記的列在它自己或它的鄰居移動時保有標記
- 佇列不變，不論它是從哪裡填入的（0005：佇列屬於播放器，並持有自己的副本）

被拒絕時，會顯示訊息且不移動任何東西：

| 當 | 訊息 |
|---|---|
| 視窗有保留的篩選 | `Clear the filter to move tracks` |
| 動作模式開啟 | `Leave motion mode to move tracks` |

正在輸入篩選時，按鍵屬於篩選（0012）。在其他任何地方，包括收藏的播放清單的頁面，這些按鍵什麼都不做。

<a id="holding-the-key"></a>
#### 按住按鍵

一次移動需要前一次移動回傳的 ETag，所以同一時間只能有一次移動在進行。按住按鍵時，列會繼續在畫面上移動，中間的移動會被合併：

- 沒有移動在進行時，按一次按鍵就送出一次
- 有移動在進行時，對同一首曲目的按鍵只在畫面上移動該列。回應抵達時，一個請求把曲目從 Tidal 目前所在的位置移到畫面所顯示的位置。若是同一個位置，就什麼都不送
- 有移動在進行時，對不同曲目的按鍵什麼都不做，也沒有訊息，直到移動都完成為止

離開頁面不會停止已經按下的移動：其餘的仍會送出。成功的移動不顯示訊息，也不重新取得頁面：畫面已經是對的，下一次移動使用回應的 ETag。

<a id="how-it-looks"></a>
### 外觀

播放清單頁面的標題，在公開的播放清單上多一個 `public`，不論擁有者是誰：

```
Late night · 39 tracks · 2:41:07 · public
```

有描述的播放清單把它顯示在標題下方的一列，以 `…` 截斷到頁面的寬度，而視窗往下移一列：這一列與它的規則同 mix 的副標題（0011），所以太短而放不下的頁面會省略它。描述中的換行以空格顯示。沒有描述的播放清單沒有這一列。

<a id="keymap"></a>
### 按鍵對應

`CreatePlaylist`（`N`）、`MovePlaylistItemUp`（`C-k`）與 `MovePlaylistItemDown`（`C-j`）離開略過清單，成為指令，列在按鍵說明中（連同它們作用的頁面）與 `examples/keymap.toml` 中。

<a id="what-the-player-fetches-and-sends"></a>
### 播放器取得與送出什麼

| 變更 | 請求 | 回應 |
|---|---|---|
| 建立 | `POST /users/{user}/playlists` 表單 `title`、`description` | `201`，該播放清單 |
| 移動 | `POST /playlists/{uuid}/items/{from}` 表單 `toIndex`，`If-None-Match: <etag>` | `200`，新的 ETag 在 `ETag` 標頭中 |
| 重新命名、描述 | `POST /playlists/{uuid}` 表單 `title` 和／或 `description`（只含有變更的） | `200`，空的 |
| 公開、私人 | `PUT https://api.tidal.com/v2/playlists/{uuid}/set-public`、`…/set-private`，`countryCode` 照常 | `200`，空的 |

`toIndex` 是曲目的**最終**位置，以 Tidal 的整份播放清單計算。列的位置來自 `Window::positions`，而不是它的列號，因為影片項目從清單中被捨棄，但在 Tidal 上仍然佔有它們的位置（0006 AC21）。把曲目移過它的鄰居，要求的是鄰居的位置，不論中間隔著哪些影片。

從 `p` 移到 `q` 之後，客戶端依 Tidal 的方式更新位置：往下移時，`(p, q]` 中的項目各減一；往上移時，`[q, p)` 中的項目各加一；該曲目取得 `q`。

<a id="protocol"></a>
### 協定

`tidal_player_core::library`：

- `LibraryRequest::CreatePlaylist { title, description }`（新增 `description`）
- `LibraryRequest::MovePlaylistItem { uuid, from, to, etag }` → `LibraryResponse::Moved { etag: Option<String> }`
- `LibraryRequest::EditPlaylist { uuid, title: Option<String>, description: Option<String> }` → `Done`
- `LibraryRequest::SetPlaylistPublic { uuid, public }` → `Done`
- `PlaylistSummary` 新增 `description: Option<String>`（空的時候是 `None`）與 `public: bool`

客戶端與播放器是同一個版本（0005 的問候），所以不需要維持相容。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — `MovePlaylistItem` 把 `toIndex` 作為表單欄位，連同 `If-None-Match`，POST 到 `/playlists/{uuid}/items/{from}`，以回應的 `ETag` 回應 `Moved`，遇到 `412` 時在一次請求後以 `The playlist changed: nothing was changed, try again` 失敗，不重試
- **AC2** — `EditPlaylist` 向 `/playlists/{uuid}` POST 的恰好是給定的欄位，不帶 `If-None-Match`
- **AC3** — `SetPlaylistPublic` 在 v2 基底上向 `/v2/playlists/{uuid}/set-public` 或 `/set-private` 送出 `PUT`，帶有 `countryCode`，並在空的 `200` 上回應 `Done`
- **AC4** — `PlaylistSummary` 從播放清單列表與播放清單標頭帶有 `description`（空字串是 `None`）與 `public`；`CreatePlaylist` POST 它所拿到的描述
- **AC5** — 建立：在任何音樂庫視窗中按 `N` 開啟名稱提示，然後是描述提示，然後只送出一次 `CreatePlaylist`；在任一步驟按 `Esc` 都不送出任何東西；空白的名稱關閉提示且不送出任何東西；標題會被修剪；*New playlist…* 採用相同的步驟，並在之後加入它的曲目；在其他任何頁面按 `N` 什麼都不做
- **AC6** — 編輯動作：自己的播放清單列在 *Play next* 與 *Delete playlist* 之間列出 *Rename…*、*Edit description…*，以及 *Make public*、*Make private* 其中恰好一個；收藏的播放清單與搜尋的列都不列出
- **AC7** — 編輯提示：它們開啟時帶著目前的文字；`Enter` 只以有變更的欄位送出 `EditPlaylist`；空白的名稱或沒有改變的文字什麼都不送；空的描述送出 `description: Some("")`；*Make public*/*Make private* 立即送出 `SetPlaylistPublic`；每次成功都給出它的訊息，並重新取得音樂庫與該播放清單已開啟的頁面
- **AC8** — 移動：在自己的播放清單的曲目視窗中，`C-j`/`C-k` 與鄰居交換該列，游標留在該曲目上，而 `MovePlaylistItem` 帶著曲目的位置與鄰居的位置（來自 `Window::positions`，包括它們之間的影片）以及頁面的 ETag；之後 `Window::positions` 與 Tidal 的順序一致；第一列上移與最後一列下移什麼都不做；還有更多要載入的最後一個已載入的列會載入下一頁而不移動
- **AC9** — 合併：有移動在進行時，對該曲目的後續按鍵只移動該列；回應送出一個請求，從 Tidal 的位置到畫面的位置，帶著回應的 ETag，若兩者相等則不送；對另一首曲目的按鍵什麼都不做；離開頁面後移動仍然繼續；完成的移動不顯示訊息、不取得任何東西，並讓頁面持有最後的 ETag
- **AC10** — 失敗的移動：尚未送出的移動被捨棄，錯誤顯示在訊息列，該播放清單已開啟的頁面重新取得，`412` 與任何其他失敗都一樣。沒有 ETag 的 `Moved` 會重新取得頁面
- **AC11** — 選取與拒絕：被標記的列在移動中保有它的標記，不論它是被移動的列還是鄰居；在有保留篩選的視窗中，以及在動作模式中，按鍵顯示它們的訊息且不改變任何東西；在收藏的播放清單的頁面與任何其他頁面上，它們什麼都不做
- **AC12** — 繪製：公開的播放清單的頁面標題以 `· public` 結尾；描述佔一列，以 `…` 截斷，換行以空格顯示；沒有描述時版面不變；描述提示繪製它的標籤
- **AC13** — 按鍵對應與文件：`CreatePlaylist`、`MovePlaylistItemUp`、`MovePlaylistItemDown` 是預設綁定到 `N`、`C-k`、`C-j` 的指令，不在略過清單上，並出現在按鍵說明與 `examples/keymap.toml` 中；`docs/tui.md`、`docs/config.md`、`README.md` 與它們的 zh-TW 版本涵蓋它們

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 與播放器失去連線 | `write` 既有的行為：該列不移動，也不排入任何東西 |
| 播放清單在移動之前於別處被改變 | `412`：AC10 的訊息，頁面重新載入，曲目回到 Tidal 所在的位置 |
| 播放清單在別處縮短，而該位置已不存在 | `400`/`1002` `Invalid indices`：與任何失敗的移動相同（AC10） |
| 同時在別處重新命名 | Tidal 對重新命名、描述或公開不檢查 ETag：最後的寫入勝出，沒有任何警告。在 `docs/tui.md` 中說明 |
| 只有一首曲目的播放清單 | 兩個按鍵都什麼都不做 |
| 同一首曲目在播放清單中出現兩次 | 列以位置區分，所以各自獨立移動 |
| 非常長的標題或描述 | Tidal 接受了 300 與 1 000 個字元，所以這裡不設限制。提示顯示文字的結尾（0006） |
| 在這裡編輯含有換行的描述 | 提示只有一行：換行變成空格，儲存時寫入的就是那樣。儲存沒有改變的文字不送出任何東西，所以除非編輯了文字，否則換行會保留 |
| 提示或彈出視窗開啟時按 `N` | 文字進入提示；彈出視窗保留它們自己的按鍵（選取彈出視窗中的 `N` 屬於規格 0020） |
| 移動期間工作階段過期 | `LoginRequired` 的訊息，與任何失敗的移動相同（AC10） |
| 移動還在完成時在曲目上按 `Enter` | 依畫面上的順序（客戶端的列）播放清單 |

<a id="test-plan"></a>
## 測試計畫

| AC | 測試 | 斷言 | 預期的紅燈失敗 |
|---|---|---|---|
| AC1 | `crates/api/tests/library.rs` :: `ac1_move_item_sends_to_index_and_etag`, `ac1_move_item_answers_the_new_etag`, `ac1_move_item_412_is_not_retried` | 路徑、表單、標頭、`Moved.etag`、`412` 時只有一次請求 | `LibraryRequest::MovePlaylistItem` 沒有被處理（在紅燈 commit 中存根為 `unimplemented` 錯誤） |
| AC2 | `crates/api/tests/library.rs` :: `ac2_edit_playlist_sends_only_given_fields`（表格：標題、描述、兩者、空描述） | 表單本文、沒有 `If-None-Match` | 請求沒有送出 |
| AC3 | `crates/api/tests/library.rs` :: `ac3_set_public_uses_the_v2_base`（表格：公開、私人） | `PUT`、路徑、`countryCode`、空本文時的 `Done` | 請求沒有送出 |
| AC4 | `crates/api/tests/library.rs` :: `ac4_summary_carries_description_and_public`, `ac4_create_sends_the_description` | 來自錄製的列表與標頭的欄位；表單本文 | `description` 一直是 `None`，`public` 一直是 `false` |
| AC5 | `crates/core/src/ui/browse/tests.rs` :: `ac5_create_from_the_library`, `ac5_create_cancelled_or_blank`（表格）, `ac5_new_playlist_asks_the_description`, `ac5_n_elsewhere_does_nothing` | 提示、一次請求、之後加入曲目 | `N` 什麼都不做 |
| AC6 | `crates/core/src/ui/browse/tests.rs` :: `ac6_actions_of_a_playlist`（表格：自己的私人、自己的公開、收藏、搜尋） | 動作清單與順序 | 缺少這些動作 |
| AC7 | `crates/core/src/ui/browse/tests.rs` :: `ac7_edit_prompts`（表格）, `ac7_public_at_once`, `ac7_success_messages_and_refetch` | 預先填入、請求、訊息、重新取得 | 提示沒有開啟 |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_move_swaps_and_keeps_the_cursor`, `ac8_move_over_videos_uses_positions`, `ac8_ends_of_the_list`, `ac8_last_loaded_row_loads_more`；`crates/core/src/ui/page.rs` tests :: `ac8_positions_after_a_move`（表格） | 列、游標、請求、位置 | 按鍵什麼都不做 |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_presses_merge_into_one_request`, `ac9_back_to_where_it_was_sends_nothing`, `ac9_other_track_waits`, `ac9_moves_continue_after_leaving`, `ac9_settled_move_is_silent` | 每次回應送出的請求、使用的 ETag、不重新取得 | 每次按鍵一個請求 |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_failed_move_reloads`（表格：`412` 訊息、其他錯誤）, `ac10_no_etag_reloads` | 訊息、重新取得、待處理的移動被捨棄 | 失敗後留下樂觀（optimistic）的順序 |
| AC11 | `crates/core/src/ui/browse/tests.rs` :: `ac11_marks_follow_a_move`（表格：被移動的列、鄰居）, `ac11_refused_with_a_filter_or_in_motion`, `ac11_not_an_own_playlist` | 標記、訊息、什麼都不送 | 標記留在列號上 |
| AC12 | `crates/app/src/tui/render.rs` tests :: `ac12_playlist_title_public`, `ac12_description_row`, `ac12_description_cut_and_flattened`, `ac12_no_description_unchanged`, `ac12_description_prompt`（80 × 24 與 150 × 42 的快照） | 繪製出的頁面 | 缺少新的快照 |
| AC13 | `crates/core/src/ui/keymap/tests.rs` :: `ac13_playlist_commands_bound`, `ac13_off_the_skip_list`；`crates/core/src/ui/help/tests.rs` :: `ac13_keys_help_lists_them`；`examples/keymap.toml` 由既有的 `crates/app/tests/examples.rs`（0008 AC18）檢查 | 綁定、略過清單、說明 | 這些指令被略過並帶有警告 |
| AC13（文件） | 驗收時閱讀：沒有測試能判斷文字敘述。`cargo xtask wiki` 建立它們 | | |

**手動操作檢查**（真實的 Tidal，一個用完即刪的播放清單）：帶描述的 `N`；重新命名；編輯並清除描述；設為公開與私人；在十列上按住 `C-j`，然後重新載入頁面並比較順序；在含有影片的播放清單上做同樣的事；在另一個 Tidal 應用程式中編輯播放清單之後再移動（`412`）。`C-j` 與 `C-k` 在 tmux 內與直接使用時都要檢查，因為終端機可能把 `C-j` 讀成 `Enter`。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`library`（型別）、`ui`（按鍵、提示、移動 state、位置：全是純函式）
- `tidal-player-api`：`library.rs`（請求）。客戶端有一個以 `/v1` 結尾的基底 URL：v2 呼叫使用路徑 `../v2/playlists/…`，由 `Url` 解析，所以測試不需要第二個基底就能連到 mock 伺服器
- `tidal-player`：`ui.rs`、`ui/pages.rs`（標題與描述列）

<a id="facts-vs-assumptions"></a>
## 事實與假設

2026-11-05 以一個含三首曲目、用完即刪的播放清單對真實 API 探測（已確認 `404`）：

- **移動**：`toIndex` 是最終位置。`ABC` 把 0 移到 1 得到 `BAC`；`BAC` 把 0 移到 2 得到 `ACB`；`ACB` 把 2 移到 0 得到 `BAC`。超過尾端時限制在最後一個位置（`ACB` 把 0 移到 99 得到 `CBA`）。同一個位置是 `200`。可以給多個索引（`CBA` 把 `0,1` 移到 3 得到 `ACB`），這裡沒有使用
- **移動的錯誤**：沒有 ETag 或過期的 ETag：`412`，`subStatus` 為 `7002`。不存在的位置：`400`，`subStatus` 為 `1002`，`Invalid indices`
- **移動的 ETag**：`200` 的 `ETag` 標頭等於下一次 `GET` 的
- **編輯**：單獨的 `title` 保留描述，單獨的 `description` 保留標題。`description=` 會清除它。`title=` **被忽略而且沒有錯誤**（`200`，標題不變）。全是空格的標題**照原樣儲存**。CJK 與表情符號都保留。300 與 1 000 個字元都被接受；沒有找到限制
- **編輯的 ETag**：**不檢查**：沒有 ETag 與帶著過期 ETag 的重新命名都套用了
- **公開**：`PUT /v2/playlists/{uuid}/set-public` 與 `set-private` 回應 `200`，本文是空的，也沒有 ETag；ETag 同樣不檢查。列表立即顯示該變更
- **列表**：`playlistsAndFavoritePlaylists` 的列帶有 `description` 與 `publicPlaylist`

在實機檢查中確認（2026-11-05，一個私人播放清單，含一首曲目、一部影片與 14 首曲目，用完即刪）：

- 移過被隱藏的影片遵守位置規則：上移與下移越過它，畫面上的順序與重新載入後的順序相同
- 一次按十下 `C-j`，結束時的位置與畫面所顯示的相同，重新載入之後也是
- `C-j` 與 `C-k` 在 tmux 內能送達播放器。tmux 之外留給維護者檢查：這次執行沒有別的終端機

<a id="decisions"></a>
## 決策

1. **列立即移動，請求合併**（而不是在移動進行時忽略按鍵）：按住按鍵時保持流暢，長距離的移動只花幾個請求，因為 Tidal 一個請求就能移動任何距離
2. **完成的移動不取得任何東西**：回應的 ETag 與本地的位置就夠了，而取得會在按住按鍵時重設清單。失敗是唯一會重新載入的情況
3. **名稱提示中的 `Enter` 在兩條路徑上都進入第二個提示**：issue 要求從音樂庫與選擇器都有選用的描述；一個流程比兩個更容易學，而略過它只需要一個按鍵
4. ***Make public* 不詢問**：它在動作彈出視窗中只有兩步之遙，並可用同一個動作還原。`confirm_removals` 涵蓋無法還原的事
5. **這裡拒絕空白的名稱**：Tidal 默默忽略空的名稱，並儲存全是空格的名稱，所以客戶端否則會回報一次沒有發生的重新命名，或造成一個無法閱讀的列
6. **在篩選下拒絕移動**：畫面上相鄰的不是播放清單中相鄰的，所以無法從顯示的內容預測結果
7. **不為重新命名做假的保護**：Tidal 在那裡不檢查 ETag，所以文件說最後的寫入勝出，而不是送出一個沒有作用的 ETag

<a id="out-of-scope"></a>
## 不在範圍內

- 播放清單資料夾（#87）與 CLI 播放清單指令（`playlist new|delete|fork|import`，#88）
- 一次移動多個選取的列（API 允許；0020 的選取按鍵需要設計）
- 從播放清單自己的頁面編輯：那裡的動作屬於列，而 spotify-player 的 `ShowActionsOnCurrentContext` 仍然略過
- 編輯播放清單圖片、協作者
- 排序清單（#52）：到時候，排序過的視窗會像篩選過的一樣拒絕移動
