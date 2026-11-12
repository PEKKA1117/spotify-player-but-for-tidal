[English](../0014-device-selection.md) | 正體中文

<a id="0014--choosing-the-output-device-while-the-player-runs"></a>
# 0014 — 播放器執行時選擇輸出裝置

- **狀態**：implemented（2026-10-10；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0003（implemented：引擎的 `SetDevice`、`tidal-player devices`）、0004（implemented：播放器與它的 effect）、0005（implemented：協定、客戶端、`playback`、暫停時釋放）、0008（implemented：`output_device`、按鍵對應、按鍵說明）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Output device」；[`docs/daemon.md`](../../zh-TW/daemon.md) 新增 `playback device`；[`docs/playback.md`](../../zh-TW/playback.md) 與 [`docs/config.md`](../../zh-TW/config.md) 說明使用哪個裝置（AC12）

<a id="context"></a>
## 背景

目前輸出裝置只在播放器啟動時選擇一次：`--device`、`TIDAL_PLAYER_DEVICE` 或 `app.toml` 中的 `output_device`（0008），否則為 `default`。從 DAC 切換到內建音效卡，意味著要編輯 `app.toml`（或 systemd unit）並重新啟動播放器，這會中斷正在播放的內容。自 0003 起，引擎就支援把正在播放的曲目移到另一個裝置（`SetDevice`，AC21：沒有遺失或重複任何 frame），但沒有任何東西能到達它：0004、0005 與 0009 都各自把「裝置挑選器」延後到之後的規格，而 `SwitchDevice` 是按鍵對應略過的 spotify-player 指令之一（0008）。

spotify-player 有 `SwitchDevice`（預設按鍵 `D`），它會開啟一個列出 Spotify Connect 裝置的彈出視窗；`Enter` 把播放移到那裡。它的 CLI 有 `spotify_player connect --name/--id` 與 `spotify_player get key devices`。

tidalt 的 TUI 有裝置挑選器。在客戶端中選擇裝置會呼叫該客戶端自己的播放器，而它沒有開啟任何裝置，所以這個選擇不會有任何作用（0005 背景，錯誤 4）。這裡只有播放器能列出並切換裝置（0005「Sync」規則 1）。

<a id="behaviour"></a>
## 行為

<a id="the-players-device"></a>
### 播放器的裝置

播放器有一個**選定的裝置**：一個 ALSA PCM 名稱，與 `--device` 接受的相同。啟動時它是設定的裝置，與現在相同：`--device`、`TIDAL_PLAYER_DEVICE`、`app.toml` 中的 `output_device`、`default`（0008 的優先順序）。在執行期間選擇的裝置只在這次執行有效：重新啟動的播放器又會從設定的裝置開始（決策 2）。

快照帶著它（`PlayerSnapshot.device`），所以每個客戶端顯示相同的裝置。

`SetDevice(name)`（新的 `Command`）讓 `name` 成為選定的裝置，可來自任何客戶端：

- **播放中或緩衝中**：引擎在曲目的目前位置把它移到新裝置（0003 AC21）。正在播放的詳細資訊中的輸出與位元完美判定，會依新裝置的 `OutputInfo` 更新（新的引擎事件 `OutputChanged`）
- **已暫停**：舊裝置被關閉，新裝置以暫停狀態開啟。在按 `space` 之前什麼都不會播放
- **暫停時已釋放**（0005）、**已停止**或**載入中**：不開啟任何東西。下一次曲目開始或繼續播放時才開啟新裝置
- **與選定的裝置相同**：什麼都不會發生（不重新開啟）
- 空的名稱會被拒絕（`Reply` 錯誤 `Device name is empty`）。任何其他名稱都照原樣接受。不存在的名稱在開啟時會失敗，就像錯誤的 `--device`

<a id="a-failed-switch-falls-back"></a>
### 失敗的切換會退回

開啟新裝置可能失敗（忙碌、不存在、保留被拒絕、格式被拒絕且沒有後備方案）。這時引擎會**回到最後一個成功開啟的裝置**並在那裡繼續（決策 3）：

- 引擎記住**最後一個正常的裝置**：它最後一次成功開啟的那一個（設定的裝置在第一次開啟成功之前沒有這個）
- 當開啟選定的裝置失敗，且它不是最後一個正常的裝置時，引擎讓最後一個正常的裝置重新成為目前裝置，以相同的狀態重新開啟它（在該位置播放，沒有遺失或重複任何東西，或是暫停），並回報 `DeviceFallback { tried, error, device }`。然後播放器選定的裝置又變回最後一個正常的裝置，其訊息是 `Cannot switch to hw:1,0: <0003's message for the error>; staying on default`。不會跳過任何曲目，佇列不變
- 這適用於新裝置第一次被開啟的任何地方：立即（播放中、緩衝中、已暫停），或稍後（在已停止、載入中或已釋放時發生的切換，於下一次曲目開始或繼續播放時）
- 當最後一個正常的裝置也失敗，或根本沒有（設定的裝置從未開啟成功）時，這就是一般的輸出失敗（0004「Failures」：停止，絕不跳過，保留佇列，使用 0003 的訊息）
- 忙碌的裝置仍然絕不降級為 `plughw:`（0003）：退回的意思是前一個裝置，而不是同一個裝置上的另一種輸出

<a id="listing-devices"></a>
### 列出裝置

`DeviceList`（新的客戶端請求，只回答給提出請求的客戶端，與 `Library` 相同）回傳**播放器**的機器此刻擁有的裝置：0003 的清單（`default` 在前，然後是每個有播放串流的 `hw:C,D` 與音效卡的名稱），每次請求都重新讀取，所以啟動後才插入的 DAC 會出現。選定的裝置是快照的一部分。當它不在清單中時（`plughw:1,0`、自訂的 PCM、已拔除的 DAC），客戶端會把它放在最前面，標示為選定，說明為 `not found`。

<a id="tui-the-devices-popup"></a>
### TUI：裝置彈出視窗

`D`（`SwitchDevice`，spotify-player 的指令名稱與預設按鍵）在任何頁面都會開啟 **Devices** 彈出視窗。它不再是略過的 spotify-player 指令之一。

```
┌Devices─────────────────────────────────────────┐
│● default   shared, through the system mixer    │
│  hw:0,0    HDA Intel PCH: ALC892 Analog        │
│  hw:1,0    E30 II: USB Audio                   │
└────────────────────────────────────────────────┘
```

- 清單送達之前顯示 `Loading devices…`；請求失敗時顯示 `Cannot list devices: <reason>`
- `●` 標示選定的裝置。游標從它開始
- `j`/`k`（與方向鍵）移動；`Enter` 對該列送出 `SetDevice` 並關閉彈出視窗；`esc`（或 `q`）關閉它而不改變任何東西；`r` 重新讀取清單
- 選擇選定的裝置只會關閉彈出視窗
- 按鍵說明在彈出視窗自己的區段下列出它的按鍵，與其他彈出視窗相同。失去連線時 `D` 是淡色的
- 正在播放列的輸出那一行，在引擎開啟新輸出後立即顯示它。在沒有開啟任何東西時，選定的裝置是彈出視窗與 `playback status` 中顯示的那一個

<a id="cli"></a>
### CLI

| 指令 | 作用 |
|---|---|
| `tidal-player playback device` | 向執行中的播放器詢問它的裝置，並像 `tidal-player devices` 一樣印出，`*` 標在**播放器**選定的裝置上。沒有播放器在執行時，以 1 結束並印出 0005 的訊息 |
| `tidal-player playback device NAME` | 送出 `SetDevice(NAME)`，在播放器套用之後以 0 結束，或以播放器的錯誤結束碼 1 結束。它不會等待裝置開啟：開啟失敗會顯示在 `playback status` 與 TUI 中，與任何輸出失敗相同 |
| `tidal-player playback status` | 第二行在音量之後加上 ` · hw:1,0`（選定的裝置）。`--json` 把它放在快照中 |
| `tidal-player devices` | 本機的清單。有播放器在執行時（它的 socket 在 1 秒內回答），`*` 標示**播放器**選定的裝置，而當設定的裝置（`--device`／環境變數／`app.toml`）不同時，它那一列以 `  (configured)` 結尾；清單缺少的播放器裝置會以 `not found` 排在最前面。沒有播放器（或沒有及時回答）時與以前相同：`*` 在設定的裝置上。一律以 0 結束（見「錯誤」，AC13） |
| `tidal-player play --device PCM` | 不變：它啟動時使用的裝置 |

<a id="not-remembered"></a>
### 不會被記住

選定的裝置不會儲存在 `playback.json`（0009）。要讓某個裝置永久生效，請在 `app.toml` 中設定 `output_device`（或 `--device`/`TIDAL_PLAYER_DEVICE`）。這更新了 0009 的決策 2，它原本預期挑選器的選擇會被記住。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 協定：`Command::SetDevice(String)`、`ClientMessage::Devices { id }`、`ServerMessage::DevicesReply { id, result: Result<Vec<DeviceEntry>, String> }`（`DeviceEntry { name, description }`）與 `PlayerSnapshot.device: String` 能通過編解碼器往返。沒有版本欄位變更：0005 的問候已經拒絕其他建置的客戶端
- **AC2** — 播放器：`SetDevice` 在每個階段（已停止、載入中、播放中、緩衝中、已暫停、已釋放）都會發出 `EngineSetDevice(name)` 與帶有新 `device` 的快照。與選定的相同名稱不會發出任何東西。空的名稱會被拒絕且不改變任何東西（表格：階段 × 名稱）
- **AC3** — 播放器：`DeviceFallback { tried, error, device }` 把選定的裝置設回 `device`，保持階段、位置與佇列，不跳過任何東西，並設定訊息 `Cannot switch to <tried>: <0003's message>; staying on <device>`。`SetDevice` 之後的一般輸出 `Error`（無法退回）依 0004 停止，且新裝置保持選定。`OutputChanged` 會更新 `now_playing.output` 與位元完美判定（0004 的音量與靜音規則仍然適用）
- **AC4** — 引擎：暫停時的 `SetDevice` 以暫停狀態開啟新裝置：在 `Resume` 之前 sink 不會收到任何 frame，之後則從暫停的位置收到 frame，沒有遺失或重複。`SetDevice` 造成的每次重新開啟之後（播放中或暫停）都會發出 `OutputChanged(OutputInfo)`，而曲目開始時絕不會發出（那是 `Started`）。對已經開啟的裝置做 `SetDevice` 不會重新開啟它
- **AC5** — 引擎退回：用一個拒絕開啟的假裝置（忙碌、不存在、開啟時遺失），播放中的 `SetDevice` 會重新開啟最後一個正常的裝置，且那裡的 sink 從該位置起恰好收到每個 frame 一次（與沒有切換的執行逐位元組比對），並且 `DeviceFallback` 只發出一次；暫停時它以暫停狀態重新開啟；已停止或已釋放時，退回發生在下一次 `Play` 或 `Resume`；沒有最後一個正常的裝置，或最後一個正常的也失敗時，曲目如現在一樣以 `Error(Output(..))` 失敗；忙碌的裝置絕不會以 `plughw:` 開啟（表格：階段 × 錯誤）
- **AC6** — 啟動與持久化：對獨立模式的 TUI、`daemon` 與 `play`，播放器啟動時選定的裝置是設定的裝置（0008 的優先順序，測試不變），且 `PlayerSnapshot.device` 與引擎的第一個裝置一致；`SavedPlayback` 沒有裝置，所以 `SetDevice` 之後再儲存並還原，會從設定的裝置開始
- **AC7** — 列出：播放器在請求時，以從 `/proc/asound`（測試中為 `TIDAL_PLAYER_ASOUND_DIR`）解析的清單回答 `Devices`。在兩次請求之間更改測試資料（fixture）會改變答案。讀取錯誤是回覆的 `Err`。回覆只送給提出請求的客戶端
- **AC8** — TUI model：`D` 開啟 Devices 彈出視窗並送出 `Devices`；回覆填入它，游標在選定的裝置上；選定的裝置若不在清單中，會以 `not found` 顯示在最前面；`Enter` 對該列發出 `SetDevice` 並關閉（已經選定時不發出任何東西）；`esc`/`q` 關閉而沒有作用；`r` 再問一次；失敗的回覆顯示它的訊息。`SwitchDevice` 是受支援的按鍵對應指令（不再在略過清單中），預設為 `D`，且按鍵說明列出彈出視窗的按鍵
- **AC9** — TUI 繪製：彈出視窗在 80 × 24（載入中、含選定裝置的清單、缺少的選定裝置、失敗）與 40 × 12 的快照；在 1 × 1 到 200 × 60 的任何尺寸下繪製都不 panic
- **AC10** — CLI：`playback device` 印出播放器的清單，`*` 在它選定的裝置上；`playback device NAME` 送出 `SetDevice(NAME)` 並以 0 結束，以播放器的錯誤結束碼 1 結束（空名稱：在送出前以 2 結束），沒有播放器在執行時以 0005 的訊息結束碼 1 結束；`playback status` 在它的第二行顯示選定的裝置（以真正的常駐程式、測試用的 `asound` 檔案與假引擎做整合測試）
- **AC11** — 端對端：在帶有假引擎的常駐程式中，TUI 端的 `SetDevice` 以 `SetDevice` 到達引擎，每個已訂閱的客戶端都收到帶有新 `device` 的快照，且來自引擎的 `OutputChanged` 以更新後的 `now_playing.output` 到達它們
- **AC12** — 更新 `docs/tui.md`（Output device：`D`、彈出視窗、它的按鍵）、`docs/daemon.md`（`playback device`）、`docs/playback.md`（使用哪個裝置與優先順序、`devices` 對 `playback device`）、`docs/config.md`（`SwitchDevice` 受支援；執行期間的選擇只在這次執行有效；`output_device` 使它永久）、它們的 zh-TW 副本、`examples/keymap.toml`（`D`）、`README.md` 與它的 zh-TW 副本；`CLAUDE.md` 的「Features」新增該條目
- **AC13** — 有播放器在執行時，`tidal-player devices` 把播放器選定的裝置標為 `*`，設定的裝置在不同時標為 `(configured)`；本機清單缺少的播放器裝置會以 `not found` 排在最前面；沒有播放器，或 1 秒內沒有 `Welcome`，就如以前印出清單，`*` 在設定的裝置上；每種情況都以 0 結束

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 新裝置忙碌（另一個應用程式佔用了 `hw:` 音效卡） | 回到最後一個正常的裝置，仍在播放：`Cannot switch to hw:1,0: Output hw:1,0 is busy (used by <app>): …; staying on default` |
| 新裝置不存在（透過 `playback device` 打錯字） | 被接受；開啟它會失敗，引擎會退回。已停止時不會開啟任何東西，所以退回與它的訊息發生在下一次播放 |
| 選定的 DAC 在播放時被拔除 | 與 0004 相同（`Output hw:1,0 was lost`，已停止）：失去已開啟的裝置不算失敗的切換。它仍是選定的，並在彈出視窗中顯示為 `not found` |
| 在設定的裝置從未開啟成功時切換到某個裝置（剛啟動，還沒播放過任何東西） | 沒有最後一個正常的裝置：開啟失敗就是一般的輸出失敗 |
| 已釋放時的 `SetDevice`（暫停，裝置已交還） | 現在不開啟任何東西；`space` 在該位置開啟新裝置（0005 AC19 的規則：沒有東西被開啟或釋放兩次） |
| 載入期間的 `SetDevice` | 曲目開始時在新裝置上開啟 |
| 無縫播放的預先載入進行中 | 保留：預先載入在新裝置上播放（引擎的預先載入不綁定裝置） |
| 兩個客戶端同時選擇不同的裝置 | 依序處理；最後一個勝出，兩者都在下一個快照中看到它 |
| 沒有音效卡（容器、沒有 `/proc/asound`） | 清單只有 `default`，與 `tidal-player devices` 相同 |
| `alsa` feature 關閉 | `SetDevice` 被接受並儲存；播放如現在一樣失敗（`this build has no ALSA output`） |
| 極小的終端機 | 彈出視窗與其他彈出視窗一樣被裁切；少於 8 列時沒有頁面區域、沒有彈出視窗（0004） |
| 音量低於 100 % 或靜音 | 切換之後的判定保留 0004 的原因（`volume below 100%`），優先於引擎的 |
| MPRIS | 沒有變更：MPRIS2 沒有裝置屬性（0010） |

<a id="test-plan"></a>
## 測試計畫

每個自動化測試都以它的條件命名。Red 是針對存根型別與本體為存根的函式的失敗斷言（`SetDevice` 被忽略、`Devices` 以空清單回答、彈出視窗什麼都不畫），沒有 `todo!()` 也沒有編譯錯誤，與 0001–0013 相同。沒有測試會開啟音訊裝置或碰網路。

| AC | 測試（file :: name） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac1_device_messages_round_trip` | 新的訊息與快照欄位能往返 | 存根把 `device` 序列化為空 |
| AC2 | `crates/core/src/player/tests.rs` :: `ac2_set_device`（表格：階段 × 名稱） | 每一列的 effect 與快照 `device` | 存根忽略 `SetDevice`（沒有 effect，device 不變） |
| AC3 | `crates/core/src/player/tests.rs` :: `ac3_device_fallback`、`ac3_set_device_failure`、`ac3_output_changed` | 裝置還原、階段／位置／佇列保留、訊息；一般失敗會停止；輸出與判定已更新 | 存根忽略 `DeviceFallback` 並保留舊的輸出行 |
| AC4 | `crates/audio/tests/engine.rs` :: `ac4_set_device_while_paused`、`ac4_output_changed`、`ac4_set_same_device` | `Resume` 之前沒有 frame、逐位元組連續；每次重新開啟發出一次事件；相同裝置不重新開啟 | 存根引擎在暫停時就開始寫入新裝置／不發出 `OutputChanged` |
| AC5 | `crates/audio/tests/engine.rs` :: `ac5_device_fallback`（表格：階段 × 錯誤、沒有最後一個正常的裝置、兩者都失敗） | 重新開啟的順序、逐位元組的 frame、事件一次、一般失敗的列 | 存根引擎因新裝置的錯誤而使曲目失敗 |
| AC6 | `crates/app/src/player/runtime.rs` :: `ac6_snapshot_device_is_configured` + `crates/core/src/player/tests/persistence.rs` :: `ac6_device_not_remembered` | 快照的 device 等於引擎的第一個裝置；`SetDevice` 之後還原使用設定的裝置 | 存根快照的 device 為空 |
| AC7 | `crates/app/src/ipc/server/tests.rs` :: `ac7_devices_reply`（兩次請求之間更改測試資料目錄；無法讀取的目錄） | 每次請求都是新的清單；錯誤；只有提出請求者收到 | 存根只回答 `default` |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_devices_popup`（表格：按鍵 → effect、狀態）+ `crates/core/src/ui/keymap/tests.rs` :: `ac8_switch_device_supported` + `crates/core/src/ui/help/tests.rs` :: `ac8_devices_popup_help` | 彈出視窗狀態、游標、`SetDevice`、關閉；按鍵對應；說明區段 | 存根 `D` 什麼都不做；`SwitchDevice` 仍被略過 |
| AC9 | `crates/app/src/tui/render.rs` :: `ac9_devices_popup`（快照）、`ac17_no_panic_any_size`（新的列） | 版面；不 panic | 存根不畫彈出視窗 |
| AC10 | `crates/app/src/cli/oneshot.rs` :: `ac10_parse_device`、`ac10_status_device_line` + `crates/app/tests/daemon.rs` :: `ac10_playback_device` | 解析與結束碼；狀態行；對常駐程式的列出與切換 | 存根什麼都不印也不送出 |
| AC11 | `crates/app/src/player/runtime.rs` :: `ac11_set_device_reaches_engine`（假引擎、兩個訂閱者） | 引擎呼叫；兩個快照；輸出更新 | 存根 runtime 丟棄 `EngineSetDevice` |
| AC12 | `crates/app/tests/docs.rs` :: `ac12_device_selection_documented` + zh-TW 副本、README 與 `CLAUDE.md` 在驗收時審閱；`crates/app/tests/examples.rs`（範例 keymap 就是預設 keymap，既有測試） | 文件提到 `D`、`SwitchDevice`、`playback device`；範例 keymap 相符 | 文件缺少這些詞；範例缺少 `D` |
| AC13 | `crates/app/src/cli/oneshot.rs` :: `ac13_local_device_lines`（表格：沒有播放器、相同裝置、不同裝置、裝置不在清單中）+ `crates/app/tests/daemon.rs` :: `ac13_devices_shows_player_device`（常駐程式在 `hw:0,0`，`playback device hw:1,0`，然後 `devices`）+ `crates/app/tests/cli.rs` :: `ac25_devices_marks_configured`（不變，沒有播放器） | 每一列的標記；對常駐程式的真正指令 | `devices` 忽略播放器：`*` 仍在 `hw:0,0` 上 |

驗收時在使用者的機器上手動檢查（結果寫在 PR 說明中）：在 `default` 上播放，`D` → DAC（`hw:`）會移動曲目，除了重新開啟之外沒有聽得見的間隙，且輸出那一行顯示 `bit-perfect`；從另一個終端機執行 `playback device hw:0,0` 會再次移動它；彈出視窗關閉期間插入的 DAC 會在下一次 `D` 出現；忙碌的 `hw:` 會回報忙碌，關閉另一個應用程式之後按 `space` 會播放；忙碌的 `hw:` 會以退回的訊息繼續在前一個裝置上播放；重新啟動常駐程式會從設定的裝置開始。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`Command::SetDevice`、`ClientMessage::Devices`、`ServerMessage::DevicesReply`、`DeviceEntry`、`PlayerSnapshot.device`；`PlayerState` 新增選定的裝置、`PlayerEffect::EngineSetDevice`、`EngineEvent::OutputChanged(TrackDetails)`、`EngineEvent::DeviceFallback`；`ui` 中的 Devices 彈出視窗與 `SwitchDevice`。沒有 I/O
- `tidal-player-audio`：`Event::OutputChanged(OutputInfo)`、`Event::DeviceFallback { tried, error, device }` 與最後一個正常的裝置；暫停時的 `SetDevice` 以暫停狀態開啟；相同裝置為空操作
- `tidal-player`：runtime 對應 `EngineSetDevice` 與 `OutputChanged`；伺服器透過裝置列舉器接縫（seam，讀取 `/proc/asound` 或 `TIDAL_PLAYER_ASOUND_DIR`）回答 `Devices`；啟動時快照的 device；`playback device`；彈出視窗的繪製
- `xtask layering`：沒有變更（core 仍然對 audio crate 一無所知：`DeviceEntry` 是它自己的型別）

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-10，從程式碼）：引擎的 `SetDevice` 會移動正在播放的曲目並重播尚未聽到的 frame（`crates/audio/src/engine.rs` 的 `set_device`、0003 AC21），但不發出帶有新 `OutputInfo` 的事件；在沒有開啟任何東西時，它只記住名稱；它在開啟之後呼叫 `sync_pause`，所以暫停的引擎應該保持暫停（由 AC4 證明）；播放器沒有裝置的概念（`PlayerState`、`PlayerSnapshot`）；裝置清單是對 `/proc/asound/{cards,pcm}` 的 `parse_devices`，測試用 `TIDAL_PLAYER_ASOUND_DIR`（`crates/audio/src/devices.rs`、`crates/app/src/main.rs` 的 `devices`）；`SwitchDevice` 在 0008 的略過清單中，且 `D` 預設沒有綁定；`playback.json` 是 `SavedPlayback` 第 1 版（`crates/core/src/player/saved.rs`）。

假設（驗收時檢查）：讀取 `/proc/asound` 就能反映啟動後才插入的 USB DAC，不需要任何其他東西（它是核心狀態）；在兩張 `hw:` 音效卡之間切換時，會先釋放第一張卡的保留，再取得第二張（0003 AC13 涵蓋切換裝置時的釋放）。

<a id="decisions-answered-by-the-user-2026-10-10"></a>
## 決策（由使用者回答，2026-10-10）

1. **CLI**：*回答：照提議*：`playback device [NAME]` 從執行中的播放器列出或切換；`devices` 不變。*2026-10-11 修訂*：`devices` 的標記取自執行中的播放器（見「錯誤」）
2. **記住**：*回答：只在這次執行*。重新啟動會從設定的裝置開始；`output_device` 使選擇永久
3. **失敗的切換**：*回答：退回舊裝置*，並在那裡繼續播放，附上一則訊息
4. **按鍵**：*回答：照提議*：只有 `D` / `SwitchDevice`

<a id="implementation-notes"></a>
## 實作備註

規格沒有說明之處所做的選擇：

- **引擎（slice A）**：播放中或暫停時，切換期間的退回會發出 `DeviceFallback`，然後是 `OutputChanged`（最後一個正常的裝置的資訊）；在曲目開始或繼續播放時，只有 `DeviceFallback`，然後是 `Started`/`Resumed`。當兩個裝置都失敗時，回報的錯誤是嘗試的那個裝置的，且它仍是選定的。釋放後繼續播放時兩者都失敗，會得到 `ResumeFailed`（保持暫停，如 0005 AC21）。對選定的裝置做 `SetDevice` 在每個階段都是空操作。`SinkScript::open_errors` 跨 sink 依裝置計數
- **Core（slice B）**：空的名稱被 core 忽略（`Reply` 錯誤來自 runtime）。載入期間的 `DeviceFallback` 會在 `Started` 之後保留它的訊息。`OutputChanged` 保留來源行，且在沒有任何東西開始時被忽略。`SwitchDevice` 位於按鍵說明的 Playback 區段（「choose the output device」），在 `ToggleAutoplay` 之後。在彈出視窗中，`q` 與 `r` 是固定的按鍵（`q` 關閉，不會離開程式）；載入中或失敗時 `Enter` 什麼都不做；在另一個彈出視窗之上按 `D` 什麼都不做。失去連線時：`D` 以連線訊息開啟失敗狀態的彈出視窗；過期的回覆 ID 會被丟棄
- **App（slice C）**：
  - runtime 對客戶端的 `SetDevice("")` 以 `Reply` 錯誤 `Device name is empty` 回答，而不把它傳給播放器；`playback device ""` 在連線之前以相同的文字以 2 結束
  - `Devices` 透過 `DeviceLister` 接縫（seam）在播放器執行緒上回答（`AsoundDevices`：`TIDAL_PLAYER_ASOUND_DIR`，否則 `/proc/asound`），每次請求都讀取 `cards` 與 `pcm`。檔案遺失表示沒有音效卡（只有 `default`，如邊界情況表所示）；任何其他讀取錯誤都是回覆的 `Err`（`cannot read <path>: <error>`）。`tidal-player devices` 在清單無法讀取時仍只列出 `default`（不變）
  - 播放器啟動時使用的裝置是 `Settings.device`（旗標、環境變數、`app.toml`、`default`），由 `settings::with_device` 複製到 `PlayerConfig.device`；引擎以同一個 `PlayerConfig.device` 啟動
  - `DeviceFallback` 的訊息是 0003 對該錯誤的輸出訊息（以 `EngineError::Output` 呼叫 `engine_error_message`）；`OutputChanged` 被對應為 `TrackDetails`，其輸出描述與判定與 `Started` 時相同，來源為空（播放器保留它自己的）
  - `playback status`：裝置在音量之後、`device released` 之前，且快照沒有裝置時（較舊的播放器）會省略
  - `playback device` 送出 `Devices` 與 `Subscribe`，在兩者都收到之後（順序不拘）才透過 `format_devices` 印出，清單缺少選定的裝置時，播放器選定的裝置會以 `not found` 排在最前面（與彈出視窗相同）。清單失敗時會在 stderr 印出 `Cannot list devices: <reason>`，結束碼 1
  - 彈出視窗的列沒有前導空格：`●` 或一個空格、補齊到最長名稱的名稱、三個空格、說明（如繪圖所示）；方框是最寬的列再加 6 欄（最多 50）；`Loading devices…` / `Cannot list devices: …` 是一個淡色的列
- **`devices` 的標記（AC13）**：`devices` 只連到 socket 一次（不探測鎖定、不重試），送出 `Subscribe` 並取 `Welcome` 的 device；整個交換在一個限時 1 秒的輔助執行緒上執行，所以接受連線卻從不問候的 socket 無法卡住它（過期的 socket 會立即失敗）。播放器的與設定的名稱在 `hw:C` → `hw:C,0` 之後才比較，如 `format_devices` 所做，所以設定為 `hw:0` 而實際使用 `hw:0,0` 是同一個裝置，不會得到 `(configured)`。`(configured)` 只會顯示在清單的某一列上：清單缺少的設定裝置不會被加入

<a id="out-of-scope"></a>
## 不在範圍內

- 在裝置出現或消失時自動切換（跟隨熱插拔、監看 udev/PipeWire）：清單只在被詢問時才讀取
- 以名稱指定的 PipeWire/PulseAudio sink（`default` 仍是到達系統混音器的方式）
- 每個裝置的設定（音質、釋放延遲、緩衝區大小），以及 `hw:` 緩衝區問題（[#6](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/6)）
- Windows 裝置（[#21](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/21)）
- 透過 MPRIS 的裝置屬性

<a id="bugs"></a>
## 錯誤

<a id="tidal-player-devices-shows-the-configured-device-after-a-switch-2026-10-11"></a>
### `tidal-player devices` 在切換之後顯示設定的裝置（2026-10-11）

- **預期**（由使用者在真實硬體上回報）：在 TUI 中選擇 `default` 之後，`tidal-player devices` 標示 `default`，也就是播放器正在使用的裝置
- **實際**：它仍把 `*` 留在 `hw:3,0`，也就是設定的裝置
- **根本原因**：不是程式碼的錯誤。規格讓 `devices` 保持在本機且與播放器無關（決策 1，「`devices` 不變」），所以它的 `*` 意思是「已設定」，而 `playback device` 印出同一份清單，`*` 的意思卻是「使用中」。兩份看起來一樣、標記卻不同的清單誤導了使用者。決策經使用者重新檢視：有播放器時，`devices` 會詢問執行中的播放器
- **修正**：「CLI」下的 `tidal-player devices` 那一列與 AC13。清單本身仍在本機（播放器是同一個使用者在同一台機器上，透過 Unix socket）；只有標記來自播放器。1 秒內沒有回答的播放器會被視為不存在，所以 `devices` 絕不會卡住
