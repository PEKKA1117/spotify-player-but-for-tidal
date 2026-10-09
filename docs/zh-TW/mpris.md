[English](../mpris.md) | 繁體中文

<a id="desktop-controls-and-media-keys-mpris"></a>
# 桌面控制與多媒體鍵 (MPRIS)

播放器會在你的工作階段匯流排 (session bus) 上以 **MPRIS2** 媒體播放器的身分發布自己；MPRIS2 是每個 Linux 桌面用於媒體控制的 D-Bus 介面。有了它，下列項目都能控制 tidal-player，並顯示正在播放的內容（包含專輯封面）：

- GNOME 與 KDE Plasma 的媒體控制（頂端列、鎖定畫面、通知）
- waybar、polybar 等狀態列，以及 KDE Connect
- 鍵盤的多媒體鍵（播放/暫停、下一首、上一首、停止）
- 藍牙耳機的按鈕（透過 BlueZ）
- `playerctl`

每個播放器都會發布自己：獨立模式的 TUI、`tidal-player daemon` 與 `tidal-player play`。附加的 TUI 與 `tidal-player playback …` 是該播放器的用戶端，從不使用匯流排。設計：[spec 0010](../specs/0010-mpris.md)。

<a id="the-bus-name"></a>
## 匯流排名稱

播放器擁有 `org.mpris.MediaPlayer2.tidal_player`（`playerctl -p tidal_player`）。同一匯流排上的第二個播放器（以另一個 `TIDAL_PLAYER_RUNTIME_DIR` 啟動）會改用 `org.mpris.MediaPlayer2.tidal_player.instance<pid>`，這是 MPRIS 規格的要求。

<a id="playerctl"></a>
## `playerctl`

```sh
playerctl -p tidal_player play-pause
playerctl -p tidal_player next
playerctl -p tidal_player pause          # 暫停；已暫停的播放器維持暫停
playerctl -p tidal_player position 90    # 曲目的 1:30
playerctl -p tidal_player position 10+   # 前進 10 秒
playerctl -p tidal_player volume 0.5     # 50 %
playerctl -p tidal_player shuffle On
playerctl -p tidal_player loop Playlist  # 重複佇列（None、Playlist、Track）
playerctl -p tidal_player open https://tidal.com/browse/album/123
playerctl -p tidal_player metadata --follow
```

`open` 接受與 [`tidal-player playback load`](playback.md#items) 相同的內容（曲目、專輯或播放清單連結、`tidal://…`）：它會取代佇列並開始播放。

<a id="what-the-desktop-sees"></a>
## 桌面看到的內容

| 顯示為 | 來自播放器 |
|---|---|
| Playing、Paused、Stopped | 播放中（曲目載入或緩衝時也是）、已暫停（裝置釋放時也是）、已停止 |
| 標題 | 曲目標題，有版本時以括號附上（`Hell Above (Live)`） |
| 演出者 | 曲目的所有演出者 |
| 專輯、封面 | 專輯與其封面（見[專輯封面](#album-covers)） |
| 長度、位置 | 曲目長度與目前位置；跳轉會立即通知 |
| 音量 | 音量（靜音時為 `0`）；設定音量會解除靜音 |
| 隨機、循環 | 隨機播放；重複 `off`、`queue`、`track` 對應 `None`、`Playlist`、`Track` |
| 下一首／上一首按鈕 | 目前曲目之後還有曲目，或重複播放開啟時，下一首可用；有目前曲目時，上一首可用 |

按鈕與指令依播放器的實際狀態動作，與 TUI 中的 `Space`、`n`、`p` 相同：

| 指令 | 作用 |
|---|---|
| Play | 暫停或停止時開始播放；已在播放時不做任何事 |
| Pause | 播放時暫停；已暫停時不做任何事 |
| Play/pause | 切換，與 `Space` 相同 |
| Stop | 停止、釋放裝置並回到 `0:00`；之後的 Play 會從頭播放該曲目 |
| Next、previous | 與 `n`、`p` 相同 |
| Seek、set position | 與跳轉鍵相同；超過曲目結尾的位置會移到下一首 |
| Quit、raise | 不做任何事：桌面小工具無法停止常駐程式（`tidal-player daemon stop` 與 `q` 可以） |

播放器無法執行的指令會以播放器的錯誤回應（`Album 123 was not found`）；5 秒內沒有回應的播放器會以逾時回應。曲目本身的失敗（輸出裝置忙碌、曲目無法播放）則是播放器的訊息，與 TUI 中相同。

還原的播放器（[恢復上次的工作階段](playback.md#resuming-the-last-session)）會以停止狀態發布，並帶有還原的曲目，因此桌面的播放按鈕可以開始播放。

<a id="album-covers"></a>
## 專輯封面

播放器會將目前曲目的專輯封面下載到它的**快取目錄**，並將檔案交給桌面，因此每個小工具與通知都能顯示它：

- **位置**：`$TIDAL_PLAYER_CACHE_DIR`，否則 `$XDG_CACHE_HOME/tidal-player`，否則 `~/.cache/tidal-player`；封面位於 `covers/`，每個一個 `<cover>.jpg`（640×640）
- **數量**：最多 `max_cover_arts` 個（在 [`app.toml`](config.md#apptoml) 中，預設 `20`，`0` 到 `1000`），或 `TIDAL_PLAYER_MAX_COVER_ARTS`；最久未使用的先刪除。調低的上限於下次啟動時生效
- **`max_cover_arts = 0`**：不下載也不寫入任何檔案；桌面會取得 Tidal 的圖片網址並自行下載（GNOME 與 KDE 會；有些通知程式只顯示本機檔案）
- 封面下載期間，曲目先不帶封面顯示，之後再帶封面顯示。無法下載或寫入的封面會改用 Tidal 的網址顯示，記錄檔會說明原因一次
- `tidal-player logout` 不會清除快取：封面是公開圖片，不是你帳號的資料。要清空就刪除該目錄

<a id="media-keys"></a>
## 多媒體鍵

**GNOME、KDE Plasma** 等完整桌面：不需要任何設定。多媒體鍵會送到最後播放的播放器。

**sway、i3**（`~/.config/sway/config` 或 `~/.config/i3/config`）：

```
bindsym XF86AudioPlay exec playerctl -p tidal_player play-pause
bindsym XF86AudioPause exec playerctl -p tidal_player pause
bindsym XF86AudioStop exec playerctl -p tidal_player stop
bindsym XF86AudioNext exec playerctl -p tidal_player next
bindsym XF86AudioPrev exec playerctl -p tidal_player previous
```

**Hyprland**（`~/.config/hypr/hyprland.conf`；`bindl` 在鎖定畫面也有效）：

```
bindl = , XF86AudioPlay, exec, playerctl -p tidal_player play-pause
bindl = , XF86AudioPause, exec, playerctl -p tidal_player pause
bindl = , XF86AudioStop, exec, playerctl -p tidal_player stop
bindl = , XF86AudioNext, exec, playerctl -p tidal_player next
bindl = , XF86AudioPrev, exec, playerctl -p tidal_player previous
```

省略 `-p tidal_player` 則控制 `playerctl` 自行選擇的播放器。TUI 本身不讀取多媒體鍵：終端機很少收到它們，而桌面無論如何都會將它們送到這裡。

<a id="turning-it-off"></a>
## 關閉

在 [`app.toml`](config.md#apptoml) 中設定 `mpris = false`，或 `TIDAL_PLAYER_MPRIS=off`：播放器不會為 MPRIS 連線到工作階段匯流排（也不下載封面）。播放、TUI 與常駐程式的用戶端都不受影響。

<a id="no-session-bus"></a>
## 沒有工作階段匯流排

透過 SSH、在無介面的主機或容器中，通常沒有工作階段匯流排。播放器會照常執行，只是沒有 MPRIS；常駐程式的記錄（`journalctl --user -u tidal-player`）會說一次 `MPRIS is not available: …`。這不是錯誤，播放器執行期間也不會重試：匯流排出現後重新啟動播放器即可。

在 systemd 下，常駐程式會連到你使用者工作階段的匯流排（`dbus.socket`）。透過 SSH 執行的 `tidal-player play` 會發布在 `DBUS_SESSION_BUS_ADDRESS` 指定的匯流排上（例如轉送過來的匯流排）。

<a id="troubleshooting"></a>
## 疑難排解

| 症狀 | 檢查項目 |
|---|---|
| `playerctl -l` 沒有列出 `tidal_player` | 是否有播放器在執行（`tidal-player playback status`）？`mpris` 是否關閉？播放器看到的匯流排是否與桌面相同（否則常駐程式的記錄會說 `MPRIS is not available`）？ |
| 多媒體鍵控制了另一個播放器 | 在 GNOME/KDE 上，先在 tidal-player 播放一次。使用 `playerctl` 綁定時，加上 `-p tidal_player` |
| 沒有封面 | 該曲目的專輯在 Tidal 上沒有封面，或封面無法下載（記錄會說明原因）；`max_cover_arts = 0` 則交由桌面自行下載 |
