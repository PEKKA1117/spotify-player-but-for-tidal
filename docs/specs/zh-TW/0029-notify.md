[English](../0029-notify.md) | 正體中文

<a id="0029--desktop-notification-on-track-change"></a>
# 0029 — 換曲目時的桌面通知

- **狀態**：approved（2026-11-16）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#55](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/55)
- **相依於**：0004（implemented：播放器的狀態與已開始的曲目）、0005（implemented：常駐程式）、0008（implemented：`app.toml`）、0010 與 0016（implemented：封面快取）、0025（implemented：runtime 對已開始曲目所知的資訊）、0027（implemented：記錄檔）
- **使用者文件**：[`docs/config.md`](../../config.md)（四個設定與新增的「Desktop notifications」一節）與它的 zh-TW 版本、`examples/app.toml`、`README.md` 與它的 zh-TW 版本（AC16）

<a id="context"></a>
## 背景

播放器在常駐程式中，或 TUI 在另一個工作區時，螢幕上沒有任何東西說明剛剛開始的是哪一首曲目。spotify-player 為此有 `notify` 功能：每首新曲目顯示一則桌面通知，以 `enable_notify`、`notify_format`、`notify_timeout_in_secs` 與 `notify_transient` 設定。本規格加入同樣的設定與相同的意義，另外多做 spotify-player 沒有做的三件事：新通知取代前一則而不是堆疊、內文在把它當標記語言讀取的伺服器上是安全的，而且沒有通知伺服器絕不算錯誤。

tidalt 沒有這些。

<a id="behaviour"></a>
## 行為

<a id="what-is-shown"></a>
### 顯示的內容

一首曲目開始時，桌面會顯示一則通知，內有該曲目的專輯封面、一行摘要與內文：

```
[cover]  Song (Live) • First Artist, Second Artist
         The Album
```

不論是哪一種播放器，都由播放器送出：獨立模式、常駐程式或 `play`。連上常駐程式的 TUI 不送任何東西，所以不論連了多少個客戶端，一首曲目只會有一則通知，而常駐程式在沒有任何客戶端時也會通知。

通知會取代播放器的前一則通知，所以在佇列中快速跳過時，螢幕上留下的是一則通知，而不是一疊。

<a id="the-settings"></a>
### 設定

```toml
enable_notify = true
notify_format = { summary = "{track} • {artists}", body = "{album}" }
notify_timeout_in_secs = 0
notify_transient = false
```

| 設定 | 值 | 預設 | 環境變數 |
|---|---|---|---|
| `enable_notify` | 布林值 | `true` | `TIDAL_PLAYER_ENABLE_NOTIFY` |
| `notify_format` | 一個表格：`summary` 與 `body`，兩者皆為選用的字串 | spotify-player 的，見上 | |
| `notify_timeout_in_secs` | 整數 0–3600（`0`：用通知伺服器自己的時間） | `0` | `TIDAL_PLAYER_NOTIFY_TIMEOUT_IN_SECS` |
| `notify_transient` | 布林值（`true`：伺服器不把它留在歷史紀錄中） | `false` | `TIDAL_PLAYER_NOTIFY_TRANSIENT` |

名稱、預設與意義都是 spotify-player 的。優先順序依 0008（環境變數優先於設定檔）。`notify_format` 只存在於 `app.toml`，與所有表格形式的設定相同。

<a id="the-format"></a>
### 格式

`summary` 與 `body` 是帶有預留位置的文字：

| 預留位置 | 值 |
|---|---|
| `{track}` | 標題；若有版本，則把版本放在括號中（`Song (Live)`），與事件 hook 的 `TIDAL_PLAYER_TRACK_TITLE` 相同 |
| `{artists}` | 藝人，以 `, ` 連接 |
| `{album}` | 專輯的標題；曲目沒有專輯時為空 |

其餘一律照寫保留，`\n` 也是。不是這三個之一的 `{name}` 是 `app.toml` 錯誤，所以打錯字會在啟動時被發現，而不是變成通知中的一個空洞。`summary` 不可為空；`body` 可以（`body = ""` 會給出只有摘要的通知）。未來的 `playback_format`（播放視窗的文字）會使用相同的預留位置。

<a id="which-tracks-notify"></a>
### 哪些曲目會通知

通知跟隨**開始**的曲目：它的第一個影格已寫入，或播放無縫地移入它。這是事件 hook 的 `Changed`（0025），只有一個差異：與前一首開始的曲目 ID 相同的曲目不會再通知一次。所以：

- 在佇列中快速跳過時，只有真正開始的那首曲目會通知
- 單曲重複，以及佇列中連續兩次同一首曲目，只通知一次
- 停止後再播放一首曲目，中間沒有其他曲目，不會再通知
- `A`、`B`，然後又是 `A`，會通知三次
- 還原的工作階段（0009）在有曲目開始之前不通知
- 暫停、繼續、跳轉、音量與裝置的改變都不通知

<a id="the-cover"></a>
### 封面

封面是播放器保存在封面快取中的檔案（0010、0016）。快取在曲目成為目前曲目時就開始下載封面，那是在曲目開始之前，所以檔案通常已經在了。

- **已快取**：通知帶著它
- **尚未快取**：通知最多等它 2 秒，然後不帶封面送出。通知只送一次，封面之後才到也不會再送
- **沒有封面**（曲目沒有專輯封面，或 `max_cover_arts = 0`）：通知立刻送出，不帶封面

一首曲目在另一首的通知還在等待時開始，就取代它的位置：等待中的那則永遠不會送出。

<a id="what-the-server-receives"></a>
### 伺服器收到的內容

在工作階段匯流排上對 `org.freedesktop.Notifications.Notify` 的一次呼叫：

| 引數 | 值 |
|---|---|
| `app_name` | `tidal-player` |
| `replaces_id` | 伺服器給播放器前一則通知的 ID，第一則為 `0` |
| `app_icon` | 空 |
| `summary` | 繪製後的 `summary` |
| `body` | 繪製後的 `body` |
| `actions` | 無 |
| `hints` | `image-path`：有封面時，封面的絕對路徑。`transient`：設定了 `notify_transient` 時為 `true` |
| `expire_timeout` | `notify_timeout_in_secs` 為 `0` 時是 `-1`（由伺服器決定），否則是以毫秒表示的秒數 |

宣告了 `body-markup` 能力的伺服器會把內文當標記語言讀取，像 `Rock & Roll <Live>` 這樣的標題會被截斷或丟掉。對這樣的伺服器，內文中的 `&`、`<` 與 `>` 會變成 `&amp;`、`&lt;` 與 `&gt;`。摘要絕不是標記語言，照原樣送出。能力在第一則通知時詢問，並在呼叫失敗之後再問一次（伺服器可能已被換掉）。

<a id="when-it-cannot-notify"></a>
### 無法通知時

通知絕不值得一個錯誤：訊息列不會出現任何東西，播放也絕不等待匯流排。

- **沒有工作階段匯流排**（SSH、純主控台）：啟動時在記錄檔寫一行 `info`，這次執行中不再重試
- **沒有通知伺服器**，或呼叫失敗或超過 5 秒：在記錄檔寫一行 `warn`。之後的失敗在有通知成功之前都不說話。下一首曲目會再試一次，所以在常駐程式之後才啟動的伺服器（播放器的服務先起來的登入情形）從那時起就能運作
- **`enable_notify = false`**：不啟動執行緒，也不為它開啟連線

通知使用自己的工作階段匯流排連線，所以在 `mpris = false` 時也能運作。

<a id="acceptance-criteria"></a>
## 驗收條件

1. **AC1 設定**：`enable_notify`、`notify_timeout_in_secs` 與 `notify_transient` 從 `app.toml` 與它們的環境變數讀取，優先順序依 0008，預設如上；`notify_format` 接受一個表格，內有選用的 `summary` 與 `body` 字串，缺少的保持預設。型別錯誤的值、超出 0–3600 的 `notify_timeout_in_secs`、`notify_format` 中的未知欄位與空的 `summary`，都是 `app.toml` 錯誤，結束碼為 2，措辭依 0008
2. **AC2 繪製**：格式依「The format」所說繪製 `{track}`、`{artists}` 與 `{album}`：版本放在括號中、多位藝人以 `, ` 連接、沒有的專輯是空的文字、用了兩次的預留位置、預留位置周圍的文字與 `\n` 保留
3. **AC3 未知的預留位置**：`summary` 或 `body` 中，三者之外的 `{name}` 是 `app.toml` 錯誤，結束碼為 2，並指名該預留位置；沒有閉合的 `{` 保留為文字
4. **AC4 請求**：對「有封面或沒有」、`notify_transient` 開與關、`notify_timeout_in_secs` 為 `0` 與 `7`（`-1` 與 `7000`）的每一種組合，`Notify` 的引數都是「What the server receives」所列的
5. **AC5 哪些曲目會通知**：從依序開始的曲目來看：第一首通知；不同的曲目通知；同一個曲目 ID 再次出現不通知，停止之後也一樣；`A`、`B`、`A` 給出三次
6. **AC6 標記語言**：伺服器的能力中有 `body-markup` 時，內文中的 `&`、`<` 與 `>` 會被跳脫，摘要不會；沒有時內文照原樣送出
7. **AC7 取代**：第二則通知帶著伺服器為第一則回傳的 ID 作為 `replaces_id`，第三則帶著第二則的；第一則帶 `0`
8. **AC8 封面**：已快取的封面立刻送出；在 2 秒內出現的，在它出現時送出；超過 2 秒後通知不帶 `image-path` 送出，且不再送一次；沒有封面的曲目，或沒有封面快取的播放器，不等待
9. **AC9 最新的優先**：一首曲目在通知等待封面時，或呼叫進行中時開始，就是下一個送出的，而在這之間開始的曲目永遠不會送出
10. **AC10 真實序列**：透過 `PlayerRuntime` 與假的引擎（fake engine）驅動，一首開始的曲目、一次無縫轉換與一次跳過，都把開始的那首曲目連同它的中繼資料交給通知器；跳過從未開始的曲目、暫停、繼續、跳轉與裝置移動則什麼都不交給它
11. **AC11 失敗**：失敗的呼叫、不存在的伺服器與超過 5 秒的呼叫，各自記錄一行 `warn`，且不對播放器的訊息送出任何東西；之後的失敗在成功之前不記錄；成功會重新啟用它；失敗之後會嘗試下一首曲目；失敗之後會再詢問能力
12. **AC12 絕不阻塞**：通知在等待封面或等待伺服器時，`PlayerRuntime::handle` 就會回傳
13. **AC13 每一種播放器**：設定會傳到獨立模式、常駐程式與 `play`，它們啟動通知器，而連上的 TUI 不啟動。`enable_notify = false` 時不啟動通知器。通知開啟時，`play` 播放單一曲目 ID 會請求該曲目的中繼資料，就像它為 hook 所做的（0025 AC13）
14. **AC14 沒有匯流排**：沒有工作階段匯流排時，播放器仍會啟動、播放並記錄一行 `info`；不重試
15. **AC15 真實匯流排**：在一個私有的 `dbus-daemon` 上，搭配一個會記錄呼叫的通知伺服器，播放兩首曲目的常駐程式送出兩次帶著預期引數的 `Notify` 呼叫，第二次取代第一次；該匯流排上沒有伺服器時，常駐程式繼續播放並記錄一行 `warn`
16. **AC16 文件**：`docs/config.md`、`README.md` 與它們的 zh-TW 版本描述四個設定、預留位置、封面，以及沒有伺服器時會發生什麼；`examples/app.toml` 以預設值示範這四個設定；`CLAUDE.md` 有這個功能的條目，且 `TIDAL_PLAYER_ENABLE_NOTIFY=off` 在手動檢查的隔離設定之中

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 沒有專輯的曲目 | `{album}` 是空的；預設的內文是空的，通知只有摘要 |
| 中繼資料不明的曲目（`play` 一個中繼資料請求失敗的 ID） | 不通知：沒有東西可顯示 |
| 快速跳過 | 只有開始的曲目是候選（0025 決策 1），而最新的一首取代任何在等待的 |
| 封面在 2 秒之後才到 | 它在曲目下次播放時已在快取中；這則通知維持已送出的樣子 |
| 伺服器忘了被取代的 ID（通知已過期或被關閉） | 那是伺服器的事：通知規格讓它成為一則新通知 |
| 通知伺服器重新啟動 | 下一次呼叫在新的伺服器上失敗或成功；失敗之後會再詢問能力 |
| 伺服器卡住 | 呼叫在 5 秒後放棄；這段期間開始的曲目只保留最新的 |
| 播放器關閉 | 最後一則通知任它過期；等待中的丟掉 |
| 含換行的標題，或比伺服器顯示的更長 | 照原樣送出；由伺服器截斷 |
| 執行中修改了 `app.toml` | 只在啟動時讀取一次，與每個設定相同 |
| 整合測試 | 它們已經在私有或不存在的匯流排上執行播放器（`DBUS_SESSION_BUS_ADDRESS`），所以沒有測試會通知開發者的桌面 |

<a id="test-plan"></a>
## 測試計畫

| AC | 測試 | 驗證內容 | 預期的紅燈失敗 |
|---|---|---|---|
| AC1 | `crates/app/src/config.rs` :: `ac1_notify_settings`（表格） + `crates/app/src/config.rs` :: `ac1_notify_precedence`（表格） | 解析出的值、預設、每個錯誤的措辭；環境變數優先於設定檔 | 未知的鍵 `enable_notify` |
| AC2 | `crates/core/src/notify.rs` :: `ac2_render`（表格） | 格式與曲目 → 文字 | 存根回傳未繪製的格式 |
| AC3 | `crates/core/src/notify.rs` :: `ac3_unknown_placeholders`（表格） + `crates/app/src/config.rs` :: `ac3_notify_format_errors` | 解析器的錯誤指名該預留位置；`app.toml` 的措辭 | 存根接受每一種格式 |
| AC4 | `crates/core/src/notify.rs` :: `ac4_request`（表格） | 每種組合的引數 | 存根不送 hints，且逾時為 `0` |
| AC5 | `crates/core/src/notify.rs` :: `ac5_which_tracks_notify`（表格） | 已開始的曲目 ID → 哪些通知 | 存根對每次開始都通知 |
| AC6 | `crates/core/src/notify.rs` :: `ac6_markup`（表格） | 有該能力時內文被跳脫、沒有時不跳脫、摘要不跳脫 | 存根從不跳脫 |
| AC7 | `crates/app/src/player/notify.rs` :: `ac7_replaces_the_previous`（fake 伺服器） | 三則通知的 `replaces_id` | 每次呼叫都帶 `0` |
| AC8 | `crates/app/src/player/notify.rs` :: `ac8_cover`（表格、暫存目錄、短的等待） | 已快取、在等待時間內才到、太晚到、沒有封面、沒有快取 | 存根從不等待，也從不設定 `image-path` |
| AC9 | `crates/app/src/player/notify.rs` :: `ac9_latest_wins`（由 barrier 卡住的 fake 伺服器） | 一連串開始所發出的呼叫 | 每次開始都送出 |
| AC10 | `crates/app/src/player/runtime.rs` :: `ac10_notifier_gets_started_tracks`（表格） | 每種序列所記錄的曲目 | 沒有記錄到任何東西 |
| AC11 | `crates/app/src/player/notify.rs` :: `ac11_failures`（表格、fake 伺服器） | 記錄的行與它們的重新啟用；沒有 `RuntimeInput`；再次詢問能力 | 沒有記錄任何一行 |
| AC12 | `crates/app/src/player/notify.rs` :: `ac12_never_blocks` | 一個被 barrier 卡住的伺服器；`send` 立刻回傳 | `send` 在等待 |
| AC13 | `crates/app/src/config.rs` :: `ac13_notify_reaches_the_settings` + `crates/app/src/player/notify.rs` :: `ac13_off_starts_nothing` + `crates/app/src/cli/play.rs` :: `ac13_play_asks_for_the_notifications_metadata` + 手動檢查 | 三種播放器都由它啟動的設定；關閉時 `start` 不給通知器；開啟時會取得單一曲目 ID；下面的檢查在連上 TUI 的情況下執行它，因為 `main` 啟動播放器而沒有接縫（seam） | 設定中沒有通知欄位；啟動了通知器；曲目沒有名稱 |
| AC14 | `crates/app/tests/notify.rs` :: `ac14_no_bus` | 在不存在的匯流排上的常駐程式服務一個客戶端，且它的記錄檔有那一行 | 找不到那一行 |
| AC15 | `crates/app/tests/notify.rs` :: `ac15_real_bus` + `crates/app/tests/notify.rs` :: `ac15_no_server` | 記錄到的呼叫；`warn` 那一行 | 沒有記錄到任何呼叫 |
| AC16 | `crates/app/tests/docs.rs` :: `ac16_notify_documented` + `crates/app/tests/examples.rs`（範例可解析） + 驗收時審閱 | 四個設定出現在 `docs/config.md`；文字說明由人工閱讀 | 找不到這些設定 |

驗收時手動檢查，對真實 API 與維護者的通知伺服器（mako）：以發行版建置、在它自己的 runtime 與狀態目錄下作為常駐程式執行，通知開啟。播放一首曲目、跳到下一首、快速跳過五首、讓一首曲目無縫地播完、設定單曲重複讓它循環；`makoctl list` 一次只顯示一則通知，帶著封面、摘要與內文，而 `makoctl history` 只在 `notify_transient` 關閉時才有被取代的通知。含 `&` 的標題完整顯示。連著 TUI 時，每首曲目仍然只有一則通知；獨立模式的 TUI 與播放單一曲目 ID 的 `play` 也會通知。然後在 mako 停止的情況下：播放器繼續播放，記錄檔有一行 `warn`。通知看起來如何（封面的大小、放在哪裡）是伺服器的事，留給維護者判斷。TUI 沒有任何新的顯示，所以 PR 沒有螢幕截圖一節。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`，新的 `notify` 模組：`NotifyFormat`（解析後的 `summary` 與 `body`，附預留位置檢查）、對 `Track` 的繪製、`NotifyTracker`（哪些已開始的曲目會通知）、標記語言的跳脫，以及 `Request`：由繪製後的文字、封面的路徑、設定與前一個 ID 組成的 `Notify` 引數。全部是純邏輯。它放在 `core` 是因為未來的 `playback_format` 會在 TUI 中繪製相同的預留位置
- `tidal-player`（`crates/app`），新的 `player/notify.rs`：工作執行緒（一個存放最新曲目的槽、等待封面、前一個 ID、失敗記錄），位於兩個接縫（seam）之後：`Server`（能力與 `Notify`；zbus 實作與一個 fake）與封面的路徑（快取目錄，或沒有）
- `crates/app/src/player/runtime.rs`：`with_notifier`，如同 `with_hook`：`Handled.started` 設定時，`handle` 把目前的曲目交給通知器。`Command`、`Event` 與 socket 協定不變
- `crates/app/src/config.rs` 與 `settings.rs`：四個設定；`main.rs` 為三種播放器啟動通知器；`cli/play.rs`：中繼資料請求

<a id="facts-vs-assumptions"></a>
## 事實與假設

已於 2026-11-16 驗證：

- spotify-player（`master`，`spotify_player/src/config/mod.rs` 與 `client/mod.rs`）：四個設定與它們的預設（`enable_notify = true`、`notify_timeout_in_secs = 0`、`notify_transient = false`、上面的格式）；預留位置是 `{track}`、`{artists}` 與 `{album}`，其他任何 `{…}` 都被默默丟掉；只有設定大於 `0` 時才設定逾時；封面是快取檔案的路徑，檔案存在時作為圖示給出；它在封面取得之後才通知；它從不取代通知，也不跳脫內文
- 在 `b0093e9` 的程式碼中：`Handled.started` 在任何曲目開始時設定，包括跳過所導向的那首，而從不為只是載入的曲目設定；只要 `max_cover_arts > 0`，不論 MPRIS 開或關，封面工作執行緒都會執行，並在項目成為目前曲目時要求封面；已快取的封面是 `<cache dir>/covers/<cover>.jpg`（`cover_cache::cached_file`）；`zbus` 是相依套件，且有它的阻塞式 API；整合測試把 `DBUS_SESSION_BUS_ADDRESS` 設為不存在或私有的匯流排來執行播放器
- 在維護者的機器上：通知伺服器是 mako（通知規格 1.2），具備 `body`、`body-markup`、`actions` 與 `icon-static` 能力

假設，實作時確認；假設錯了就回到本規格：

- mako、dunst、GNOME Shell 與 KDE Plasma 會顯示以絕對路徑給出的 `image-path` hint（通知規格允許 `file://` URI 或路徑）。mako 在驗收時手動檢查；若它需要 URI 或 `app_icon`，「What the server receives」就要改
- 通知開啟時，`play` 播放單一曲目 ID 可以原樣重用 hook 的中繼資料路徑（`hooked`）
- `dbus-daemon` 測試可以像 `tests/mpris.rs` 容納它的監聽器那樣，容納一個會記錄呼叫的通知伺服器

<a id="decisions"></a>
## 決策

1. **預設開啟。** *已回答（使用者，2026-11-16）：開啟，與 spotify-player 相同。* 預設關閉落選：升級後什麼都不會變，但這個功能會一直不為人知
2. **每一種播放器都通知。** *已回答（使用者，2026-11-16）：獨立模式、常駐程式與 `play`，與 MPRIS 和事件 hook 相同。* 把獨立模式的 TUI 排除在外落選：TUI 常常在另一個工作區
3. **最多等封面 2 秒。** *已回答（使用者，2026-11-16）。* 立刻送出、封面到了再更新落選：有些伺服器會把更新顯示成第二個彈出視窗。立刻送出但不帶封面落選：第一次播放一張專輯時幾乎不會有封面
4. **同一首曲目再次出現不通知。** *已回答（使用者，2026-11-16）。* 像 hook 的 `Changed` 那樣每次開始都通知落選：單曲重複每隔幾分鐘就會通知
5. **用 runtime，而不是 hub 的 peer**（tech-lead，同 0025 決策 2）。hub 的 peer 在一被要求播放某首曲目時就看到 `current` 移動，所以快速跳過會為沒有人聽到的曲目通知
6. **未知的預留位置是錯誤**（tech-lead）。spotify-player 默默丟掉它；這裡 `app.toml` 中的打字錯誤會在啟動時被發現，與其他每個設定相同
7. **自己的匯流排連線、自己的工作執行緒**（tech-lead）。共用 MPRIS 的連線會把通知綁在 `mpris = true` 上
8. **失敗寫入記錄檔，不進訊息列**（tech-lead；issue：「nothing happens, no error」）。沒有通知伺服器的桌面是正常的，不同於設定了卻失敗的 hook
9. **通知開啟時，`play` 播放單一曲目 ID 會取得它的中繼資料**（tech-lead）。沒有它，通知就沒有標題；另一個做法，也就是這種曲目不通知，會讓 `play` 成為預設下唯一保持沉默的播放器

<a id="out-of-scope"></a>
## 不在範圍內

- 播放視窗的 `playback_format`（之後的規格；預留位置是共用的）
- 通知上的動作（下一首、暫停）、`desktop-entry` hint、緊急程度、音效
- `notify_streaming_only`（spotify-player 的：這裡沒有 Spotify Connect）
- 已開始曲目以外任何東西的通知（暫停、錯誤、登入）
- 播放器停止或關閉時關閉通知
- macOS 與 Windows 的通知系統
- 讓 2 秒的封面等待或 5 秒的呼叫限制可設定
