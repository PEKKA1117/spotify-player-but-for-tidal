[English](../0025-player-event-hook.md) | 正體中文

<a id="0025--player-event-hook"></a>
# 0025 — 播放器事件 hook

- **狀態**：implemented（2026-11-05）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#56](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/56)
- **相依於**：0004（implemented：播放器的狀態與事件）、0005（implemented：常駐程式）、0008（implemented：`app.toml`）、0014（implemented：輸出裝置）
- **使用者文件**：[`docs/config.md`](../../config.md)（新增的「Running a command on player events」一節）與它的 zh-TW 版本、`examples/app.toml`、`README.md` 與它的 zh-TW 版本（AC15）

<a id="context"></a>
## 背景

狀態列（waybar、i3blocks）、scrobble 腳本與記錄都想知道播放器做了什麼。MPRIS（0010）涵蓋了一部分，但腳本得一直連著匯流排，而且無法分辨一首曲目是播到結尾還是被跳過。spotify-player 有 `player_event_hook_command`：在每個播放器事件時執行的指令。本規格加入同名設定與相同的引數，另外多了幾個事件與環境變數。

tidalt 沒有這些。

<a id="behaviour"></a>
## 行為

<a id="the-setting"></a>
### 設定

```toml
player_event_hook_command = { command = "/home/me/bin/on-player-event", args = ["--quiet"] }
```

`command` 是路徑，或在 `PATH` 中找得到的名稱，直接執行，不經過 shell：不會展開 `~`。`args` 為選用，排在事件的引數之前。這個設定只存在於 `app.toml`：帶引數的指令沒有合理的環境變數形式。沒有設定時什麼都不會執行，也不會為它啟動執行緒。

不論是哪一種播放器，都由播放器執行 hook：獨立模式、常駐程式或 `play`。連上常駐程式的 TUI 不執行任何東西，所以不論連了多少個客戶端，一個事件只會執行一次指令。

<a id="events"></a>
### 事件

事件的引數接在 `args` 之後，所以以上面的例子來說，換曲目會執行 `on-player-event --quiet Changed 12345`。

| 事件 | 引數 | 時機 |
|---|---|---|
| `Changed` | `TRACK_ID` | 一首曲目開始：它的第一個影格已寫入，或播放無縫地移入它。包括同一首曲目的重複播放、停止後再播放一首曲目，以及從儲存的位置開始播放還原的曲目 |
| `Playing` | `TRACK_ID POSITION_MS` | 播放開始或繼續：緊接在 `Changed` 之後，或離開暫停時 |
| `Paused` | `TRACK_ID POSITION_MS` | 一首播放中的曲目被暫停 |
| `Stopped` | `TRACK_ID POSITION_MS` | 播放器停止，且自上一次 `Stopped` 以來有曲目開始過：`Stop`、佇列播完或被清空、找不到可播放內容的跳過，或播放器正在關閉。`TRACK_ID` 是最後開始的那首曲目 |
| `EndOfTrack` | `TRACK_ID` | 曲目播到結尾，無縫與否皆然。跳過或停止不算結尾 |
| `Seeked` | `TRACK_ID POSITION_MS` | 播放中或暫停中的曲目，其位置被手動改變 |
| `VolumeChanged` | `PERCENT` | 聽到的音量改變：`0` 到 `100`，靜音時為 `0` |
| `DeviceChanged` | `NAME` | 選取的輸出裝置改變（0014），由客戶端或由後備機制造成 |

`POSITION_MS` 是事件之後的位置，所以 `Changed` 之後的 `Playing` 不一定是 `0`。`Buffering` 在這裡不是狀態變化：停頓與其恢復都不執行任何東西。曲目只是在載入時也不執行任何東西，所以在佇列中快速跳過時，只有真正開始的那首曲目會執行 `Changed`。

一個輸入可以造成好幾個事件，它們依以下順序執行：舊曲目的 `EndOfTrack`、`Changed`、`Playing`、`Paused`、`Stopped`、`Seeked`、`VolumeChanged`、`DeviceChanged`。一首播到結尾、後面還有另一首的曲目，會依序給出 `EndOfTrack OLD`、`Changed NEW` 與 `Playing NEW 0`；佇列中的最後一首則給出 `EndOfTrack LAST`，接著是 `Stopped LAST POSITION_MS`。

啟動播放器不是事件：還原的工作階段（0009）在有東西開始播放之前不執行任何東西。

<a id="the-environment"></a>
### 環境變數

指令繼承播放器的環境。與曲目有關的事件（除了 `VolumeChanged` 與 `DeviceChanged`）會另外加上：

| 變數 | 值 |
|---|---|
| `TIDAL_PLAYER_TRACK_TITLE` | 標題；若有版本，則把版本放在括號中（`Song (Live)`） |
| `TIDAL_PLAYER_TRACK_ARTISTS` | 藝人，以 `, ` 連接 |
| `TIDAL_PLAYER_TRACK_ALBUM` | 專輯的標題；曲目沒有專輯時不設定 |
| `TIDAL_PLAYER_TRACK_DURATION_MS` | 長度；中繼資料沒有時不設定 |

所以腳本不需要 Tidal 工作階段，就能顯示或 scrobble 正在播放的內容。`play` 播放單一曲目 ID 時，通常會略過中繼資料請求（0004）；設定了 hook 時它會發出請求，所以這裡的變數也帶有該曲目的內容。

<a id="how-it-runs"></a>
### 如何執行

同一個播放器的指令一次執行一個，依事件的順序，在它們自己的執行緒上執行：播放絕不等待它們。

- **等待中的事件**：最多 64 個。滿了時丟掉最舊的。排在最後的 `Seeked` 或 `VolumeChanged` 會被同種類的較新事件取代，所以按住按鍵只花一次執行，而不是上百次，而且事件的順序不會改變
- **時間限制**：指令執行超過 30 秒就會被終止，然後執行下一個
- **輸入與輸出**：stdin 與 stdout 是 `/dev/null`。stderr 在常駐程式與 `play` 下是播放器的 stderr（在 systemd 下就是 journal），在獨立模式下則是 `/dev/null`，因為它會畫在 TUI 上

<a id="failures"></a>
### 失敗

指令無法啟動、以非零狀態結束，或因超過時間限制被終止，都不影響播放。播放器會說一次，作為每個客戶端都會顯示的播放器訊息（和任何訊息一樣，保留到下一首曲目開始），在常駐程式與 `play` 下也會寫到 stderr：

```
Player event hook failed: on-player-event: No such file or directory (os error 2)
Player event hook failed: on-player-event exited with status 3
Player event hook failed: on-player-event was killed after 30 s
Player event hook is too slow: events were dropped
```

之後的失敗在有指令成功之前都不再說話，所以壞掉的 hook 不會在每首曲目都蓋掉訊息列。

<a id="shutting-down"></a>
### 關閉

播放器在有曲目播放中或暫停中時關閉，會送出 `Stopped`，讓狀態列不會一直顯示那首曲目。hook 最多有 2 秒完成正在等待的事件；剩下的丟掉，仍在執行的指令則任它自己跑完。`play` 在佇列播完時給它同樣的時間，否則最後的事件會隨行程一起消失。

<a id="acceptance-criteria"></a>
## 驗收條件

1. **AC1 設定**：`app.toml` 中的 `player_event_hook_command` 接受一個表格，內有 `command`（非空字串）與選用的 `args`（字串陣列）。其他任何形式（字串、空的 `command`、缺少 `command`、未知欄位、不是字串陣列的 `args`）都是 `app.toml` 錯誤，結束碼為 2，措辭依 0008。沒有設定時不執行 hook，也不啟動執行緒
2. **AC2 曲目事件**：根據一個輸入前後的狀態，開始一首曲目（包括重複播放與停止後再播放）時執行 `Changed` 與 `Playing`，離開暫停時執行 `Playing`，暫停播放中的曲目時執行 `Paused`，只有播到結尾的曲目才執行 `EndOfTrack`，順序依「Events」
3. **AC3 Stopped**：播放器在有曲目開始後停止時，不論是 `Stop`、空佇列、被清空的佇列，或找不到可播放內容的跳過，`Stopped` 都只執行一次，並指名最後開始的那首曲目，即使它已離開佇列；沒有曲目開始過就停止的播放器不執行任何東西
4. **AC4 Seeked**：對播放中或暫停中的曲目跳轉，以及 `Previous` 讓它從頭重播時，`Seeked` 帶著新位置執行；停止中或載入中的跳轉、跳到相同位置，以及移到另一首曲目的 `Previous` 則不執行
5. **AC5 音量**：聽到的音量改變時執行 `VolumeChanged`，靜音時為 `0`；改變後仍相同的變動則不執行
6. **AC6 裝置**：`device` 改變時執行 `DeviceChanged`，而把播放中的曲目移到另一個裝置不執行 `Changed`、`Playing` 或 `Seeked`
7. **AC7 靜默**：載入、緩衝、從緩衝恢復、暫停時釋放輸出、暫停仍在載入的曲目、佇列編輯與還原的工作階段，都不執行任何東西；快速跳過一些從未開始的曲目，也不為它們執行任何東西。在這樣的暫停下開始的曲目只執行 `Changed`，繼續時才執行 `Playing`
8. **AC8 引數與環境變數**：指令得到 `args`，然後是事件的名稱與引數；與曲目有關的事件依「The environment」設定那四個變數，缺少專輯或長度時對應的變數不設定，而 `VolumeChanged` 與 `DeviceChanged` 不設定其中任何一個
9. **AC9 佇列**：事件一次執行一個，依順序；第 65 個等待中的事件丟掉最舊的；排在最後的 `Seeked` 或 `VolumeChanged` 被同種類較新的事件取代，後面還有其他事件的則不被取代
10. **AC10 真實序列**：透過 `PlayerRuntime` 與假的引擎（fake engine）驅動，無縫轉換、非無縫的結尾、佇列的結尾與跳過，給出「Events」所說的序列，而且跳過不給出 `EndOfTrack`
11. **AC11 失敗**：無法啟動的指令、非零結束碼與逾時，各自只給出一次播放器訊息；之後的失敗在成功之前不說話；成功會重新啟用它；整個過程中播放不受影響
12. **AC12 絕不阻塞**：`PlayerRuntime::handle` 在 hook 指令仍在執行時就會回傳
13. **AC13 每一種播放器**：獨立模式、常駐程式與 `play` 都執行 hook，連上的 TUI 不執行。佇列播完的 `play` 會像關閉時一樣等待 hook，而設定了 hook 的 `play` 播放單一曲目 ID 時會請求該曲目的中繼資料
14. **AC14 關閉**：在有曲目播放中或暫停中時關閉會送出 `Stopped`，hook 最多有 2 秒處理等待中的事件，而且指令卡住時播放器仍在這段時間內結束
15. **AC15 文件**：`docs/config.md`、`README.md` 與它們的 zh-TW 版本描述這個設定、事件、變數與失敗；`examples/app.toml` 以註解的形式示範這個設定
16. **AC16 真實行程**：真實的腳本收到引數與環境變數，不從 stdin 讀取任何東西，而執行超過限制的腳本會被終止

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| `command` 缺少或不可執行 | 第一個事件給出失敗訊息；播放繼續 |
| 卡住的指令 | 30 秒時終止；這段期間事件等待，超過 64 個就丟掉最舊的 |
| 啟動背景行程的腳本 | 那是它自己的事：播放器只等待指令本身 |
| 佇列中連續兩次同一首曲目 | `EndOfTrack`，然後第二首的 `Changed`：它們是兩個項目 |
| 啟動失敗的曲目 | 沒有 `Changed`：它從未播放。它造成的跳過也不執行任何東西 |
| 從佇列移除正在播放的曲目 | 由播放器的行為（0004）決定：停止時給出帶著被移除曲目的 `Stopped` |
| 曲目失敗的跳過，且沒有其他可播放內容 | 帶著最後開始的那首曲目的 `Stopped`：狀態列不可繼續顯示它 |
| 暫停時釋放輸出，然後繼續 | 只有 `Paused` 與 `Playing`：`released` 不是事件 |
| 跳轉超過結尾 | 播放器的行為（0004）：讓曲目結束的跳轉，在引擎回報結尾時給出 `EndOfTrack` |
| 靜音，然後取消靜音 | `VolumeChanged 0`，然後 `VolumeChanged PERCENT` |
| `Previous` 讓曲目從頭重播 | 一次跳到開頭的跳轉：`Seeked TRACK_ID 0` |
| 執行中修改了 `app.toml` | 只在啟動時讀取一次，與其他所有設定相同 |

<a id="test-plan"></a>
## 測試計畫

| AC | 測試 | 驗證內容 | 預期的紅燈失敗 |
|---|---|---|---|
| AC1 | `crates/app/src/config.rs` :: `ac1_player_event_hook_command`（表格） | 解析出的值與每個錯誤的措辭 | 未知的鍵 `player_event_hook_command` |
| AC2 | `crates/core/src/hook/tests.rs` :: `ac2_track_events`（表格） | 之前／之後／已開始／已結束 → 事件，依順序 | 沒有推導出任何事件 |
| AC3 | `crates/core/src/hook/tests.rs` :: `ac3_stopped`（表格） | 每一種停止方式；已不在佇列中的曲目 | 沒有推導出任何事件 |
| AC4 | `crates/core/src/hook/tests.rs` :: `ac4_seeked`（表格） | 播放中、暫停中、停止中、載入中、相同位置、`Previous` 的兩種情況 | 沒有推導出任何事件 |
| AC5 | `crates/core/src/hook/tests.rs` :: `ac5_volume`（表格） | 音量、靜音、沒有變化 | 沒有推導出任何事件 |
| AC6 | `crates/core/src/hook/tests.rs` :: `ac6_device` + `crates/app/src/player/runtime.rs` :: `ac6_device_move_runs_only_device_changed` | 該事件；透過 runtime 移動播放中的曲目 | 沒有推導出任何事件；沒有記錄到任何東西 |
| AC7 | `crates/core/src/hook/tests.rs` :: `ac7_silence`（表格） | 每一種情況都沒有事件 | 對每個變化都推導事件的存根會讓它失敗 |
| AC8 | `crates/core/src/hook/tests.rs` :: `ac8_arguments_and_environment`（表格） | 每個事件的 argv 與變數；缺少專輯與長度 | 空的 argv |
| AC9 | `crates/core/src/hook/tests.rs` :: `ac9_queue`（表格） + `crates/app/src/player/hook.rs` :: `ac9_one_at_a_time` | 順序、上限、取代最後一個；絕不同時執行兩個指令 | 佇列什麼都沒留下；沒有東西執行 |
| AC10 | `crates/app/src/player/runtime.rs` :: `ac10_real_sequences`（表格） | 四種序列記錄到的事件 | 沒有記錄到任何東西 |
| AC11 | `crates/app/src/player/hook.rs` :: `ac11_failures`（表格，fake runner） | 訊息與它們的重新啟用 | 沒有回報訊息 |
| AC12 | `crates/app/src/player/hook.rs` :: `ac12_never_blocks` | 一個被 barrier 卡住的 runner；`send` 立刻回傳 | `send` 在等待 |
| AC13 | `crates/app/src/config.rs` :: `ac13_hook_reaches_the_settings` + `crates/app/src/player/runtime.rs` :: `ac13_play_waits_for_the_hook` + `crates/app/src/cli/play.rs` :: `ac13_play_asks_for_the_hooks_metadata` + 親手檢查 | `app.toml` 的指令在三種播放器都由它啟動的設定中；佇列播完的 `play` 會完成 hook；只有設定了 hook 時才取得單一曲目 ID；上述檢查在連上 TUI 的情況下執行它，因為 `main` 啟動播放器而沒有接縫（seam） | 設定中沒有 hook；hook 從未完成；曲目沒有名稱 |
| AC14 | `crates/app/src/player/hook.rs` :: `ac14_shutting_down` + `crates/app/src/player/runtime.rs` :: `ac14_shutdown_sends_stopped` | 在 runner 卡住時的 2 秒限制；播放中與暫停中時的 `Stopped`，停止時則沒有 | 沒有排空；沒有記錄到任何東西 |
| AC15 | `crates/app/tests/examples.rs`（範例可解析） + 驗收時審閱 | 文件由人工閱讀：文字說明沒有自動化檢查 | 找不到註解掉的範例 |
| AC16 | `crates/app/tests/hook.rs` :: `ac16_real_process` | 一個 `/bin/sh` 腳本把它收到的內容寫入檔案；一個睡眠中的腳本在短的限制下被終止 | 沒有行程執行 |

驗收時對真實 API 親手檢查：以發行版建置、在它自己的 runtime 與狀態目錄下作為常駐程式執行，並用一個把內容附加到檔案的腳本。播放、暫停、跳轉、跳過、改變音量與裝置、讓一首曲目無縫地播完、停止常駐程式；檔案中有預期的行，每行一次，且連著一個 TUI。獨立模式的 TUI 改變音量，以及播放單一曲目 ID 直到結尾的 `play`，各自再加上它們的檢查：`play` 的行指名該曲目並以 `Stopped` 結束。然後是一個不存在的指令：TUI 的訊息列只顯示一次失敗。訊息列是 TUI 唯一增加的東西，所以 PR 的親手檢查（hands-on）一節就是這一個情境。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`，新的 `hook` 模組：`HookEvent`、`HookTracker`（從一個輸入前後的快照推導事件，並記住最後開始的曲目）、事件的 argv 與環境變數、等待佇列。全部是純邏輯
- `tidal-player`（`crates/app`），新的 `hook` 模組：工作執行緒、失敗回報，以及位於 `Runner` 接縫（seam）之後的行程
- `crates/app/src/player/runtime.rs`：`handle` 在設定了 hook 時也會在輸入之前取得快照，並把事件交給 hook。`Command`、`Event` 與 socket 協定不變
- 失敗以新的 `RuntimeInput::HookFailed(String)` 抵達播放器，並像儲存失敗那樣轉成 `PlayerInput::Notice`

<a id="facts-vs-assumptions"></a>
## 事實與假設

已在 `463e3e5` 的程式碼中驗證：

- `Handled.ended` 在曲目播到結尾時設定，在無縫的 `Transitioned` 時也設定，跳過時絕不設定；`Handled.started` 在任何曲目開始時設定，包括跳過所導向的那首
- `PlayerRuntime::handle` 已經會計算一個輸入是不是跳轉，並在持久化開啟時於輸入之前取得快照
- `PlayerSnapshot.position` 跟隨引擎的 `Position` 事件，而 `device` 是選取的裝置
- 整個 workspace 都沒有安裝 `tracing` subscriber，所以 `tracing::warn!` 不會寫出任何東西：issue 中的「logged」不可能是指 `tracing`
- 在 `Shutdown` 時 runtime 會略過 `track_change`，所以快照不會顯示停止：`Stopped` 需要它自己的處理
- `Command::Previous` 不在 `handle` 的跳轉檢查中，雖然它可以讓曲目從頭重播（一次 `seek_to(0)`）：hook 的檢查加入了它，而 `Seeked` 需要相同的 `current` 與改變的位置，所以移到另一首曲目的 `Previous` 或跳到相同位置的跳轉不執行 `Seeked`

假設，實作時確認；假設錯了就回到本規格：

- 裝置的移動經過 `OutputChanged`，不會給出 `Started`（AC6 測試它）

<a id="decisions"></a>
## 決策

1. **`Changed` 跟隨已開始的曲目，而不是 `current`。** 一被要求播放某首曲目，`current` 就移動，所以快速跳過會為沒有人聽到的曲目執行 hook
2. **用 runtime，而不是 hub 的 peer。** MPRIS 只看得到快照，無法分辨自然結束與跳過。runtime 知道，而且用它不必動協定
3. **一次一個。** 為每個事件各產生一個行程比較簡單，但事件可能亂序執行，卡住的腳本也會讓行程堆積。有上限的佇列保住順序；時間限制則避免一個卡住的指令讓之後所有的指令都無聲無息
4. **失敗使用播放器的訊息。** 沒有可寫入的記錄，而默默失敗的 hook 對正在設定它的人來說是最糟的情況
5. **獨立模式丟掉 stderr。** 寫入它會畫在 TUI 上

<a id="out-of-scope"></a>
## 不在範圍內

- 記錄檔或 `tracing` subscriber（這要由記錄頁 #64 來決定）
- 指令的 shell 形式、`~` 展開
- 隨機播放、重複播放、自動播放、佇列或登入的事件
- 透過旗標或環境變數設定 hook（`-o`、#65 會涵蓋）
- 讓 30 秒的限制或 64 個事件的上限可設定
- 終止指令留在背景的東西
