[English](../0010-mpris.md) | 正體中文

<a id="0010--mpris2-and-media-keys"></a>
# 0010 — MPRIS2 與媒體鍵

- **狀態**：implemented（2026-10-09；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0002/0006（implemented：解析曲目的中繼資料與音樂庫客戶端）、0004（implemented：播放器與其協定）、0005（implemented：每位使用者一個播放器、客戶端的 hub、一次性指令）、0008（implemented：`app.toml` 及其優先順序）、0009（implemented：還原的播放器是停止的）
- **使用者文件**：新增的 [`docs/mpris.md`](../../zh-TW/mpris.md)（桌面控制、媒體鍵、`playerctl`）；[`docs/daemon.md`](../../zh-TW/daemon.md)（新的一次性指令、「`playerctl` 可用」）、[`docs/config.md`](../../zh-TW/config.md)（`mpris`、`max_cover_arts`、快取目錄）、[`docs/playback.md`](../../zh-TW/playback.md)（連到新頁面的連結）及它們的 zh-TW 副本（AC17）

<a id="context"></a>
## 背景

播放器只能從它自己的 TUI 與 `tidal-player playback …` 控制。桌面環境（GNOME、KDE Plasma）、狀態列（waybar、polybar）、鎖定畫面、藍牙耳機（透過 BlueZ 的 AVRCP 橋接）、KDE Connect 與 `playerctl` 都透過 **MPRIS2** 控制播放器，也就是 D-Bus 介面 `org.mpris.MediaPlayer2`；鍵盤的媒體鍵在每個現行的 Linux 桌面上也是以同樣的方式送達播放器（`gsd-media-keys`、KWin，以及平鋪式視窗管理員對 `playerctl` 的綁定）。0001 規劃了它，0004 把「setter」指令延後到這裡，0005 則決定 MPRIS 是**建立在播放器之上的公開轉接器**，與客戶端 socket 分開（0005 決策 1）。

本規格讓**每個播放器行程**（獨立模式的 TUI、`daemon`、`play`）都在工作階段匯流排（session bus）上把自己發佈為 MPRIS2 媒體播放器，即時反映它的狀態，並透過客戶端所送的同一批 `Command` 來回應 MPRIS 呼叫。

<a id="what-tidalt-did"></a>
### tidalt 的做法

取自 tidalt 的 `internal/mpris/server.go`、`internal/ui/model.go`（`SetState`）與 commit `3bace90`、`c7d5e2d`、`9660ad1`、`c914a5d`：

- `org.mpris.MediaPlayer2.tidalt` 位於 `/org/mpris/MediaPlayer2`，有 `org.mpris.MediaPlayer2`、`org.mpris.MediaPlayer2.Player`、手寫的 `org.freedesktop.DBus.Properties` 與 `Introspectable`，以及同一個物件上的私有 `io.tidalt.App`。取得名稱同時也是「單一執行個體」的鎖（0005「tidalt 的做法」）
- UI 模型把 `PlayerState`（曲目與播放清單的 JSON 字串）推入由 mutex 保護的副本；`Get`/`GetAll` 需要時才讀取
- `Player` 實作了 `PlayPause`、`Next`、`Previous`；**`Play` 與 `Pause` 都送出播放／暫停切換**；`Stop` 什麼也不做
- 呼叫變成 4 格通道上的事件，並以**非阻塞的送出**傳遞：UI 忙碌時呼叫被丟棄，呼叫者仍得到成功
- 屬性：`PlaybackStatus`、`Metadata`、`CanPlay`/`CanPause`/`CanGoNext`/`CanGoPrevious` 一律為 `true`、`CanSeek` 為 `false`、`Rate` 為 1.0。**未實作**：`Position`、`Seek`、`SetPosition`、`Seeked`、`Volume`、`Shuffle`、`LoopStatus`、`OpenUri`
- `Metadata`：`mpris:trackid` 為 `/org/mpris/MediaPlayer2/Track/<tidal track id>`、`xesam:title`、`xesam:artist`（**只有第一位藝人**）、`xesam:album`、`mpris:length`
- `Set` 對所有屬性都回答 `PropertyReadOnly`，而且**從未發出 `PropertiesChanged`**
- 沒有 `mpris:artUrl`，儘管 TUI 畫出了專輯封面（Kitty graphics，之後是 sixel：`internal/ui/coverart.go`、`becf508`），封面取自 `tidal.CoverURL(track.Album.Cover, "640x640")`：`https://resources.tidal.com/images/<cover UUID with each - replaced by />/<size>.jpg`

<a id="what-went-wrong-there-and-the-criterion-that-covers-it-here"></a>
### 當時出了什麼問題，以及這裡由哪個驗收條件負責

1. **沒有 `PropertiesChanged`**：GNOME 的媒體小工具、waybar 與 `playerctl --follow` 只在收到該訊號時才更新，因此只顯示它們第一次查看時播放器的曲目 → `Player` 屬性除了 `Position` 以外的每一次變更，都在狀態變更之後、依序發出一個 `PropertiesChanged`，只帶有確實變更的屬性（AC5）
2. **`Play`/`Pause` 是切換**：對已暫停的播放器執行 `playerctl pause` 會讓它繼續播放，耳機的暫停鍵也一樣 → `Play`、`Pause`、`Stop` 是明確的播放器指令，由播放器依照它的真實狀態決定，絕不是用過時的副本算出來的切換（AC1、AC8）
3. **呼叫被默默丟棄**（非阻塞的送出）→ 每個呼叫都像任何客戶端的請求一樣，經由客戶端 hub 送達播放器，在播放器處理後才回應；播放器拒絕或逾時未回應的呼叫會回傳 D-Bus 錯誤（AC9）
4. **能力是假的**（佇列為空卻 `CanGoNext`、TUI 有跳轉卻 `CanSeek` 為 false）→ `Can*` 屬性由狀態推導，並隨之改變（AC3）
5. **由 Tidal ID 產生的曲目 ID**（排入佇列兩次的曲目只有一個 ID，而且 `/org/mpris` 是 MPRIS 規格為自己的路徑保留的）→ `mpris:trackid` 由**佇列項目 ID** 產生，位於我們自己的路徑之下（AC2）
6. **只有一位藝人，沒有封面**：`xesam:artist` 列出曲目的每一位藝人，`mpris:artUrl` 給出專輯封面（AC4、AC18）
7. **匯流排名稱是單一執行個體的鎖**：沒有工作階段匯流排就完全沒有播放器，過期的名稱還會擋住啟動 → 播放器的鎖仍是 0005 的檔案鎖；MPRIS 是選用的：沒有工作階段匯流排，或名稱已被佔用，都絕不會讓播放器停止（AC11、AC12）

<a id="behaviour"></a>
## 行為

<a id="where-it-runs"></a>
### 在哪裡執行

每個播放器行程（獨立模式的 TUI、`tidal-player daemon`、`tidal-player play`）在啟動時連上**工作階段匯流排**，時機在載入還原的狀態（0009）之後、接受客戶端之前，並擁有：

- 匯流排名稱 `org.mpris.MediaPlayer2.tidal_player`（`playerctl -p tidal_player`），以 `DoNotQueue` 請求。若名稱已被佔用（以另一個 `TIDAL_PLAYER_RUNTIME_DIR` 執行的播放器），它就依 MPRIS 規格對第二個執行個體的規定，改取 `org.mpris.MediaPlayer2.tidal_player.instance<pid>`
- 物件 `/org/mpris/MediaPlayer2`，帶有 `org.mpris.MediaPlayer2` 與 `org.mpris.MediaPlayer2.Player`（以及標準的 `Properties`、`Introspectable`、`Peer`）

連上的 TUI 與一次性指令從不碰匯流排（它們是客戶端，0005）。播放器結束時，名稱會釋放（它的匯流排連線關閉），時機在 `ShuttingDown` 送達客戶端之後。

沒有工作階段匯流排（`DBUS_SESSION_BUS_ADDRESS` 未設定且沒有 `$XDG_RUNTIME_DIR/bus`，例如透過 SSH 連上的無頭機器）、連線被拒，或匯出物件失敗：播放器照常執行，只是沒有 MPRIS，並在 `info` 等級記錄一行（`MPRIS is not available: <reason>`）。這**不是**播放器的訊息：沒有桌面的機器很正常。這次執行期間絕不會重試匯流排。

<a id="turning-it-off"></a>
### 關閉

`app.toml` 中的 `mpris`（預設為 `true`），或 `TIDAL_PLAYER_MPRIS`（`on`/`off`），優先順序依 0008。關閉：播放器絕不為 MPRIS 連上工作階段匯流排（0003 的裝置保留不受影響）。

<a id="orgmprismediaplayer2"></a>
### `org.mpris.MediaPlayer2`

| 屬性／方法 | 值 |
|---|---|
| `Identity` | `tidal-player` |
| `CanQuit` | `false`；`Quit()` 什麼也不做（桌面小工具不該停止常駐程式；`daemon stop` 與 `q` 才可以） |
| `CanRaise` | `false`；`Raise()` 什麼也不做 |
| `HasTrackList` | `false` |
| `SupportedUriSchemes` | `["tidal", "https"]` |
| `SupportedMimeTypes` | `[]` |
| `DesktopEntry`、`Fullscreen`、`CanSetFullscreen` | 不提供（不安裝 `.desktop` 檔案） |

<a id="orgmprismediaplayer2player-properties"></a>
### `org.mpris.MediaPlayer2.Player`：屬性

來自播放器的狀態（每個客戶端取得的快照，0004）：

| 屬性 | 值 |
|---|---|
| `PlaybackStatus` | `Playing` 對應 `Playing`、`Buffering`，以及未被暫停保持住的 `Loading`；`Paused` 對應 `Paused`（不論是否已釋放，0005）與被保持住的載入；`Stopped` 對應 `Stopped` |
| `LoopStatus`（可讀寫） | `None`、`Playlist`、`Track`，分別對應重複播放 `off`、`queue`、`track` |
| `Shuffle`（可讀寫） | 隨機播放 |
| `Volume`（可讀寫） | 音量 / 100（80 % 為 `0.8`）；靜音時為 `0.0` |
| `Position` | 目前項目的位置，單位為 µs：最後已知的位置，加上 `Playing` 時自回報以來經過的時間（上限為曲目長度）；沒有目前項目時為 `0` |
| `Rate`、`MinimumRate`、`MaximumRate`（可讀寫） | `1.0`；設定 `Rate` 什麼也不做 |
| `Metadata` | 見下 |
| `CanGoNext` | 有目前項目，而且 `Next` 會讓另一個項目成為目前項目：播放順序中它之後還有項目，或重複播放為 `queue`/`track`（0004：重複播放開啟時 `Next` 會繞回） |
| `CanGoPrevious` | 有目前項目（`Previous` 會回上一首或重新開始，0004） |
| `CanPlay` | 有目前項目 |
| `CanPause` | 有目前項目 |
| `CanSeek` | 有目前項目，而且它的長度已知 |
| `CanControl` | `true`（常數，MPRIS 規格要求如此） |

目前項目的 **`Metadata`**；沒有目前項目時，只有 `mpris:trackid` `/org/mpris/MediaPlayer2/TrackList/NoTrack`：

| 鍵 | 值 |
|---|---|
| `mpris:trackid` | `/tidal_player/entry/<entry id>`（物件路徑；每個佇列項目唯一，因此排入佇列兩次的曲目有兩個） |
| `mpris:length` | 佇列項目的長度，單位為 µs（快照所帶的那個），未知時省略 |
| `xesam:title` | 曲名，曲目有版本時加上 ` (<version>)`（與 TUI 的顯示相同，0006） |
| `xesam:artist` | 每一位藝人，依 Tidal 的順序 |
| `xesam:album` | 專輯標題，沒有時省略 |
| `xesam:url` | `https://tidal.com/browse/track/<track id>` |
| `mpris:artUrl` | 專輯封面：封面進入快取後為 `file://<cache dir>/covers/<cover>.jpg`；快取關閉或下載失敗時為 Tidal 的 `https://` URL；下載期間，以及曲目沒有專輯或 Tidal 沒有提供封面時省略（見「封面」） |
| `xesam:trackNumber`、`xesam:albumArtist` | 不提供（不在佇列的 `Track` 中） |

<a id="cover-art"></a>
### 封面

Tidal 隨每首曲目回傳專輯的封面圖片 ID（`album.cover`，一個 UUID，位於 `/tracks`、專輯、播放清單、收藏、搜尋與 mix 頁面的 v1 曲目物件中）。本規格把它一路帶下去：

- `AlbumRef` 新增 `cover: Option<String>`，由中繼資料與音樂庫客戶端在建立曲目的每個地方填入（0002 的 `MetadataClient`、0006 的 `LibraryClient`）；缺少或為 `null` 的封面是 `None`
- 它隨佇列移動（`PlayerSnapshot`，0004），並記錄在 `playback.json`（0009）：在本規格之前寫成、沒有該欄位的檔案仍可載入（欄位預設為無；`version` 不變），它的項目要到再次加入佇列才有封面
- 圖片 URL 由單一純函式建立（`tidal_player_core::track::cover_url(cover, size)`）：`https://resources.tidal.com/images/<cover with each - replaced by />/<size>.jpg`。播放器使用 `640x640`；TUI 的封面規格（不在範圍內，見下）會重用它與快取

<a id="the-cover-cache"></a>
### 封面快取

播放器（絕不是客戶端）下載目前項目的封面，並提供桌面一個本機檔案，讓每個 MPRIS 消費者都能顯示它，只接受 `file://` 的通知常駐程式也包括在內（決策 6）。這是播放器保留的唯一一份磁碟快取；0009 決策 3（沒有中繼資料快取）其餘不變。

- **位置**：**快取目錄**：`$TIDAL_PLAYER_CACHE_DIR`，否則 `$XDG_CACHE_HOME/tidal-player`，再否則 `~/.cache/tidal-player`；封面放在 `covers/`（以 `0700` 建立），每個封面 ID 一個檔案：`<cover>.jpg`
- **時機**：當一個項目成為目前項目，而它的封面不在快取中時，播放器就下載它（一次一個；較新的目前項目的封面會取代等待中的封面；沒有目前項目時不下載任何東西；0016 讓播放器不論 MPRIS 是開啟、關閉或無法使用都下載，這樣 TUI 才能顯示封面）。下載不等待曲目：`Metadata` 立刻發佈、不帶 `mpris:artUrl`，等檔案就位後，若該項目仍是目前項目，再帶著 `file://` URL 發佈一次（`PropertiesChanged`）
- **下載**：對 `640x640` URL 做 `GET`，不帶 Tidal 憑證，逾時 10 秒，大小上限 5 MB；除了在上限之內、本文為 `image/*` 的 `200` 以外，一律算失敗。檔案先寫成 `<cover>.jpg.tmp`，再重新命名，因此快取裡絕不會有半個檔案
- **上限**：`app.toml` 中的 `max_cover_arts`（預設 **20**，整數 0–1000），或 `TIDAL_PLAYER_MAX_COVER_ARTS`，優先順序依 0008。每個新檔案之後，會刪除最久沒用的封面，直到剩下不超過 `max_cover_arts` 個；使用快取中的封面（它再次成為目前項目）會讓它成為最近使用的（它的修改時間會被更新）。目前項目的封面絕不會被刪除。`0` 關閉快取：不下載也不寫入任何東西，而 `mpris:artUrl` 是 `https://` URL
- **失敗**：下載失敗或快取目錄無法寫入時，該項目使用 `https://` URL（桌面仍可能自己取得），每個封面在 `warn` 等級記錄一次，且只有在該封面再次成為目前項目時才重試。絕不是播放器的訊息：缺少的封面不可蓋過播放失敗
- **啟動**：`covers/` 中不是 `<uuid>.jpg` 的檔案（當機留下的 `.tmp`、其他任何東西）會被刪除；上限只套用一次（調低的 `max_cover_arts` 在下次啟動時生效）
- `logout` 留下快取：封面是公開的圖片，不是帳號的資料

<a id="signals"></a>
### 訊號

- 在 `org.mpris.MediaPlayer2.Player` 上的 **`PropertiesChanged`**：在每個改變了上述一個或多個屬性（`Position` 除外）的播放器事件之後，發出一個訊號，恰好帶有變更的屬性及其新值（`Metadata` 為整體）。沒有變更就不發：只有位置事件絕不會發出它
- **`Seeked(position)`**：當目前項目的位置跳動時：與之前是同一個項目，而且新位置比最後一個位置更前面，或領先它超過自那時起經過的時間再加 1 秒（跳轉、`Previous` 的重新開始、重複播放 `track` 從頭開始、客戶端的 `SeekTo`）。目前項目的更換是 `Metadata` 變更，不是跳轉，除非新項目從 `0` 以外的位置開始（播放了還原的位置，0009）

<a id="orgmprismediaplayer2player-methods-and-writes"></a>
### `org.mpris.MediaPlayer2.Player`：方法與寫入

每一個都對應到一個播放器 `Command`，並像客戶端的請求一樣（0005）經由客戶端 hub 送出：播放器處理完之後呼叫才返回。

| 呼叫 | Command |
|---|---|
| `PlayPause()` | `TogglePause` |
| `Play()` | `Play`（新）：已暫停、已停止但有目前項目，或載入中被保持住時播放；已在播放或沒有目前項目時什麼也不做 |
| `Pause()` | `Pause`（新）：播放中、緩衝中或載入中時暫停；其他情況什麼也不做 |
| `Stop()` | `Stop`（新）：停止引擎（釋放裝置），保留目前項目，位置 `0:00`；之後 `Play` 從頭開始，如 MPRIS 規格所說 |
| `Next()`、`Previous()` | `Next`、`Previous` |
| `Seek(offset µs)` | `SeekBy(offset in ms, rounded toward zero)`；沒有目前項目時什麼也不做 |
| `SetPosition(trackid, position µs)` | `SetPosition { entry, position }`（新）：只有當 `entry` 仍是目前項目且 `0 ≤ position ≤ duration` 時才套用，否則什麼也不做（MPRIS 規格：過時的 `trackid` 會被忽略）。不是我們的 `trackid` 會被忽略 |
| `OpenUri(uri)` | `Open { items, at: None }`（0005）：URI 被解析為[項目](../../zh-TW/playback.md#items)（`https://tidal.com/browse/album/…`、`tidal://track/…`）；取代佇列並開始播放。無法解析的 URI，或 `Open` 拒絕的項目種類（藝人），會是帶有解析器訊息的 D-Bus 錯誤 |
| `Shuffle = b` | `SetShuffle(b)`（新）：不同時等同 `ToggleShuffle`，相同時什麼也不做 |
| `LoopStatus = s` | `SetRepeat(mode)`（新）；未知的字串是 `InvalidArgs` 錯誤 |
| `Volume = v` | `SetVolume(round(v × 100))`，限制在 0–100（因此 `v < 0` 是 `0`，如 MPRIS 規格所要求）；`SetVolume` 本來就會取消靜音（0004） |

新的指令是 0004 的切換旁邊的 setter。兩個根據過時畫面行動的客戶端仍可能用它們互相拉扯；MPRIS 需要它們，因為它的客戶端說出的是想要的結果，播放器則依照目前的狀態而非轉接器的副本來決定每一個。

**一次性指令**也獲得同樣的 setter（0005 決策 6 把它們延後到這裡）：`tidal-player playback play`、`pause`、`stop`、`shuffle on|off`、`repeat off|queue|track`。不帶引數的 `shuffle` 與 `repeat` 仍然是切換與循環。

<a id="errors"></a>
### 錯誤

| 情況 | MPRIS 的回答 |
|---|---|
| 播放器處理了指令（不論有沒有改變任何東西） | 成功 |
| 播放器回答 `Err(message)`（`OpenUri` 的 `Album 123 was not found`） | 帶著該訊息的 `org.mpris.MediaPlayer2.tidal_player.Error.Failed`（屬性寫入：`org.freedesktop.DBus.Error.Failed`） |
| 5 秒內沒有回答（0005 的一次性期限） | `org.mpris.MediaPlayer2.tidal_player.Error.Timeout`（屬性寫入：`org.freedesktop.DBus.Error.NoReply`；zbus 只讓屬性寫入回答標準錯誤） |
| 錯誤的引數（`LoopStatus = "Shuffle"`、不是物件路徑的 `trackid`、項目解析器拒絕的 `OpenUri`） | `org.freedesktop.DBus.Error.InvalidArgs` |
| 播放器正在關閉 | `org.mpris.MediaPlayer2.tidal_player.Error.Failed` `The player is shutting down` |

播放器自己的失敗（無法播放的曲目、忙碌的輸出）不是 MPRIS 錯誤：它是播放器的訊息，對每個客戶端都一樣。

<a id="media-keys"></a>
### 媒體鍵

在 Linux 上，MPRIS 是媒體鍵送達播放器的途徑：GNOME 與 KDE 把 `XF86AudioPlay`/`Pause`/`Next`/`Prev`/`Stop` 路由給作用中的 MPRIS 播放器，其他視窗管理員則把它們綁到 `playerctl play-pause` 與類似的指令。`docs/mpris.md` 列出 sway/i3 與 Hyprland 的綁定。TUI 本身不讀取媒體鍵（決策 4）。

<a id="acceptance-criteria"></a>
## 驗收條件

核心（`tidal_player_core::player`，純邏輯）：

- **AC1** — 新的指令（以狀態為列的表：空、已停止但有目前項目、載入中、載入中被保持住、播放中、緩衝中、已暫停、已暫停且已釋放）：在 `TogglePause` 會開始或繼續播放的地方，`Play` 產生與 `TogglePause` 相同的效果，在 `TogglePause` 會暫停的地方則沒有效果；`Pause` 相反；當引擎持有曲目時，`Stop` 送出 `EngineStop`，保留目前項目，把位置設為 `0:00`、狀態設為 `Stopped`，而對位置已在 `0:00` 的已停止播放器則是 no-op；接著的 `Play` 從 `0:00` 開始
- **AC2** — `SetShuffle`、`SetRepeat` 與 `SetPosition`（表）：`SetShuffle(x)` 在 `x` 不同時等於 `ToggleShuffle`，否則什麼也不做（逐一比較效果與快照）；`SetRepeat(m)` 設定模式，並像 `CycleRepeat` 一樣協調預先載入；在長度之內的目前項目上，`SetPosition` 等於 `SeekTo`，而在其他項目上、超過長度，或沒有目前項目時什麼也不做。每個新的 `Command` 變體都加入 `protocol::tests::all_commands`（JSON 來回轉換）

MPRIS 模型（`tidal-player::mpris::model`，純邏輯，沒有 D-Bus 型別：自己的值列舉）：

- **AC3** — `player_properties(&snapshot, last_position)`（以快照為列的表：空；已停止但有目前項目；載入中；被保持住的載入；播放中並開啟隨機播放與重複播放 queue；緩衝中；已暫停；已釋放；音量 40 % 的靜音；重複播放關閉時的最後一個項目；重複播放 track 時的最後一個項目；長度未知）：`Player` 表中的每個屬性，包含 `Can*` 的規則
- **AC4** — `metadata(&snapshot)`（表）：沒有目前項目 → 只有 `NoTrack`；有版本、多位藝人、沒有專輯、長度未知的曲目，以及排入佇列兩次的同一首曲目（兩個 trackid）；`xesam:url`；有封面的曲目有 `mpris:artUrl`，沒有封面的則無
- **AC5** — `changes(old, new)`（表）：沒有變更 → 無；只有位置變更 → 無；一個屬性 → 只有那一個；音量與靜音一起 → `Volume` 一次；曲目變更 → `Metadata` 與有變的 `Can*`；來自 `ToggleShuffle`、`SetShuffle` 與客戶端切換的 `Shuffle` 都一樣
- **AC6** — `seeked(old, new, elapsed)`（表）：前進了經過的時間 → 無；後退 → `Seeked`；前進了經過的時間加 2 秒 → `Seeked`；位置在 `0` 的另一個項目 → 無；位置在還原位置的另一個項目 → `Seeked`；沒有目前項目 → 無
- **AC7** — `Position`（表，假時鐘）：`Playing` 時為最後回報的位置加上經過的時間，暫停、緩衝、載入或停止時凍結；以長度為上限
- **AC8** — `command_for(call)`（表）：方法與寫入表的每一列，`Volume` `-0.5` → `0`、`1.7` → `100`、`0.333` → `33`；`Seek` 的 `-1500` µs → `SeekBy(-1)`；帶有外來路徑的 `SetPosition` → 無；`LoopStatus` `"Shuffle"` → `InvalidArgs`；帶有藝人連結與亂碼的 `OpenUri` → 解析器的錯誤

轉接器與執行期（`tidal-player`）：

- **AC9** — hub 路徑（0005 的行程內測試框架，假引擎，沒有匯流排：轉接器的 hub 端在接縫（seam）之後）：呼叫的指令以請求的方式送達播放器，呼叫在播放器的 `Reply` 之後才完成；播放器的 `Err` 變成帶有其訊息的 `Failed` 錯誤；在期限內沒有回答的播放器（暫停的執行期、假時鐘）給出 `Timeout`；連發的 100 個呼叫全部依序套用（沒有被丟棄的：tidalt 的非阻塞送出）
- **AC10** — 在匯流排上（由測試啟動的私有 `dbus-daemon --session`；缺少 `dbus-daemon` 時測試失敗，絕不略過）：0005 測試框架下的常駐程式擁有 `org.mpris.MediaPlayer2.tidal_player`；對已載入佇列，兩個介面的 `GetAll` 與 AC3/AC4 相符；已暫停的播放器上的 `Pause()` 仍保持暫停（tidalt 的切換）；`Set(Volume, 0.5)` 改變音量，而且訂閱的客戶端看得到；客戶端的 `ToggleShuffle` 發出一個只帶 `Shuffle` 的 `PropertiesChanged`；`SeekTo` 發出 `Seeked`；`OpenUri` 一個模擬專輯會載入它；`Quit()` 讓常駐程式繼續執行；在 `daemon stop` 時名稱被釋放
- **AC11** — 沒有匯流排（`DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus`，0005 的測試已如此設定）：常駐程式照常啟動、服務客戶端並播放；記錄中有一次 `MPRIS is not available:`；播放器的訊息是空的。0005 既有的常駐程式與 CLI 測試維持綠燈、不更動
- **AC12** — 名稱被佔用（測試先在私有匯流排上擁有 `org.mpris.MediaPlayer2.tidal_player`）：常駐程式取得 `…tidal_player.instance<pid>` 並在那裡運作
- **AC13** — `mpris`（0008 的 `app.toml` 與優先順序表新增它的列：`true`、`false`、錯誤的型別、檔案中的 `true` 之上的 `TIDAL_PLAYER_MPRIS=off`、無效的變數是錯誤）：關閉 → 私有匯流排從不看到常駐程式的連線（以 `ListNames`/`NameOwnerChanged` 檢查）
- **AC14** — 客戶端從不碰匯流排：連上 `mpris = false` 的常駐程式的 TUI 與 `playback status`，都不會讓私有匯流排出現新的連線
- **AC15** — 一次性 setter：`playback play|pause|stop`、`shuffle on|off`、`repeat off|queue|track` 分別送出 `Play`、`Pause`、`Stop`、`SetShuffle`、`SetRepeat`（0005 的引數表新增這些列；`shuffle maybe` 以 2 結束）；單獨的 `shuffle` 與 `repeat` 仍送出切換
- **AC16** — 關閉順序：收到 `Shutdown` 時，轉接器停止接受呼叫（太晚的呼叫得到關閉中的錯誤），而匯流排連線在客戶端收到 `ShuttingDown` 之後才關閉

封面與其快取（`tidal-player-core`、`tidal-player-api`、`tidal-player`）：

- **AC18** — 封面 ID 被解析並保留（錄製的測試資料（fixture），如 0002/0006）：來自 `metadata/track.json` 的曲目、專輯頁、播放清單頁、收藏的曲目與搜尋頁，其 `album.cover == Some("…")`；一個 `"cover": null` 的測試資料列與一個沒有該鍵的 → `None`。`cover_url`（表）：UUID → 每個 `-` 換成 `/`、大小為 `640x640` 的 URL；其他大小照所給的。`protocol::tests` 中的每個 `Track` 都加上封面（JSON 來回轉換）
- **AC20** — 封面快取的決策（`tidal-player::mpris::covers`，純邏輯，表）：給定帶有最後使用時間的檔案、`max_cover_arts` 與目前的封面，要刪除哪些檔案（20 個上限下有 21 個檔案 → 最舊的；目前的最舊 → 次舊的；0 → 不寫入任何東西，且為 `https://`）；各狀態下的 `artUrl`（已快取 → `file://`；下載中 → 無；失敗或關閉 → `https://`；沒有封面 → 無）；啟動時的清理（刪除 `.tmp` 與不屬於它的名稱）。快取目錄的解析（表：`TIDAL_PLAYER_CACHE_DIR`、`XDG_CACHE_HOME`、`HOME`）
- **AC21** — 封面快取的 I/O（`tidal-player::mpris::covers`，暫存目錄，`wiremock`）：`200` `image/jpeg` → 經由 `.tmp` 與重新命名寫成 `covers/<cover>.jpg`，位於 `0700` 目錄中、模式為 `0600`；`404`、`text/html` 本文、超過 5 MB 的本文，以及慢於逾時的回應（暫停的時鐘）→ 失敗，不留下檔案；已快取的封面不會再次下載，且它的修改時間會被更新；第一個封面下載期間第二個項目成為目前項目 → 接下來只取最新的；轉接器上的 `Metadata` 序列：先沒有 `artUrl`，再帶著 `file://` URL（一個 `PropertiesChanged`），而且在此期間項目改變時什麼也不發。0008 的 `app.toml` 與優先順序表中的 `max_cover_arts`（預設 `20`、`0`、`1000`，`1001` 與 `-1` 是錯誤，變數優先於檔案）
- **AC19** — 舊的狀態檔：0009 寫成的 `playback.json`（沒有 `cover` 的測試資料）載入時處處是 `cover: None`，而且沒有 `.bad` 重新命名；帶有封面的已儲存狀態會原樣來回轉換（0009 的 AC1/AC6 表新增這些列）

文件：

- **AC17** — `docs/mpris.md`（哪些可用、匯流排名稱、以使用者的用語寫的屬性與方法表、`playerctl` 範例、GNOME/KDE 的媒體鍵綁定（不必做任何事）、sway/i3 與 Hyprland、`mpris = false`、SSH 上的「沒有工作階段匯流排」）、`docs/daemon.md`（新的一次性指令；`playerctl` 可搭配常駐程式使用）、`docs/config.md`（`mpris`）、`docs/playback.md`（連結）、它們的 zh-TW 副本、`README.md`/`README.zh-TW.md` 與 `CLAUDE.md` 的「Features」；0001、0003、0004、0005、0008 與 0009 中的「MPRIS (0010)」備註指向這裡。一個測試檢查 `mpris` 與 `max_cover_arts` 出現在 `docs/config.md`，新的一次性指令出現在 `docs/daemon.md`（0008 AC17 的測試，新增的列）

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 沒有工作階段匯流排（SSH、在工作階段匯流排之前啟動的 systemd 使用者服務） | 不帶 MPRIS 執行，一行 `info` 記錄（AC11）。在 systemd 下使用者匯流排通常存在（`dbus.socket`）；unit 不新增相依 |
| 執行時工作階段匯流排消失（工作階段結束） | MPRIS 停止運作，一行 `warn` 記錄；播放器繼續播放並服務 socket 客戶端；不重試 |
| 名稱已被佔用 | `.instance<pid>`（AC12） |
| 還原的播放器（0009） | 以停止狀態發佈，帶有還原的目前項目的 `Metadata`，且 `CanPlay` 為 true：桌面小工具可以啟動它 |
| 空的佇列 | `Stopped`、`NoTrack`，除了 `CanControl` 之外的每個 `Can*` 都是 false；呼叫被接受但什麼也不做 |
| 需要登入（0002） | MPRIS 不變；`Play` 與任何播放一樣失敗，帶著播放器的訊息 |
| 快取目錄唯讀、已滿或缺少權限 | `https://` URL，每個封面一次 `warn`；播放不受影響 |
| 兩個執行階段目錄不同的播放器共用一個快取目錄 | 兩者都可能寫入同一個 `<cover>.jpg`；重新命名讓任一寫入都是完整的；其中一個的淘汰可能刪掉另一個的檔案，需要時會再下載 |
| 封面在 Tidal 上以同一個 ID 變更 | 絕不會：Tidal 的封面 ID 指名的是一張圖片；快取的檔案照原樣使用 |
| 長度未知的曲目 | 沒有 `mpris:length`、`CanSeek` 為 false、`SetPosition` 被忽略，`Seek` 仍然送出（0004 處理它） |
| 暫停期間裝置已釋放（0005） | `Paused`；`Play` 像 `space` 一樣重新開啟裝置；重新取得失敗時維持 `Paused`，並帶著播放器的訊息 |
| 小工具每秒輪詢 `Position` 許多次 | 由轉接器的副本回答，絕不向播放器詢問 |
| 客戶端狂寫 `Volume` | 每一次都是一個請求；播放器不合併任何東西，合併的是持久化（0009） |
| `Seek` 超過結尾 | `SeekBy`（0004：跳轉超過結尾會移到下一首，如 MPRIS 規格所要求） |
| 同一個匯流排上有兩個播放器（執行階段目錄不同） | 各有自己的名稱；`playerctl` 依它自己的規則挑選其一 |
| 透過 SSH 搭配 X 轉送或轉送匯流排執行 `tidal-player play` | 它發佈在 `DBUS_SESSION_BUS_ADDRESS` 所指的任何匯流排上；已寫入文件 |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的驗收條件命名（`ac3_…`）。紅燈是針對存根（stub）型別與函式、其本體為存根（沒有 `todo!()`、沒有編譯錯誤）的斷言失敗，如 0001–0009。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/player/tests/setters.rs` :: `ac1_play_pause_stop` (table) | 各狀態下的效果與快照 | 存根的 `Play`/`Pause`/`Stop` 什麼也不做 |
| AC2 | `crates/core/src/player/tests/setters.rs` :: `ac2_set_shuffle_repeat_position` (table); `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) | 等於切換／`SeekTo`；來回轉換 | 存根的 setter 什麼也不做 |
| AC3 | `crates/app/src/player/mpris/model.rs` :: `ac3_player_properties` (table) | 各快照下的每個屬性 | 存根回傳 tidalt 的常數（`Can*` 為 true、`CanSeek` 為 false） |
| AC4 | `crates/app/src/player/mpris/model.rs` :: `ac4_metadata` (table) | 鍵與值 | 存根只回傳 `NoTrack` |
| AC5 | `crates/app/src/player/mpris/model.rs` :: `ac5_changes` (table) | 變更的集合 | 存根不回傳任何變更（tidalt） |
| AC6 | `crates/app/src/player/mpris/model.rs` :: `ac6_seeked` (table) | `Seeked` 何時觸發 | 存根從不觸發 |
| AC7 | `crates/app/src/player/mpris/model.rs` :: `ac7_position` (table, fake clock) | 推算的位置 | 存根回傳最後一次回報 |
| AC8 | `crates/app/src/player/mpris/model.rs` :: `ac8_command_for` (table) | 各呼叫的指令或錯誤 | 存根把 `Play` 與 `Pause` 對應到 `TogglePause`（tidalt） |
| AC9 | `crates/app/src/player/mpris/hub.rs` :: `ac9_calls_reach_player`, `ac9_error_and_timeout`, `ac9_burst_in_order` | 回應、錯誤、沒有東西被丟棄 | 存根在回應之前就完成呼叫，通道滿了就丟棄 |
| AC10 | `crates/app/tests/mpris.rs` :: `ac10_on_the_bus` | 匯流排情境 | 存根轉接器從不連線 |
| AC11 | `crates/app/tests/mpris.rs` :: `ac11_no_bus`; the 0005 tests unchanged | 執行、記錄一次、沒有訊息 | 存根在缺少匯流排時結束 |
| AC12 | `crates/app/tests/mpris.rs` :: `ac12_name_taken` | `.instance<pid>` | 存根在名稱被佔用時放棄 |
| AC13 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/tests/mpris.rs` :: `ac13_mpris_off` | 解析、優先順序、沒有連線 | 存根忽略該設定 |
| AC14 | `crates/app/tests/mpris.rs` :: `ac14_clients_stay_off_the_bus` | 客戶端沒有新的匯流排連線 | 存根客戶端也會發佈 |
| AC15 | `crates/app/src/cli/oneshot.rs` :: the 0005 argument table (new rows) | 解析出的指令、以 2 結束 | 存根只解析切換 |
| AC16 | `crates/app/src/player/mpris/hub.rs` :: `ac16_shutdown_order` | `Shutdown` 之後拒絕；`ShuttingDown` 之後關閉 | 存根先關閉 |
| AC18 | `crates/api/tests/metadata.rs` :: `ac18_cover` (table, fixtures); `crates/core/src/track.rs` :: `ac18_cover_url` (table) | 解析出的封面；URL | 存根 DTO 丟掉該欄位；存根 `cover_url` 回傳 `""` |
| AC19 | `crates/app/src/player/persist.rs` :: `ac6_load` (new row); `crates/core/src/player/tests/persistence.rs` :: `ac1_saved_round_trip` (new row) | 舊檔案可載入；封面保留下來 | 沒有 `#[serde(default)]` 的存根 `cover` 讓舊檔案失敗 |
| AC20 | `crates/app/src/player/cover_cache.rs` :: `ac20_eviction`, `ac20_art_url`, `ac20_start_cleanup`, `ac20_cache_dir` (tables) | 刪除了什麼；各狀態下的 URL；目錄 | 存根保留每個檔案，且永遠給 `https://` |
| AC21 | `crates/app/src/player/cover_cache.rs` :: `ac21_download`, `ac21_download_failures`, `ac21_latest_wins`, `ac21_metadata_sequence`; `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows) | 檔案、模式、失敗、順序、訊號、設定 | 存根下載直接就地寫入，且從不失敗 |
| AC17 | `crates/app/tests/docs.rs` :: `ac17_every_command_documented` (new rows) + reviewed at acceptance | 文件 | 存根文件缺少這些列 |

在驗收時於使用者的機器上手動檢查（結果寫在 PR 說明中），因為需要真正的桌面：GNOME（或 KDE）的媒體小工具顯示曲目，在曲目變更、暫停與跳轉時沒有延遲地更新，它的按鈕有作用，而且顯示專輯封面；鍵盤的播放／暫停、下一首與上一首鍵在沒有開啟 TUI 時控制常駐程式；`playerctl -p tidal_player metadata --follow` 跟隨曲目變更；藍牙耳機的按鈕控制播放；從小工具暫停之後裝置會被釋放（0005）。

CI：`dbus-daemon` 隨 `dbus` 套件而來；CI 工作的 `apt-get install` 新增 `dbus`，讓 AC10–AC14 在那裡執行（沒有它們會失敗，絕不略過）。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`AlbumRef::cover`（`#[serde(default)]`）與 `track::cover_url`；新的 `Command` 變體（`Play`、`Pause`、`Stop`、`SetShuffle(bool)`、`SetRepeat(RepeatMode)`、`SetPosition { entry, position }`）及它們在 `player` 中的處理。沒有 D-Bus，沒有新的相依套件
- `tidal-player-api`：中繼資料與音樂庫專輯 DTO 中的 `cover` 欄位，對應到 `AlbumRef`
- `tidal-player`（執行檔）：`mpris/model.rs`（純邏輯：屬性、中繼資料、變更、`Seeked`、位置、呼叫 → 指令，帶有自己的值型別）、`mpris/hub.rs`（作為 hub 的行程內客戶端的轉接器：沒有串流的 `Peer`，訂閱並送出帶 id 的 `Request`，以期限等待每個 `Reply`）、`mpris/bus.rs`（薄薄的 zbus 層：名稱、物件、介面、訊號；由 AC10–AC14 與手動檢查）、`mpris/covers.rs`（快取：純邏輯的淘汰與 URL 規則，在接縫之後以 `reqwest` 下載），接到獨立模式、`daemon` 與 `play` 播放器；`config` 中的 `mpris`、`max_cover_arts` 與快取目錄；`cli::oneshot` 中的一次性 setter。`zbus` 已經是相依套件（0003 的保留所用的阻塞式 API）；轉接器可以在播放器的 tokio 執行期上使用它的非同步 API（只是 workspace 的功能變更）
- `xtask layering`：沒有變更（core 與 audio 仍然沒有 D-Bus）
- 在 `AlbumRef` 新增欄位會牽動每個以 struct 字面值建立它的測試；新增它的那個切片會更新它們（或改用輔助函式）

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-09，從程式碼）：

- tidalt：如「tidalt 的做法」之下所述（那裡指名了檔案與 commit）
- 本儲存庫：hub 接受行程內的 peer（`Peer::new(outbox, None)`），並把它們的 `Subscribe`/`Request` 與其他每個輸入排定順序（`crates/app/src/ipc/server.rs`）；已停止的播放器上的 `TogglePause` 從 `position` 開始目前項目；重複播放開啟時 `Next` 會繞回（`skip_target`），而之後什麼都沒有時就停止；`SetVolume` 會取消靜音；`zbus` 5 是 workspace 相依套件，帶有 `blocking-api` 與 `async-io`；0005 的常駐程式與 CLI 測試設定 `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus`；`dbus-daemon` 安裝在這個開發容器中；錄製的 v1 曲目物件把 `album.cover` 帶為 UUID，在 `vibrantColor` 與 `videoCover` 旁邊（`crates/api/tests/fixtures/metadata/track.json`、`album_page.json`、`playlist_page_with_video.json`，以及三個音樂庫測試資料），而我們的 DTO 把它丟掉了（`metadata.rs` 與 `library.rs` 的 `AlbumDto`）；播放器還沒有快取目錄（只有狀態目錄，`store_setup.rs`）

假設，於驗收時檢查：

- MPRIS2 規格（2.2）：匯流排名稱 `org.mpris.MediaPlayer2.<name>`，更多執行個體用 `.instance<pid>`；除了 `/org/mpris/MediaPlayer2/TrackList/NoTrack` 之外，`/org/mpris` 的路徑都是保留的；`Position` 不由 `PropertiesChanged` 發出，跳動由 `Seeked` 涵蓋；`SetPosition` 忽略過時的 `trackid`；小於 0 的 `Volume` 視為 0；`Stop` 之後 `Play` 從曲目開頭開始。在撰寫 AC3–AC8 之前，對照規格文字重新閱讀
- 圖片 URL `https://resources.tidal.com/images/<a/b/c/d/e>/640x640.jpg`（tidalt 用過它；大小有 `80x80`、`160x160`、`320x320`、`640x640`、`1280x1280`）不需要認證。`resources.tidal.com` 無法從開發容器連上，所以在撰寫 AC18 之前，在使用者的機器上檢查（對真實曲目的封面執行 `curl -I`）
- GNOME Shell 與 KDE Plasma 的媒體小工具會自己載入 `https://` 的 `mpris:artUrl`（有些通知常駐程式只接受 `file://`；它們不顯示封面）
- `playerctl`、`gsd-media-keys` 與 KDE 的媒體控制器只靠 `PropertiesChanged` 就會找到播放器，不需要 `DesktopEntry`
- `ubuntu-latest` runner 可以安裝 `dbus`，並在沒有系統匯流排的情況下執行私有的 `dbus-daemon --session`
- spotify-player（憑記憶，需重新確認）：MPRIS 透過 `souvlaki` crate，在它的 `media-control` 功能之後，在 Linux 上預設開啟，匯流排名稱 `org.mpris.MediaPlayer2.spotify_player`

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-09"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-09）

- **函式庫**：純 `zbus` 5（`#[zbus::interface]`，在它自己的 `async-io` 執行器上用非同步 API，訊號用阻塞式 API）；不使用 `mpris-server`，workspace 的 `zbus` 功能不變。執行檔的 dev-dependencies 啟用 tokio 的 `test-util`（暫停的時鐘），只供測試使用
- **執行緒**：轉接器在自己的執行緒（`mpris`）上清空它的 outbox，因此 hub 絕不會發現它滿了。呼叫從 zbus 的執行器橋接到播放器的 tokio 執行期（`spawn`，再等待 `JoinHandle`），5 秒期限在那裡執行。訊號由看到變更的執行緒（轉接器的執行緒，或封面 worker 的任務）以阻塞式 API 在轉接器的鎖之下發出，因此依序送出
- **啟動**：`mpris::start` 在 `spawn_runtime` 之後、socket 開始服務之前執行（常駐程式、獨立模式）；在 `play` 中，則在 socket 開始服務之前、`play_queue` 執行播放器之前，播放器隨後在它的迴圈中處理轉接器的 `Attach`/`Subscribe`。轉接器不等待 `Welcome`：在它到達之前，屬性是空播放器的，然後一個 `PropertiesChanged` 把它們更新到最新。封面快取的啟動清理只在匯流排連上之後執行
- **關閉**：轉接器從看到 `ShuttingDown` 的那一刻起，或它的 outbox 關閉時（`play` 沒有 `Shutdown` 就結束）拒絕呼叫；等待中的呼叫以 `The player is shutting down` 失敗。`Mpris::close`（離開 hub、關閉連線）在 socket 伺服器被釋放之後呼叫，也就是在 socket 客戶端收到它們最後的訊息之後
- **記錄**：執行檔不安裝 `tracing` subscriber。常駐程式把轉接器的行（`MPRIS is not available: …`、`Cannot cache the cover …`、`MPRIS stopped working: …`）寫到 stderr，也就是它的記錄（journal）；獨立模式的 TUI（擁有終端機）與 `play`（它的 stderr 帶有自己的訊息）則把它們送到 `tracing`
- **錯誤**：方法透過手寫的 `DBusError` 回答我們自己的名稱。屬性寫入（`Volume`、`Shuffle`、`LoopStatus`）在 zbus 中只能回答標準錯誤：`InvalidArgs`、`Failed`，以及逾時的 `NoReply`。無法解析的 URI 或藝人連結的 `OpenUri` 是帶有解析器訊息的 `InvalidArgs`（錯誤的引數）。不是物件路徑的 `trackid` 在轉接器看到之前就被 D-Bus 的型別檢查拒絕（`InvalidArgs`）；模型也會檢查
- **自我描述（introspection）**：`LoopStatus`、`Shuffle`、`Volume`、`Rate` 與 `Position` 帶有 `EmitsChangedSignal=false`。否則 zbus 會在每次寫入之後送出它自己的 `PropertiesChanged`，在轉接器的那個旁邊多一個（轉接器的在播放器套用寫入、帶有每個變更的屬性之後才來）；`Position` 從不發出訊號（MPRIS）。常數（`CanControl`、速率、根介面的）是 `const`
- **`Seeked`**：跳動是位置落在最後一個位置之前，或比最後一個位置加上自那時起的播放時間領先超過 1 秒。另一個項目從 `0:00` 算起，因此只有當它從超過那 1 秒處開始（還原的位置）時才是跳轉。`Welcome` 不發 `Seeked`；不是轉接器目前項目的項目的位置事件會被忽略
- **長度**：`mpris:length`、`CanSeek` 與 `Position` 的上限使用佇列項目的長度，也就是快照所帶的那個（它沒有另外的串流長度）
- **封面**：已快取的封面在它的項目成為目前項目時就被查找（並更新時間），因此立刻以它的 `file://` URL 發佈；只有未快取的才交給 worker，一個保存最新需求的 `watch` 通道。封面 ID 是 Tidal 給的小寫 UUID：其他任何東西都絕不下載，而且啟動時 `covers/` 中的任何其他名稱（包含大寫的）都會被刪除。下載之後，上限會保留新的封面與目前項目的封面。啟動時 `max_cover_arts = 0` 會刪除每個已快取的封面（上限只套用一次）。啟動時無法讀取的 `covers/` 會被記錄（`warn`），播放器繼續執行。`file://` URL 會跳脫 `[A-Za-z0-9/._~-]` 以外的每個位元組。在 debug 建置中，`TIDAL_PLAYER_IMAGES_BASE` 取代下載用的 `https://resources.tidal.com`（如同 `TIDAL_PLAYER_API_BASE` 之於 API）；`https://` 的備用 URL 永遠是 Tidal 的
- **MPRIS 2.2 的規則**（上述假設），依回憶中的規格檢查（容器無法連上 specifications.freedesktop.org 重新閱讀）：匯流排名稱 `org.mpris.MediaPlayer2.<name>`，更多執行個體用 `.instance<pid>`；除了 `/org/mpris/MediaPlayer2/TrackList/NoTrack` 之外，`/org/mpris` 是保留的；`Position` 不發出訊號，跳動用 `Seeked`；`SetPosition` 忽略過時的 `trackid`；小於 0 的 `Volume` 是 0；`Stop` 之後 `Play` 從頭開始。封面 URL 在沒有憑證時能否連上，仍是使用者在驗收時的檢查（`resources.tidal.com` 無法從開發容器連上）
- **測試**：`tests/mpris.rs` 有自己的小型測試框架（0005 的 `Machine` 概念，加上設定、快取與匯流排），讓 `tests/daemon.rs` 維持不變。每個匯流排測試都啟動 `dbus-daemon --config-file=<temp>/session.conf --nofork --print-address`（暫存目錄中 socket 上的工作階段匯流排，`allow own="*"`，監聽者允許竊聽）。串流從不解析完成，但解析器在 10 秒之後放棄（`RESOLVE_TIMEOUT`），因此 AC10 在每個需要載入中（或被保持住）曲目的步驟之前載入一個新的佇列：新的載入讓較早的解析過時，它的逾時不改變任何事，而且沒有任何步驟取決於測試已執行多久（以步驟之間 11 秒的暫停檢查過）。AC13 與 AC14 的端對端測試在紅燈時通過了（存根從不連線，所以它們無法失敗；它們防止的是客戶端或 `mpris = false` 的播放器連線）。AC9 的逾時在暫停的 current-thread 執行期上執行；AC21 的慢回應是它自己的測試（`ac21_download_timeout`），而 `ac20_cache_on_disk` 涵蓋磁碟上的啟動清理與淘汰。AC17 的檢查是 0008 的旁邊的一個新測試，`tests/docs.rs :: ac17_mpris_documented`（0008 的迭代按鍵對應的指令）；0008 AC18 的範例測試新增了 `mpris` 與 `max_cover_arts`

<a id="decisions-answered-by-the-user-2026-10-09-2-and-6-as-noted-the-rest-as-proposed"></a>
## 決策（使用者於 2026-10-09 回答：2 與 6 如註記，其餘照提議）

1. **匯流排名稱** `org.mpris.MediaPlayer2.tidal_player`（spotify-player 的風格：`playerctl -p tidal_player`）。備選：`tidal-player`（連字號合法，但不建議用於匯流排名稱）
2. **每個播放器都發佈**，包含 `play`，預設開啟（`mpris = true`），`mpris = false` 可關閉。*使用者回答（2026-10-09）：預設開啟，`mpris = true`*
3. **核心中的 setter**（`Play`、`Pause`、`Stop`、`SetShuffle`、`SetRepeat`、`SetPosition`），依播放器的真實狀態決定，並且同樣的 setter 也作為一次性指令。備選：轉接器從它的副本算出切換（有競爭條件；tidalt 的那一類錯誤）
4. **TUI 不讀取媒體鍵**：桌面透過 MPRIS 路由它們，而終端機只有在少數終端機中以 kitty 鍵盤協定才看得到它們。備選：在 `keymap.toml` 中對應 crossterm 的媒體鍵碼
5. **`Quit` 什麼也不做**（`CanQuit = false`），因此小工具無法停止常駐程式。備選：只對獨立模式的 TUI 設 `CanQuit = true`
6. **包含封面、有快取、沒有 `TrackList`**：*使用者回答（2026-10-09）：包含封面；「封面快取由 `max_cover_arts` 限制，預設為 20」；TUI 的封面是另一份規格。* 封面 ID 加入 `Track` 的專輯；播放器把目前的封面下載到快取目錄（最多 `max_cover_arts` 個，預設 20，淘汰最久沒用的），並給出 `file://` URL，無法時給出 Tidal 的 `https://` URL。`HasTrackList = false`
7. **`Loading` 讀作 `Playing`**（使用者要求播放；小工具顯示暫停鈕）。備選：在串流開始之前是 `Paused`
8. **靜音讀作 `Volume = 0.0`**，而寫入的 `Volume` 會取消靜音（如 `SetVolume`）。備選：回報音量並忽略靜音

<a id="out-of-scope"></a>
## 不在範圍內

- `org.mpris.MediaPlayer2.TrackList` 與 `Playlists`（決策 6）
- **TUI 中的封面**（spotify-player 的 `image` 功能、tidalt 的 Kitty/sixel 面板）：[0016](0016-cover-art.md)，讀取本規格的封面快取
- 預先擷取下一個項目的封面；其他圖片大小；`.desktop` 檔案（`DesktopEntry`）
- 由 TUI 自己讀取的媒體鍵（決策 4）
- 透過 MPRIS 的自動播放（沒有標準屬性）
- macOS 或 Windows 上的 MPRIS（播放器僅支援 Linux，0003）
- mix 與電台（0011）、篩選與排序（0012）
