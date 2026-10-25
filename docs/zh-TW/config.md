[English](../config.md) | 正體中文

<a id="configuration"></a>
# 設定

tidal-player 由三個選用檔案設定：**`app.toml`** 存放設定，**`keymap.toml`** 存放按鍵，**`theme.toml`** 存放顏色，都採用 [spotify-player](https://github.com/aome510/spotify-player) 的格式，因此可以直接複製它的 `[[keymaps]]` 與 `[[themes]]` 區塊過來。設計：[spec 0008](../specs/0008-keymap-and-config.md) 與 [spec 0017](../specs/0017-themes.md)（佈景主題）。只有 TUI 會讀取 `theme.toml`；播放器和常駐程式從不讀取它。

<a id="where-the-files-are"></a>
## 檔案位置

**設定資料夾**依下列順序，取第一個找到的：

1. `-c DIR` / `--config-folder DIR`
2. `$TIDAL_PLAYER_CONFIG_DIR`
3. `$XDG_CONFIG_HOME/tidal-player`
4. `~/.config/tidal-player`

空的變數視同未設定。旗標要放在子命令**之後**：`tidal-player daemon -c DIR`、`tidal-player play -c DIR 123`，TUI 則直接用 `tidal-player -c DIR`。

資料夾和三個檔案都可以不存在：此時使用預設值，程式也絕不會在那裡建立或寫入任何東西。檔案只在**啟動時讀取一次**；常駐程式 (daemon) 要重新啟動（`systemctl --user restart tidal-player`）才會讀到修改過的檔案。

**起點**：[`examples/app.toml`](../../examples/app.toml) 和 [`examples/keymap.toml`](../../examples/keymap.toml) 列出了所有預設值（每項設定附上範圍，每個按鍵綁定附上命令），[`examples/theme.toml`](../../examples/theme.toml) 則收錄 spotify-player 的外觀與它的部分範例佈景主題，可供複製。原封不動複製過去不會改變任何東西；修改你想改的部分，其餘刪掉即可：

```sh
mkdir -p ~/.config/tidal-player
cp examples/app.toml examples/keymap.toml examples/theme.toml ~/.config/tidal-player/
```

每個行程只使用與自己相關的部分：播放器（獨立執行的 TUI、常駐程式、`play`）使用播放器設定，TUI（獨立執行或連接到常駐程式）使用 TUI 設定、`keymap.toml` 和 `theme.toml`。連接中的 TUI 使用自己的 `keymap.toml`、`theme.toml` 和 TUI 設定，而不是常駐程式的。不過，每個會啟動播放器或 TUI 的行程都會完整檢查 `app.toml` 和 `keymap.toml`：按鍵裡的一個拼字錯誤也會讓常駐程式停止，因此重新啟動一次就能發現。`theme.toml` 只由 TUI 檢查，因此顏色上的拼字錯誤絕不會讓音樂停止。

<a id="apptoml"></a>
## `app.toml`

扁平的鍵，每一個都是選用的。

| 鍵 | 可接受的值 | 預設值 | 可被覆寫於 | 讀取者 |
|---|---|---|---|---|
| `quality` | `"hi-res"`、`"lossless"`、`"high"` | `"hi-res"` | `--quality`、`TIDAL_PLAYER_QUALITY` | 播放器 |
| `output_device` | 任何 ALSA PCM 名稱（見 `tidal-player devices`） | `"default"` | `--device`、`TIDAL_PLAYER_DEVICE` | 播放器 |
| `volume_step` | 整數 1–25（%） | `5` | `TIDAL_PLAYER_VOLUME_STEP` | TUI |
| `seek_duration_secs` | 整數 1–600 | `5` | `TIDAL_PLAYER_SEEK_STEP` | TUI |
| `previous_restart_secs` | 整數 0–60 | `3` | `TIDAL_PLAYER_PREVIOUS_RESTART` | 播放器 |
| `autoplay` | `true`、`false` | `false` | `--autoplay`、`TIDAL_PLAYER_AUTOPLAY` | 播放器 |
| `release_paused_secs` | 整數 0–3600，或 `"never"` | `10` | `TIDAL_PLAYER_RELEASE_PAUSED` | 播放器 |
| `remember_playback` | `true`、`false` | `true` | `TIDAL_PLAYER_REMEMBER_PLAYBACK`（`on`、`off`） | 播放器 |
| `mpris` | `true`、`false` | `true` | `TIDAL_PLAYER_MPRIS`（`on`、`off`） | 播放器 |
| `max_cover_arts` | 整數 0–1000（`0`：不使用封面快取） | `20` | `TIDAL_PLAYER_MAX_COVER_ARTS` | 播放器 |
| `theme` | 佈景主題的名稱 | `"default"` | `-t`/`--theme`（TUI）、`TIDAL_PLAYER_THEME` | TUI |
| `key_hints` | `true`、`false` | `true` | `TIDAL_PLAYER_KEY_HINTS`（`on`、`off`） | TUI |
| `key_hints_delay_ms` | 整數 0–10 000（毫秒；`0`：立即顯示） | `1000` | `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` | TUI |
| `confirm_removals` | `true`、`false` | `true` | `TIDAL_PLAYER_CONFIRM_REMOVALS`（`on`、`off`） | TUI |
| `cover_img` | `true`、`false` | `true` | `TIDAL_PLAYER_COVER_IMG`（`on`、`off`） | TUI |
| `cover_img_rows` | 整數 4–20（顯示封面時播放視窗的列數） | `6` | `TIDAL_PLAYER_COVER_IMG_ROWS` | TUI |
| `cover_img_protocol` | `"auto"`、`"sixel"`、`"kitty"`、`"iterm2"`、`"halfblocks"` | `"auto"` | `TIDAL_PLAYER_COVER_IMG_PROTOCOL` | TUI |
| `page_size` | 整數 1–10 000 | `100` | `TIDAL_PLAYER_PAGE_SIZE` | 播放器、TUI |
| `search_page_size` | 整數 1–1000 | `20` | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` | 播放器、TUI |
| `hide_versions` | 字串陣列（`[]` 表示不隱藏任何東西） | [設定](playback.md#settings)中列出的十六個詞 | `TIDAL_PLAYER_HIDE_VERSIONS` | 播放器 |
| `[layout] library = { playlist_percent, album_percent }` | 各為整數 1–98，總和最多 99；*Artists*（藝人）佔用其餘部分 | `40`、`40` | | TUI |

各項設定的作用說明於[設定](playback.md#settings)；`output_device` 是播放器啟動時使用的裝置：執行期間選擇的裝置（`D`、`tidal-player playback device`）只在該次執行有效，要保留請設定 `output_device`（見[選擇輸出裝置](playback.md#choosing-the-output-device)）；音樂庫的版面配置說明於[視窗](tui.md#windows)；`key_hints` 與 `key_hints_delay_ms` 說明於[按鍵提示](tui.md#key-hints)；`cover_img`、`cover_img_rows` 與 `cover_img_protocol` 說明於[專輯封面](tui.md#album-cover)；`mpris` 與 `max_cover_arts` 說明於[桌面控制與多媒體鍵](mpris.md)；`theme` 指定 [`theme.toml`](#themetoml) 中的一個佈景主題（沒有檔案時為 `default`），TUI 執行期間可按 `T` 切換（見[佈景主題](tui.md#themes)）。專輯封面存放在**快取目錄**：`$TIDAL_PLAYER_CACHE_DIR`，否則 `$XDG_CACHE_HOME/tidal-player`，否則 `~/.cache/tidal-player`（見[專輯封面](mpris.md#album-covers)）。

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

**優先順序**，依每項設定分別判斷：旗標 > 環境變數 > `app.toml` > 預設值。當環境變數與檔案不一致時，環境變數優先，且不會有任何提示。`hide_versions` 的比對方式與該環境變數相同（轉為小寫，忽略空格、連字號、底線、句點和 `+`）。

<a id="themetoml"></a>
## `theme.toml`

TUI 的顏色，採用 spotify-player 的格式：任意數量的 `[[themes]]`，每個有 `name`、選用的 `[themes.palette]` 和選用的 `[themes.component_style]`。spotify-player 的 `theme.toml` 可以原封不動複製過來。只有 TUI 會讀取它，因此顏色上的拼字錯誤會讓 `tidal-player` 無法啟動，但不會影響常駐程式或 `play`。設計：[spec 0017](../specs/0017-themes.md)。

```toml
# ~/.config/tidal-player/theme.toml
[[themes]]
name = "dracula"
[themes.palette]
background = "#1e1f29"
foreground = "#f8f8f2"
red = "#ff5555"
cyan = "#8be9fd"
bright_black = "#555555"

[themes.component_style]
block_title = { fg = "Magenta", modifiers = ["Bold"] }
selection = { fg = "Black", bg = "Cyan" }
playback_progress_bar = { fg = "#bd93f9" }
```

用 `app.toml` 中的 `theme = "dracula"`、`TIDAL_PLAYER_THEME=dracula` 或 `tidal-player -t dracula` 選擇佈景主題（旗標 > 環境變數 > 檔案 > 預設值；空的變數視同未設定），也可以在 TUI 執行期間按 `T` 切換（見[佈景主題](tui.md#themes)）。每個 TUI 使用自己的佈景主題：連到同一個常駐程式的兩個 TUI 可以使用不同的。

- **`name`**（必填，不可為空）：`theme =` 和 Themes 彈出視窗用來稱呼該佈景主題的名稱
- **`palette`**（選用）：`background`、`foreground`，以及十六種終端機顏色 `black`、`red`、`green`、`yellow`、`blue`、`magenta`、`cyan`、`white`、`bright_black`、`bright_red`、`bright_green`、`bright_yellow`、`bright_blue`、`bright_magenta`、`bright_cyan`、`bright_white`。值可以是顏色名稱（忽略大小寫、空格、`-` 和 `_`：`red`、`Bright Black`、`light-red`、`dark_gray`、`reset`；`bright` 與 `light` 意思相同，`grey` 與 `gray` 也相同）、`0`–`255` 的數字（終端機的索引色），或 `#rrggbb`。請留意 ratatui 的命名：`white` 是**亮**白色，`gray` 才是一般的那個（ANSI 7）。省略的顏色就是終端機本身的顏色，省略的 `background` 和 `foreground` 也一樣
- **`component_style`**（選用）：每個元件一種樣式（見下表），各為含 `fg`、`bg` 和 `modifiers` 的行內表格。`fg` 和 `bg` 是調色盤名稱（`Black` … `White`、`BrightBlack` … `BrightWhite`，忽略大小寫、`_`、`-` 和空格，會取用該佈景主題調色盤中的那個顏色）或 `#rrggbb`。`modifiers` 是 `Bold`、`Dim`、`Italic`、`Underlined`、`RapidBlink`、`Reversed`、`Hidden`、`CrossedOut` 的清單，忽略大小寫。樣式會**整個取代**該元件的預設值：`selection = { fg = "Cyan" }` 是青色文字，不再有預設值的反白

`#rrggbb` 顏色會原樣送給終端機；不支援 24 位元色彩的終端機會自行決定如何顯示。

<a id="themes-order"></a>
### 佈景主題的順序

佈景主題依序為內建的 **`default`**，接著是檔案中的（依檔案順序）；Themes 彈出視窗也照此列出。`default` 就是今天的外觀。檔案中名為 `default` 的佈景主題會取代內建的那個（仍排在第一位）。兩個檔案中的佈景主題同名則是錯誤。沒有 `theme.toml`，或其中沒有 `[[themes]]` 時，`default` 是唯一的佈景主題。

<a id="components"></a>
### 元件

每個元件畫在哪裡、它的預設值（`default` 佈景主題的外觀，也是任何佈景主題對沒寫出的元件所得到的樣式），以及作為對照的 spotify-player 預設值。`*` 標示本播放器新增的兩個。

| 元件 | 繪製位置 | 預設值 | spotify-player 的預設值 |
|---|---|---|---|
| `block_title` | 每個邊框上的標題：外框的 `tidal-player`、各視窗與其分頁、佇列、彈出視窗、按鍵說明、按鍵提示 | 無 | `fg = "Magenta"` |
| `border` | 每個邊框 | 無 | 無 |
| `playback_status` | 播放視窗的狀態符號（`▶ ⏸ … ■`）及其指示（`shuffle`、`repeat: …`、`autoplay`、音量） | `Bold` | `Cyan`、`Bold` |
| `playback_track` | 曲目標題（或 `Nothing playing`） | `Bold` | `Cyan`、`Bold` |
| `playback_artists` | 標題後面的 ` · ` 與藝人 | `Bold` | `Cyan`、`Bold` |
| `playback_album` | 專輯那一列 | 無 | `fg = "Yellow"` |
| `playback_metadata` | 詳細資訊那一列（音質、格式、裝置、bit-perfect）與時間 `1:23 / 3:32` | 無 | `fg = "BrightBlack"` |
| `playback_progress_bar` | 進度條已播放的部分（`━`） | 無 | `fg = "Green"`、`bg = "BrightBlack"` |
| `playback_progress_bar_unfilled` | 進度條其餘的部分（`─`） | 無 | `bg = "BrightBlack"` |
| `current_playing` | 佇列中目前項目的那一列 | `Bold` | `Green`、`Bold` |
| `page_desc` | 頁面的標題列（`Album · …`、`Search`、…） | `Bold` | `Cyan`、`Bold` |
| `playlist_desc` | mix 頁面的副標題列 | `Dim` | `BrightBlack`、`Dim` |
| `table_header` | 按鍵說明的區段標題（`Lists`、`Playback`、…） | `Bold` | `fg = "Blue"` |
| `selection` | 游標所在列：清單、佇列、彈出視窗、按鍵說明、搜尋的最佳結果；以及窗格標題中使用中的分頁名稱 | `Reversed` | `Reversed`、`Bold` |
| `secondary_row` | 清單或佇列的每隔一列（清單中的第 2、4、… 列） | 無 | 無 |
| `like` | 已追蹤播放清單上的 `♥` | 無 | 無 |
| `error`* | 訊息列與載入失敗處（頁面、視窗、失敗的 `Loading more…`） | `fg = "Red"` | `fg = "Red"` |
| `prompt`* | 開啟中的提示（`Add to queue: …`、`Play next: …`）、播放清單名稱提示、搜尋輸入框 | `Bold` | `Bold` |

佈景主題沒寫出的元件維持今天的外觀，**不會**採用 spotify-player 色彩繽紛的預設值：只有調色盤的佈景主題（例如 spotify-player 的 `dracula`）只會改變背景、前景和錯誤的紅色。想要 spotify-player 的外觀，請複製 [`examples/theme.toml`](../../examples/theme.toml) 中的 `spotify_player` 佈景主題（它把預設值全部寫了出來）再修改。調色盤的 `background` 和 `foreground` 是整個畫面的樣式：每個儲存格都從它們開始，上表的樣式再疊上去。

本播放器沒有繪製的 spotify-player 元件（`playback_genres`、`lyrics_played`、`lyrics_playing`、`visualization`）會被接受並略過，因此它的佈景主題可以原封不動複製過來。TUI 的訊息列會在啟動時說明一次略過了哪些：`theme.toml: 2 spotify-player styles not used here: lyrics_played, lyrics_playing`（每個名稱只出現一次，依檔案順序；若有按鍵對應的通知，則接在它之後，以 ` · ` 相連）。其他名稱都是錯誤。

<a id="what-the-tui-adds-on-top"></a>
### TUI 疊加的效果

無論用哪個佈景主題，TUI 都會在元件樣式之上加上這些狀態提示，因此佈景主題無法把它們藏起來（疊加的樣式會在有設定的地方取代下層的顏色，並加上它的修飾）：

- **列**：每列從畫面的樣式開始，或在每隔一列使用 `secondary_row`；佇列的目前項目疊上 `current_playing`；無法播放的曲目、自動播放建議和 `── Suggested` 分隔線加上 `Dim`；游標所在列疊上 `selection`，且當它的視窗沒有焦點時加上 `Dim`
- **視窗**：有焦點的視窗標題加上 `Bold`；其他視窗的標題和邊框加上 `Dim`。彈出視窗、按鍵說明和按鍵提示的標題加上 `Bold`，輸入佇列篩選時佇列的標題也是。外框的標題不加任何東西
- **搜尋輸入框**：有焦點時為加上 `Bold` 的 `prompt`；沒有焦點時為去掉 `Bold` 的 `prompt`，再加上 `Dim`
- **狀態文字**（`Loading…`、`Loading more…`、空清單、`No rows match …`、彈出視窗的狀態、按鍵說明的頁尾與變暗的列、按鍵提示中變暗的列）：在該列的樣式上加 `Dim`

使用 `default` 佈景主題時，這就是沒有佈景主題時 TUI 的外觀。

<a id="keymaptoml"></a>
## `keymap.toml`

這些項目會**新增或取代**下方的預設按鍵。

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

- **`[[keymaps]]`**：`key_sequence` 和 `command`。`command` 是命令名稱、`"None"`（移除該按鍵序列原本綁定的任何東西），或帶參數命令的單一項目行內表格。一個項目只會取代**相同**按鍵序列的綁定：命令會保留它的其他按鍵（上例中 `n` 仍然會跳到下一首）。後面的項目優先於前面的項目
- **`[[actions]]`**：`key_sequence`、`action` 和選用的 `target`：`"SelectedItem"`（預設：選取的列）或 `"PlayingTrack"`。按下該按鍵會直接執行[動作選單](tui.md#actions)中的那個項目，不必開啟選單。若動作選單不會列出該項目（在藝人上的 *Go to album*（前往專輯）、在正在播放的曲目上的 *Play next*（下一首播放）、沒有任何曲目在播放時任何使用 `PlayingTrack` 的項目），則什麼都不做
- 沒有任何按鍵的命令無法使用，也不會出現在按鍵說明中。`OpenCommandHelp` 可以取消綁定；`Quit` 必須至少保留一個按鍵

<a id="key-syntax"></a>
### 按鍵語法

**按鍵序列**是以單一空格分隔的按鍵（`g g`、`s l a`）。一個按鍵可以是：

- 單一字元：`a`、`A`（Shift-a）、`>`、`?`、`-`
- 名稱：`enter`、`space`、`tab`、`backtab`（Shift-Tab）、`backspace`、`esc`、`left`、`right`、`up`、`down`、`insert`、`delete`、`home`、`end`、`page_up`、`page_down`、`f1` … `f12`
- 上述任一種加上 `C-`（Control）或 `M-`（Alt）前綴：`C-s`、`M-enter`、`C-space`、`M-p`。`C-S` 等同 `C-s`：終端機回報 Control 加字母時不分大小寫

按下的按鍵會累積：當你按下的內容不是任何綁定的開頭時，會從最後一個按鍵單獨重新開始累積（`g x` 的作用等同 `x`）；當它是一個綁定時，就執行該綁定。沒有逾時。若某個按鍵序列是另一個序列的開頭，就無法完成，因此檔案中有兩個序列呈這種關係（`s` 與 `s l a`）會被拒絕，見[錯誤](#errors)。檔案中的序列若與**預設**綁定衝突，則以檔案為準：預設綁定會被捨棄，訊息列會在啟動時說明一次，並以 ` · ` 與下方略過名稱的通知相接：`keymap.toml: "d" replaces the default "d d" (RemoveSelectedItem)`，或 `keymap.toml: "g" replaces the defaults "g g" (SelectFirstOrScrollToTop), "g l" (LibraryPage), …`（依預設的順序全部列出；你的每個序列各一段，以 `;` 相接）。因此從舊範例複製來、把 `RemoveFromQueue` 綁在 `d` 的 `keymap.toml`，在新預設 `d d` 旁仍然可用。

**不可靠的按鍵**：許多終端機無法將 `C-enter`、`C-tab`、`C-backspace` 或 Control 加方向鍵當作獨立的按鍵送出；這類綁定會被接受，但可能永遠不會觸發。終端機會把 `C-i` 送成 `tab`、`C-m` 送成 `enter`、`C-[` 送成 `esc`，所以請改為綁定這些名稱。`M-` 按鍵需要終端機將 Alt 送成 Escape + 按鍵（大多數終端機都如此；在 macOS 上請啟用「Use Option as Meta」）。按鍵說明（`?`）會顯示已綁定的內容；按下該按鍵即可得知它是否有送達。

<a id="where-keys-act"></a>
### 按鍵的作用範圍

綁定只在其命令有作用的地方生效（下表的「作用範圍」欄）；在其他地方什麼都不做。在彈出視窗中只有它自己的按鍵有作用：清單命令、`ChooseSelected`、`ClosePopup` 和 `OpenCommandHelp`。

有些按鍵是**固定的**，不在按鍵對應中：文字輸入（`o`/`O` 提示、搜尋輸入框、播放清單名稱提示、正在輸入的視窗篩選）會在按鍵對應之前接收所有可列印字元和貼上內容，其編輯按鍵為 `backspace`、`C-u`、`enter`、`esc`（在搜尋輸入框中還有 `tab`/`backtab`，`C-c` 結束程式，`C-q` 返回；在篩選中 `up`、`down`、`page_up`、`page_down` 在符合的列之間移動，`C-c` 結束程式）；角色篩選器的 `space`；詢問中的 `y` 和 `n`；按鍵說明的 `/` 篩選。

<a id="commands"></a>
### 命令

| 命令 | 預設按鍵 | 作用範圍 | 說明文字 |
|---|---|---|---|
| `ResumePause` | `space` | 所有地方 | play / pause（播放／暫停） |
| `NextTrack` | `n` | 所有地方 | next track（下一首曲目） |
| `PreviousTrack` | `p` | 所有地方 | previous track（上一首曲目） |
| `SeekForward`（`duration`：1–600 秒，選用） | `>` | 所有地方 | seek forward (by `duration`, default `seek_duration_secs`)（向前跳轉，幅度為 `duration`，預設為 `seek_duration_secs`） |
| `SeekBackward`（`duration`：1–600 秒，選用） | `<` | 所有地方 | seek backward（倒轉） |
| `SeekStart` | `^` | 所有地方 | back to the start of the track（回到曲目開頭） |
| `Shuffle` | `C-s` | 所有地方 | shuffle on / off（隨機播放開／關） |
| `Repeat` | `C-r` | 所有地方 | repeat: off → queue → track（重複播放：關 → 佇列 → 曲目） |
| `ToggleAutoplay` | `A` | 所有地方 | autoplay on / off（自動播放開／關） |
| `SwitchDevice` | `D` | 所有地方 | choose the output device（選擇輸出裝置，開啟裝置彈出視窗） |
| `SwitchTheme` | `T` | 所有地方 | choose a theme（選擇佈景主題） |
| `VolumeUp` | `+` | 所有地方 | volume up by `volume_step`（音量增加 `volume_step`） |
| `VolumeDown` | `-` | 所有地方 | volume down by `volume_step`（音量減少 `volume_step`） |
| `VolumeChange`（`offset`：−25…25，不可為 0） | 無 | 所有地方 | volume by `offset` %（音量調整 `offset` %） |
| `Mute` | `_` | 所有地方 | mute / unmute（靜音／取消靜音） |
| `AddToQueuePrompt` | `o` | 所有地方 | add a link or ID to the end of the queue（將連結或 ID 加到佇列末端） |
| `PlayNextPrompt` | `O` | 所有地方 | add a link or ID to play next（將連結或 ID 加為下一首播放） |
| `SelectNextOrScrollDown` | `j`、`down`、`C-n` | 清單、彈出視窗 | move down（向下移動） |
| `SelectPreviousOrScrollUp` | `k`、`up`、`C-p` | 清單、彈出視窗 | move up（向上移動） |
| `PageSelectNextOrScrollDown` | `C-f`、`page_down` | 清單、彈出視窗 | move down a window（向下移動一個視窗） |
| `PageSelectPreviousOrScrollUp` | `C-b`、`page_up` | 清單、彈出視窗 | move up a window（向上移動一個視窗） |
| `SelectFirstOrScrollToTop` | `g g` | 清單、彈出視窗 | move to the top（移到最上方） |
| `SelectLastOrScrollToBottom` | `G`、`end` | 清單、彈出視窗 | move to the last loaded row（移到最後一個已載入的列） |
| `ChooseSelected` | `enter` | 清單、彈出視窗 | play the track with its list / open / run（播放曲目並連同其清單／開啟／執行） |
| `AddSelectedItemToQueue` | `Z`、`C-z` | 清單 | add to the end of the queue（加到佇列末端） |
| `RemoveSelectedItem`* | `d d` | 佇列、你的播放清單的曲目、收藏的曲目、音樂庫的清單 | remove from the list（從清單移除；先詢問 `y/n`；[移除什麼](tui.md#confirming-removals)） |
| `RemoveFromQueue` | | 佇列 | remove from the queue（從佇列移除；先詢問 `y/n`；沒有預設按鍵，由 `RemoveSelectedItem` 執行） |
| `ShowActionsOnSelectedItem` | `g a`、`C-space` | 清單 | actions on the selected row（對選取列的動作） |
| `ShowActionsOnCurrentTrack` | `a` | 所有地方 | actions on the playing track（對正在播放曲目的動作） |
| `FocusNextWindow` | `tab` | 頁面 | next pane（下一個窗格） |
| `FocusPreviousWindow` | `backtab` | 頁面 | previous pane（上一個窗格） |
| `NextTab` | `]` | 有分頁的窗格 | next tab（下一個分頁） |
| `PreviousTab` | `[` | 有分頁的窗格 | previous tab（上一個分頁） |
| `RoleFilter` | `f` | 藝人的 *All tracks*（所有曲目） | the role filter（角色篩選器） |
| `Queue` | `z` | 所有地方 | the queue page（佇列頁面） |
| `LibraryPage` | `g l` | 所有地方 | the library（音樂庫） |
| `LikedTrackPage` | `g y` | 所有地方 | favorite tracks（收藏的曲目） |
| `SearchPage` | `g s` | 所有地方 | the search page (on one: its input)（搜尋頁面；已在搜尋頁面時：其輸入框） |
| `MixesPage` | `g m` | 所有地方 | your mixes（你的 mix） |
| `Search` | `/` | 清單 | filter the rows（篩選列；見[篩選清單](tui.md#filtering-a-list)） |
| `PreviousPage` | `backspace`、`C-q` | 所有地方 | back（返回） |
| `ClosePopup` | `esc` | 彈出視窗、提示、載入中、篩選 | close / cancel（關閉／取消；在頁面上：清除焦點視窗的篩選） |
| `OpenCommandHelp` | `?`、`C-h` | 所有地方 | this help（此說明） |
| `Quit` | `q`、`C-c` | 所有地方 | quit (an attached TUI detaches)（結束；連接中的 TUI 會中斷連接） |

`*` 標示本播放器在 spotify-player 之外新增的命令。

帶參數的命令寫成行內表格：`command = { VolumeChange = { offset = -10 } }`、`command = { SeekBackward = { duration = 15 } }`、`command = { SeekForward = { } }`（使用設定的幅度）。

<a id="actions"></a>
### 動作

用於 `[[actions]]`；只有 `GoToRadio` 有預設按鍵（`r`，作用於選取的列）。

| 動作 | 作用 | 適用對象 |
|---|---|---|
| `GoToAlbum` | 前往專輯 | 有專輯的曲目 |
| `GoToArtist` | 前往（第一位）藝人 | 曲目、專輯 |
| `GoToRadio` | 前往電台（一個頁面；按 `Enter` 前不會播放任何東西） | 曲目、藝人 |
| `AddToQueue` | 加到佇列末端 | 曲目、專輯、播放清單 |
| `PlayNext` | 下一首播放 | 曲目（正在播放的除外）、專輯、播放清單 |
| `AddToLiked` | 加入收藏 | 曲目、專輯、藝人、你追蹤的播放清單 |
| `DeleteFromLiked` | 從收藏移除（先詢問 `y/n`） | 曲目、專輯、藝人、你追蹤的播放清單 |
| `AddToPlaylist` | 加到播放清單…（開啟播放清單選擇器） | 曲目、專輯 |
| `DeleteFromPlaylist` | 從此播放清單移除（先詢問 `y/n`） | 你自己的播放清單頁面上的曲目 |
| `RemoveFromQueue` | 從佇列移除（先詢問 `y/n`） | 佇列項目、正在播放的曲目 |
| `DeletePlaylist` | 刪除播放清單（仍會詢問 `y/n`） | 你自己的播放清單 |

除非 `confirm_removals` 為 `false`，這三個移除動作都會像動作選單一樣先詢問 `y/n`，不會因為是綁定的按鍵就略過；見[移除前的確認](tui.md#confirming-removals)。*Delete playlist* 不論設定為何都會詢問。

<a id="spotify-player-names-not-supported-here"></a>
### 此處不支援的 spotify-player 名稱

這些名稱會被略過而不視為錯誤，因此 spotify-player 的 `keymap.toml` 可以原封不動複製過來。TUI 的訊息列會在啟動時說明一次略過了哪些：`keymap.toml: 3 spotify-player commands not supported here: PlayRandom, LyricsPage, SortTrackByTitle`。

- 命令：`PlayRandom`、`RefreshPlayback`、`RestartIntegratedClient`、`ShowActionsOnCurrentContext`、`JumpToHighlightTrackInContext`、`JumpToCurrentTrackInContext`、`BrowseUserPlaylists`、`BrowseUserFollowedArtists`、`BrowseUserSavedAlbums`、`CurrentlyPlayingContextPage`、`TopTrackPage`、`RecentlyPlayedTrackPage`、`LyricsPage`、`BrowsePage`、`OpenSpotifyLinkFromClipboard`、`SortTrackByTitle`、`SortTrackByArtists`、`SortTrackByAlbum`、`SortTrackByDuration`、`SortTrackByAddedDate`、`ReverseTrackOrder`、`SortLibraryAlphabetically`、`SortLibraryByRecent`、`MovePlaylistItemUp`、`MovePlaylistItemDown`、`CreatePlaylist`、`OpenLogs`
- 動作：`GoToShow`、`AddToLibrary`、`DeleteFromLibrary`、`ShowActionsOnAlbum`、`ShowActionsOnArtist`、`ShowActionsOnShow`、`ToggleLiked`、`CopyLink`、`Follow`、`Unfollow`

拼錯的名稱（`NxtTrack`）仍然是錯誤。

<a id="errors"></a>
## 錯誤

損壞的 `app.toml` 或 `keymap.toml` 會讓 `tidal-player`、`tidal-player daemon` 和 `tidal-player play` 在任何東西啟動之前（不建立 socket，不變更終端機）以結束碼 2 停止，並在 stderr 輸出一行以該檔案路徑開頭的訊息：

| 問題 | 訊息 |
|---|---|
| TOML 語法 | `…/app.toml: line 3, column 9: <parser message>` |
| 未知的設定（任何位置，包括 `[layout]`） | `…/app.toml: unknown setting "volum_step"` |
| 型別或範圍錯誤 | `…/app.toml: invalid volume_step: expected an integer from 1 to 25, got 0` |
| `keymap.toml` 中未知的欄位 | ``…/keymap.toml: line 2, column 1: unknown field `comand`, expected `command` or `key_sequence` `` |
| 未知的命令或動作 | `…/keymap.toml: keymaps[4]: unknown command "NxtTrack"`（項目從 0 開始計數） |
| 錯誤的按鍵 | `…/keymap.toml: keymaps[2]: unknown key "ctrl+s" in "ctrl+s"` |
| 錯誤的參數 | `…/keymap.toml: keymaps[5]: VolumeChange offset must be from -25 to 25 and not 0, got 40` |
| 檔案中某個按鍵序列是檔案中另一個序列的開頭 | `…/keymap.toml: "s" is bound to Search and is the start of "s l a" (LibraryPage); unbind those with command = "None" first` |
| 所有 `Quit` 按鍵都被取消綁定 | `…/keymap.toml: Quit has no key left` |
| 無法讀取的檔案（權限、是資料夾） | `…/app.toml: <error>` |

`theme.toml` 的問題只會讓 `tidal-player`（TUI，不論獨立執行或連接常駐程式）以同樣的方式停止（結束碼 2，stderr 一行訊息，終端機尚未變更）；`tidal-player daemon` 和 `tidal-player play` 不會讀取這個檔案。項目從 0 開始計數：

| 問題 | 訊息 |
|---|---|
| TOML 語法 | `…/theme.toml: line 3, column 9: <parser message>` |
| 佈景主題或其調色盤中未知的欄位 | `…/theme.toml: themes[1]: unknown field "colour"` / `…/theme.toml: themes[1].palette: unknown colour "orange"` |
| 未知的元件 | `…/theme.toml: themes[1].component_style: unknown style "blok_title"` |
| 樣式中未知的欄位 | `…/theme.toml: themes[1].component_style.border: unknown field "colour", expected fg, bg or modifiers` |
| 錯誤的顏色 | `…/theme.toml: themes[1].palette.red: invalid colour "#ff55"` / `…/theme.toml: themes[1].component_style.border.fg: invalid colour "Purpel" (a palette name such as BrightBlack, or #rrggbb)` |
| 錯誤的修飾 | `…/theme.toml: themes[1].component_style.selection: unknown modifier "Blink"` |
| 沒有名稱、名稱為空，或名稱已被使用 | `…/theme.toml: themes[3]: missing name` / `…: themes[3]: empty name` / `…: themes[3]: "dracula" is already themes[1]` |
| 未知的 `theme` | `…/app.toml: unknown theme "draculla" (themes: default, dracula, gruvbox_dark)`；來自變數時為 `TIDAL_PLAYER_THEME: unknown theme …`，來自旗標時為 `--theme: unknown theme …` |
| 無法讀取的檔案 | `…/theme.toml: <io error>` |

`theme` 的型別錯誤（`…/app.toml: invalid theme: expected a string, got 5`）會讓每個行程都停止，如同 `app.toml` 的任何一個鍵。找不到 `theme.toml` 不是錯誤。

帶有 UTF-8 位元組順序標記 (BOM) 或 Windows 換行字元的檔案也能正常讀取。
