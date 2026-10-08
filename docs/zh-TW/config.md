[English](../config.md) | 繁體中文

<a id="configuration"></a>
# 設定

tidal-player 由兩個選用檔案設定：**`app.toml`** 存放設定，**`keymap.toml`** 存放按鍵，採用 [spotify-player](https://github.com/aome510/spotify-player) 的格式，因此可以直接複製它的 `[[keymaps]]` 區塊過來。設計：[spec 0008](../specs/0008-keymap-and-config.md)。

<a id="where-the-files-are"></a>
## 檔案位置

**設定資料夾**依下列順序，取第一個找到的：

1. `-c DIR` / `--config-folder DIR`
2. `$TIDAL_PLAYER_CONFIG_DIR`
3. `$XDG_CONFIG_HOME/tidal-player`
4. `~/.config/tidal-player`

空的變數視同未設定。旗標要放在子命令**之後**：`tidal-player daemon -c DIR`、`tidal-player play -c DIR 123`，TUI 則直接用 `tidal-player -c DIR`。

資料夾和兩個檔案都可以不存在：此時使用預設值，程式也絕不會在那裡建立或寫入任何東西。檔案只在**啟動時讀取一次**；常駐程式 (daemon) 要重新啟動（`systemctl --user restart tidal-player`）才會讀到修改過的檔案。

**起點**：[`examples/app.toml`](../../examples/app.toml) 和 [`examples/keymap.toml`](../../examples/keymap.toml) 列出了所有預設值（每項設定附上範圍，每個按鍵綁定附上命令）。原封不動複製過去不會改變任何東西；修改你想改的部分，其餘刪掉即可：

```sh
mkdir -p ~/.config/tidal-player
cp examples/app.toml examples/keymap.toml ~/.config/tidal-player/
```

每個行程只使用與自己相關的部分：播放器（獨立執行的 TUI、常駐程式、`play`）使用播放器設定，TUI（獨立執行或連接到常駐程式）使用 TUI 設定和 `keymap.toml`。連接中的 TUI 使用自己的 `keymap.toml` 和 TUI 設定，而不是常駐程式的。不過，每個會啟動播放器或 TUI 的行程都會完整檢查**兩個**檔案：按鍵裡的一個拼字錯誤也會讓常駐程式停止，因此重新啟動一次就能發現。

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
| `page_size` | 整數 1–10 000 | `100` | `TIDAL_PLAYER_PAGE_SIZE` | 播放器、TUI |
| `search_page_size` | 整數 1–1000 | `20` | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` | 播放器、TUI |
| `hide_versions` | 字串陣列（`[]` 表示不隱藏任何東西） | [設定](playback.md#settings)中列出的十六個詞 | `TIDAL_PLAYER_HIDE_VERSIONS` | 播放器 |
| `[layout] library = { playlist_percent, album_percent }` | 各為整數 1–98，總和最多 99；*Artists*（藝人）佔用其餘部分 | `40`、`40` | | TUI |

各項設定的作用說明於[設定](playback.md#settings)；音樂庫的版面配置說明於[視窗](tui.md#windows)。

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

按下的按鍵會累積：當你按下的內容不是任何綁定的開頭時，會從最後一個按鍵單獨重新開始累積（`g x` 的作用等同 `x`）；當它是一個綁定時，就執行該綁定。沒有逾時。若某個按鍵序列是另一個序列的開頭（`g g` 已綁定時又綁定 `g`），會被拒絕，見[錯誤](#errors)。

**不可靠的按鍵**：許多終端機無法將 `C-enter`、`C-tab`、`C-backspace` 或 Control 加方向鍵當作獨立的按鍵送出；這類綁定會被接受，但可能永遠不會觸發。終端機會把 `C-i` 送成 `tab`、`C-m` 送成 `enter`、`C-[` 送成 `esc`，所以請改為綁定這些名稱。`M-` 按鍵需要終端機將 Alt 送成 Escape + 按鍵（大多數終端機都如此；在 macOS 上請啟用「Use Option as Meta」）。按鍵說明（`?`）會顯示已綁定的內容；按下該按鍵即可得知它是否有送達。

<a id="where-keys-act"></a>
### 按鍵的作用範圍

綁定只在其命令有作用的地方生效（下表的「作用範圍」欄）；在其他地方什麼都不做。在彈出視窗中只有它自己的按鍵有作用：清單命令、`ChooseSelected`、`ClosePopup` 和 `OpenCommandHelp`。

有些按鍵是**固定的**，不在按鍵對應中：文字輸入（`o`/`O` 提示、搜尋輸入框、播放清單名稱提示）會在按鍵對應之前接收所有可列印字元和貼上內容，其編輯按鍵為 `backspace`、`C-u`、`enter`、`esc`（在搜尋輸入框中還有 `tab`/`backtab`，`C-c` 結束程式，`C-q` 返回）；角色篩選器的 `space`；詢問中的 `y` 和 `n`；按鍵說明的 `/` 篩選。

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
| `RemoveFromQueue` | `d` | 佇列 | remove from the queue（從佇列移除） |
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
| `Search` | `/` | 搜尋頁面 | back to the search input（回到搜尋輸入框） |
| `PreviousPage` | `backspace`、`C-q` | 所有地方 | back（返回） |
| `ClosePopup` | `esc` | 彈出視窗、提示、載入中 | close / cancel（關閉／取消） |
| `OpenCommandHelp` | `?`、`C-h` | 所有地方 | this help（此說明） |
| `Quit` | `q`、`C-c` | 所有地方 | quit (an attached TUI detaches)（結束；連接中的 TUI 會中斷連接） |

帶參數的命令寫成行內表格：`command = { VolumeChange = { offset = -10 } }`、`command = { SeekBackward = { duration = 15 } }`、`command = { SeekForward = { } }`（使用設定的幅度）。

<a id="actions"></a>
### 動作

用於 `[[actions]]`；沒有任何動作有預設按鍵。

| 動作 | 作用 | 適用對象 |
|---|---|---|
| `GoToAlbum` | 前往專輯 | 有專輯的曲目 |
| `GoToArtist` | 前往（第一位）藝人 | 曲目、專輯 |
| `AddToQueue` | 加到佇列末端 | 曲目、專輯、播放清單 |
| `PlayNext` | 下一首播放 | 曲目（正在播放的除外）、專輯、播放清單 |
| `AddToLiked` | 加入收藏 | 曲目、專輯、藝人、你追蹤的播放清單 |
| `DeleteFromLiked` | 從收藏移除 | 曲目、專輯、藝人、你追蹤的播放清單 |
| `AddToPlaylist` | 加到播放清單…（開啟播放清單選擇器） | 曲目、專輯 |
| `DeleteFromPlaylist` | 從此播放清單移除 | 你自己的播放清單頁面上的曲目 |
| `RemoveFromQueue` | 從佇列移除 | 佇列項目、正在播放的曲目 |
| `DeletePlaylist` | 刪除播放清單（仍會詢問 `y/n`） | 你自己的播放清單 |

<a id="spotify-player-names-not-supported-here"></a>
### 此處不支援的 spotify-player 名稱

這些名稱會被略過而不視為錯誤，因此 spotify-player 的 `keymap.toml` 可以原封不動複製過來。TUI 的訊息列會在啟動時說明一次略過了哪些：`keymap.toml: 3 spotify-player commands not supported here: PlayRandom, LyricsPage, SwitchTheme`。

- 命令：`PlayRandom`、`RefreshPlayback`、`RestartIntegratedClient`、`SwitchTheme`、`SwitchDevice`、`ShowActionsOnCurrentContext`、`JumpToHighlightTrackInContext`、`JumpToCurrentTrackInContext`、`BrowseUserPlaylists`、`BrowseUserFollowedArtists`、`BrowseUserSavedAlbums`、`CurrentlyPlayingContextPage`、`TopTrackPage`、`RecentlyPlayedTrackPage`、`LyricsPage`、`BrowsePage`、`OpenSpotifyLinkFromClipboard`、`SortTrackByTitle`、`SortTrackByArtists`、`SortTrackByAlbum`、`SortTrackByDuration`、`SortTrackByAddedDate`、`ReverseTrackOrder`、`SortLibraryAlphabetically`、`SortLibraryByRecent`、`MovePlaylistItemUp`、`MovePlaylistItemDown`、`CreatePlaylist`、`OpenLogs`
- 動作：`GoToRadio`、`GoToShow`、`AddToLibrary`、`DeleteFromLibrary`、`ShowActionsOnAlbum`、`ShowActionsOnArtist`、`ShowActionsOnShow`、`ToggleLiked`、`CopyLink`、`Follow`、`Unfollow`

拼錯的名稱（`NxtTrack`）仍然是錯誤。

<a id="errors"></a>
## 錯誤

損壞的檔案會讓 `tidal-player`、`tidal-player daemon` 和 `tidal-player play` 在任何東西啟動之前（不建立 socket，不變更終端機）以結束碼 2 停止，並在 stderr 輸出一行以該檔案路徑開頭的訊息：

| 問題 | 訊息 |
|---|---|
| TOML 語法 | `…/app.toml: line 3, column 9: <parser message>` |
| 未知的設定（任何位置，包括 `[layout]`） | `…/app.toml: unknown setting "volum_step"` |
| 型別或範圍錯誤 | `…/app.toml: invalid volume_step: expected an integer from 1 to 25, got 0` |
| `keymap.toml` 中未知的欄位 | ``…/keymap.toml: line 2, column 1: unknown field `comand`, expected `command` or `key_sequence` `` |
| 未知的命令或動作 | `…/keymap.toml: keymaps[4]: unknown command "NxtTrack"`（項目從 0 開始計數） |
| 錯誤的按鍵 | `…/keymap.toml: keymaps[2]: unknown key "ctrl+s" in "ctrl+s"` |
| 錯誤的參數 | `…/keymap.toml: keymaps[5]: VolumeChange offset must be from -25 to 25 and not 0, got 40` |
| 某個按鍵序列是另一個序列的開頭 | `…/keymap.toml: "g" is bound to LibraryPage and is the start of "g g" (SelectFirstOrScrollToTop), …; unbind those with command = "None" first` |
| 所有 `Quit` 按鍵都被取消綁定 | `…/keymap.toml: Quit has no key left` |
| 無法讀取的檔案（權限、是資料夾） | `…/app.toml: <error>` |

帶有 UTF-8 位元組順序標記 (BOM) 或 Windows 換行字元的檔案也能正常讀取。
