[English](../0005-daemon-and-clients.md) | 正體中文

<a id="0005--daemon--client-mode"></a>
# 0005 — 常駐程式與客戶端模式

- **狀態**：implemented（2026-10-07；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0001（implemented：執行模式、`Command`/`Event` 邊界）、0002（implemented：常駐程式的通關密語來源、`LoginRequired`/`LoginRestored`）、0003（implemented：引擎與裝置保留）、0004（implemented：播放器執行期、TUI）
- **使用者文件**：新增 [`docs/daemon.md`](../../zh-TW/daemon.md)；[`docs/playback.md`](../../zh-TW/playback.md)、[`docs/tui.md`](../../zh-TW/tui.md) 與 [`docs/login.md`](../../zh-TW/login.md) 去掉各自的「until 0005」註記（AC24）

<a id="context"></a>
## 背景

0004 讓播放器在 TUI 的行程中執行。本規格讓它能獨立執行（`tidal-player daemon`，一個 systemd 使用者服務），並讓任意數量的客戶端連上它：TUI 客戶端、一次性指令（`tidal-player playback next`），以及第二個 `tidal-player --add-to-queue <link>`。它也讓引擎在暫停時釋放音訊裝置，讓其他應用程式能使用 DAC，如同 tidalt 的常駐程式所做的。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 `cmd/tidalt/{main,daemon}.go`、`internal/mpris/server.go`、`internal/ui/{model,queue}.go`、`docs/client-server.md`，以及它已關閉的 issue #3–#8 與 #19–#21：

- 第一個啟動的 `tidalt` 會取得 D-Bus 名稱 `org.mpris.MediaPlayer2.tidalt` 並成為**父行程**。之後每次啟動都發現名稱已被占用，就成為它的客戶端，不論父行程是常駐程式還是 TUI。`tidalt daemon` 是同一個程式，只是沒有 UI
- 客戶端透過私有的 D-Bus 介面 `io.tidalt.App` 與父行程溝通，每個功能一個方法，隨著發現錯誤逐一加入（`OpenURL`、`PlayTrackID`、`PlayPlaylist`、`Enqueue`、`Dequeue`、`SetShuffle`、`SetDevice` 等）
- 播放器的邏輯（佇列、隨機播放、自動前進）存在於 BubbleTea 的 UI 模型中。父行程的模型負責執行它；客戶端則針對自己的佇列副本執行同一個模型的第二份
- 客戶端以每秒**輪詢** `GetState` 的方式得知父行程的狀態。`GetState` 以 JSON 字串回傳目前曲目與整個播放清單，加上狀態、位置、長度、音量、裝置與隨機播放模式
- 父行程以非阻塞傳送（`select { case ch <- ev: default: }`）把每個 D-Bus 呼叫轉成通道上的事件：通道已滿時，該呼叫會被**丟棄**，呼叫者卻仍收到成功
- 常駐程式只在播放時才開啟裝置。暫停會關閉 PCM 並釋放 `ReserveDevice1` 名稱；恢復播放時再重新取得兩者
- `tidalt setup --daemon` 會寫入一個 systemd 使用者單元（`Restart=on-failure`、`RestartSec=5s`）並啟用它

<a id="what-went-wrong-there-and-the-criterion-that-covers-it-here"></a>
### 當時出了什麼問題，以及這裡由哪個條件涵蓋

1. **在客戶端所做的編輯被下一次輪詢復原**（#6；`0092235`、`3519907`）：「Add to queue」、「Play next」與「Remove」改的是客戶端自己的副本。父行程從未聽說這些改動，一秒內輪詢就還原了它的清單。加入的曲目從未播放；移除的曲目照樣播放 → 客戶端沒有自己的佇列：每個編輯都是送給播放器的 `Command`，客戶端只顯示播放器送回來的內容（0004 AC22；此處為 AC5、AC12）
2. **從客戶端播放播放清單時只載入一首曲目**（#5；`a932e57`、`ce05a23`）：除非某個只有一條程式路徑會設定的旗標為 true，否則客戶端會送出 `PlayTrackID`，於是父行程的佇列縮成只剩那一首 → 只有一種載入方式：`LoadQueue`/`Open` 帶著整個清單（AC7）
3. **隨機播放被套用兩次，然後遺失**（#19、#20、#21；`fb4721c`）：客戶端在每次輪詢時把父行程已打亂的清單再打亂一次；客戶端中的 `s` 從未被轉送；從已隨機播放的客戶端送出的播放清單以顯示順序送達，父行程把它存成原始順序且關閉隨機播放 → 隨機播放只存在於播放器中（0004 AC3）；客戶端以原始順序送出項目或曲目，絕不送出播放順序（AC7）
4. **客戶端的動作在本機執行而沒有任何效果**（#4 的後續 `2c94bbd`：在客戶端選擇裝置時，呼叫的是客戶端自己的播放器，而它沒有開啟任何裝置）→ 客戶端行程沒有播放器、沒有引擎、沒有工作階段（AC14）
5. **呼叫被默默丟棄**（每個 `io.tidalt.App` 方法上的 `select … default:`）：忙碌的父行程遺失指令，而客戶端回報成功 → 每個請求都恰好得到一個回覆，在它被套用之後；客戶端連線期間，客戶端與播放器之間沒有任何東西被丟棄（AC5、AC6）
6. **輪詢**讓每個客戶端最多落後一秒，並與編輯產生競態（#6）→ 播放器推送每一項變更；客戶端落後的時間絕不超過遞送一則訊息所需的時間（AC4）
7. **重複項目有歧義**：`Dequeue` 移除「第一個具有這個曲目 ID 的項目」，因為兩個各自獨立打亂的副本中位置不同 → 使用 0004 的項目 ID 來定址項目；播放器的順序是唯一的順序（0004 AC10）
8. **TUI 開著時，`tidalt daemon` 不停重新啟動**（#8；`69f6744`）：名稱被占用時常駐程式以 1 結束，單元每 5 秒不限次數地重新啟動它，日誌的重新啟動計數達到 109 → 「另一個播放器正在執行」以 **3** 結束，而這裡產生的單元設定 `RestartPreventExitStatus=1 2 3` 與啟動次數限制（AC11、AC18）
9. **兩個行程搶用同一個資料庫**（`1d3faed`、`a8b5e7e`；#8 中的 bolt 鎖逾時）：父行程與每個客戶端都開啟同一個儲存庫 → 客戶端除了 socket 之外，不在磁碟上開啟任何東西（AC14）。持久化（0009）只屬於播放器行程
10. **釋放後恢復播放時當機或播放了錯誤的曲目**（`f93fa38`、`2afa977`）：恢復播放時重新取得失敗，導致 PCM 被釋放兩次（libasound 中的 use-after-free）；卡住的重新取得拖過 `stop()` 的 3 秒期限，第二個播放迴圈因而啟動，兩個迴圈搶用裝置；對一個從不回應的持有者發出的 `RequestRelease` 永遠阻塞；保留錯誤被誤認為曲目結束，而在迴圈中自動前進 → 釋放與重新取得都在引擎執行緒中進行，只有它擁有裝置，因此絕不會有第二個輸出；重新取得失敗時，播放器維持**暫停**並顯示訊息，絕不跳過（AC19–AC21，以及 0003 AC13）
11. **父行程是誰先來誰就是**：結束碰巧是父行程的那個 TUI，會讓所有客戶端底下的音樂停止，而客戶端無從得知 → 客戶端會被告知（`ShuttingDown`，或連線中斷）、顯示出來，並在播放器再次出現時重新連線（AC12、AC13）

這些問題大多（1–7、9、11）是同一個錯誤的不同形式：**客戶端與播放器失去同步**，因為客戶端自己持有狀態（佇列、隨機順序、裝置、資料庫），在本機修改它，並且太晚（輪詢）或從未（被丟棄的呼叫）得知播放器的版本。本規格移除的是成因，而非逐一處理症狀；「同步」之下的規則，就是下方每個條件所要強制的內容。

<a id="behaviour"></a>
## 行為

<a id="sync"></a>
### 同步

1. **播放器的狀態是唯一的狀態。**客戶端對於播放一無所有，只有最後收到的 `Welcome`/`Event`：沒有自己的佇列、順序、模式、裝置、儲存庫。它自己的狀態只限於其他行程都不需要的部分：游標、開啟中的輸入提示、頁面
2. **客戶端在本機不改變任何東西。**每個影響播放的按鍵都送出一個 `Command`，而畫面上不做任何改變；畫面在播放器的事件抵達時才改變（隔一則訊息，不輪詢）
3. **每項變更都依序、恰好一次地推送給每個客戶端**，從一個與之一致的 `Welcome` 開始（快照與其後的事件之間沒有缺口也沒有重疊）
4. **沒有任何東西被默默丟棄。**指令在被套用之後才得到回覆；跟不上的客戶端會被中斷連線（然後從新的 `Welcome` 重新同步），絕不被跳過
5. **指令帶的是意圖，而非位置。**項目以項目 ID 定址；清單以原始順序送出；模式在播放器中切換（0004）

由此得出的結論，並以此測試（AC4）：不論任意數量的客戶端送出的指令如何交錯，一旦播放器閒置，每個已連線客戶端的檢視都等於播放器的快照。

<a id="roles"></a>
### 角色

**播放器**行程執行 `tidal_player_core::player` 與引擎。**客戶端**對播放器送出指令並顯示它的事件。每位使用者同一時間**最多有一個播放器**。

| 呼叫方式 | 沒有播放器在執行 | 已有播放器在執行 |
|---|---|---|
| `tidal-player [ITEM]...` | 成為播放器（獨立模式，如 0004）並顯示 TUI | 將 TUI 客戶端**連上**它；若有項目，先送出 `Open`（見下） |
| `tidal-player daemon` | 成為播放器，無畫面 | 以 **3** 結束：`Another player is running (pid N)` |
| `tidal-player play ITEM...` | 成為播放器，無畫面，在前景執行（0004） | 以 **3** 結束，訊息相同，另加 `: use "tidal-player playback load"` |
| `tidal-player playback …` | 以 1 結束：`No player is running: start "tidal-player" or "tidal-player daemon"` | 送出一個請求，印出它的答覆，然後結束 |
| `tidal-player daemon stop` | 以 1 結束，訊息相同 | 要求播放器關閉；等到它消失為止（最多 5 秒）；以 0 結束 |

每個播放器（獨立模式的 TUI、`daemon`、`play`）都提供 socket，因此一次性指令與 TUI 客戶端對其中任何一種都能運作。獨立模式 TUI 自己的畫面本身也是一個客戶端，在行程內與它的播放器相連（0001），使用相同的訊息。

結束（`q`）**客戶端** TUI 只是把它中斷連線；播放器繼續播放。結束**獨立模式**播放器的 TUI 會停止該播放器，如 0004；它的其他客戶端會看到 `The player shut down`（見下）。

<a id="transport"></a>
### 傳輸

**執行期目錄**中的一個 Unix domain socket，`player.sock`：

1. `$TIDAL_PLAYER_RUNTIME_DIR`，若已設定且不為空（測試、特殊的環境）
2. `$XDG_RUNTIME_DIR/tidal-player`
3. `/tmp/tidal-player-<uid>`

目錄以權限 `0700` 建立。已存在但不是目錄、不屬於這位使用者，或群組或其他人可存取的目錄會被拒絕（`Runtime directory <path> is not private: …`，以 1 結束），因此沒有其他使用者能連到播放器。socket 建立在其中。

**每位使用者一個播放器**由同一目錄中的 `player.lock` 強制執行：播放器在它的整個生命週期中對它取得獨占鎖（`File::try_lock`），並把自己的 pid 寫入其中。「有播放器在執行」指的就是鎖被持有。取得鎖的播放器會在綁定之前移除遺留的 `player.sock`（當機的播放器）。結束時它會移除 socket；鎖則隨行程消失，不論行程如何結束。

發現鎖被持有的客戶端會連到 socket，最多重試 2 秒（播放器可能仍在啟動）。若仍無法連線：`A player is running (pid N) but not answering on <socket>`（以 1 結束）。

<a id="messages"></a>
### 訊息

socket 上以換行分隔的 JSON：每行一則訊息，UTF-8，每行最多 **16 MiB**（幾千首曲目的 `LoadQueue` 小於 1 MiB）。型別位於 `tidal_player_core::protocol`；JSON 編碼位於執行檔中（core 沒有 `serde_json`，0001 AC3）。

1. 接受連線時，播放器送出**問候**，`{"tidal_player":"<version>"}`。它的形狀是固定的：之後每個版本都原封不動地送出它，如此舊客戶端與新播放器（或反過來）能分辨彼此是什麼。版本不同的客戶端會關閉連線：`The running player is tidal-player <theirs>, this is <ours>: restart it ("tidal-player daemon stop", or "systemctl --user restart tidal-player")`（一次性指令以 1 結束；TUI 會顯示它而不重試）
2. 接著客戶端送出 `ClientMessage`：
   - `Subscribe`：播放器回答 `Welcome { snapshot, login_required }`，之後把每個 `Event` 送給這個客戶端
   - `Request { id, command }`：播放器套用 `command` 並回答 `Reply { id, result: Ok | Err(message) }`
3. 播放器送出 `ServerMessage`：`Welcome`、`Event(Event)`、`Reply`

順序：

- 訂閱的客戶端會得到一個反映播放器在它之前處理過的每個輸入的 `Welcome`，然後依序恰好一次地得到之後的**每個**事件。套用 `Welcome` 再套用這些事件的客戶端，最終會得到播放器的狀態（沒有缺口，沒有過時的快照）
- `Reply` 在其指令所造成的事件之後送出。事件會送給每個訂閱者（若發送者有訂閱，也包括它）
- 所有客戶端的指令會一次一個、依播放器收到的順序套用。0004 的切換型指令讓兩個檢視過時的客戶端不會互相復原對方

送出的某行不是有效訊息或超過大小上限的客戶端會被中斷連線；播放器與其他客戶端繼續運作。停止讀取的客戶端絕不會阻塞播放器：每個客戶端有一個容納 **1024** 則訊息的發送佇列，發送佇列已滿的客戶端會被中斷連線（它可以重新連線並得到新的 `Welcome`）。TUI 至少每 100 ms（0004 的畫面更新）就會清空它的 socket，因此只有停止或被暫停的客戶端（Ctrl-Z）才會走到這一步，大約在四分鐘的位置事件之後。

<a id="opening-items-in-the-player"></a>
### 在播放器中開啟項目

`Command::Open { items, at }`，新增：`items` 是 0004 的項目（曲目 ID、曲目／專輯／播放清單連結），已由客戶端這一側的 `parse_item` 檢查過（不良項目會像 0004 一樣以 2 結束或在該處顯示訊息，不會聯絡播放器）。由**播放器**展開它們（0004「填入佇列」），然後：

- `at: None`：`LoadQueue { tracks, start: 0 }`（取代佇列，播放第一首）
- `at: Some(End | Next)`：`AddToQueue { tracks, at }`，然後當播放器為 `Stopped` 且沒有目前項目，或佇列原本是空的時，對第一個加入的項目執行 `PlayEntry`（0004 AC28 的規則，從 TUI 的執行期移到播放器中）

`Reply` 在曲目都排入佇列之後送出，或帶著展開時的錯誤訊息（`Album 1 was not found` 等）；錯誤不會改變任何東西。`Open` 依收到的順序套用，即使較晚的那個先完成展開。TUI 的開啟輸入提示（`o`/`O`）與它的啟動項目也都經由 `Open`，因此客戶端不需要自己的 API 存取（決策 3）。

<a id="the-tui-as-a-client"></a>
### 作為客戶端的 TUI

- 收到 `Welcome` 時，客戶端狀態取得快照（0004 AC22）與登入狀態，播放視窗顯示播放器
- **已中斷連線**（連線關閉或失敗，或 `Event::ShuttingDown`）：最後的快照保留在畫面上；訊息列顯示 `Disconnected from the player: reconnecting…`，或在 `ShuttingDown` 之後顯示 `The player shut down: waiting for it to come back…`。會送出指令的按鍵不做任何事；游標按鍵與 `q` 仍有作用。客戶端每秒嘗試重新連線；成功時再次訂閱，由 `Welcome` 取代所有內容
- 版本不符會被顯示（上述訊息）而不重試
- 客戶端 TUI 的 `q` 只中斷連線：不送出 `Shutdown`

<a id="one-shot-commands"></a>
### 一次性指令

`tidal-player playback <command>`：連線、送出一個 `Request`（`status` 則是 `Subscribe`）、等待答覆（最多 5 秒，否則 `The player did not answer` 並以 1 結束）、印出、結束。

| 指令 | 送出 |
|---|---|
| `play-pause` | `TogglePause` |
| `next`、`previous` | `Next`、`Previous` |
| `seek +S`、`seek -S`、`seek S` | `SeekBy(±S s)`、`SeekTo(S s)`；`S` 以秒為單位，可含小數 |
| `volume +N`、`volume -N`、`volume N` | `ChangeVolume(±N)`、`SetVolume(N)`；`N` 為 0–100 |
| `mute`、`shuffle`、`repeat`、`autoplay` | `ToggleMute`、`ToggleShuffle`、`CycleRepeat`、`ToggleAutoplay` |
| `load ITEM...` | `Open { at: None }` |
| `add [--next] ITEM...` | `Open { at: Some(End) }`，加上 `--next` 則為 `Next` |
| `status [--json]` | `Subscribe`；印出 `Welcome` 並中斷連線 |

輸出：成功時沒有任何輸出（以 0 結束），`status` 除外；收到 `Err` 時，答覆的訊息輸出到 stderr 並以 1 結束；參數或項目有誤時以 2 結束（不送出任何東西）。`status` 印出三行：

```
▶ Hell Above · Pierce The Veil · Collide With The Sky
1:23 / 3:32 · shuffle · repeat: queue · 80%
Queue: 2 of 12
```

（狀態符號與指示符號取自 0004 的 TUI；沒有目前項目時（佇列為空，或佇列中沒有目前項目）只顯示 `Nothing playing`；若有訊息，則多一個第四行放訊息，需要登入時則是 `Session expired: run "tidal-player login"`）。`--json` 把 `Welcome` 印成一行 JSON，供指令碼使用。

`tidal-player daemon stop` 送出 `Shutdown` 並等到鎖被釋放。

<a id="the-daemon"></a>
### 常駐程式

`tidal-player daemon`：

- 啟動，依序：取得鎖（否則以 3 結束）、載入工作階段（0002：沒有工作階段 → 以 1 結束；已加密但沒有通關密語 → 以 1 結束）、從環境讀取設定（0003/0004；無效 → 以 2 結束）、綁定 socket、告知 systemd 已就緒（設定了 `$NOTIFY_SOCKET` 時，送 `READY=1` 到它），然後等待客戶端。訊息輸出到 stderr（在 systemd 下則是 journal）
- 在有東西播放之前，它絕不開啟音訊裝置（0003 的引擎在 `Play` 時才開啟）
- `SIGTERM` 或 `SIGINT`，或客戶端的 `Shutdown`：對每個訂閱者送出 `Event::ShuttingDown`，引擎停止（裝置關閉、保留釋放），socket 被移除，以 0 結束
- 登入：常駐程式的 `Authenticator` 狀態以 `Event::LoginRequired` / `LoginRestored` 廣播（0002 AC14）；`Welcome` 帶著它。在任何終端機執行 `tidal-player login` 之後，常駐程式會取用新的工作階段（0002 AC9）
- 佇列一開始是空的；自 0009 起，它會從上一次執行還原，並維持停止狀態

**systemd 使用者服務。**`tidal-player daemon unit` 把單元印到 stdout，其中 `ExecStart` 設為目前執行檔的絕對路徑。由使用者安裝它（`docs/daemon.md`）：

```
tidal-player daemon unit > ~/.config/systemd/user/tidal-player.service
systemctl --user daemon-reload
systemctl --user enable --now tidal-player
```

```ini
[Unit]
Description=tidal-player daemon
Documentation=https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/main/docs/daemon.md
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=notify
ExecStart=<absolute path> daemon
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

程式本身從不寫入單元，也不自己執行 `systemctl`（決策 5）。

<a id="releasing-the-device-while-paused"></a>
### 暫停時釋放裝置

引擎在暫停時釋放輸出，讓其他應用程式能使用音效卡：

- 在 `Paused` 狀態經過**釋放延遲**之後（一項設定，`TIDAL_PLAYER_RELEASE_PAUSED`：`0`–`3600` 秒或 `never`；預設 **10**，決策 4），引擎關閉 PCM，獨占輸出時並釋放 `ReserveDevice1` 名稱。它會保留曲目：來源、解碼器與位置。事件 `Released`
- **其他應用程式提出要求**（對我們持有的名稱發出 `RequestRelease`）：在 `Paused` 期間（不論是否已釋放），引擎關閉 PCM、回答 `true`，然後釋放名稱。在載入、播放或緩衝期間，它回答 `false`，如同 0003 現在所做的。它在 400 ms 內回答（0003 的呼叫者等待 500 ms）。即使延遲設為 `never`，這一點仍然成立
- 釋放後**恢復播放**：保留（0003 的表格）、重新開啟並協商，如同曲目開始時，然後從第一個尚未聽到的影格繼續：釋放當時仍在裝置緩衝區中的影格會從解碼器重新播放，因此沒有任何東西遺失或重複。事件 `Resumed`
- **恢復失敗**（忙碌、消失）：引擎維持暫停，沒有任何東西開啟；事件 `ResumeFailed(error)`。播放器在相同位置維持 `Paused`，並顯示 0003 的訊息（`Output hw:1,0 is busy (used by …)`）；它絕不跳過（這不是曲目失敗）。播放／暫停會再試一次；訊息會在下一次成功的 `Resumed` 時清除
- 釋放期間的跳轉會移動位置，並在恢復播放時生效。釋放期間的 `Play`、`Next`、`Stop` 與 `SetDevice` 的行為與未釋放時相同（`Stop` 不會把任何東西關閉兩次，也不會釋放兩次）。`released` 會在曲目開始與播放器停止時清除。沒有開啟任何東西的引擎對 `RequestRelease` 回答 `true`（它什麼都沒持有）
- 共用輸出以同樣方式關閉與重新開啟（沒有保留）
- 快照的正在播放詳細資料新增 `released: bool`；它為 true 時，TUI 的第三列以 ` · device released` 結尾；`playback status` 顯示相同內容

設定表的新增項目（0004「Settings」；在 0008 之前仍來自環境）：

| 設定 | 環境變數 | 接受的值 | 預設值 |
|---|---|---|---|
| 暫停多久後釋放裝置（秒） | `TIDAL_PLAYER_RELEASE_PAUSED` | 整數 0–3600，或 `never` | `10` |

<a id="acceptance-criteria"></a>
## 驗收條件

協定（`tidal_player_core::protocol`，純邏輯）：

- **AC1** — `ClientMessage`（`Subscribe`、`Request { id, command }`）、`ServerMessage`（`Welcome { snapshot, login_required }`、`Event(Event)`、`Reply { id, result }`）、`Command::Open { items, at: Option<InsertAt> }`、`NowPlaying` 中面向引擎的 `released` 欄位，以及其他每個新的變體，都能通過 JSON 往返（0001 AC11 的測試，加以擴充）
- **AC2** — 分幀（`tidal-player::ipc`，對位元組緩衝區的純邏輯）：`encode` 寫出一行 JSON；解碼器能組合出跨多次讀取的訊息、從一次讀取解出多則訊息、拒絕超過 16 MiB 的行而不將它整個緩衝，並拒絕無效的 JSON 與未知的變體，且指出問題所在（表格）
- **AC3** — 問候：播放器的第一行恰好是 `{"tidal_player":"<CARGO_PKG_VERSION>"}`；`check_greeting` 接受相同的版本，對其他版本給出版本不符的訊息（含雙方版本），對任何其他第一行給出 `Not a tidal-player socket: <path>`（表格）

播放器伺服端（`tidal-player`，使用 0004 的假引擎與工作，客戶端走 socket 配對或記憶體內連線）：

- **AC4** — 收斂（「同步」）：在指令處理期間訂閱的客戶端會得到一個 `Welcome`，然後依序恰好一次地得到之後的每個事件；套用它們所得到的，恰好是執行期最終的快照（表格：在 50 個指令的一輪之前、之間與之後訂閱；第二個訂閱者從它的 `Welcome` 起的序列等於第一個的）。並且，在 200 次以種子產生的隨機執行中：3 個客戶端送出每個佇列、模式與音量指令的隨機交錯（編輯針對它們看過的項目，那些項目可能已被移除），其中一個在執行途中中斷連線並重新訂閱；一旦播放器閒置，每個客戶端的 `ui::State` 播放副本都等於播放器的快照（佇列順序、目前項目、模式、音量）。失敗的那次執行的種子會被印出
- **AC5** — 請求：每個 `Request` 恰好得到一個帶有其 `id` 的 `Reply`，在它所造成的事件之後；兩個客戶端的指令依到達順序套用；兩個訂閱者看到相同的事件。沒有任何指令被丟棄：4 個客戶端的 1000 個請求得到 1000 個回覆，且播放器看到 1000 個指令
- **AC6** — 隔離：從不讀取的訂閱者在 1024 則訊息等待時被中斷連線，其他訂閱者繼續接收，播放器繼續處理輸入（在停滯的那個仍待處理時，它會在 1 秒內回答另一個客戶端的請求）；在一行中途中斷連線、送出無效 JSON 或超大行的客戶端會被丟棄，而不影響播放器或其他客戶端
- **AC7** — `Open`：展開在播放器行程中執行（假的中繼資料）；`at: None` 載入並播放；`End`/`Next` 加入，並在停止且沒有目前項目、或佇列原本為空時開始第一個加入的項目；展開錯誤是 `Reply` 的 `Err` 且不改變任何東西；當第二個先完成展開時，兩個 `Open` 仍依收到的順序套用。佇列以項目順序送出，絕不是播放順序（tidalt #21）
- **AC8** — 登入：`Authenticator` 狀態的變化會以 `Event::LoginRequired`/`LoginRestored` 廣播給每個訂閱者，且 `Welcome.login_required` 是目前的狀態（假的狀態來源）

每位使用者一個播放器（`tidal-player`）：

- **AC9** — `runtime_dir(env, uid)`（純函式）依「傳輸」選出目錄（表格：每個變數已設定、為空或未設定）；`prepare_runtime_dir` 以 `0700` 建立它，並拒絕是檔案、屬於另一個 uid（假的中繼資料）或群組／所有人可存取（暫存目錄）的路徑
- **AC10** — 鎖：當鎖被另一個行程持有時，`daemon` 與 `play` 以 3 結束並顯示 `Another player is running (pid N)`（`play` 另帶它的提示）；鎖空閒時遺留的 `player.sock` 會被移除，新的播放器開始監聽；同時啟動的兩個常駐程式恰好留下一個在執行，另一個以 3 結束（整合測試，`crates/app/tests/daemon.rs`，暫存的 `TIDAL_PLAYER_RUNTIME_DIR`，wiremock API，沒有音訊裝置）
- **AC11** — 自動連上：有播放器在執行時，`tidal-player` 會作為客戶端連上，而不是啟動第二個播放器（不取得鎖，不建立引擎）；有項目時先送出 `Open`（`--add-to-queue` → `End`，`--play-next` → `Next`，一般 → `None`）；播放器啟動中（鎖被持有、socket 尚未綁定）時，客戶端重試 2 秒，然後以 `A player is running (pid N) but not answering` 失敗

客戶端：

- **AC12** — TUI 客戶端模型（`tidal_player_core::ui`，純邏輯）：`Welcome` 取代快照與登入狀態；`Disconnected { shut_down }` 保留最後的快照並設定對應的訊息；中斷連線期間沒有任何按鍵會發出 `Send`（表格，涵蓋每個指令按鍵），游標按鍵與 `q` 仍有作用；客戶端模式下的 `q` 發出 `Quit`，絕不發出 `Send(Shutdown)`；版本不符會設定它的訊息並停止重新連線
- **AC13** — 客戶端連線（假的連接器）：連線中斷後，客戶端每秒再試一次，成功時訂閱，下一個 `Welcome` 取代狀態；客戶端絕不顯示播放器沒有送出的佇列編輯（用 `o` 加入的項目只在播放器的快照中有它之後才出現：tidalt #6）
- **AC14** — 客戶端行程不開啟工作階段儲存庫、金鑰圈、通關密語來源或音訊裝置：在 `TIDAL_PLAYER_STATE_DIR` 指向空目錄且 `TIDAL_PLAYER_NO_KEYRING=1` 時，`tidal-player playback status` 與 TUI 客戶端的連上都會成功（否則它們會說 `Not logged in`）；客戶端模式的啟動函式不接受儲存庫與引擎（整合測試，加上由結構保證）
- **AC15** — `playback`（純解析，加上對測試播放器的整合測試）：「一次性指令」表格的每一列都對應到它的訊息（表格，含 `seek 90`、`seek +5`、`seek -2.5`、`volume 80`、`volume +5`、`volume -100`）；`volume 101`、`seek x`、不良項目 → 以 2 結束且不送出任何東西；沒有播放器 → 以 1 結束並顯示訊息；`Err` 回覆 → 它的訊息輸出到 stderr，以 1 結束；5 秒內沒有回覆 → 以 1 結束；`status` 印出三行（純格式化函式，表格：播放中、暫停且已釋放、沒有播放、工作階段已過期），`--json` 印出一行 JSON
- **AC16** — `daemon stop` 送出 `Shutdown`，等到鎖被釋放並以 0 結束；沒有播放器 → 以 1 結束

常駐程式（`tidal-player`）：

- **AC17** — 有工作階段的 `daemon`：取得鎖、綁定 socket，在設定了 `$NOTIFY_SOCKET` 時（測試用的 datagram socket）只有在 socket 接受連線之後才送出 `READY=1`；收到 `SIGTERM`（以及客戶端的 `Shutdown`）時，每個訂閱者都收到 `ShuttingDown`，引擎停止，socket 檔案被移除，以 0 結束。0002 AC12 的結束碼仍然成立（沒有工作階段、沒有通關密語 → 1）
- **AC18** — `daemon unit` 印出「systemd 使用者服務」之下的單元，其中 `ExecStart` 是給定的執行檔路徑（路徑的純函式，以 `insta` 快照由人工審閱，加上對 `Type=notify`、`RestartPreventExitStatus=1 2 3`、`StartLimitBurst=5` 與絕對路徑的 `contains` 檢查）

引擎與播放器（使用 0003 假實作的 `tidal-player-audio`；`tidal_player_core::player`）：

- **AC19** — 暫停時釋放：經過釋放延遲之後（假的時鐘；`0` 立即釋放，`never` 永不釋放），引擎關閉 sink 一次、釋放保留一次，並發出 `Released`；`Resume` 再次保留並開啟，且 sink 從恰好第一個尚未聽到的影格開始接收影格（會回報延遲的假 sink：沒有影格遺失或重複，16 位元與 24 位元的測試資料，與未暫停的執行逐位元組比較）；釋放期間的跳轉會在目標處恢復；釋放期間的 `Play`、`Stop`、`SetDevice` 不會把任何東西開啟或關閉兩次
- **AC20** — `RequestRelease`：暫停期間（不論是否已釋放、任何延遲設定）在 400 ms 內回答 `true`（PCM 在回答之前關閉，名稱在回答之後釋放），載入、播放或緩衝期間回答 `false`（表格，涵蓋各引擎狀態，假的保留）
- **AC21** — 恢復失敗：恢復播放時裝置忙碌或不存在，會得到 `ResumeFailed(error)`，沒有任何東西維持開啟，之後的 `Resume` 會再試一次；在播放器中，`ResumeFailed` 在相同位置維持 `Paused` 並顯示 0003 的訊息，不發出 `Resolve` 也不跳過，且 `TogglePause` 會再次發出 `EngineResume`（表格，涵蓋 `Busy`、`NotFound`、`Lost`）
- **AC22** — `Released`/`Resumed` 在下一個快照中設定與清除 `NowPlaying.released`；它被設定時，TUI 的第三列以 ` · device released` 結尾（算繪快照，80×24）
- **AC23** — 設定：`resolve_player_config` 中的 `TIDAL_PLAYER_RELEASE_PAUSED`（0004 AC25 的表格，加以擴充：未設定 → 10 秒、`0`、`3600`、`never`、`3601`、`-1`、`x`，空值 = 未設定）

文件：

- **AC24** — `docs/daemon.md` 記載角色表、自動連上、systemd 設定（以及為什麼以 1/2/3 結束時不重新啟動）、`playback` 與 `daemon stop`、暫停時釋放裝置及其設定，以及疑難排解的訊息（另一個播放器正在執行、版本不符、沒有回應、執行期目錄不是私有的）；`docs/playback.md`、`docs/tui.md` 與 `docs/login.md` 去掉它們的「until 0005」註記並連結到它；`CLAUDE.md` 的「Status」指名這份規格；這份規格連結到以上所有文件

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 一個 TUI 開著（獨立模式），而 `systemctl --user start tidal-player` 執行 | 常駐程式以 3 結束；單元不會重新啟動它（tidalt #8）。TUI 結束後常駐程式即可啟動 |
| 常駐程式在執行，使用者啟動 `tidal-player` | TUI 客戶端；結束它會讓音樂繼續播放 |
| 獨立模式的 TUI 在其他 TUI 客戶端連上時結束 | 它們顯示 `The player shut down: waiting for it to come back…`，並連上下一個啟動的播放器 |
| 有客戶端連上時執行 `systemctl --user restart tidal-player` | `ShuttingDown`，然後客戶端重新連線到新的常駐程式（自 0009 起，佇列會還原並維持停止） |
| 播放器當機（panic、`SIGKILL`） | 客戶端看到連線中斷並重新連線；鎖隨行程消失；下一個播放器移除過時的 socket |
| 舊的常駐程式還在執行時升級執行檔 | 新的客戶端以版本訊息拒絕它；遇到未知的訊息時不會有任何東西當機 |
| 瀏覽器處理程式連續快速地執行兩次 `tidal-player --add-to-queue …` | 兩個 `Open` 依收到的順序套用（AC7） |
| 客戶端被 Ctrl-Z 暫停了很久 | 在 1024 則訊息之後被中斷連線；恢復時帶著新的 `Welcome` 重新連線（AC6、AC13） |
| 未設定 `$XDG_RUNTIME_DIR`（沒有 logind，例如某些 SSH 工作階段） | `/tmp/tidal-player-<uid>`，私有；systemd 下的常駐程式與沒有這個變數的 shell 於是使用不同的目錄，因此 `docs/daemon.md` 說明要在兩者中都設定 `XDG_RUNTIME_DIR`（或 `TIDAL_PLAYER_RUNTIME_DIR`） |
| 執行期目錄屬於別人，或所有人可讀 | 被拒絕，以 1 結束（AC9）；絕不默默地 chmod |
| 暫停超過釋放延遲，然後另一個應用程式在 DAC 上播放 | 它取得音效卡；恢復播放時顯示 `Output hw:1,0 is busy (used by …)` 並維持暫停，直到音效卡空閒（AC21） |
| 暫停中，WirePlumber 要求音效卡 | 立即釋放（AC20） |
| 播放中，另一個應用程式要求音效卡 | 被拒絕，如 0003 |
| 串流 URL 在長時間暫停期間過期 | 0003 在恢復播放時，對 `403`/`410` 重新解析 |
| 常駐程式執行期間工作階段過期 | 對每個客戶端送出 `LoginRequired`（`playback status` 會這麼說）；`tidal-player login` 之後送出 `LoginRestored`（AC8、0002 AC9） |
| 播放器啟動中時執行 `playback …` | 重試連線 2 秒（AC11） |
| `play`（前景）在執行，然後執行 `playback next` | 可行：`play` 也提供 socket |

<a id="test-plan"></a>
## 測試計畫

每個自動化測試都以它的條件命名（`ac5_…`）。紅燈（red）是針對存根型別與函式本體為存根的函式（沒有 `todo!()`，沒有編譯錯誤）的斷言失敗，如 0001–0004。伺服端測試透過 `UnixStream::pair()` 或暫存目錄中的 socket 執行真正的伺服端程式碼，使用 0004 的假引擎與工作；沒有任何測試開啟音訊裝置或接觸網路（API 呼叫透過 `TIDAL_PLAYER_API_BASE` 送到 `wiremock`，如 0004 的 CLI 測試）。時間（釋放延遲、重新連線間隔、請求逾時）是注入的；沒有任何測試睡得比 2 秒的連線重試更久，而且只有 AC10/AC11 的整合測試會這樣。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (extended) | 每個新變體的往返 | 新型別以手寫的存根 `Serialize` 寫出 `null` |
| AC2 | `crates/app/src/ipc/codec.rs` :: `ac2_framing` (table) | 每列的訊息或錯誤；超大行的記憶體上限 | 存根解碼器對每個輸入都不回傳任何東西 |
| AC3 | `crates/app/src/ipc/codec.rs` :: `ac3_greeting` (table) + `crates/app/src/ipc/server/tests.rs` :: `ac3_server_greets_first` | 確切的問候文字；接受／版本不符／不是我們的 | 存根接受每個問候 |
| AC4 | `crates/app/src/ipc/server/tests.rs` :: `ac4_subscribe_ordering` (table), `ac4_clients_converge` (200 seeded runs) | `Welcome` 加事件之後的客戶端狀態等於執行期的快照；序列相同；閒置時每個客戶端都等於播放器 | 存根從執行期執行緒之外取得的快照送出 `Welcome`，因此中間的事件遺失或重複 |
| AC5 | `crates/app/src/ipc/server/tests.rs` :: `ac5_requests_replied_in_order`, `ac5_nothing_dropped` | 每個 id 一個回覆，在它的事件之後；計數 | 存根在套用之前就回覆，並在輸入通道忙碌時丟棄 |
| AC6 | `crates/app/src/ipc/server/tests.rs` :: `ac6_stalled_client_disconnected`, `ac6_bad_clients_dropped` (table) | 在 1024 時中斷連線；其他客戶端不受影響；回覆延遲 | 存根同步地寫入每個客戶端，因此停滯的那個會阻塞播放器 |
| AC7 | `crates/app/src/player/runtime.rs` :: `ac7_open` (table), `ac7_open_errors_and_order` | 套用的指令、開始規則、錯誤回覆、順序 | 存根在每個 `Open` 展開完成時就套用它 |
| AC8 | `crates/app/src/ipc/server/tests.rs` :: `ac8_login_status` | 事件與 `Welcome.login_required` | 存根永遠送出 `login_required: false` 且不送事件 |
| AC9 | `crates/app/src/ipc/paths.rs` :: `ac9_runtime_dir` (table), `ac9_private_dir` | 選出的路徑；建立時的權限；拒絕 | 存根對每個輸入都回傳 `/tmp/tidal-player` 且從不檢查 |
| AC10 | `crates/app/tests/daemon.rs` :: `ac10_second_player_exits_3`, `ac10_stale_socket_removed`, `ac10_race_one_wins` | 結束碼、訊息、之後客戶端可以連線 | 存根常駐程式從不取得鎖，因此第二個也會啟動 |
| AC11 | `crates/app/tests/daemon.rs` :: `ac11_tui_attaches` + `crates/app/src/client.rs` :: `ac11_startup_open`, `ac11_connect_retry` | 不取得鎖也不建立引擎就連上；送出的 `Open`；重試然後顯示訊息 | 存根不論如何都啟動獨立模式播放器 |
| AC12 | `crates/core/src/ui/mod.rs` :: `ac12_client_connection_states` (table) | 狀態、訊息、中斷連線期間沒有 `Send`、`q` | 存根忽略 `Welcome`/`Disconnected` |
| AC13 | `crates/app/src/client.rs` :: `ac13_reconnect`, `ac13_no_local_edits` | 重新連線的節奏、重新訂閱、狀態被取代；在快照之前佇列不變 | 存根在第一次失敗後就放棄 |
| AC14 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` | 狀態目錄為空時，以 0 結束並有輸出 | 存根客戶端先載入工作階段並說 `Not logged in` |
| AC15 | `crates/app/src/cli/oneshot.rs` :: `ac15_parse` (table), `ac15_status_lines` (table), `ac15_execute` (fake link: `Ok`/`Err` replies, the 5 s timeout injected, `status`/`--json`) + `crates/app/tests/daemon.rs` :: `ac15_exit_codes` | 每列的訊息；結束碼；印出的行 | 存根對每個指令都送出 `TogglePause` |
| AC16 | `crates/app/tests/daemon.rs` :: `ac16_daemon_stop` | 以 0 結束且之後鎖已釋放；沒有播放器時為 1 | 存根不送出任何東西就以 0 結束 |
| AC17 | `crates/app/tests/daemon.rs` :: `ac17_ready_and_sigterm` | socket 接受連線之後收到 `READY=1`；客戶端看到 `ShuttingDown`；socket 消失；以 0 結束 | 存根從不通知，並留下 socket 檔案 |
| AC18 | `crates/app/src/cli/daemon.rs` :: `ac18_unit` | 快照加上 `contains` 檢查 | 存根印出空的單元 |
| AC19 | `crates/audio/tests/engine.rs` :: `ac19_release_while_paused` (table: 16/24-bit, delay 0/10 s/never, seek/play/stop/device while released) | 關閉與釋放的次數；逐位元組比對的影格；位置 | 存根從不釋放（計數為 0） |
| AC20 | `crates/audio/tests/engine.rs` :: `ac20_request_release` (table over states) | 回答、關閉／回答／釋放的順序、延遲 | 存根永遠拒絕（0003 的行為） |
| AC21 | `crates/audio/tests/engine.rs` :: `ac21_resume_failure` + `crates/core/src/player/tests.rs` :: `ac21_resume_failed_stays_paused` (table) + `crates/app/src/player/runtime.rs` :: `ac21_runtime_maps_release_events` (the runtime's mapping of the engine's `Released`/`Resumed`/`ResumeFailed`) | 事件、沒有任何東西開啟、重試；播放器狀態、訊息、不跳過 | 存根播放器把 `ResumeFailed` 當作曲目失敗並跳過 |
| AC22 | `crates/core/src/player/tests.rs` :: `ac22_released_flag` + `crates/app/src/tui/render.rs` :: `ac22_released_row` | 快照旗標；緩衝區中的 ` · device released`，快照由人工審閱 | 存根從不設定旗標 |
| AC23 | `crates/app/src/settings.rs` :: `ac25_player_config` (extended rows) | 值與錯誤 | 存根忽略該變數 |
| AC24 | —（驗收時審閱） | 文件存在、與本規格相符、已連結 | — |

沒有以自動化測試涵蓋，這是刻意的，並在驗收時於使用者的機器上手動檢查（結果寫在 PR 說明中）：

- 在 systemd 下產生的單元：`enable --now` 啟動它，`systemctl --user status` 顯示它為 active（已通知就緒），`stop` 乾淨地結束；在獨立模式 TUI 開著時，`start` 以狀態 3 失敗一次且不會重新啟動
- 在 `hw:` DAC 上暫停，等待超過釋放延遲：另一個應用程式（`speaker-test -D hw:1,0`，或透過 PipeWire 的瀏覽器）可以使用音效卡；結束它，恢復播放：曲目從原處繼續，聽不到跳過或重複
- 暫停，然後某個 PipeWire 客戶端在該音效卡上啟動：WirePlumber 會送出 `RequestRelease` 嗎（它會立刻得到音效卡嗎）？記錄發生了什麼；下方的假設取決於此
- 原本預期 `playerctl` 在這裡不能用；自從 MPRIS（[0010](0010-mpris.md)，implemented）之後它可以了

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::protocol`：`ClientMessage`、`ServerMessage`、`Command::Open`、`NowPlaying.released`。`player`：`ResumeFailed`、`Released`/`Resumed` 的處理。`ui`：客戶端連線狀態。沒有傳輸，沒有 `serde_json`（0001 AC3）
- `tidal-player-audio`：釋放延遲、釋放／重新取得、`RequestRelease` 的回答（透過既有的 `Reserver` 接縫（seam））、新的事件；仍然沒有 tokio
- `tidal-player`：`ipc::{codec, paths, server}`（std 的 `UnixListener`/`UnixStream`，每個客戶端一個讀取執行緒與一個寫入執行緒，餵給執行期的輸入通道、發送佇列）、`client.rs`（連線、重試、重新連線、TUI 客戶端迴圈）、`oneshot.rs`（`playback`）、`daemon.rs`（啟動、訊號、`sd_notify`、單元文字）、`reserve.rs`（匯出的物件改為詢問引擎，而不是拒絕）。沒有新的第三方相依套件：`File::try_lock` 自 1.89 起就在 std 中，`UnixDatagram` 涵蓋 `sd_notify`，tokio 的 `signal` 功能已經啟用
- `xtask layering`：沒有變動

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-07）：

- 「背景」下 tidalt 的設計與錯誤，取自它的原始碼、歷史（`git log`，指名的 commit）與已關閉的 issue #3–#8 及 #19–#21
- 0004 的執行期以單一個 `mpsc` 通道接收指令，並只有一個事件接收者，而 TUI 透過 `Expander` 自己展開項目（`crates/app/src/player/runtime.rs`、`crates/app/src/main.rs`）：因此需要扇出與 `Open`
- `std::fs::File::try_lock` 與 `SocketAddr::from_abstract_name`（用於抽象的 `$NOTIFY_SOCKET`）在固定的工具鏈（rustc 1.97.1）上能編譯，行為也如預期（對同一個檔案的第二次 `try_lock` 會失敗）
- `tidal_player_core::Item` 已經衍生 `Serialize`/`Deserialize`
- `reserve.rs` 匯出的物件目前會拒絕每一個 `RequestRelease`

在實作期間或驗收時要檢查的假設：

- 0003 的假 sink 能回報裝置延遲，因此 AC19 的「第一個尚未聽到的影格」是可測試的；若不能，該切片會把它加到假實作中
- 當 PipeWire 客戶端想要被保留的音效卡時，WirePlumber 會對持有者送出 `RequestRelease`（上方的手動檢查）。若不會，其他應用程式只有在釋放延遲之後才能取得音效卡，這正是需要這個延遲的原因
- 0003 的保留流程加上重新開啟，使釋放後的恢復播放在使用者的 DAC 上遠低於一秒（手動檢查）
- 瀏覽器的 `tidal://` 處理程式不屬於本規格；`docs/daemon.md` 可以展示 `tidal-player playback add` 作為其中一種

<a id="decisions-answered-by-the-user-2026-10-07-all-as-proposed"></a>
## 決策（使用者已回覆，2026-10-07：全部照提案）

1. **傳輸**：採用 JSON 行的 Unix socket（*accepted*），而不是 tidalt 那樣的私有 D-Bus 介面。`Command`/`Event` 型別本來就能序列化成 JSON；socket 不需要工作階段匯流排（透過 SSH 連上的無畫面機器可能沒有），免費帶來推送事件與連線生命週期（連上／中斷），並且可以在行程內測試。MPRIS（[0010](0010-mpris.md)，implemented）是 D-Bus，並維持獨立：它是建立在播放器之上的公開轉接器，而不是客戶端的通道
2. **沒有常駐程式在執行時，誰是播放器**：第一個 `tidal-player`（獨立模式，如 0004），它也提供 socket（*accepted*）。替代方案：`tidal-player` 永遠啟動一個背景常駐程式，自己只當客戶端，因此結束 TUI 絕不會停止音樂。提案保留 0001 的三種模式與 spotify-player 的行為（結束就停止）
3. **在播放器中展開項目**（`Command::Open`，*accepted*）：客戶端不需要工作階段、通關密語或 API 存取，一次性指令也只是一則訊息。替代方案：由客戶端展開並送出曲目（每個客戶端都需要工作階段，而 0006 的瀏覽反正也會需要）。目前先照提案；0006 決定它的瀏覽頁面如何取得資料
4. **釋放延遲**：預設 10 秒，`0` 是 tidalt 的立即釋放，`never`（*accepted*）。不論設定為何，暫停期間都會接受 `RequestRelease`
5. **systemd 單元**：由 `tidal-player daemon unit` 印出，由使用者安裝（*accepted*），而不是 tidalt 的 `setup --daemon` 自己寫入並執行 `systemctl`
6. **一次性指令**：「一次性指令」下的表格（*accepted*）：只有切換型指令，如 0004 的協定所有的；明確的 `play`/`pause`/`set` 指令隨 MPRIS 的設定器一起出現（[0010](0010-mpris.md)，implemented：`play`、`pause`、`stop`、`shuffle on|off`、`repeat off|queue|track`）

<a id="out-of-scope"></a>
## 不在範圍內

- MPRIS2、媒體鍵、`playerctl`（[0010](0010-mpris.md)，implemented）
- 在常駐程式重新啟動之間記住佇列、位置與音量（已在 0009 完成）
- socket 啟動、系統層級（多使用者）的常駐程式、透過 TCP 的遠端客戶端
- `tidal://` URL 處理程式與 `.desktop` 檔案（tidalt 的 `setup`）
- 在客戶端中瀏覽（音樂庫、搜尋）：0006/0007 決定它是透過播放器取得，還是以自己的工作階段取得
- 明確的 play/pause/set 一次性指令（決策 6）
- 從客戶端選擇輸出裝置（0004「Out of scope」；`SetDevice` 尚未在協定中）
- 播放器消失時，由客戶端接手成為播放器
