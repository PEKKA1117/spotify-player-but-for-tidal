[English](../0008-keymap-and-config.md) | 正體中文

<a id="0008--keymap-config-files-and-the-keys-help"></a>
# 0008 — 按鍵對應、設定檔與按鍵說明

- **狀態**：implemented（2026-10-08；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0003–0007（implemented：本規格移入檔案的每一項設定與按鍵）
- **使用者文件**：新增 [`docs/config.md`](../../zh-TW/config.md)（兩個檔案、每一項設定、每一個指令、按鍵語法）；[`docs/tui.md`](../../zh-TW/tui.md) 的「按鍵」標明每個按鍵的指令，並新增按鍵說明；[`docs/playback.md`](../../zh-TW/playback.md) 的「設定」與 [`docs/daemon.md`](../../zh-TW/daemon.md) 指向 `app.toml`（AC17）

<a id="context"></a>
## 背景

設定只來自環境變數（0003–0007 的「設定」，共十一個變數），所以常駐程式的設定要寫成 systemd unit 裡的 `Environment=` 行（0005）；按鍵則是寫死的 `match`，散在 `tidal_player_core::ui` 的 `key_press`、`browse_key`、`popup_key`、`whole_list_key` 與 `search::window_key` 之中。0003 以來每一份規格都承諾過這一份：設定用 **`app.toml`**（變數仍然有效），按鍵用 **`keymap.toml`**，採用 spotify-player 的格式，讓 `[[keymaps]]` 區塊可以直接複製過來；另外（0006、已捨棄的 0012 草稿、使用者 2026-10-08）還有**按鍵說明**彈出視窗（`?`），由同一組綁定產生，因此不會與實際不符。

按鍵說明依照使用者的要求（2026-10-08，沿用自 0012 草稿）：這是 lazygit 版的 spotify-player 說明：**目前所在位置**的按鍵排最前面，其次是全域按鍵，可用 `/` 篩選，`Enter` 執行反白的按鍵。

<a id="what-tidalt-did"></a>
### tidalt 的做法

取自 tidalt 的 `internal/ui/appconfig.go`、`keymap.go`、`help.go`（`ee9c24e`、`f66b3f0`、`ee0070e`）：

- `~/.config/tidalt/app.toml`（只管版面：一個預設組合與幾個開關）與 `keymap.toml`（仿照 spotify-player 的 `[[keymaps]]`，含 `command`、`key_sequence`），另有一個 `preset` 鍵用來選整組 tidalt 或 spotify-player 的按鍵
- **未知的設定與指令是錯誤**（`unknown setting "…"`、`keymaps[3]: unknown command "…"`），但檔案損壞時**程式不會停止**：它退回該預設組合的按鍵，並在 UI 中顯示錯誤。常駐程式沒有 UI，所以錯誤只進記錄檔，使用者看到的是預設值，卻不知道原因
- 沒有 `[[actions]]`，沒有參數（`VolumeChange { offset }`），播放設定（音質、裝置）還留在環境變數裡，所以同一個程式有兩個地方在設定
- 說明彈出視窗由按鍵對應產生，但在每個地方都顯示所有按鍵，包括在目前檢視中沒有作用的（0012 草稿）
- 預設組合的切換讓 `app.toml` 改變了 `keymap.toml` 的意義（`ee0070e` 修正了一組沒有跟著版面預設組合走的按鍵）：兩個檔案互相依賴

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及涵蓋它的驗收條件

1. **打錯字被默默忽略**（`volum_step = 10` 沒有作用；tidalt 抓到了這點）→ 未知的設定、指令、動作與按鍵都是錯誤，訊息指出檔案、項目與問題（AC2、AC10）
2. **無頭的常駐程式忽略損壞的檔案**（tidalt：寫進記錄檔，使用預設值）→ 每一種模式遇到損壞的檔案都拒絕啟動，結束碼 2，且在動到終端機或 socket 之前（決策 2）（AC12）
3. **說明或文件說謊**（列出了沒有作用的按鍵，或漏列了）→ 分派、說明與文件都讀同一份按鍵對應；一個測試把每個綁定都跑過分派，另一個檢查每個指令都有文件（AC3、AC17）
4. **重新綁定一個按鍵就無法打字**：`q` 被重新綁定後，在輸入提示或搜尋輸入框中仍然必須能打出 `q` → 文字輸入框先於按鍵對應接收可列印的按鍵（AC6）
5. **永遠無法觸發的序列**：單獨綁定 `g` 會讓 `g g`、`g l` 等在 spotify-player 中無法觸達（第一個完全相符的就勝出）→ 一個序列是另一個序列的前綴時是錯誤，訊息指出兩者（決策 4）（AC2）
6. **同一項設定有兩個資料來源**：環境變數、旗標與檔案 → 一套優先順序，旗標 > 環境變數 > 檔案 > 預設值，逐項設定測試（AC11）
7. **重構造成行為改變**：把按鍵搬進表格不能改變任何按鍵的作用 → 0004–0007 的按鍵測試維持不變並保持通過（AC3）

<a id="behaviour"></a>
## 行為

<a id="where-the-files-are"></a>
### 檔案位置

**設定目錄**是 `--config-folder <DIR>`（`-c`，spotify-player 的旗標，每個指令都有），否則是 `$TIDAL_PLAYER_CONFIG_DIR`，否則是 `$XDG_CONFIG_HOME/tidal-player`，否則是 `~/.config/tidal-player`（0001 的「命名」）。其中放 `app.toml` 與 `keymap.toml`；兩者都可以不存在（使用預設值），目錄也可以不存在。絕不會在那裡寫入任何東西：不建立檔案，不傾印預設值。空的變數視同未設定。

檔案在**啟動時讀取一次**，由每個行程讀取自己那一部分：播放器（獨立模式的 TUI、常駐程式、`play`）讀播放器設定，TUI（獨立模式或連上的）讀客戶端設定與按鍵對應。常駐程式在重新啟動時（`systemctl --user restart tidal-player`）才會讀到修改過的檔案；沒有即時重新載入。

<a id="apptoml"></a>
### `app.toml`

扁平的鍵，每項既有設定一個，加上 spotify-player 的音樂庫版面。每個鍵都是選用的。

| 鍵 | 可接受的值 | 預設值 | 取代（仍會讀取，且優先於檔案） | 讀取者 |
|---|---|---|---|---|
| `quality` | `"hi-res"`、`"lossless"`、`"high"` | `"hi-res"` | `--quality`、`TIDAL_PLAYER_QUALITY` | 播放器 |
| `output_device` | 任何 ALSA PCM 名稱 | `"default"` | `--device`、`TIDAL_PLAYER_DEVICE` | 播放器 |
| `volume_step` | 整數 1–25（%） | `5` | `TIDAL_PLAYER_VOLUME_STEP` | TUI |
| `seek_duration_secs`（spotify-player 的名稱） | 整數 1–600 | `5` | `TIDAL_PLAYER_SEEK_STEP` | TUI |
| `previous_restart_secs` | 整數 0–60 | `3` | `TIDAL_PLAYER_PREVIOUS_RESTART` | 播放器 |
| `autoplay` | `true`、`false` | `false` | `--autoplay`、`TIDAL_PLAYER_AUTOPLAY` | 播放器 |
| `release_paused_secs` | 整數 0–3600，或 `"never"` | `10` | `TIDAL_PLAYER_RELEASE_PAUSED` | 播放器 |
| `page_size` | 整數 1–10 000 | `100` | `TIDAL_PLAYER_PAGE_SIZE` | 播放器、TUI |
| `search_page_size` | 整數 1–1000 | `20` | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` | 播放器、TUI |
| `hide_versions` | 字串陣列（`[]` 表示不隱藏任何東西） | 0006 的十六個詞 | `TIDAL_PLAYER_HIDE_VERSIONS` | 播放器 |
| `[layout] library = { playlist_percent, album_percent }`（spotify-player 的） | 各為整數 1–98，總和最多 99；*Artists* 取剩下的 | `40`、`40` | （原本固定，0006） | TUI |

```toml
# ~/.config/tidal-player/app.toml
quality = "lossless"
output_device = "hw:1,0"
seek_duration_secs = 10
release_paused_secs = "never"
hide_versions = ["instrumental", "karaoke"]

[layout]
library = { playlist_percent = 30, album_percent = 50 }
```

- **優先順序**：旗標 > 環境變數 > `app.toml` > 預設值，逐項設定（決策 5）。`hide_versions` 沿用 0006 的比對方式（轉成小寫，並去除空格、連字號、底線、句點與 `+`）
- **錯誤**（結束碼 2，什麼都不啟動；決策 2）：
  - TOML 語法：`<path>: line 3, column 9: <parser message>`
  - 任何位置的未知鍵：`<path>: unknown setting "volum_step"`
  - 型別或範圍錯誤：`<path>: invalid volume_step: expected an integer from 1 to 25, got 0`（字串寫成 `got "x"`）；訊息的讀法與變數的（0004 的「設定」）相同，只是以鍵取代變數
  - 檔案存在但無法讀取（權限、是目錄）：`<path>: <io error>`
- 每個行程都驗證**整個**檔案，不論它讀的是哪一部分：TUI 按鍵裡的拼字錯誤也會讓常駐程式失敗，因此重新啟動一次就能看到

<a id="keymaptoml"></a>
### `keymap.toml`

spotify-player 的格式：項目**新增或取代**預設綁定。

```toml
# ~/.config/tidal-player/keymap.toml
[[keymaps]]
command = "NextTrack"
key_sequence = "g n"          # g n now also skips; n still does

[[keymaps]]
command = "None"
key_sequence = "q"            # q no longer quits (C-c still does)

[[keymaps]]
command = { SeekForward = { duration = 30 } }
key_sequence = "L"

[[keymaps]]
command = { VolumeChange = { offset = 1 } }
key_sequence = "="

[[actions]]
action = "GoToAlbum"
key_sequence = "g B"
target = "PlayingTrack"       # or "SelectedItem" (the default)
```

- **按鍵序列**（spotify-player 的語法）：按鍵之間以單一空格分隔。一個按鍵是單一字元（`a`、`A` 代表 Shift-a、`>`、`?`），或一個名稱：`enter`、`space`、`tab`、`backtab`、`backspace`、`esc`、`left`、`right`、`up`、`down`、`insert`、`delete`、`home`、`end`、`page_up`、`page_down`、`f1`–`f12`；可加前綴 `C-`（Control）或 `M-`（Alt）：`C-s`、`M-enter`、`C-space`。`C-S` 就是 `C-s`（終端機回報 Control 加字母時不分大小寫）。其他一律是錯誤，訊息指出該按鍵：`<path>: keymaps[2]: unknown key "ctrl+s" in "ctrl+s"`
- **`[[keymaps]]`**：`key_sequence` 與 `command`；`command` 是一個名稱、`"None"`（移除該序列原本綁定的東西），或給有參數的指令用的單一項目的行內表格。一個項目只取代**相同**序列的預設綁定；指令保留它的其他序列（如上，`n` 仍然是下一首）。後面的項目優先於前面的
- **`[[actions]]`**：`key_sequence`、`action`，以及選用的 `target`（`"SelectedItem"`，預設，或 `"PlayingTrack"`）：不開啟彈出視窗，直接在選取的列或正在播放的曲目上執行動作彈出視窗（0006 的「動作」）中的那一項。在彈出視窗不會列出該項的列上（在藝人上用 *Go to album*、在正在播放的曲目上用 *Play next*），什麼都不做
- **錯誤**（結束碼 2，什麼都不啟動）：TOML 語法與未知欄位與 `app.toml` 相同；未知的指令或動作（`<path>: keymaps[4]: unknown command "NxtTrack"`）；錯誤的參數（`<path>: keymaps[5]: VolumeChange offset must be from -25 to 25 and not 0, got 40`）；空的序列；檔案所綁定的兩個序列之間的**前綴衝突**：`<path>: "s" is bound to Search and is the start of "s l a" (LibraryPage); unbind those with command = "None" first`（決策 4）。檔案中的序列若與**預設**衝突，則改為捨棄該預設並顯示一則通知（決策 4，經 0019 修訂）
- **本播放器沒有的 spotify-player 指令**（`PlayRandom`、`SwitchTheme`、`SwitchDevice`、`BrowseUserPlaylists`、`SortTrackByTitle`……，清單在 `docs/config.md`；`OpenLogs` 隨 [0027](0027-logs-page.md) 離開了這份清單）：該項目被略過，TUI 的訊息列在啟動時說一次，`keymap.toml: 3 spotify-player commands not supported here: PlayRandom, TopTrackPage, SwitchTheme`（決策 3）。因此 spotify-player 的 `keymap.toml` 可以原封不動複製過來

<a id="commands-and-their-default-keys"></a>
### 指令與它們的預設按鍵

預設值就是 0004–0007 的按鍵，再加上 spotify-player 對 `?`/`C-h`、`C-n`/`C-p`、`end` 與 `/` 的預設值（本專案原本沒有這些）。行為相同時沿用 spotify-player 的名稱，否則用新名稱（標 *）。

| 指令 | 預設按鍵 | 作用範圍 | 說明文字 |
|---|---|---|---|
| `ResumePause` | `space` | 所有地方 | play / pause |
| `NextTrack` | `n` | 所有地方 | next track |
| `PreviousTrack` | `p` | 所有地方 | previous track |
| `SeekForward { duration? }` | `>` | 所有地方 | seek forward (seconds; default `seek_duration_secs`) |
| `SeekBackward { duration? }` | `<` | 所有地方 | seek backward |
| `SeekStart` | `^` | 所有地方 | back to the start of the track |
| `Shuffle` | `C-s` | 所有地方 | shuffle on / off |
| `Repeat` | `C-r` | 所有地方 | repeat: off → queue → track |
| `ToggleAutoplay`* | `A` | 所有地方 | autoplay on / off |
| `VolumeUp`*、`VolumeDown`* | `+`、`-` | 所有地方 | volume up / down by `volume_step` |
| `VolumeChange { offset }` | — | 所有地方 | volume by `offset` % (−25…25, not 0) |
| `Mute` | `_` | 所有地方 | mute / unmute |
| `AddToQueuePrompt`*、`PlayNextPrompt`* | `o`、`O` | 所有地方 | add a link or ID to the end of the queue / to play next |
| `SelectNextOrScrollDown` | `j`、`down`、`C-n` | 清單、彈出視窗 | move down |
| `SelectPreviousOrScrollUp` | `k`、`up`、`C-p` | 清單、彈出視窗 | move up |
| `PageSelectNextOrScrollDown` | `C-f`、`page_down` | 清單、彈出視窗 | move down a window |
| `PageSelectPreviousOrScrollUp` | `C-b`、`page_up` | 清單、彈出視窗 | move up a window |
| `SelectFirstOrScrollToTop` | `g g` | 清單、彈出視窗 | move to the top |
| `SelectLastOrScrollToBottom` | `G`、`end` | 清單、彈出視窗 | move to the last loaded row |
| `ChooseSelected` | `enter` | 清單、彈出視窗 | play the track with its list / open / run |
| `AddSelectedItemToQueue` | `Z`、`C-z` | 清單 | add to the end of the queue |
| `RemoveFromQueue`* | `d` | 佇列 | remove from the queue |
| `ShowActionsOnSelectedItem` | `g a`、`C-space` | 清單 | actions on the selected row |
| `ShowActionsOnCurrentTrack` | `a` | 所有地方 | actions on the playing track |
| `FocusNextWindow`、`FocusPreviousWindow` | `tab`、`backtab` | 頁面 | next / previous pane |
| `NextTab`*、`PreviousTab`* | `]`、`[` | 有分頁的窗格 | next / previous tab |
| `RoleFilter`* | `f` | 藝人的 *All tracks* | the role filter |
| `Queue` | `z` | 所有地方 | the queue page |
| `LibraryPage` | `g l` | 所有地方 | the library |
| `LikedTrackPage` | `g y` | 所有地方 | favorite tracks |
| `SearchPage` | `g s` | 所有地方 | the search page (on one: its input) |
| `Search` | `/` | 清單 | filter the rows (spotify-player's in-page search: [0012](0012-filter.md); until 0012, "back to the search input" on the search page only) |
| `PreviousPage` | `backspace`、`C-q` | 所有地方 | back |
| `ClosePopup` | `esc` | 彈出視窗、輸入提示、載入中 | close / cancel |
| `OpenCommandHelp` | `?`、`C-h` | 所有地方 | this help |
| `Quit` | `q`、`C-c` | 所有地方 | quit (an attached TUI detaches) |

給 `[[actions]]` 用的動作（0006 的彈出視窗項目；沿用 spotify-player 的名稱，本專案新增的標 *）：`GoToAlbum`、`GoToArtist`（第一位藝人；彈出視窗會逐一列出）、`AddToQueue`、`PlayNext`*、`AddToLiked`、`DeleteFromLiked`、`AddToPlaylist`（開啟播放清單選擇器）、`DeleteFromPlaylist`（自己的播放清單頁）、`RemoveFromQueue`*（佇列項目）、`DeletePlaylist`*（自己的播放清單，仍然會問 `y/n`）。spotify-player 沒有對應項目的動作（`GoToRadio`、`CopyLink`、`ToggleLiked`、`Follow`……）會像不支援的指令一樣被略過。

<a id="key-sequences-and-where-keys-act"></a>
### 按鍵序列與按鍵在哪裡有作用

- **序列**（spotify-player 的規則，如 0006 對 `g` 的處理）：按下的按鍵會累積；當累積的內容不是任何綁定的開頭時，重新以最後一個按鍵單獨開始累積（`g x` 的作用與 `x` 相同）；當它等於某個綁定時，該綁定執行，並重新開始累積。沒有逾時。序列可以是任意長度（`s l a`）
- **按鍵在哪裡有作用**：綁定只在它的指令有作用的地方有作用（「作用範圍」欄）；其他地方什麼都不做（就像現在不在佇列上的 `d`）。彈出視窗沿用 0006 的規則，「只有它自己的按鍵有作用」：在彈出視窗中，清單指令、`ChooseSelected`、`ClosePopup` 與 `OpenCommandHelp` 透過按鍵對應而有作用
- **固定按鍵**（不在按鍵對應中，決策 6）：文字輸入框（`o`/`O` 提示、搜尋輸入框、播放清單名稱提示）先於按鍵對應接收可列印的字元與貼上，並帶有 0004/0006/0007 的編輯按鍵（`Backspace`、`C-u`、`Enter`、`Esc`，以及搜尋輸入框中的 `Tab`/`BackTab`；`C-c` 離開，而在搜尋輸入框中 `C-q` 回到上一頁）；角色篩選的 `Space` 與確認詢問的 `y`/`n`

<a id="the-keys-help-"></a>
### 按鍵說明（`?`）

`OpenCommandHelp`（`?`、`C-h`）在除了文字輸入框以外的任何地方開啟它。置中，佔頁面區域的 80 %（至少 40 × 10，並裁切到終端機大小），標題為 `Keys`：

```
┌Keys────────────────────────────────────────────────────┐
│ Library · Playlists                                    │
│   enter            open the playlist                   │
│   Z  C-z           add to the end of the queue         │
│ Lists                                                  │
│   j  down  C-n     move down                           │
│   k  up  C-p       move up                            │
│   …                                                    │
│ Pages                                                  │
│   z                the queue page                      │
│   g l              the library                         │
│ Playback                                               │
│   space            play / pause                        │
│ App                                                    │
│   ?  C-h           this help                           │
│   q  C-c           quit                                │
│ / filter · enter run · esc close                       │
└────────────────────────────────────────────────────────┘
```

- **顯示的按鍵**是實際生效的按鍵（套用 `keymap.toml` 之後），以該檔案的語法書寫（`C-s`、`backtab`、`space`），因此說明中顯示的內容可以直接貼進檔案。`[[actions]]` 綁定列在 `Actions` 之下
- **區段**：開啟中彈出視窗的（從彈出視窗開啟時），接著是焦點視窗的（`Queue`、`Library · Playlists`、`Album`、`Artist · All tracks`、`Search · Tracks`……），然後是 `Lists`（在清單視窗上）、`Pages`、`Playback`、`Actions`、`App`。一個區段列出它所屬情境的綁定；目前不會有作用的（佇列為空時的播放按鍵、失去連線時）以淡色顯示，不隱藏。沒有按鍵的指令不列出
- **移動**：清單指令，透過按鍵對應，只在綁定列之間移動（跳過標題）
- **篩選**（lazygit 的 `/`，這裡是固定按鍵）：輸入的字元會保留按鍵、指令名稱或說明文字含有它們的列（不分大小寫）；空的區段會消失；`Backspace` 刪除；`Enter` 結束輸入並保留篩選；`Esc` 清除。沒有符合的：`No keys match "xyz"`
- **執行**（lazygit 的 `Enter`）：在某一列上按 `Enter` 會關閉說明並執行該綁定，與在開啟說明的檢視中按它的按鍵完全相同
- **關閉**：`Esc`（沒有篩選時）、`?`、`q` 關閉它且沒有任何效果。開啟期間其他按鍵都沒有作用

<a id="the-library-layout"></a>
### 音樂庫版面

`[layout] library` 設定音樂庫頁面的視窗寬度（0006：40 / 40 / 20）：*Playlists* 為 `playlist_percent`，*Albums* 為 `album_percent`，*Artists* 取剩下的，在並排繪製時（內側 60 欄或以上；低於這個寬度則是一個視窗，與現在相同）。

<a id="acceptance-criteria"></a>
## 驗收條件

按鍵對應（`tidal_player_core::ui::keymap`，純函式；這裡沒有 TOML：它接收已經反序列化的項目）：

- **AC1** — 按鍵語法（表格）：每個按鍵名稱、單一字元（`A`、`>`、`?`、`-`）、`C-`/`M-` 搭配字元與名稱、`C-S` = `C-s`、多按鍵序列都能解析；`""`、`"ctrl+s"`、`"C-"`、`"X-a"`、兩個空格、結尾的空格都是錯誤，訊息指出該按鍵。每個解析後的序列都能印回原本的寫法（說明與文件使用的標籤）
- **AC2** — 由預設值與項目建立按鍵對應（表格）：預設值等於「指令與它們的預設按鍵」表；一個項目新增一個序列；與預設序列相同的項目取代它；`None` 移除它；後面的項目優先於前面的；參數（`VolumeChange` 在範圍內、超出範圍、0；`SeekForward` 有與沒有 `duration`，`duration` 為 0 與 601 被拒絕）；`[[actions]]` 的兩種目標與預設值；未知的指令或動作 → 帶有該項目索引的錯誤；沒有對應項目的 spotify-player 指令或動作 → 略過，並列在結果的通知清單中；前綴衝突（已綁定 `g`；`s` 與 `s l a`）→ 指出兩個序列的錯誤；用 `None` 移除衝突的預設後即為有效；取消綁定每個 `Quit` 按鍵 → `Quit has no key left`
- **AC3** — 分派經過按鍵對應：`key_press`、彈出視窗處理器與搜尋視窗按鍵都在 `State` 的按鍵對應中查找按鍵。0004–0007 的按鍵測試在預設按鍵對應下**維持不變**並保持通過；一個表格測試在指令有作用的狀態下執行每個預設綁定，並檢查其效果等於直接執行該指令
- **AC4** — 重新綁定（表格、模型）：`NextTrack` 綁在 `g n` → `g n` 送出 `Next`，`n` 仍然如此；`q` 綁 `None` → `q` 什麼都不做，`C-c` 離開；`ResumePause` 移到 `M-p` → `M-p` 切換，`space` 什麼都不做；綁定 `s l a` → `s` 等待，`s l a` 執行，`s x` 的作用與 `x` 相同；`VolumeChange { offset = 1 }` 送出 `ChangeVolume(1)`；`SeekForward { duration = 30 }` 送出 `SeekBy(30000)`
- **AC5** — `[[actions]]`（表格）：每個動作搭配 `SelectedItem`，在每種列上送出的內容，等於在動作彈出視窗中選擇該項所送出的；搭配 `PlayingTrack` 時，等於用 `a` 開啟的彈出視窗所送出的；在彈出視窗不會列出它的列上，什麼都不送；沒有播放任何東西且為 `PlayingTrack` 時，什麼都不送
- **AC6** — 文字輸入框先於按鍵對應（表格）：在 `q`→`NextTrack`、`j`→`Quit`、`?` 已綁定的情況下，在 `o` 提示、搜尋輸入框與播放清單名稱提示中輸入 `qj?`，打出的是 `qj?` 且不送出任何東西；它們的編輯按鍵、角色篩選的 `Space` 與確認詢問的 `y`/`n`，不論按鍵對應怎麼寫都有作用

按鍵說明（`tidal_player_core::ui::help`，純函式）：

- **AC7** — 內容（`help(&State) -> Vec<HelpSection>`，涵蓋各情境的表格：佇列頁面；音樂庫與各個視窗的焦點；收藏的曲目；專輯；自己的與追蹤的播放清單；藝人與它的四個視窗；搜尋的輸入框與各個視窗；每個開啟的彈出視窗）：區段標題依序排列，每個區段恰好是其情境的綁定與它們實際生效的按鍵（一個案例使用自訂按鍵對應：顯示移動後的按鍵、移除的按鍵消失、`[[actions]]` 區段存在），在指令不會有作用的地方（佇列為空、失去連線）顯示淡色列
- **AC8** — 按鍵（表格）：`?` 與 `C-h` 從每個頁面與彈出視窗開啟它，並在每個文字輸入框中輸入；清單指令只在綁定列之間移動，並有邊界限制；`/` 依按鍵、指令名稱與說明文字篩選，不分大小寫，`Backspace` 編輯，`Enter` 保留，`Esc` 清除，第二次 `Esc` 關閉；在某一列上按 `Enter` 會關閉說明，並送出與在原本的檢視中按該綁定的按鍵所送出的完全相同的內容（`space` → `Send(TogglePause)`、`g l` → 推入並取得音樂庫、`[[actions]]` 的列 → 它的動作）；`?`/`q` 關閉且沒有任何效果；其他按鍵都沒有作用

設定檔（`tidal-player::config`，對字串與假的環境運作的純函式）：

- **AC9** — 設定目錄（表格）：旗標；`TIDAL_PLAYER_CONFIG_DIR`；空的變數被忽略；`XDG_CONFIG_HOME`；只有家目錄；旗標優先於變數
- **AC10** — `app.toml`（表格）：每個鍵在邊界值以及各超出一格的值、錯誤的型別、`"never"`、`hide_versions = []`、版面的邊界（1、98、總和 99 與 100）；未知的頂層鍵與 `[layout]` 下的未知鍵；指出行與欄的語法錯誤；空檔案 = 預設值。每則錯誤訊息如「錯誤」之下所列
- **AC11** — 優先順序（表格，逐項設定）：預設值；檔案；環境變數優先於檔案；旗標優先於兩者（`--quality`、`--device`、`--autoplay`）；空的變數會落到檔案；即使檔案有效，無效的變數仍是錯誤。`resolve_player_config` 讓 0003–0007 的測試維持不變並保持通過

- **AC12** — 載入（`assert_cmd`，暫存的設定目錄）：缺少目錄或檔案時以預設值執行；損壞的 `app.toml` 或 `keymap.toml` 會讓 `tidal-player daemon`、`tidal-player play …` 與 `tidal-player` 以結束碼 2 結束，訊息寫在 stderr，在綁定 socket 之前、在進入 raw 模式之前（測試的 stderr 是乾淨的文字，沒有跳脫序列）；含有不支援的 spotify-player 指令的 `keymap.toml` 可以啟動，且 TUI 的第一個畫面顯示通知（通知清單 → `State::message` 的模型測試）

TUI（`tidal-player`）：

- **AC13** — 按鍵解碼（`input.rs`，表格）：`M-` 按鍵、`left`、`right`、`home`、`end`、`insert`、`delete`、`f1`–`f12` 都能解碼（目前 Alt 與 F 鍵會被丟棄）；0004–0007 解碼過的一切仍然以相同方式解碼
- **AC14** — 繪製（`insta`，80×24，以肉眼檢視，加上 `contains`）：佇列頁面上的說明、*Albums* 為焦點的音樂庫上的說明、以 `fav` 篩選的說明、從動作彈出視窗開啟的說明、使用自訂按鍵對應的說明；40×12 保留按鍵欄，並以 `…` 截斷說明文字。`playlist_percent = 30, album_percent = 50` 的音樂庫。從 0×0 到 120×40 都不會 panic（0004 AC17 的測試，新增列）
- **AC15** — 串接：TUI 的 `State` 從檔案取得它的按鍵對應、間隔、頁面大小與音樂庫版面（`tui_state`，表格：預設值、檔案、變數優先於檔案）；常駐程式的 `PlayerSettings` 來自 `app.toml`（0005 AC14 測試框架的測試播放器，其 `quality`、`release_paused_secs` 與 `page_size` 來自暫存設定目錄中的檔案）
- **AC16** — 連上常駐程式的 TUI 若使用與常駐程式不同的 `keymap.toml`，則使用它自己的：按鍵只屬於客戶端（兩個客戶端連同一個測試播放器，其中一個重新綁定了 `n`）

文件：

- **AC17** — `docs/config.md` 記載兩個檔案、每個鍵、每個指令與動作及其預設按鍵、按鍵語法、錯誤，以及不支援的 spotify-player 名稱；`docs/tui.md` 的「按鍵」標明每個按鍵的指令並記載按鍵說明；`docs/playback.md` 的「設定」與 `docs/daemon.md` 指向 `app.toml`（unit 的 `Environment=` 行仍然有效）。一個測試檢查按鍵對應中的每個指令與動作都帶著預設按鍵出現在 `docs/config.md` 中。`CLAUDE.md` 的「Status」點名本規格；本規格連結到它們

預設檔案（2026-10-08 依使用者要求新增，「gen a default keymap and app toml」）：

- **AC18** — `examples/app.toml` 把每個 `app.toml` 鍵都設為預設值，且在沒有旗標或變數時解析它，得到的恰好是預設值；`examples/keymap.toml` 對每個預設綁定有一個 `[[keymaps]]` 項目（「指令與它們的預設按鍵」表，相同的序列與指令，沒有其他東西），並建立出預設按鍵對應。把其中任一個複製到設定目錄不會改變任何東西，因此它們是可供編輯的起點。`docs/config.md` 與 `README.md` 連結它們

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 沒有設定目錄 | 預設值；不建立任何東西 |
| `app.toml` 是目錄，或無法讀取 | 結束碼 2，`<path>: <io error>` |
| 含有 UTF-8 BOM 或 CRLF 換行的檔案 | 依 TOML 允許的方式讀取（`toml` crate 兩者都接受）；各一列測試 |
| `keymap.toml` 中相同序列出現兩次 | 後面的項目勝出；不是錯誤 |
| 沒有綁定任何按鍵的指令 | 無法觸達，不在說明中；不是錯誤 |
| 每個 `Quit` 按鍵都被取消綁定 | 拒絕：`<path>: Quit has no key left`（結束碼 2），讓 TUI 永遠可以離開 |
| `?` 被綁到別的東西，`OpenCommandHelp` 未綁定 | 只要 `Quit` 還有按鍵就允許；說明無法觸達 |
| 終端機無法送出的按鍵（許多終端機的 `C-enter`） | 接受；永遠不會觸發。`docs/config.md` 註明哪些不可靠 |
| `C-i`、`C-m`、`C-[` | 終端機送出的是 `tab`、`enter`、`esc`；有記載，不特別處理 |
| 在小於 40 × 10 的終端機中開啟說明 | 裁切到現有大小；不會 panic（AC14） |
| 連上的 TUI 與常駐程式讀不同的設定目錄 | 各用各的：設定是依「檔案位置」所列，以行程為單位 |
| `--config-folder` 指向不存在的目錄 | 預設值（與不存在的預設目錄相同）；不是錯誤 |
| 環境變數與檔案不一致 | 環境變數勝出，不出聲（它是明確的、每次執行的選擇） |

<a id="test-plan"></a>
## 測試計畫

每個測試以它的條件命名（`ac4_…`）。紅燈是對存根型別與函式（本體為存根，沒有 `todo!()`，沒有編譯錯誤）的斷言失敗，與 0001–0007 相同。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/keymap/tests.rs` :: `ac1_key_syntax`（表格） | 解析、錯誤、標籤來回轉換 | 存根解析器只接受單一字元 |
| AC2 | `crates/core/src/ui/keymap/tests.rs` :: `ac2_build_keymap`（表格） | 預設值、合併、錯誤、通知 | 存根 `build` 回傳預設值並忽略項目 |
| AC3 | `crates/core/src/ui/mod.rs`、`browse/tests.rs`、`search/tests.rs` 中既有的按鍵測試（不變）+ `crates/core/src/ui/keymap/tests.rs` :: `ac3_every_binding_dispatches` | 舊行為；綁定 = 指令 | 存根 `run_command` 什麼都不做，所以涵蓋測試失敗（舊測試維持通過：它們是重構的安全網，不是它的紅燈） |
| AC4 | `crates/core/src/ui/keymap/tests.rs` :: `ac4_rebinding`（表格） | 自訂按鍵對應下每個按鍵的效果 | 存根分派忽略 state 的按鍵對應，使用預設值 |
| AC5 | `crates/core/src/ui/keymap/tests.rs` :: `ac5_action_bindings`（表格） | 與彈出視窗相同的效果 | 存根動作綁定什麼都不做 |
| AC6 | `crates/core/src/ui/keymap/tests.rs` :: `ac6_text_inputs_first`（表格） | 輸入的文字，沒有效果 | 存根在輸入框之前查找按鍵對應 |
| AC7 | `crates/core/src/ui/help/tests.rs` :: `ac7_help_sections`（表格）、`ac7_help_custom_keymap`、`ac7_help_keys_and_texts`、`ac7_help_dim_rows` | 標題、順序、成員、按鍵、淡色 | 存根回傳一個扁平的區段 |
| AC8 | `crates/core/src/ui/help/tests.rs` :: `ac8_help_keys`（表格）、`ac8_help_moves`、`ac8_help_filter`、`ac8_help_runs_row`、`ac8_help_runs_every_row`、`ac8_help_no_other_key_acts`、`ac8_help_text_inputs` | 開啟、移動、篩選、執行、關閉 | 存根 `?` 什麼都不做 |
| AC9 | `crates/app/src/config.rs` :: `ac9_config_dir`（表格） | 解析出的目錄 | 存根永遠給出 `~/.config/tidal-player` |
| AC10 | `crates/app/src/config.rs` :: `ac10_app_toml`（表格）、`ac10_load_app_toml` | 值與訊息 | 存根忽略未知鍵 |
| AC11 | `crates/app/src/config.rs` :: `ac11_precedence`（表格）+ `crates/app/src/settings.rs` :: `ac25_player_config`（不變） | 各來源的勝出值 | 存根讓檔案優先於環境變數 |
| AC12 | `crates/app/tests/config.rs` :: `ac12_broken_config_exits_2`、`ac12_missing_config_runs`、`crates/core/src/ui/keymap/tests.rs` :: `ac12_unsupported_notice`、`ac12_notice_after_welcome`、`crates/app/tests/config.rs` :: `ac12_unsupported_keymap_starts` | 結束碼、stderr、通知 | 存根載入器忽略檔案 |
| AC13 | `crates/app/src/tui/input.rs` :: `ac18_key_events`（新增列） | 解碼的按鍵 | 存根丟棄 Alt 與 F 鍵（目前的程式碼） |
| AC14 | `crates/app/src/tui/render.rs` :: `ac14_help_80x24`（每列一個快照）、`ac14_help_40x12`、`ac14_library_percentages`、`ac17_no_panic_any_size`（新增列） | `contains` + 快照 | 存根繪製不畫彈出視窗，並維持 40/40/20 |
| AC15 | `crates/app/src/main.rs` :: `ac15_tui_state_from_config`（表格）+ `crates/app/tests/daemon.rs` :: `ac15_daemon_reads_app_toml` | 來自檔案的 state 與播放器設定 | 存根 `tui_state` 忽略設定 |
| AC16 | `crates/app/tests/daemon.rs` :: `ac16_keymap_is_per_client` | 各客戶端的 `n` | 存根送出預設按鍵對應的指令 |
| AC17 | `crates/app/tests/docs.rs` :: `ac17_every_command_documented` + 驗收時檢視 | `docs/config.md` 中的名稱與預設按鍵 | 存根 `docs/config.md` 缺少這些指令 |
| AC18 | `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults`、`ac18_example_keymap_is_the_defaults` | 每個鍵都存在且等於預設值；每個預設綁定一個項目，建立出 `Keymap::default()` | 範例檔案提交時是空的 |

驗收時在使用者的機器上手動檢查（結果寫在 PR 說明中）：複製一份真正的 spotify-player `keymap.toml`，啟動時帶著通知，且它支援的綁定可以運作；常駐程式以設定了 `output_device` 與 `quality` 的 `app.toml` 重新啟動後，以該音質播放到該裝置；`M-` 與 F 鍵能送達使用者終端機中的 TUI。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::ui`：`keymap`（按鍵語法、`KeySequence`、帶參數的 `UiCommand`、`ActionBinding`、預設值、帶通知的 `build(entries) -> Result<Keymap, KeymapError>`、查找與前綴比對、標籤；項目型別 derive `Deserialize`，讓二進位檔把 TOML 直接反序列化進去）、`help`（區段、篩選、說明彈出視窗的 state 與按鍵）；`Key` 新增 `Alt`、`Left`、`Right`、`Home`、`End`、`Insert`、`Delete`、`F(u8)`；`State` 持有按鍵對應與待決的序列（取代 `pending_g`）。沒有 I/O，沒有新的相依套件
- `tidal-player`：`config`（目錄解析、讀取兩個檔案、`app.toml` 解析與優先順序）、`-c/--config-folder` 旗標、`input.rs`（新按鍵）、說明與版面的繪製；`toml` 加入 `[workspace.dependencies]`（最新的 1.x）且只加入二進位檔
- `xtask layering`：不變

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-08，來自程式碼）：

- spotify-player（`docs/config.md`、`spotify_player/src/key.rs`、`command.rs`、`config/keymap.rs`、`event/mod.rs`，master，2026-10-08 讀取）：檔案位於 `$HOME/.config/spotify-player`，`-c/--config-folder`；`[[keymaps]]` 為 `{command, key_sequence}`，`command = "None"` 移除，有參數的指令寫成 `{ VolumeChange = { offset = 1 } }`、`{ SeekForward = { duration = 10 } }`、`{ SeekBackward = { } }`；`[[actions]]` 為 `{action, key_sequence, target}`，搭配 `PlayingTrack`/`SelectedItem`；按鍵語法為 `C-`/`M-` + `enter space tab backtab backspace esc left right up down insert delete home end page_up page_down f1…f12` 或一個非空格字元；序列比對：當累積的按鍵不是任何綁定的前綴時，以最後一個按鍵重新開始，並由第一個完全相符的執行（所以綁定了 `g` 就會隱藏 `g …`，而它不會回報這一點）。它的預設按鍵對應是上表中名稱與按鍵的來源；`layout.library = { playlist_percent, album_percent }`，預設 40/40；`seek_duration_secs` 是它的跳轉設定
- tidalt（見上）：TOML 檔案，未知名稱是錯誤，損壞的檔案退回預設值並在 UI 中顯示錯誤
- 本儲存庫：十一個變數與它們的範圍（`crates/app/src/cli/play.rs` 的 `resolve_with`，0003–0007）；按鍵在 `key_press`（`crates/core/src/ui/mod.rs`）、`browse_key`、`popup_key`、`whole_list_key`（`browse.rs`）、`search::input_key`/`window_key` 中分派；Alt 組合鍵與 F 鍵被 `input::key_to_action` 丟棄；音樂庫的 40/40/20 分割在 `crates/app/src/tui/render/pages.rs`
- `toml` 1.1.7 是最新發行版（`cargo info toml`，2026-10-08）

假設，驗收時手動檢查：使用者的終端機實際會送出哪些 `C-`/`M-` 按鍵（crossterm 的解碼有測試，終端機沒有）。

<a id="decisions-answered-by-the-user-2026-10-08-14-as-proposed-then-58-lgtm"></a>
## 決策（使用者於 2026-10-08 回答：1–4 照提案，5–8 為「lgtm」）

1. **範圍**：*回答：照提案*：本規格只處理設定檔、按鍵對應與按鍵說明；spotify-player 的頁內篩選（任何頁面上的 `/` 彈出視窗）與排序（`s t`、`s a`……），0006 與 0007 曾承諾給「0008」，移到自己的規格（0012，「Filter and sort」），透過本按鍵對應綁定。替代方案：全部放在 0008
2. **損壞的檔案**：*回答：照提案*：在每種模式下，在任何東西啟動之前以結束碼 2 結束（常駐程式無法顯示 UI 內的錯誤；環境變數已經這樣運作）。替代方案：tidalt 的做法（以預設值執行，在 TUI 中顯示錯誤，在常駐程式中寫入記錄檔）
3. **我們沒有的 spotify-player 名稱**：*回答：照提案*：略過，並在 TUI 中顯示一行通知，讓 spotify-player 的 `keymap.toml` 可以原封不動複製；拼錯的名稱仍然是錯誤。替代方案：每個未知名稱都是錯誤
4. **一個序列是另一個序列的前綴**：*回答：照提案*：指出兩者的錯誤（spotify-player 默默讓較短的勝出）。替代方案：spotify-player 的行為。*經 0019 修訂（2026-10-25）*：只有檔案中兩個序列之間的衝突是錯誤；與預設衝突的檔案序列會捨棄該預設，並由訊息列說明是哪一個（`keymap.toml: "d" replaces the default "d d" (RemoveFromQueue)`），讓為較早的預設按鍵對應所寫的 `keymap.toml` 仍然能啟動
5. **優先順序**：*回答：照提案*：旗標 > 環境變數 > `app.toml` > 預設值（0003–0007 承諾過變數持續有效）。替代方案：檔案優先於環境變數
6. **文字輸入框的按鍵**：*回答：照提案*：在本規格中固定（`Backspace`、`C-u`、`Enter`、`Esc`……），不放進按鍵對應。替代方案：按鍵對應中採用 spotify-player 風格的輸入指令
7. **`app.toml` 的鍵名**：*回答：照提案*：如表所列（spotify-player 的 `seek_duration_secs` 與 `[layout] library`，其餘用我們自己的，時間長度用 `_secs` 後綴）
8. **客戶端憑證**（0002 的「可能在 0008 重新檢討」）：*回答：照提案*：維持為常數，不放進 `app.toml`

<a id="out-of-scope"></a>
## 不在範圍內

- 頁內篩選（搜尋以外的頁面上的 `/`）與排序（決策 1）。篩選是 [0012](0012-filter.md)（implemented）；排序仍然不在範圍內
- 任一檔案的即時重新載入；印出實際生效設定的指令
- 佈景主題與顏色（spotify-player 的 `theme.toml`）、圖示、邊框與進度條樣式、`playback_format`
- 播放視窗的位置與高度（spotify-player 其餘的 `[layout]`）、藝人頁面的分割
- spotify-player 的數量前綴（`5j`）與滑鼠支援
- 可設定的客戶端憑證（決策 8）
- 跨次執行記住任何東西（0009：播放器的 `remember_playback` 鍵）、MPRIS（[0010](0010-mpris.md)，implemented：它的 `mpris` 與 `max_cover_arts` 鍵加入了 `app.toml`）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-08"></a>
## 實作備註（規格未載明處所做的選擇，2026-10-08）

- **設定目錄與旗標**：`-c/--config-folder` 接受放在子指令之後（`tidal-player daemon -c DIR`、`tidal-player play -c DIR …`），TUI 則單獨使用（`tidal-player -c DIR`），不放在子指令之前：CLI 的 `args_conflicts_with_subcommands` 會把 `tidal-player -c DIR daemon` 讀成項目。`docs/config.md` 有說明。`login`、`logout`、`playback` 與 `daemon stop`/`unit` 不讀任何設定；`devices` 讀 `app.toml`（它的 `*` 標出 `output_device`）。TUI、`daemon` 與 `play` 驗證兩個檔案；只有 TUI 行程使用按鍵對應
- **`app.toml`**：未知的巢狀鍵以它的點分路徑指出（`unknown setting "layout.libary"`），依字母順序檢查。`[layout] library` 中缺少的百分比，在檢查總和之前退回 40。空的 `output_device` 被拒絕。空的 `TIDAL_PLAYER_HIDE_VERSIONS` 仍然表示「不隱藏任何東西」（0006），且優先於檔案。解析之前會去掉 UTF-8 BOM，讓行與欄保持正確；只有「找不到」才算缺少檔案
- **按鍵對應**：項目索引從 0 開始計算（`keymaps[0]` 是第一個）。`[[actions]]` 合併在 `[[keymaps]]` 之後（TOML 會失去兩個清單之間的順序），所以序列上的動作會取代序列上的指令。前綴衝突的訊息會指出每個較長的序列與它的指令。通知在只有一個名稱時用單數，會去除重複，依檔案順序，動作也包含在內；它保存在 `State::notice`，並在第一個 `Welcome` 之後顯示（否則 `Welcome` 會清掉客戶端訊息）。`GoToArtist` 執行第一個 *Go to artist* 項目。彈出視窗現在也接受 `G`/`end`、`g g`、`C-f`/`C-b`、`page_up`/`page_down` 與 `C-n`/`C-p`，依表中的「清單、彈出視窗」。`State::pending_g` 保留為待決序列的鏡像（0004 的 `ac21_key_sequence` 讀它）
- **按鍵解碼**：Control 加 Alt 同時按下，以及 `f13` 以上會被丟棄；具名按鍵上的 Shift 被忽略；Control 加具名按鍵（`C-enter`、`C-up`）現在解碼為它本身，而不是一般的按鍵，所以 0004/0006 的 `ac18_key_events` 中 `F(1)`、`Home` 與 Alt-x 的列依本規格更新了（它們原本預期這些按鍵被丟棄）
- **按鍵說明**：在彈出視窗上方開啟時，只列出在那裡有作用的東西：彈出視窗自己的區段（`Popup · Actions`……，帶有它的固定按鍵，如 `space`、`y`、`n`，這些是沒有指令的列）、彈出視窗有清單時的 `Lists`，以及帶有 `OpenCommandHelp` 的 `App`；視窗、`Pages` 與 `Playback` 區段省略，因為它們的按鍵在彈出視窗之下沒有作用（「說謊的說明」條件）。`FocusNextWindow`/`FocusPreviousWindow` 只在有多個窗格的頁面列出，`NextTab`/`PreviousTab` 只在有窗格帶分頁的地方列出。整份清單載入期間 `?` 什麼都不做（只有 `Esc`/`q` 有作用）。輸入篩選時按 `Esc` 會清除它並結束輸入；已設定篩選時按 `Esc` 會清除它，然後關閉。某一列上的 `Enter` 會在原本的彈出視窗仍然開啟的情況下，把它的第一個按鍵序列送進一般的按鍵路徑；沒有按鍵的列什麼都不做。在 80×24 時彈出視窗是 62×14，並隨反白捲動；輸入時頁尾顯示 `/fav▏ · enter keep · esc clear`，設定之後顯示 `/fav · enter run · esc clear`
- **串接**：`ui::LibraryLayout` 放在 core（`config::LibraryLayout` 重新匯出它）；`configured_tui_state(settings, keymap)` 同時建立獨立模式與連上的 TUI 的 state。`ac15_daemon_reads_app_toml` 在 mock API 上觀察 `page_size` 與 `quality`；`release_paused_secs` 只展示它被接受（觀察釋放需要音訊裝置）。`ac16_keymap_is_per_client` 在一個真正的常駐程式上執行兩個行程內的客戶端工作階段（連上的 TUI 需要終端機）

<a id="bugs"></a>
## 錯誤

- **CLI 測試讀取開發者的設定**（2026-10-25，由 0019 的驗收發現）。預期：`crates/app/tests/cli.rs` 中二進位檔的測試以自己的設定目錄執行，如 `tests/config.rs` 與 `tests/mpris.rs` 一樣。實際：`bin_in` 設定了狀態、執行階段與快取目錄，卻沒有設定設定目錄，所以測試讀取了 `~/.config/tidal-player`，並在 `keymap.toml` 無法建立的機器上失敗。根本原因：`bin_in` 缺少 `TIDAL_PLAYER_CONFIG_DIR`。修正：`bin_in` 把它指向 `<state>/config`（空的）；CLI 測試就是回歸檢查（沒有這項修正時，`~/.config/tidal-player` 中有損壞的 `keymap.toml` 會讓它們失敗）
