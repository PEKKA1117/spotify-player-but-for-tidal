[English](../0010-mpris.md) | 正體中文

<a id="0010--mpris2-and-media-keys"></a>
# 0010 — MPRIS2 與媒體鍵

- **狀態**：implemented（2026-10-09；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0002/0006（implemented：解析曲目的中繼資料與音樂庫用戶端）、0004（implemented：播放器與其協定）、0005（implemented：每位使用者一個播放器、用戶端的 hub、一次性指令）、0008（implemented：`app.toml` 及其優先順序）、0009（implemented：還原的播放器是停止的）
- **使用者文件**：新增的 [`docs/mpris.md`](../../zh-TW/mpris.md)（桌面控制、媒體鍵、`playerctl`）；[`docs/daemon.md`](../../zh-TW/daemon.md)（新的一次性指令、「`playerctl` 可用」）、[`docs/config.md`](../../zh-TW/config.md)（`mpris`、`max_cover_arts`、快取目錄）、[`docs/playback.md`](../../zh-TW/playback.md)（連到新頁面的連結）及它們的 zh-TW 副本（AC17）

<a id="context"></a>
## 背景

播放器只能從它自己的 TUI 與 `tidal-player playback …` 控制。桌面環境（GNOME、KDE Plasma）、狀態列（waybar、polybar）、鎖定畫面、藍牙耳機（透過 BlueZ 的 AVRCP 橋接）、KDE Connect 與 `playerctl` 都透過 **MPRIS2** 控制播放器，也就是 D-Bus 介面 `org.mpris.MediaPlayer2`；鍵盤的媒體鍵在每個現行的 Linux 桌面上也是以同樣的方式送達播放器（`gsd-media-keys`、KWin，以及平鋪式視窗管理員對 `playerctl` 的綁定）。0001 規劃了它，0004 把「setter」指令延後到這裡，0005 則決定 MPRIS 是**建立在播放器之上的公開轉接器**，與用戶端 socket 分開（0005 決策 1）。

本規格讓**每個播放器行程**（獨立模式的 TUI、`daemon`、`play`）都在工作階段匯流排（session bus）上把自己發佈為 MPRIS2 媒體播放器，即時反映它的狀態，並透過用戶端所送的同一批 `Command` 來回應 MPRIS 呼叫。

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
3. **呼叫被默默丟棄**（非阻塞的送出）→ 每個呼叫都像任何用戶端的請求一樣，經由用戶端 hub 送達播放器，在播放器處理後才回應；播放器拒絕或逾時未回應的呼叫會回傳 D-Bus 錯誤（AC9）
4. **能力是假的**（佇列為空卻 `CanGoNext`、TUI 有跳轉卻 `CanSeek` 為 false）→ `Can*` 屬性由狀態推導，並隨之改變（AC3）
5. **由 Tidal ID 產生的曲目 ID**（排入佇列兩次的曲目只有一個 ID，而且 `/org/mpris` 是 MPRIS 規格為自己的路徑保留的）→ `mpris:trackid` 由**佇列項目 ID** 產生，位於我們自己的路徑之下（AC2）
6. **只有一位藝人，沒有封面**：`xesam:artist` 列出曲目的每一位藝人，`mpris:artUrl` 給出專輯封面（AC4、AC18）
7. **匯流排名稱是單一執行個體的鎖**：沒有工作階段匯流排就完全沒有播放器，過期的名稱還會擋住啟動 → 播放器的鎖仍是 0005 的檔案鎖；MPRIS 是選用的：沒有工作階段匯流排，或名稱已被佔用，都絕不會讓播放器停止（AC11、AC12）

<a id="behaviour"></a>
## 行為

<a id="where-it-runs"></a>
### 在哪裡執行

每個播放器行程（獨立模式的 TUI、`tidal-player daemon`、`tidal-player play`）在啟動時連上**工作階段匯流排**，時機在載入還原的狀態（0009）之後、接受用戶端之前，並擁有：

- 匯流排名稱 `org.mpris.MediaPlayer2.tidal_player`（`playerctl -p tidal_player`），以 `DoNotQueue` 請求。若名稱已被佔用（以另一個 `TIDAL_PLAYER_RUNTIME_DIR` 執行的播放器），它就依 MPRIS 規格對第二個執行個體的規定，改取 `org.mpris.MediaPlayer2.tidal_player.instance<pid>`
- 物件 `/org/mpris/MediaPlayer2`，帶有 `org.mpris.MediaPlayer2` 與 `org.mpris.MediaPlayer2.Player`（以及標準的 `Properties`、`Introspectable`、`Peer`）

連上的 TUI 與一次性指令從不碰匯流排（它們是用戶端，0005）。播放器結束時，名稱會釋放（它的匯流排連線關閉），時機在 `ShuttingDown` 送達用戶端之後。

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

來自播放器的狀態（每個用戶端取得的快照，0004）：

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

- `AlbumRef` 新增 `cover: Option<String>`，由中繼資料與音樂庫用戶端在建立曲目的每個地方填入（0002 的 `MetadataClient`、0006 的 `LibraryClient`）；缺少或為 `null` 的封面是 `None`
- 它隨佇列移動（`PlayerSnapshot`，0004），並記錄在 `playback.json`（0009）：在本規格之前寫成、沒有該欄位的檔案仍可載入（欄位預設為無；`version` 不變），它的項目要到再次加入佇列才有封面
- 圖片 URL 由單一純函式建立（`tidal_player_core::track::cover_url(cover, size)`）：`https://resources.tidal.com/images/<cover with each - replaced by />/<size>.jpg`。播放器使用 `640x640`；TUI 的封面規格（不在範圍內，見下）會重用它與快取

<a id="the-cover-cache"></a>
### 封面快取

播放器（絕不是用戶端）下載目前項目的封面，並提供桌面一個本機檔案，讓每個 MPRIS 消費者都能顯示它，只接受 `file://` 的通知常駐程式也包括在內（決策 6）。這是播放器保留的唯一一份磁碟快取；0009 決策 3（沒有中繼資料快取）其餘不變。

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
- **`Seeked(position)`**：當目前項目的位置跳動時：與之前是同一個項目，而且新位置落在最後一個位置之後方，或領先它超過自那時起經過的時間再加 1 秒（跳轉、`Previous` 的重新開始、重複播放 `track` 從頭開始、用戶端的 `SeekTo`）。目前項目的更換是 `Metadata` 變更，不是跳轉，除非新項目從 `0` 以外的位置開始（播放了還原的位置，0009）

<a id="orgmprismediaplayer2player-methods-and-writes"></a>
### `org.mpris.MediaPlayer2.Player`：方法與寫入

每一個都對應到一個播放器 `Command`，並像用戶端的請求一樣（0005）經由用戶端 hub 送出：播放器處理完之後呼叫才返回。

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

新的指令是 0004 的切換旁邊的 setter。兩個根據過時畫面行動的用戶端仍可能用它們互相拉扯；MPRIS 需要它們，因為它的用戶端說出的是想要的結果，播放器則依照目前的狀態而非轉接器的副本來決定每一個。

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

播放器自己的失敗（無法播放的曲目、忙碌的輸出）不是 MPRIS 錯誤：它是播放器的訊息，對每個用戶端都一樣。

<a id="media-keys"></a>
### 媒體鍵

在 Linux 上，MPRIS 是媒體鍵送達播放器的途徑：GNOME 與 KDE 把 `XF86AudioPlay`/`Pause`/`Next`/`Prev`/`Stop` 路由給作用中的 MPRIS 播放器，其他視窗管理員則把它們綁到 `playerctl play-pause` 與類似的指令。`docs/mpris.md` 列出 sway/i3 與 Hyprland 的綁定。TUI 本身不讀取媒體鍵（決策 4）。
