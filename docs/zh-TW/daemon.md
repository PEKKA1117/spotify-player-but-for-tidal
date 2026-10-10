[English](../daemon.md) | 繁體中文

<a id="the-daemon-and-clients"></a>
# 常駐程式 (daemon) 與用戶端

`tidal-player` 可以作為**播放器**單獨執行（`tidal-player daemon`，例如作為 systemd 使用者服務），並由任意數量的**用戶端**控制：TUI、一次性指令（例如 `tidal-player playback next`），或是第二個 `tidal-player --add-to-queue <link>`。設計：[spec 0005](../specs/0005-daemon-and-clients.md)。

<a id="players-and-clients"></a>
## 播放器與用戶端

**播放器**負責播放：它持有佇列、模式、工作階段與音訊裝置。**用戶端**只會將指令傳送給播放器，並顯示播放器回傳的內容。每位使用者同一時間最多只有一個播放器。

| 你執行 | 沒有播放器在執行 | 有播放器在執行 |
|---|---|---|
| `tidal-player [ITEM]...` | 成為播放器並顯示 [TUI](tui.md)（獨立模式） | 將 TUI **附加**到該播放器；若有項目，先將項目傳送給它（見下文） |
| `tidal-player daemon` | 成為播放器，無介面執行 | 以 3 結束：`Another player is running (pid N)` |
| `tidal-player play ITEM...` | 成為播放器，無介面，在前景執行（[`play`](playback.md#commands)） | 以 3 結束：`Another player is running (pid N): use "tidal-player playback load"` |
| `tidal-player playback …` | 以 1 結束：`No player is running: start "tidal-player" or "tidal-player daemon"` | 傳送一個指令，印出回應後結束 |
| `tidal-player daemon stop` | 以 1 結束，訊息相同 | 要求播放器關閉，並等到它結束（最多 5 秒） |

每個播放器（獨立模式的 TUI、`daemon` 與 `play`）都接受用戶端連線，因此 `playback` 與附加的 TUI 對它們任何一個都能運作。

**退出**（`q`）附加的 TUI 只會將它卸離：播放器會繼續播放。退出獨立播放器的 TUI 則會停止該播放器，與先前相同；它的其他用戶端會顯示 `The player shut down: waiting for it to come back…`，並附加到下一個啟動的播放器。

<a id="attaching-a-tui"></a>
### 附加 TUI

`tidal-player` 會先尋找正在執行的播放器。若找到，TUI 會附加到它，而不是啟動第二個播放器。若有項目，會在 TUI 開啟之前傳送給播放器：

| 指令 | 播放器 |
|---|---|
| `tidal-player ITEM...` | 以這些項目取代它的佇列，並播放第一個 |
| `tidal-player --add-to-queue ITEM...` | 將它們加到佇列的最後 |
| `tidal-player --play-next ITEM...` | 將它們加在目前曲目的正後方 |

當播放器原本沒有在播放任何東西（佇列為空，或已停止且沒有目前曲目）時，第一個加入的曲目會開始播放。由播放器取得這些項目（`Album 123 was not found` 會顯示在播放視窗中）；用戶端不需要自己登入：它從不讀取工作階段、金鑰圈 (keyring) 或密語，也從不開啟音訊裝置。

附加的 TUI 外觀與操作都和獨立模式的 TUI 相同（[TUI](tui.md)），並有自己的按鍵與 TUI 設定：它讀取自己的 `keymap.toml` 與 `app.toml`（音量與跳轉幅度、頁面大小、音樂庫版面），而不是常駐程式的（見[設定](config.md)）。它只顯示播放器傳來的內容：用 `o` 加入的項目，要等播放器加入之後才會出現在佇列中。若連線中斷，最後的畫面會保留，訊息列顯示 `Disconnected from the player: reconnecting…`（或 `The player shut down: waiting for it to come back…`），播放按鍵不會有作用，TUI 每秒重試一次。重新連上後，畫面會顯示播放器目前的狀態。游標按鍵與 `q` 在此期間都能使用。

<a id="running-the-daemon-under-systemd"></a>
## 在 systemd 下執行常駐程式

`tidal-player daemon unit` 會印出一個 systemd 使用者單元，從這個 `tidal-player` 的安裝位置啟動常駐程式。安裝並啟動它：

```sh
mkdir -p ~/.config/systemd/user
tidal-player daemon unit > ~/.config/systemd/user/tidal-player.service
systemctl --user daemon-reload
systemctl --user enable --now tidal-player
```

程式本身從不寫入單元檔，也從不自行執行 `systemctl`。單元內容如下：

```ini
[Unit]
Description=tidal-player daemon
Documentation=https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/main/docs/daemon.md
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=notify
ExecStart=/home/you/.cargo/bin/tidal-player daemon
Restart=on-failure
RestartSec=5
# 1: not logged in or the session store is unusable; 2: bad settings;
# 3: another player is running. Restarting fixes none of them.
RestartPreventExitStatus=1 2 3
# Settings (docs/playback.md#settings), e.g.:
#Environment=TIDAL_PLAYER_DEVICE=hw:1,0
# Passphrase for an encrypted session file (docs/login.md):
#LoadCredential=tidal-player-passphrase:%h/.config/tidal-player/passphrase

[Install]
WantedBy=default.target
```

- **設定**來自 [`app.toml`](config.md#apptoml)，與 TUI 相同：`~/.config/tidal-player/app.toml`（若要使用其他資料夾，`ExecStart=… daemon -c DIR`）。環境變數仍然有效，且優先於檔案：取消 `Environment=` 那一行的註解並編輯，每個變數一行（[設定](playback.md#settings)）。常駐程式只在啟動時讀取檔案一次：修改後請執行 `systemctl --user restart tidal-player`。損壞的 `app.toml` 或 `keymap.toml` 會讓它以 2 結束，訊息會寫入 journal
- **密語**：常駐程式無法詢問密語。若你的工作階段存放在加密檔案而非金鑰圈中，請以憑證提供（取消 `LoadCredential=` 那一行的註解，見[登入](login.md#giving-the-passphrase-to-the-daemon)）
- **先登入**，使用 `tidal-player login`。常駐程式從不啟動登入流程
- **結束代碼 1、2 與 3 不會重新啟動**：未登入或工作階段儲存區無法使用（1）、設定錯誤（2）、已有其他播放器在執行（3）。每隔幾秒重試也無法解決其中任何一個，因此 systemd 會讓單元維持失敗狀態。修正原因（`systemctl --user status tidal-player` 與 `journalctl --user -u tidal-player` 會顯示訊息），然後執行 `systemctl --user restart tidal-player`。其他任何失敗會在 5 秒後重新啟動，5 分鐘內最多 5 次
- 開著獨立模式的 TUI 時，服務無法啟動（以 3 結束，不會重新啟動）。請先退出 TUI，或改為將 TUI 附加到常駐程式（常駐程式執行後，`tidal-player` 就會這麼做）

常駐程式在用戶端可以連線後通知 systemd 它已就緒（`Type=notify`）。收到 `systemctl --user stop`（`SIGTERM`）、`SIGINT` 或 `tidal-player daemon stop` 時，它會通知用戶端、停止播放、釋放裝置並以 0 結束。訊息輸出到 stderr，在 systemd 下即為 journal。重新啟動的常駐程式會接續先前的佇列，停止在相同的位置，並保持靜音直到有東西播放它（見[接續上次的工作階段](playback.md#resuming-the-last-session)）。

在任一終端機執行 `tidal-player login` 之後，正在執行的常駐程式會在幾秒內取得新的工作階段（見[「Session expired」](login.md#session-expired)）。

<a id="one-shot-commands"></a>
## 一次性指令

`tidal-player playback <command>` 會將一個指令傳送給正在執行的播放器，然後結束：

| 指令 | 作用 |
|---|---|
| `play-pause` | 播放／暫停 |
| `play` | 播放或繼續播放；已在播放時不做任何事 |
| `pause` | 暫停；已暫停時不做任何事 |
| `stop` | 停止並釋放裝置；目前曲目保留，回到 `0:00` |
| `next`, `previous` | 下一首／上一首曲目（與 TUI 中的 `n`／`p` 相同） |
| `seek S`, `seek +S`, `seek -S` | 跳到曲目的第 `S` 秒，或向前／向後 `S` 秒（`seek 90`、`seek +5`、`seek -2.5`） |
| `volume N`, `volume +N`, `volume -N` | 將音量設為 `N` %（0–100），或調整 `N` 點 |
| `mute`, `shuffle`, `repeat`, `autoplay` | 切換靜音、隨機播放、自動播放；循環切換重複播放 off → queue → track |
| `shuffle on`, `shuffle off` | 開啟或關閉隨機播放（已是該狀態時不做任何事） |
| `repeat off`, `repeat queue`, `repeat track` | 設定重複播放模式 |
| `load ITEM...` | 以這些[項目](playback.md#items)取代佇列，並播放第一個 |
| `add ITEM...`, `add --next ITEM...` | 將項目加到佇列的最後，或加在目前曲目的正後方 |
| `device` | 列出播放器的輸出裝置，以 `*` 標示它正在使用的裝置（見[選擇輸出裝置](playback.md#choosing-the-output-device)） |
| `device NAME` | 將播放器切換到輸出裝置 `NAME`（ALSA PCM 名稱：`hw:1,0`、`default`…），與 TUI 中的 `D` 相同 |
| `status`, `status --json` | 印出正在播放的內容 |

當播放器完成指令時，它不印出任何內容並以 0 結束。否則：

- 錯誤的參數或項目（`volume 101`、`seek x`、`shuffle maybe`、藝人連結、空的裝置名稱）會以 2 結束，且不傳送任何東西
- 沒有播放器在執行時以 1 結束，並顯示 `No player is running: start "tidal-player" or "tidal-player daemon"`
- 播放器的錯誤（`Album 123 was not found`）會印在 stderr，以 1 結束
- 5 秒內沒有回應：`The player did not answer`，以 1 結束

`status` 會印出三行，使用 TUI 的符號與模式：

```
$ tidal-player playback status
▶ Hell Above · Pierce The Veil · Collide With The Sky
1:23 / 3:32 · shuffle · repeat: queue · 80% · hw:1,0
Queue: 2 of 12
```

沒有目前曲目時，只會顯示 `Nothing playing`。若播放器有訊息，第四行會顯示它（`Output hw:1,0 is busy …`），或是 `Session expired: run "tidal-player login"`。第二行結尾是播放器使用中的輸出裝置（`hw:1,0`），裝置已釋放時再接著 `· device released`。`status --json` 會以一行 JSON 印出播放器的完整狀態，供腳本使用。

`playback device` 會詢問正在執行的播放器，因此列出的是播放器所在機器的裝置，並標示播放器的裝置，即使是執行期間才選擇的；它的輸出格式與 [`tidal-player devices`](playback.md#tidal-player-devices) 相同，而後者讀取的是本機的清單，標示設定的裝置，不會詢問任何播放器。`playback device hw:1,0` 在播放器接受新裝置後即以 0 結束，不會等待裝置開啟：無法開啟的裝置會像其他輸出失敗一樣顯示在 `playback status` 與 TUI 中，播放器則留在原本的裝置（見[輸出裝置](tui.md#output-device)）。

`playback add` 可以做成簡單的連結處理程式：例如 `tidal-player playback add --next "$1"`。

<a id="desktop-controls-and-playerctl"></a>
### 桌面控制與 `playerctl`

常駐程式也是你工作階段匯流排上的 MPRIS 媒體播放器：GNOME 與 KDE 的媒體控制、鍵盤的多媒體鍵與 `playerctl -p tidal_player play-pause` 都能在沒有開啟 TUI 時控制它。請見[桌面控制與多媒體鍵](mpris.md)。

<a id="stopping-the-daemon"></a>
### 停止常駐程式

`tidal-player daemon stop` 會要求播放器關閉，並在它結束後返回（以 0 結束），最多 5 秒後。它對任何播放器都有效，包括獨立模式的 TUI。在 systemd 下，建議使用 `systemctl --user stop tidal-player`：`daemon stop` 會讓常駐程式以 0 結束，systemd 不會重新啟動它，但單元會維持停止狀態，直到下一次 `start`。

<a id="releasing-the-device-while-paused"></a>
## 暫停時釋放裝置

暫停時，播放器會放開音訊裝置，讓其他應用程式可以使用你的 DAC：

- 暫停達到**釋放延遲**（預設 10 秒）後，它會關閉裝置，若為獨占（`hw:`）輸出，則將音效卡交還給 PipeWire 或 PulseAudio。曲目、其位置與已緩衝的內容都會保留。此時 TUI 的第三列結尾會是 `· device released`，`playback status` 也會顯示這一點
- 暫停期間，若有其他應用程式（透過 PipeWire 的裝置保留機制）要求使用音效卡，不論延遲為何都會立即釋放。播放或載入中時，播放器會拒絕，與先前相同
- **繼續播放**會重新開啟裝置，並從停止的位置準確繼續：不會跳過或重複播放任何內容。若裝置此時忙碌或已不存在，播放器會維持暫停並顯示訊息（`Output hw:1,0 is busy (used by …)`），且從不跳過；`Space`（或 `playback play-pause`）會再試一次
- 在已釋放狀態下跳轉會移動播放位置；繼續播放時從新的位置開始

| 設定 | `app.toml` | 環境變數 | 接受值 | 預設 |
|---|---|---|---|---|
| 暫停多久後釋放裝置（秒） | `release_paused_secs` | `TIDAL_PLAYER_RELEASE_PAUSED` | 整數 0–3600，或 `never` | `10` |

`0` 會在暫停時立即釋放；`never` 會在暫停期間保持裝置開啟（其他應用程式要求使用時仍會讓出）。此設定適用於每個播放器：TUI、`play` 與常駐程式（在 `app.toml` 中設 `release_paused_secs = 0`，或在單元中設 `Environment=TIDAL_PLAYER_RELEASE_PAUSED=0`）。

<a id="where-clients-find-the-player"></a>
## 用戶端如何找到播放器

播放器與用戶端在**執行期資料夾**中會合：若有設定則為 `$TIDAL_PLAYER_RUNTIME_DIR`，否則為 `$XDG_RUNTIME_DIR/tidal-player`，再否則為 `/tmp/tidal-player-<uid>`。其中存放播放器的 socket（`player.sock`）與它的鎖定檔（`player.lock`，內含播放器的 pid）。這個資料夾建立時為私有（模式 `0700`）；若資料夾屬於其他使用者或其他人可以讀取，就會被拒絕，因此其他人無法連到你的播放器。

`XDG_RUNTIME_DIR` 由你的登入工作階段設定。在沒有設定的情況下（某些 SSH 工作階段、沒有 logind），shell 與 systemd 下的常駐程式可能會到不同的位置尋找而找不到彼此：請在兩者中都設定 `XDG_RUNTIME_DIR=/run/user/$(id -u)`（或相同的 `TIDAL_PLAYER_RUNTIME_DIR`）。

<a id="troubleshooting"></a>
## 疑難排解

| 訊息 | 意義 |
|---|---|
| `Another player is running (pid N)` | `daemon` 或 `play` 發現已有播放器在執行（TUI、另一個常駐程式、`play`）。將它當作用戶端使用（`tidal-player`、`tidal-player playback …`），或先停止它（`tidal-player daemon stop`，或退出它的 TUI）。以 3 結束 |
| `The running player is tidal-player X, this is Y: restart it ("tidal-player daemon stop", or "systemctl --user restart tidal-player")` | 播放器是此程式的另一個版本，通常是升級後仍在執行的舊常駐程式。重新啟動它，讓兩者版本相同。TUI 會顯示此訊息且不會重試 |
| `A player is running (pid N) but not answering on <socket>` | 有程序持有鎖定檔，但 2 秒內 socket 上沒有回應：可能是播放器仍在啟動中（請再試一次），或是已卡住（`kill N`）。以 1 結束 |
| `Runtime directory <path> is not private: …` | 執行期資料夾不是資料夾、屬於其他使用者，或其他人可以存取。程式從不會替你修改它：請將它刪除，或修正它的擁有者與權限模式（`chmod 700`）。以 1 結束 |
| `No player is running: start "tidal-player" or "tidal-player daemon"` | `playback` 或 `daemon stop` 找不到播放器。若確實有播放器在執行，請檢查它是否使用相同的執行期資料夾（見上文） |
| `Disconnected from the player: reconnecting…` | 附加的 TUI 失去了它的播放器（播放器當掉或被重新啟動）；它會自行重新連線 |
