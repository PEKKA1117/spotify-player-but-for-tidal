[English](../tui.md) | 繁體中文

<a id="the-tui"></a>
# 終端介面（TUI）

直接執行 `tidal-player`（可加上[項目](playback.md#items)）即開啟終端介面。若沒有正在執行的播放器，播放器與畫面在同一個行程中執行（獨立模式）：離開時會停止播放並釋放音訊裝置。若已有播放器在執行（[常駐程式](daemon.md)，或另一個 TUI），TUI 會改為連上它：畫面與按鍵完全相同，離開時音樂會繼續播放（見[連上 TUI](daemon.md#attaching-a-tui)）。設計文件：[spec 0004](../specs/0004-queue-and-controls.md)、[spec 0005](../specs/0005-daemon-and-clients.md) 與 [spec 0006](../specs/0006-library.md)（頁面與音樂庫）、[spec 0007](../specs/0007-search.md)（搜尋）、[spec 0011](../specs/0011-mixes-and-radio.md)（mix 與電台），[spec 0008](../specs/0008-keymap-and-config.md)（按鍵對應、設定檔與按鍵說明），以及 [spec 0013](../specs/0013-key-hints.md)（按鍵提示）。

<a id="the-screen"></a>
## 畫面

一個標題為 `tidal-player` 的外框，頂端是**播放視窗**（4 列），下方是一個**頁面**。第一個頁面是**播放佇列**；其他頁面列於[頁面](#pages)：

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
││  1   May These Noises Start…  Pierce The Veil    Collide With The S…   4:01││
││▶ 2   Hell Above               Pierce The Veil    Collide With The S…   3:32││
││  3   A Match Into Water       Pierce The Veil    Collide With The S…   4:22││
│…                                                                             │
││  9   I'm Low On Gas And You…  Pierce The Veil    Collide With The S…   3:43││
││  ── Suggested ─────────────────────────────────────────────────────────────││
││  10  If I'm James Dean, You…  Sleeping With Si…  With Ears To See A…   3:39││
```

<a id="playback-window"></a>
### 播放視窗

1. 狀態、曲名與藝人，右側是各種模式：
   - `▶` 播放中、`⏸` 已暫停、`…` 載入或緩衝中、`■` 已停止；佇列為空時顯示 `Nothing playing`
   - 開啟隨機播放時顯示 `shuffle`，開啟重複播放時顯示 `repeat: queue` 或 `repeat: track`，開啟自動播放時顯示 `autoplay`，接著是音量（`80%`）或 `muted`
2. 專輯
3. 播放方式：Tidal 提供的音質、格式、輸出裝置，以及 `bit-perfect` 或不是位元完美（bit-perfect）的原因（與 [`play`](playback.md#output-kinds-and-bit-perfect) 的「Track」與「Output」兩行相同）；暫停期間若裝置已[釋放](daemon.md#releasing-the-device-while-paused)，後面會加上 ` · device released`。出錯時，訊息會取代這一列（見[失敗處理](#failures)）；連上播放器的 TUI 與播放器失去連線時，這一列也會顯示 `Disconnected from the player: reconnecting…`
4. 進度條、目前位置與長度。若 Tidal 未提供長度，則不顯示進度條：`1:23 / ?:??`

啟動時播放器會接續上次的工作階段：原本的佇列，以 `■` 停止在相同的位置，模式與音量也相同；按 `Space` 從那裡開始播放（見[接續上次的工作階段](playback.md#resuming-the-last-session)）。

<a id="queue"></a>
### 佇列

佇列依播放順序排列（開啟隨機播放時為打亂後的順序）。正在播放的曲目標記為 `▶`，切換曲目時會保持在畫面中。反白的那一列是**游標**，可用下方的按鍵移動；佇列變動時游標會停留在原本的曲目上，該曲目被移除時則移到相鄰的列。由[自動播放](#shuffle-repeat-and-autoplay)加入的曲目排在 `Suggested` 列之後，並以淡色顯示。

終端機太窄時，欄位會以 `…` 截斷；先截專輯欄，再截藝人欄。終端機少於 8 列時（顯示登入已過期那一行時為 9 列），不論目前在哪個頁面，都只顯示播放視窗。登入工作階段（session）過期時，最後一列會顯示 `Session expired — run "tidal-player login" in another terminal`（見[登入](login.md)）。

<a id="keys"></a>
## 按鍵

以下為預設按鍵及各自執行的指令。每個按鍵都可在 `keymap.toml` 中更改，下方提到的設定則在 `app.toml` 中：請見[設定](config.md)，其中列出所有指令與按鍵語法（`C-s` 是 Control-s，`M-p` 是 Alt-p，`backtab` 是 Shift-Tab）。

| 按鍵 | 指令 | 作用 |
|---|---|---|
| `space` | `ResumePause` | 播放／暫停 |
| `n` / `p` | `NextTrack` / `PreviousTrack` | 下一首／上一首 |
| `>` / `<` | `SeekForward` / `SeekBackward` | 依跳轉間隔（5 秒）往後／往前跳轉 |
| `^` | `SeekStart` | 回到曲目開頭 |
| `C-s` | `Shuffle` | 開啟／關閉隨機播放 |
| `C-r` | `Repeat` | 重複播放：關 → 佇列 → 單曲 → 關 |
| `A` | `ToggleAutoplay` | 開啟／關閉自動播放 |
| `+` / `-` | `VolumeUp` / `VolumeDown` | 依音量間隔（5 %）調高／調低音量 |
| `_` | `Mute` | 靜音／取消靜音 |
| `o` | `AddToQueuePrompt` | 將連結或曲目 ID 加到佇列尾端 |
| `O` | `PlayNextPrompt` | 將連結或曲目 ID 加入為下一首播放 |
| `j`, `down`, `C-n` / `k`, `up`, `C-p` | `SelectNextOrScrollDown` / `SelectPreviousOrScrollUp` | 游標下移、上移 |
| `g g` / `G`, `end` | `SelectFirstOrScrollToTop` / `SelectLastOrScrollToBottom` | 游標移到最上方、移到最後一個已載入的列 |
| `C-f`, `page_down` / `C-b`, `page_up` | `PageSelectNextOrScrollDown` / `PageSelectPreviousOrScrollUp` | 游標下移、上移一個視窗的高度 |
| `enter` | `ChooseSelected` | 在佇列上：播放該項目；在頁面上：連同所在清單播放該曲目，或開啟專輯、播放清單或藝人（見[從頁面播放與加入佇列](#playing-and-queueing-from-a-page)）；在彈出視窗中：執行該項目 |
| `Z`, `C-z` | `AddSelectedItemToQueue` | 將選取的曲目、專輯或播放清單加到佇列尾端 |
| `d` | `RemoveFromQueue` | 從佇列移除游標所在的項目 |
| `z` | `Queue` | 開啟佇列頁面 |
| `g l` | `LibraryPage` | 開啟音樂庫 |
| `g y` | `LikedTrackPage` | 開啟收藏的曲目 |
| `g s` | `SearchPage` | 開啟[搜尋](#search)頁面（在搜尋頁面上：回到輸入框） |
| `g m` | `MixesPage` | 開啟你的 [mix](#mixes-and-radio) |
| `r` | `GoToRadio`（`[[actions]]` 項目） | 開啟選取曲目或藝人的[電台](#mixes-and-radio) |
| `/` | `Search` | 在搜尋頁面上：回到輸入框 |
| `backspace`, `C-q` | `PreviousPage` | 回到上一個頁面 |
| `tab`, `backtab` | `FocusNextWindow`, `FocusPreviousWindow` | 將焦點移到頁面的下一個、上一個窗格（在藝人頁面上為左半或右半） |
| `[`, `]` | `PreviousTab`, `NextTab` | 顯示焦點窗格的上一個、下一個分頁（藝人頁面的 *Top tracks* / *All tracks* 與 *Albums* / *Appears on*） |
| `g a`, `C-space` | `ShowActionsOnSelectedItem` | 對選取列的[動作](#actions) |
| `a` | `ShowActionsOnCurrentTrack` | 對正在播放曲目的動作 |
| `f` | `RoleFilter` | 在藝人的 *All tracks* 中：[角色篩選](#the-role-filter) |
| `esc` | `ClosePopup` | 關閉彈出視窗或開啟中的輸入提示，或取消正在載入的清單；其他情況下沒有作用 |
| `?`, `C-h` | `OpenCommandHelp` | [按鍵說明](#the-keys-help) |
| `q`, `C-c` | `Quit` | 離開（連上播放器的 TUI 會中斷連線；播放器繼續播放） |

`g g` 是按兩次 `g`；`g l`、`g y`、`g s`、`g m` 與 `g a` 是先按 `g` 再按第二個鍵。`g` 之後若接其他按鍵，就執行那個按鍵原本的作用。按下 `g` 後稍候，會有[提示](#key-hints)列出可接的第二個鍵。佇列為空時，在佇列上只有音量、靜音與模式按鍵（以及 `o`/`O`、`q`）有作用；此時設定的模式與音量會套用到之後加入的內容。

彈出視窗開啟時，只有它自己的按鍵有作用（見[動作](#actions)）；`space`、`n`、`q` 等按鍵不會傳到播放器。文字輸入框（`o`/`O` 的輸入提示、搜尋輸入框、播放清單名稱）會接收所有可列印的按鍵，不受按鍵對應影響。

間隔設定為 `app.toml` 中的 `volume_step`（1–25 %，預設 5）與 `seek_duration_secs`（1–600 秒，預設 5），或環境變數 `TIDAL_PLAYER_VOLUME_STEP` 與 `TIDAL_PLAYER_SEEK_STEP`；見[設定項目](playback.md#settings)。連上播放器的 TUI 使用自己的 `keymap.toml` 與間隔設定，而非常駐程式的。

<a id="the-keys-help"></a>
### 按鍵說明

除了在文字輸入框中，任何地方按 `?`（或 `C-h`）都會開啟標題為 `Keys` 的彈出視窗，列出**目前所在位置**有作用的按鍵：先列出開啟中的彈出視窗或焦點視窗的按鍵（`Queue`、`Library · Albums`、`Search · Tracks`……），接著是 `Lists`、`Pages`、`Playback`、`Actions`（你的 `[[actions]]` 綁定）與 `App`。顯示的是套用 `keymap.toml` 之後實際生效的按鍵，並使用該檔案的語法，因此可以直接貼進檔案中。目前按下也不會有作用的按鍵（佇列為空時的播放按鍵、失去連線時的任何按鍵）會以淡色顯示。

```
┌Keys────────────────────────────────────────────────────────┐
│ Library · Albums                                           │
│   enter           open the album                           │
│   Z  C-z          add to the end of the queue              │
│   g a  C-space    actions on the selected row              │
│ Lists                                                      │
│   j  down  C-n    move down                                │
│   …                                                        │
│ / filter · enter run · esc close                           │
└────────────────────────────────────────────────────────────┘
```

- 清單按鍵（`j`、`k`、`C-f`、`G`……）可在按鍵之間移動反白
- `/` 開始**篩選**：輸入文字後，只保留按鍵、指令名稱或說明文字中包含所輸入內容的項目（不分大小寫）；`backspace` 刪除字元，`enter` 結束輸入並保留篩選，`esc` 清除篩選。沒有符合的項目時顯示：`No keys match "xyz"`
- `enter` 關閉說明並**執行**反白的按鍵，效果如同在開啟說明的位置按下該鍵
- `esc`（沒有篩選時）、`?` 或 `q` 關閉說明。說明開啟期間，其他按鍵都沒有作用

<a id="key-hints"></a>
### 按鍵提示

按下按鍵序列的第一個鍵（`g`）後稍候，一秒後頁面底部會開啟一個**提示**框，標題是目前已按下的按鍵（`g …`），列出接下來可以按的按鍵，以及每個鍵在此處的作用：

```
│┌g …─────────────────────────────────────────────────────────────────────────┐│
││a  actions on the selected row     y  favorite tracks                       ││
││g  move to the top                 s  the search page (on one: its…         ││
││l  the library                     m  your mixes                            ││
│└────────────────────────────────────────────────────────────────────────────┘│
```

- 只列出在目前位置有作用的按鍵，說明文字與順序都與[按鍵說明](#the-keys-help)相同；目前按下也不會有作用的按鍵會以淡色顯示。你在 `keymap.toml` 中設定的序列也會列出；若某個鍵是更長序列的開頭，會顯示它通往多少個綁定（`l  +2`）：按下它即可看到下一層
- 提示不會改變任何按鍵的作用：無論提示是否出現，都照常按下一個鍵。若在延遲時間內就輸入完序列，提示不會出現。序列完成、按下不屬於任何序列的鍵（`esc` 可取消），或開啟按鍵說明時，提示就會關閉
- 項目放不下時，最後一格顯示 `… +N more`；按 `?` 可看到全部。按鍵說明開啟時、整份清單載入期間，或終端機小到無法顯示頁面時，不會顯示提示
- 在 `app.toml` 中設定 `key_hints = false`（或 `TIDAL_PLAYER_KEY_HINTS=off`）即可關閉；`key_hints_delay_ms`（0–10 000，預設 `1000`；`0` 表示立即顯示，或使用 `TIDAL_PLAYER_KEY_HINTS_DELAY_MS`）設定等待時間。連上播放器的 TUI 使用自己的設定；見 [`app.toml`](config.md#apptoml)

<a id="pages"></a>
## 頁面

播放視窗下方的區域一次顯示一個頁面。開啟頁面會將它放到**歷史紀錄**的最上層；`Backspace`（或 `Ctrl-q`）會回到下面那一頁，且完全保持離開時的樣子（列、游標與焦點），不會重新抓取。開啟頁面則一律重新抓取。TUI 啟動時顯示音樂庫，其下是佇列（按 `Backspace` 或 `z` 顯示）。佇列永遠位於歷史紀錄的最底層，無法關閉；開啟已在最上層的頁面不會有任何作用，歷史紀錄最多保留最近 50 個頁面。

| 頁面 | 開啟方式 | 視窗（`Tab` 在窗格之間移動） |
|---|---|---|
| 佇列 | `z`；啟動時在音樂庫按 `Backspace` | 佇列 |
| 音樂庫 | `g l`；啟動時的頁面 | Playlists、Albums、Artists |
| 收藏的曲目 | `g y` | 曲目 |
| 搜尋 | `g s` | 輸入框、最佳結果、Tracks、Albums、Artists、Playlists（見[搜尋](#search)） |
| Mixes | `g m` | 你的 mix（見 [Mix 與電台](#mixes-and-radio)） |
| Mix | 在 mix 上按 `Enter` | 該 mix 的曲目 |
| 電台 | 在曲目或藝人上按 `r`；*Go to radio* | 電台的曲目 |
| 專輯 | 在專輯上按 `Enter`；*Go to album* | 專輯的曲目 |
| 播放清單 | 在播放清單上按 `Enter` | 播放清單的曲目 |
| 藝人 | 在藝人上按 `Enter`；*Go to artist* | 兩個窗格，各有兩個分頁：Top tracks \| All tracks，以及 Albums \| Appears on（`[` `]` 切換窗格的分頁） |

每個頁面在視窗上方都有一列標題：`Library`、`Favorite tracks · 362 tracks`、`<album> · <artists> · <year> · 17 tracks · 1:02:15`、`<playlist> · 39 tracks · 2:41:07`、`Mixes · 7 mixes`、`<mix> · 10 tracks`、`<track> Radio · <artists>`、`<artist> Radio`，或藝人名稱。數量是 Tidal 提供的總數，第一批列一抵達就會顯示。

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────  1:23 / 3:32│
│Library                                                                       │
│┌Playlists (22)───────────────┐┌Albums (14)──────────────────┐┌Artists (196)─┐│
││  Running                  42││Collide With Th…  Pierce The…││Pierce The Ve…││
││♥ Late night               17││Misadventures     Pierce The…││Sleeping With…││
││  Gym mix                   8││Hold On Till May  Pierce The…││Bring Me The… ││
│…                                                                             │
```

<a id="windows"></a>
### 視窗

含有多個視窗的頁面，在外框內寬至少 60 欄時，會將視窗並排顯示：

- **音樂庫**：Playlists 40 %、Albums 40 %、Artists 20 %（[`app.toml`](config.md#apptoml) 中的 `[layout] library = { playlist_percent, album_percent }` 可調整前兩者；Artists 佔剩餘的寬度）
- **藝人**：左窗格（60 %）有 *Top tracks* 與 *All tracks* 兩個分頁，右窗格（40 %）有 *Albums*（先列專輯，再列 EP 與單曲）與 *Appears on*；窗格顯示目前的分頁，標題列出該窗格的分頁並反白目前的那一個，後面接 `[ ]`：`Top tracks (91) │ All tracks  [ ]`（標題太窄放不下另一個分頁名稱時就省略它）

不足 60 欄時只顯示焦點視窗，其標題後面接 `‹Tab›`（在藝人頁面上，先顯示分頁清單與 `[ ]`，再顯示代表另一個窗格的 `‹Tab›`）。`Tab` 將焦點移到下一個窗格，`Shift-Tab` 移到上一個，到底會循環，且每個窗格停在它上次顯示的分頁；`[` 與 `]` 切換焦點窗格的分頁（*All tracks* 在第一次顯示時才抓取）；在窗格只有一個分頁的頁面上沒有作用。每個視窗都有自己的游標；焦點視窗的游標反白，其他視窗的游標以淡色顯示。播放清單列顯示曲目數量，你追蹤的播放清單會加上 `♥`；專輯列顯示藝人與年份（視窗夠寬時還有 `EP` 或 `Single`）；Tidal 在你所在國家不提供串流的曲目以淡色顯示，因為播放器會略過它們。

<a id="lists-load-as-you-scroll"></a>
### 清單隨捲動載入

每個清單都是一次抓取一頁：開啟頁面時抓第一頁，之後當游標接近最後一個已載入的列、距離不到一個視窗高度時，抓取下一頁。抓取期間，最後一列顯示 `Loading more…`；若抓取失敗，該列改為顯示錯誤訊息，下次游標移近清單尾端時會再試一次。視窗標題中的總數來自 Tidal，一開始就已知道，所以 `Favorite tracks (362)` 在所有列載入前就會顯示總數。`G` 會移到目前已載入的最後一列（因此會載入下一頁）。視窗的列在音樂播放時於背景載入，絕不會延遲按鍵反應。

每頁大小為 `app.toml` 中的 `page_size` 或環境變數 `TIDAL_PLAYER_PAGE_SIZE`（1–10 000，預設 100；播放清單與參與作品（credits）清單每頁最多抓取 50 筆）；見[設定項目](playback.md#settings)。

頁面抓取期間，其視窗顯示 `Loading…`。抓取失敗時，頁面中會顯示錯誤訊息（`Could not load the library: Could not reach Tidal: …`、`Album 123 was not found`、登入已過期的訊息）；`Backspace` 可返回，重新開啟該頁面會再試一次。清單為空時會明確說明：`No playlists yet`、`No favorite albums yet`、`No favorite artists yet`、`No favorite tracks yet`、`This album has no tracks`、`This playlist has no tracks`、`No top tracks`、`No albums`、`No credits`（篩選後沒有剩下任何項目時為 `No credits (37 hidden)`）。

無法連上播放器時（連上播放器的 TUI，而播放器已離線），仍可瀏覽已載入的頁面並使用歷史紀錄，但不會抓取任何新內容：此時開啟的頁面會顯示 `Disconnected from the player: reconnecting…`，並在播放器恢復後重新抓取。

<a id="the-artists-all-tracks"></a>
### 藝人的 *All tracks*

*All tracks* 是 Tidal 的「Credits for <artist>」：該藝人以演出者、詞曲作者、製作人或錄音工程師身分列名的所有曲目，依熱門程度排序。第一次將焦點移到它時才會抓取。寬視窗會在專輯旁顯示藝人在每首曲目上的角色類別（`Performer, Songwriter`）。

以下項目會被排除，並在視窗標題中顯示隱藏的數量（`All tracks (548 · 37 hidden)`）：沒有立體聲混音的版本（只有 Dolby Atmos，播放器無法解碼），以及**替代版本**：曲目的版本名稱，或曲名中括號內的部分或 ` - …` 後綴，屬於隱藏字詞之一，例如 `Instrumental`、`TV Size`、`Sped Up` 或 `Slowed + Reverb`。`Acoustic`、`Live` 與混音版本會保留。隱藏字詞由 `TIDAL_PLAYER_HIDE_VERSIONS` 設定；見[設定項目](playback.md#settings)。其他視窗會顯示所有版本。

<a id="the-role-filter"></a>
#### 角色篩選

在 *All tracks* 中按 `f` 會開啟彈出視窗，以核取方塊列出四種角色類別（Performer、Songwriter、Producer、Engineer），預設全部勾選。`j`/`k` 移動，`Space` 切換勾選，`Enter` 套用，`Esc` 取消。只會顯示藝人具有已勾選角色的曲目，標題會註明勾選了哪些（`All tracks (548 · Performer, Songwriter · 37 hidden)`）。捲動時仍會照常載入更多列。只要該頁面還在歷史紀錄中，篩選就會持續有效。

<a id="search"></a>
## 搜尋

`g s` 開啟搜尋頁面：一列輸入框（`Search: `）且游標在其中、一列**最佳結果**，以及四個視窗：上方是 *Tracks* 與 *Albums*，下方是 *Artists* 與 *Playlists*。

```
│Search · "pierce the veil"                                                    │
│Search: pierce the veil                                                       │
│Top hit: Pierce The Veil · artist                                             │
│┌Tracks (223)──────────────────────┐┌Albums (55)──────────────────────────────┐│
││King For A Day    Pierce The Veil ││Collide With The Sky  Pierce The Veil 2012││
││Hell Above        Pierce The Veil ││The Jaws Of Life      Pierce The Veil 2023││
│┌Artists (7)───────────────────────┐┌Playlists (3)────────────────────────────┐│
││Pierce The Veil                   ││Pierce The Veil Essentials          15   ││
│…                                                                             │
```

**輸入。** 輸入框有游標時，每個按鍵都會輸入到框中，因此 `q`、`n`、`Space` 與 `g` 不會傳到播放器；也可以貼上。`Backspace` 刪除最後一個字元（輸入框為空時沒有作用），`Ctrl-u` 清空輸入框，查詢字串最長 200 個字元。`Ctrl-c` 仍會離開，`Ctrl-q` 仍會返回。

**搜尋。** `Enter` 執行搜尋（輸入框為空時沒有作用）。各視窗先顯示 `Loading…`，接著顯示 Tidal 的結果與總數，游標移到最佳結果（若無，則移到第一個有結果的視窗）。在同一頁面上再次搜尋會取代原有結果。Tidal 的**最佳結果**是它認為最符合的項目，可以是任何類型：`Pierce The Veil · artist`、`Collide With The Sky · album`、一首曲目或一個播放清單；Tidal 沒有提供時就不顯示最佳結果列。

**移動。** `Tab` 與 `Shift-Tab` 依序在輸入框 → 最佳結果 → Tracks → Albums → Artists → Playlists → 輸入框之間移動。在視窗上，每個按鍵的作用都和其他頁面相同（游標按鍵、`Enter`、`Z`、`g a`、`Backspace`……）；`/` 會回到輸入框，`g s` 也是。在輸入框中按 `Esc` 會移到結果。透過歷史紀錄回到搜尋頁面時，查詢字串與結果都保持離開時的樣子；從其他頁面按 `g s` 則會開啟一個新的空白搜尋頁面。

**結果隨捲動載入**，一次 20 筆（`TIDAL_PLAYER_SEARCH_PAGE_SIZE`，1–1000；見[設定項目](playback.md#settings)），直到視窗載入 Tidal 的所有結果為止（每個查詢 Tidal 最多只有幾百筆）。只有 Dolby Atmos 版本的曲目會被排除，因為播放器無法播放，所以標題中的總數可能略多於實際的列數。

**播放。** 在結果曲目上按 `Enter` 會播放該曲目，並將 *Tracks* 中**已載入**的曲目依結果順序排入它的前後：剛搜尋完時是 20 首；想排入更多曲目，請先往下捲動。不會為了這個佇列另外抓取任何內容。在最佳結果上按 `Enter`，若是曲目則作用相同（最佳結果曲目若不在已載入的列中，會先播放它，再接已載入的列），若是專輯、藝人或播放清單則開啟其頁面。專輯、藝人與播放清單會開啟各自的頁面；`Z` 與[動作](#actions)的作用與其他頁面相同（搜尋結果中的播放清單一律不視為你自己的）。

沒有符合的結果時顯示：`No tracks found`、`No albums found`、`No artists found`、`No playlists found`。搜尋失敗時，頁面中會顯示 `Could not search: <reason>`。

<a id="mixes-and-radio"></a>
## Mix 與電台

`g m` 開啟你的 **mix**：每日 mix、*My Daily Discovery* 與 Tidal 為你建立的其他 mix，依 Tidal 的順序排列，每列先顯示標題再顯示副標題（`My Mix 1  Pierce The Veil, Sleeping With Sirens and more`），放不下時以 `…` 截斷。在 mix 上按 `Enter` 開啟其頁面：標題列是 mix 的標題，頁面至少 8 列高時，其下以暗色顯示副標題，再來是它的曲目。mix 是像專輯一樣的曲目清單：在曲目上按 `Enter` 會播放它，並將 mix 的其餘曲目排入佇列；在 mix 上按 `Z` 沒有作用（請先開啟它），它的動作選單只有 *Open*。

在選取的曲目或藝人上按 `r`（任何頁面，包括佇列）會開啟它的**電台**：`<track> Radio · <artists>` 或 `<artist> Radio`，最多 100 首 Tidal 推薦的曲目。在你於曲目上按 `Enter`（播放它，並將電台其餘曲目排入佇列）或 `Z` 之前，不會播放任何東西，佇列也維持原樣。[動作](#actions)選單中的 *Go to radio* 作用相同，綁定在其他按鍵的 `[[actions]]` 項目亦同（見[設定](config.md#actions)）；專輯、播放清單與 mix 沒有電台。

這些頁面一次完整送達，因此沒有隨捲動載入，且每次開啟都會重新抓取；`Backspace` 會回到保持離開時樣子的頁面。頁面中的訊息：`No mixes yet`、`This mix has no tracks`、`No radio for this track`、`No radio for this artist`（Tidal 沒有它的電台，並非失敗），失敗時為 `Could not load the mixes: <reason>`、`Mix <id> was not found`、`Track 1 was not found`、`Artist 1 was not found`。曲目列與其他頁面相同，無法串流的曲目以暗色顯示。

<a id="playing-and-queueing-from-a-page"></a>
## 從頁面播放與加入佇列

| 按鍵 | 對象 | 作用 |
|---|---|---|
| `Enter` | 頁面上的曲目 | 以**該清單的所有曲目**依顯示順序取代佇列，並播放你選的那一首，播完後佇列會繼續往下播 |
| `Enter` | 佇列中的曲目 | 播放該項目 |
| `Enter` | 專輯、播放清單、藝人或 mix | 開啟其頁面（不會播放） |
| `Z`, `Ctrl-z` | 曲目 | 加到佇列尾端；若沒有在播放則開始播放 |
| `Z`, `Ctrl-z` | 專輯或播放清單 | 將其所有曲目加到佇列尾端 |
| `Z`, `Ctrl-z` | 藝人或 mix | 沒有作用 |
| `d` | 佇列中的項目 | 移除該項目（移除正在播放的項目時會接著播下一首） |

在曲目上按 `Enter` 需要完整的清單，因此只載入部分的清單會先抓取到最後：播放視窗的訊息列會顯示 `Loading 300 of 1 234…`，按 `Esc` 可取消，且不會送出任何內容。超過 40 000 首曲目的清單會被拒絕（`Too many tracks to queue at once (N)`）。

<a id="actions"></a>
## 動作

`g a` 或 `Ctrl-Space` 會開啟一個小型動作選單，列出可對選取列執行的動作，標題為該項目的名稱；`a` 則對正在播放的曲目開啟（沒有在播放時沒有作用）。`j`/`k` 移動，`Enter` 執行動作並關閉選單，`Esc` 關閉。

| 對象 | 動作（依此順序） |
|---|---|
| 頁面上的曲目 | *Go to album*、*Go to artist: <name>*（每位藝人一項）、*Go to radio*、*Add to queue*、*Play next*、*Add to favorites*、*Remove from favorites*、*Add to playlist…*，在你自己的播放清單頁面上還有 *Remove from this playlist* |
| 佇列中的項目，或正在播放的曲目 | *Go to album*、*Go to artist: …*、*Go to radio*、*Play next*（正在播放的曲目沒有此項）、*Remove from queue*、*Add to favorites*、*Remove from favorites*、*Add to playlist…* |
| 專輯 | *Open*、*Go to artist: …*、*Add to queue*、*Play next*、*Add to favorites*、*Remove from favorites*、*Add to playlist…* |
| 播放清單 | *Open*、*Add to queue*、*Play next*、*Add to favorites*、*Remove from favorites*（你自己的播放清單沒有這兩項）、*Delete playlist*（僅限你自己的） |
| 藝人 | *Open*、*Go to radio*、*Add to favorites*、*Remove from favorites* |
| Mix | *Open* |

沒有專輯的曲目不會出現 *Go to album*。Tidal 目前無法查詢單一項目是否已收藏，因此 *Add to favorites* 與 *Remove from favorites* 都會列出；不適用的那一項執行了也無妨。

<a id="favorites-and-playlists"></a>
### 收藏與播放清單

結果會顯示在播放視窗的訊息列：`Added to favorites`、`Removed from favorites`、`Added 12 tracks to <playlist>`、`Removed from <playlist>`、`Created <playlist>`、`Deleted <playlist>`，或錯誤訊息。顯示變動內容的頁面（收藏、音樂庫、播放清單）在變動後會重新抓取。

- **Add to playlist…** 會開啟第二個選單，在 *New playlist…* 之下列出你自己的播放清單（最新的在前）；`Enter` 將該曲目，或專輯的所有曲目，加到播放清單尾端。若曲目已在播放清單中（依已載入的內容判斷），會詢問 `Already in <playlist>: add again? (y/n)`
- **New playlist…** 會在頁面頂端那一列詢問 `Playlist name: `；`Enter` 以該名稱建立私人播放清單並將曲目加入；`Esc` 取消；名稱為空時沒有作用
- **Remove from this playlist** 移除該位置的曲目。若播放清單在這段期間已於其他地方被更改，則不會移除任何內容（`The playlist changed: nothing was changed, try again`），並重新抓取該頁面
- **Delete playlist** 會詢問 `Delete <playlist>? (y/n)`；`y` 刪除，`n` 或 `Esc` 取消

<a id="adding-tracks"></a>
## 加入曲目

`o` 會在佇列頂端開啟輸入提示 `Add to queue: `；`O` 則開啟 `Play next: `。輸入或貼上（使用終端機的貼上功能，大多數終端機為 `Ctrl-Shift-v`）曲目 ID，或 Tidal 的曲目、專輯或播放清單連結，格式與[命令列](playback.md#items)相同，然後：

- `Enter` 將項目送給播放器，播放器會抓取曲目並加到佇列尾端（`o`）或緊接在正在播放的曲目之後（`O`）。若沒有在播放（佇列為空，或播放器已停止且沒有目前曲目），會開始播放第一首加入的曲目。播放器加入後，佇列就會顯示這些曲目
- `Backspace` 刪除最後一個字元；`Esc` 關閉輸入提示，不加入任何內容（其他情況下 `Esc` 沒有作用：`q` 才是離開）

輸入提示開啟期間，每個按鍵都會輸入到其中：`Space`、`q` 與其他按鍵不會傳到播放器。項目無效（`Not a Tidal track, album or playlist: …`）或抓取失敗（`Album 123 was not found`）時，會關閉輸入提示並在播放視窗中顯示訊息；佇列不變。

<a id="playback"></a>
## 播放

- **上一首**（`p`）：已播放超過 3 秒時回到曲目開頭，否則回到上一首（`TIDAL_PLAYER_PREVIOUS_RESTART`，0–60 秒；設為 `0` 時一律回到上一首）。在第一首曲目上則回到開頭
- **下一首**（`n`）：在佇列尾端（且重複播放與自動播放都關閉）時，會停在最後一首曲目的 0:00；之後按 `Space` 會再次播放它
- **跳轉**超過結尾會結束該曲目，接著就像自然播完一樣往下播；跳到 0:00 之前則停在 0:00。在曲目載入期間跳轉，會設定它開始播放的位置
- **載入期間暫停**：曲目會繼續載入，但要再按一次 `Space` 才會開始播放
- 格式相同的曲目之間會無縫播放（下一首曲目會在結束前 30 秒準備好）

<a id="shuffle-repeat-and-autoplay"></a>
### 隨機播放、重複播放與自動播放

- **隨機播放**（`Ctrl-s`）會將正在播放的曲目放到最前面，並打亂其餘曲目；佇列會顯示新的順序。關閉時會恢復原本的順序。不論開或關，正在播放的曲目都會繼續播放。隨機播放開啟期間加入的曲目會放在你指定的位置，不會被打亂
- **重複播放**（`Ctrl-r`）：`queue` 在最後一首之後從第一首重新開始；`track` 在曲目結束時再播一次同一首（`n` 仍會跳到下一首）
- **自動播放**（`A`，預設關閉；`TIDAL_PLAYER_AUTOPLAY=on` 會在啟動時開啟）讓佇列播完後音樂不中斷：在最後一首曲目快結束時，向 Tidal 取得相關曲目，加入佇列（位於 `Suggested` 之下，不含已在佇列中的曲目），並無縫接著播放。重複播放的 `queue` 或 `track` 優先。Tidal 沒有可建議的曲目時，佇列會在尾端停止，並顯示 `Autoplay: no suggestions (…)`

<a id="volume"></a>
### 音量

範圍為 0 到 100 %，**預設 100 %**，每次調整一個音量間隔。在 100 % 時不會更動取樣，因此位元完美的輸出會保持位元完美；低於 100 %（或靜音）時，第三列會顯示 `not bit-perfect: volume below 100%`（或 `muted`）。音量依符合人耳感知響度的曲線變化（50 % 約為 −18 dB）。靜音（`_`）會保留音量，因此取消靜音時會恢復原音量；調整音量會取消靜音。音量與靜音會連同佇列與各模式跨次執行記住（見[接續上次的工作階段](playback.md#resuming-the-last-session)）。

<a id="failures"></a>
## 失敗處理

訊息會顯示在播放視窗中、取代格式那一列，直到下一首曲目開始或被另一則訊息取代為止。

| 失敗的部分 | 例子 | 結果 |
|---|---|---|
| 僅該曲目 | 在你的國家無法使用、僅提供試聽、找不到、無法解碼 | 顯示訊息並播放下一首。連續 5 首（若佇列較短，則為整個佇列）都無法播放時停止播放：`Stopped: 5 tracks in a row could not be played` |
| 網路或 Tidal | 重試後連線仍中斷、`429`、`5xx` | 停在該曲目原本的位置。絕不略過。按 `Space` 再試一次 |
| 輸出 | `Output hw:1,0 is busy …`、`No such output device …`、`Output hw:1,0 was lost` | 停在該曲目；保留佇列。按 `Space` 在同一個裝置上再試一次 |
| 登入工作階段 | Tidal 不再接受目前的登入 | 停止；最後一列顯示登入已過期的狀態。在另一個終端機執行 `tidal-player login` 後，按 `Space` 繼續播放 |

這些訊息與 `play` 的相同：見[錯誤](playback.md#errors)。
