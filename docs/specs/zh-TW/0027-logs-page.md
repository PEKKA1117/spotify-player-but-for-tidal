[English](../0027-logs-page.md) | 正體中文

<a id="0027--logs-a-log-file-and-the-logs-page"></a>
# 0027 — 記錄：記錄檔與記錄頁

- **狀態**：implemented（2026-11-11；approved 2026-11-10，AC16–AC19 approved 2026-11-11）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#64](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/64)
- **相依於**：0001（implemented：各 crate、`Command`/`Event`、本規格取代的「logging」一列）、0005（implemented：socket、回答給單一客戶端的請求、鎖、AC14）、0006（implemented：頁面、歷史、失去連線的規則）、0008（implemented：`app.toml`、按鍵對應、按鍵說明、略過的 spotify-player 指令）、0012（implemented：`/` 及其標題）、0013（implemented：按鍵提示）、0017（implemented：佈景主題）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Logs」一節與按鍵；[`docs/config.md`](../../zh-TW/config.md) 新增 `log_level`、`OpenLogs`，並把它從略過清單中移除；[`docs/daemon.md`](../../zh-TW/daemon.md) 說明常駐程式的記錄去哪裡；以及它們的 zh-TW 副本（AC13）

<a id="context"></a>
## 背景

程式碼有 40 多個 `tracing` 呼叫（被拒絕的更新權杖、無法保留的音效卡、被重新連線的串流、損壞的狀態檔、被丟棄的客戶端），卻**沒有 subscriber**：它們每一個都什麼也不寫（0001「logging」，由 0026 的稽核發現；0010 與 0025 各自又重新發現了一次）。當發生了訊息列沒有解釋的問題時，沒有任何東西可讀。

spotify-player 每次執行會在它的快取目錄寫一個記錄檔，並且自 v0.24 起在一個頁面中顯示這次執行的記錄行（`OpenLogs`，`g o`）：一個記憶體中的緩衝區，以行偏移捲動，`ERROR` 行是紅色，`WARN` 行是黃色。本播放器在複製來的 `keymap.toml` 中略過 `OpenLogs`（0008）。tidalt 沒有記錄。

本播放器幾乎所有的記錄行都來自**播放器**這一側（驗證、音訊裝置、串流、持久化、socket）。有常駐程式時，它是與 TUI 不同的行程，因此像 spotify-player 那樣顯示 TUI 自己緩衝區的頁面，會在最需要的時候剛好是空的。

<a id="decisions-the-users-2026-11-10"></a>
### 決策（使用者的，2026-11-10）

1. **連上的 TUI 透過協定顯示播放器的記錄。** 播放器保留它最近的記錄行並回答對它們的請求，如同歌詞與裝置；客戶端不開啟播放器在磁碟上的任何東西（0005 AC14）。TUI 自己的少數記錄行（封面、按鍵對應）會合併到同一個頁面；一行的 target（`tidal_player::tui::…`）說明它來自哪裡
2. **每個播放器都在狀態目錄寫一個記錄檔**，獨立模式、常駐程式與 `play` 都一樣，因此當機時會留下可讀的東西。常駐程式也把它的記錄行寫到 stderr，在 systemd 下那就是 journal

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及由哪個驗收條件負責

1. **記錄中的祕密**：HTTP crate 在 `debug`/`trace` 記錄標頭，而 `Authorization` 標頭就是工作階段 → 不論 `log_level` 怎麼設，其他 crate 的記錄行只取 `warn` 以上（AC2）
2. **記錄檔無止境地增長** → 在固定大小時輪替，最多兩個檔案（AC4）
3. **記錄讓音樂停止**：磁碟已滿、無法寫入的目錄 → 放棄這個檔案，播放器繼續執行，緩衝區說明原因（AC4）
4. **大量記錄行讓客戶端斷線**：以事件推送的記錄行會填滿慢速客戶端的發送佇列（0005：1024 則訊息，之後丟棄）→ 記錄行絕不推送；頁面詢問它所持有的最後一行之後的記錄行，約每秒一次，而且只在它位於最上層時（AC5、AC8）
5. **閱讀時視野跳動**：使用者閱讀較舊的一行時來了新的一行 → 視野只在位於底部時才跟隨（AC9）
6. **兩個行程寫同一個檔案** → 只有持有 `player.lock` 的播放器寫檔案；客戶端不寫（AC3）

<a id="behaviour"></a>
## 行為

<a id="what-is-logged"></a>
### 記錄什麼

- **記錄行**是一個 `tracing` 事件：它的時間、等級（`ERROR`、`WARN`、`INFO`、`DEBUG`、`TRACE`）、target（模組路徑，`tidal_player::player::reserve`）與文字。文字是事件的訊息，後面接它的其他欄位，格式為 ` name=value`，依給定的順序（`could not save the refreshed session error=…`）。文字中的換行寫成空格
- `app.toml` 中的 **`log_level`**（預設 `"info"`；`"off"`、`"error"`、`"warn"`、`"info"`、`"debug"`、`"trace"`，任何大小寫）是本程式各 crate（target 為 `tidal_player`、`tidal_player_core`、`tidal_player_api`、`tidal_player_audio` 及其模組）所保留的最低等級。`TIDAL_PLAYER_LOG_LEVEL` 優先於檔案，如每個設定；不屬於這六個之一的值是錯誤的設定（以 2 結束，0008 的訊息）
- **其他 crate 的事件**（`reqwest`、`hyper`、`zbus`、`symphonia`……）只保留 `WARN` 與 `ERROR`，在除了 `"off"` 以外的每個 `log_level` 都是如此
- `"off"`：什麼都不保留，不建立也不碰任何檔案
- 每個行程讀取自己的 `log_level`：常駐程式讀常駐程式的，連上的 TUI 讀它自己的（針對它自己的記錄行）
- 使用者必須看到的東西不會只在記錄中（0001）：錯誤仍如以往送到訊息列或 stderr。沒有任何既有的訊息移動

<a id="what-the-program-logs"></a>
### 程式記錄什麼

給 40 多個既有的呼叫一個去處還不夠：它們幾乎都是對罕見失敗的警告，所以健康的執行什麼也沒記錄，而 TUI 在失敗時給使用者看的東西則完全不在任何記錄中（於手動檢查時發現，2026-11-11）。程式記錄這些行，每一行是一個帶有所示訊息與欄位的 `tracing` 事件（欄位依此順序；AC2 把它們變成文字）：

**一次執行的生命週期，在 `INFO`**（在每個保留記錄的行程中）：

| 時機 | 訊息與欄位 |
|---|---|
| 記錄被安裝 | `starting version=<x.y.z> mode=<standalone\|daemon\|play\|client> log_level=<level> config=<config dir>` |
| 播放器的輸出裝置被開啟（第一次、釋放之後、裝置變更之後） | `device opened device=<name> format=<sample format> rate=<Hz> channels=<n> bit_perfect=<true\|false>` |
| 播放器在暫停時放掉裝置（0005），或關閉它 | `device released device=<name>` |
| 一首曲目開始播放（一個項目成為正在播放的項目，包含無縫的前進） | `playing track=<id> title=<title> quality=<the stream's quality> requested=<the configured quality>` |
| 佇列用完或播放停止 | `stopped` |
| 登入遺失／恢復（0002） | `WARN login required` / `INFO login restored` |
| 播放器或 TUI 依序關閉 | `shutting down reason=<quit\|shutdown command\|signal\|player gone>` |

**在 `DEBUG`**：播放器中的 `client connected client=<n>` 與 `client disconnected client=<n>`（每秒執行一次一次性指令的狀態列每秒都會連線一次，所以這些不是 `INFO`）。

**使用者被告知了什麼，在 `WARN`**，讓在畫面上被取代的訊息仍然找得到：

| 在哪裡 | 時機 | 訊息與欄位 |
|---|---|---|
| 播放器 | 它以 `Err` 回答請求（`Reply`、`LibraryReply`、`DevicesReply`；不含 `LogsReply`） | `request failed request=<the command's or request's name> reason=<the answer's text>` |
| 播放器 | 它的快照的 `message` 變成新的文字（`Could not save the playback state: …`、失敗的 hook、開不起來的裝置） | `message text=<the text>` |
| TUI | 連線遺失、重新連線失敗，或版本不同 | `disconnected reason=<the text shown>`；重新連線是 `INFO reconnected` |
| TUI | 啟動通知（`keymap.toml`/`theme.toml` 的項目被略過，0008、0017） | `notice text=<the text>` |
| TUI | 匯出失敗（AC14 的失敗） | `export failed reason=<reason>` |

- 獨立模式的 TUI 是同一個行程中的播放器加 TUI，只有一份記錄，所以失敗的請求只出現一次（播放器的那一行）；TUI 自己的記錄行只有表中的那些
- **絕不記錄**，在這些行或任何其他行中：權杖、`Authorization` 標頭、串流或封面的 URL（允許串流的主機名稱）、帳號的名稱、電子郵件或 ID。請求的名稱是變體的名稱（`LoadQueue`、`Page`），絕不是它的酬載
- **panic**（0001 的 hook，在每個保留記錄的行程中）：在終端機被還原、報告被印出之前，一行 `ERROR` 的 `panic location=<file:line:column> message=<the panic's message>` 會直接寫入行程的記錄檔（播放器的；客戶端沒有）與它的緩衝區，不經過 `tracing`，也不等待鎖：若記錄正忙（panic 發生在它寫入時），這一行就被略過。hook 接著以同樣的順序做它原本做的事

<a id="who-keeps-what"></a>
### 誰保留什麼

| 行程 | 緩衝區（最後 1000 行） | `tidal-player.log` | stderr |
|---|---|---|---|
| 獨立模式的 TUI | 是 | 是 | 否（終端機是 TUI 的） |
| `daemon` | 是 | 是 | 是 |
| `play` | 是 | 是 | 否（它的輸出是 0003 的） |
| 連上的 TUI | 是，它自己的記錄行 | 否 | 否 |
| 其他每個指令（`login`、`playback …`、`playlist …`、`devices`……） | 否 | 否 | 否 |

- **緩衝區**保留行程最後的 **1000** 行；最舊的先被丟棄。播放器在它的生命期內把它的記錄行編號為 `seq` 1、2、3、……
- 常駐程式的 **stderr** 行是 `LEVEL target: text`，沒有時間（journal 已有）

<a id="the-log-file"></a>
### 記錄檔

- `tidal-player.log` 位於狀態目錄（`$TIDAL_PLAYER_STATE_DIR`，否則 `$XDG_STATE_HOME/tidal-player`，再否則 `~/.local/state/tidal-player`，0009 的 `playback.json` 所在處），以模式 `0600` 建立，以附加方式寫入：每個記錄行一行，在發生時寫入，

  ```
  2026-11-10T21:04:05.123+08:00  WARN tidal_player::player::reserve: cannot reserve card 1: busy
  ```

  時間為帶有 UTC 偏移與毫秒的本地時間，等級靠右對齊到 5 欄
- **輪替**：當檔案在啟動時大於 **1 MiB**，或下一行會使它超過時，它會被重新命名為 `tidal-player.log.old`（取代已存在的那個），並開始一個新檔案。所以記錄最多佔用約 2 MiB，而前一次執行的結尾一定在兩個檔案之一
- 無法建立、寫入或輪替的檔案（沒有目錄、唯讀、磁碟已滿）會**在這次執行的其餘時間被放棄**：一行 `WARN` 的 `cannot write <path>: <reason>` 進入緩衝區（以及常駐程式的 stderr），而播放器就像沒有檔案那樣執行。這不是錯誤結束
- `logout` 留下記錄檔（它們不含工作階段）

<a id="talking-to-the-player"></a>
### 與播放器溝通

一個自己的請求，如 `Devices`（0014），只回答給提出請求的客戶端：

- `ClientMessage::Logs { id, after: Option<u64> }` → `ServerMessage::LogsReply { id, result: Result<LogBatch, String> }`
- `LogBatch { lines: Vec<LogLine>, off: bool }`：緩衝區中 `seq > after` 的記錄行（`None` 則是全部），最舊的在前；播放器的 `log_level` 是 `"off"` 時 `off` 為 `true`
- `LogLine { seq: u64, at: SystemTime, level: LogLevel, target: String, text: String }`（`tidal_player_core::log`；`at` 在傳輸上為自 Unix epoch 起的毫秒；`LogLevel` 是那五個等級）
- 大於最新 `seq` 的 `after`（播放器重新啟動過）會以每一行回答，如 `None`
- 處理 `Logs` 請求本身不記錄任何東西
- 獨立模式的 TUI 以同樣的方式透過它的行程內客戶端詢問（0001）

<a id="the-page"></a>
### 頁面

| 開啟方式 | 顯示 | 標題列 |
|---|---|---|
| 從任何地方按 `g o`（`OpenLogs`） | 播放器的記錄行與 TUI 自己的，最舊的在最上面，最新的在最下面 | `Logs` |

- 歷史（0006）中的一個頁面：被推到最上層；已在最上層時，按鍵什麼也不做；返回會離開它
- **記錄行**：每一行畫成 `HH:MM:SS LEVEL target: text`（本地時間；等級靠左對齊到 5 欄），依視窗寬度以顯示寬度換行。連上的 TUI 依時間合併播放器的記錄行與它自己的（時間相同時：播放器的在前）；獨立模式的 TUI 只有一個緩衝區，那就是兩者。頁面在合併之後保留最後 1000 行
- **顏色**：`ERROR` 行以佈景主題的 `error` 元件畫出，`WARN` 行以調色盤的 `yellow` 畫出，其餘用畫面的樣式
- **詢問**：頁面在最上層且客戶端已連線時，它詢問它所持有的最後一個 `seq` 之後的記錄行：回到最上層時一次，之後在每個回答的 **一秒** 後（絕不同時有兩個請求進行中）。不在最上層時什麼都不問；回到它時立刻詢問。在新的 `Welcome`（重新連線）之後，它忘掉播放器的記錄行，並以 `after: None` 詢問
- **跟隨**：頁面開啟時顯示最新的記錄行，最後一行在視窗的最後一列，並且*跟隨*：新的記錄行會捲動視野。清單移動鍵（`j`/`k`/`down`/`up`、`C-f`/`C-b`/`page_down`/`page_up`、`g g`/`G`）以記錄行為單位捲動，限制在這些行之內。不在底部的視野不跟隨：記錄行到來時它停在它所顯示的列上（而舊的記錄行被丟棄時，只有它自己最上面那一列被丟棄才會移動），標題顯示 `Logs · scrolled`。再次到達底部（`G`，或往下捲到那裡）或 **`Esc`**（`ClosePopup`，沒有開啟彈出視窗或輸入提示，也沒有保留的篩選時）會再次跟隨
- **篩選**（0012）：`/` 像篩選清單那樣篩選記錄行：只有畫出的文字包含篩選字串（不分大小寫）的行會留下；標題加上 0012 的各部分（`Logs · /reserve · 3 matches`）；`Enter` 保留篩選，`Esc` 清除它（先清保留的篩選，再跟隨，各按一次）；沒有任何東西符合的篩選顯示 0012 的 `No rows match "…"`。符合的新記錄行在到來時就會出現
- **視窗中的訊息**：第一個回答之前顯示 `Loading…`；沒有記錄行時顯示 `No log lines yet`；播放器的 `off` 為 `true` 且 TUI 沒有自己的記錄行時顯示 `Logging is off: set log_level in app.toml`；失敗為 `Could not load the log: <reason>`（已顯示的記錄行留著，頁面一秒後再問）；失去連線時，原樣顯示 0006 失去連線的訊息，已顯示的記錄行留著
- 作用於列的按鍵（`Enter`、`Z`、對列的動作彈出視窗、`s …`、`d …`、`r`）在記錄頁上什麼也不做；全域按鍵與別處一樣有作用

<a id="exporting"></a>
### 匯出

- 記錄頁上的 **`e`**（`ExportLogs`）把頁面持有的記錄行寫到一個新檔案：有保留的篩選時，只有符合的行（正在輸入篩選時，`e` 是它的一個字母）；是全部，不只是視野中的列；播放器的與 TUI 自己的，依頁面的順序
- 檔案是 `tidal-player-logs-YYYYMMDD-HHMMSS.log`（本地時間），位於快取目錄（`$TIDAL_PLAYER_CACHE_DIR`，否則 `$XDG_CACHE_HOME/tidal-player`，再否則 `~/.cache/tidal-player`；缺少時建立），模式 `0600`，每個記錄行一行，採用記錄檔的行格式（完整的帶偏移的本地時間）。名稱已被佔用時（一秒內兩次匯出），在 `.log` 之前加上 `-2`、`-3`、……；匯出絕不覆寫檔案
- 由 TUI 寫入（客戶端行程，連上的或獨立模式的）：它是頁面的內容，不是播放器的檔案，所以失去連線時也能用已顯示的記錄行運作
- 訊息列顯示 `Exported 42 log lines to <path>`；失敗為 `Could not export the log: <reason>`（最終名稱下不會留下寫到一半的東西）；沒有記錄行可寫時為 `No log lines to export`，而且沒有檔案
- 在記錄頁以外的任何地方，`ExportLogs` 什麼也不做

<a id="keys"></a>
### 按鍵

| 指令 | 預設按鍵 | 作用於 | 說明文字 |
|---|---|---|---|
| `OpenLogs` | `g o` | 任何地方 | `open the logs page` |
| `ExportLogs` | `e` | 記錄頁 | `export the shown log lines to a file` |

`ExportLogs` 是本播放器自己的指令。`OpenLogs` 離開 0008 略過的 spotify-player 指令清單：綁定了它的複製來的 `keymap.toml`，現在就真的綁定它。按鍵說明（0008）與按鍵提示（0013）像列出任何指令一樣列出它。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 協定：`ClientMessage::Logs`、`ServerMessage::LogsReply`、`LogBatch`、`LogLine` 與 `LogLevel` 可透過 serde 與 socket 的封包格式來回轉換（`at` 為自 epoch 起的毫秒）；未知的等級解碼失敗
- **AC2** — 擷取（純邏輯，以等級與 target 為輸入）：一行被保留，若且唯若 `log_level` 不是 `off`，而且要嘛 target 是本程式的某個 crate（或其某個模組）且等級在 `log_level` 或以上，要嘛它是其他 crate 的且等級為 `WARN` 或 `ERROR`（以每個 `log_level`、兩種 target、以及像 `tidal_player_extra` 這樣長得像的 target 為列的表）。文字是訊息，接著依序以 ` name=value` 列出欄位，換行變成空格（表）
- **AC3** — 緩衝區與角色：緩衝區依序保留最後 1000 行，先丟棄最舊的，從 1 起無間斷地編號 `seq`，而 `after(n)` 回傳 `seq > n` 的保留行（`None` 與超過最新的 `n` 則是全部）。每個執行模式安裝「誰保留什麼」表所說的東西（以模式為列的表：緩衝區、檔案、stderr）
- **AC4** — 檔案：記錄行以上述格式附加（以等級與固定的時間和偏移為列的表）；檔案以模式 `0600` 建立；啟動時超過 1 MiB 的檔案，以及會超過 1 MiB 的一行，會輪替到 `tidal-player.log.old`，取代舊的；無法寫入的路徑在緩衝區中恰好一行 `cannot write <path>: <reason>` 之後被放棄，之後的記錄行仍進入緩衝區，而且不會 panic 或結束；`log_level = "off"` 不建立檔案
- **AC5** — 處理：`Logs` 請求只回答一次，只回答給提出請求的客戶端，帶有 `after` 之後的記錄行與播放器的 `off`；其他客戶端什麼都收不到；沒有任何 `Event` 攜帶記錄行；回答不會對緩衝區新增記錄行
- **AC6** — 設定：`log_level` 從 `app.toml` 與 `TIDAL_PLAYER_LOG_LEVEL` 以通常的優先順序讀取，任何大小寫；預設 `info`；其他值以 2 結束，帶著指出 `log_level` 的 0008 的訊息；`examples/app.toml` 帶有預設值
- **AC7** — 開啟：`g o` 推入記錄頁並送出一個 `Logs { after: None }`；已在最上層時，什麼也不做；返回會彈出它；`OpenLogs` 帶著它的文字出現在預設按鍵對應、按鍵說明與按鍵提示中，不再被回報為不支援，並可被重新綁定或解除綁定
- **AC8** — 詢問：回答之後，下一個請求在一秒後到期，帶著收到的最新 `seq`，而且在有請求進行中、頁面不在最上層，或失去連線時不送出；回到最上層時立刻詢問；帶有另一個 `id` 的回答被忽略；`Welcome` 會清除播放器的記錄行，而下一個請求帶 `after: None`；失敗的回答保留記錄行並在一秒後再問
- **AC9** — 顯示狀態（純邏輯）：依時間合併的記錄行，時間相同時播放器的在前，上限 1000；在底部時跟隨；每個移動鍵的捲動，受限制；離開底部後不跟隨，視野在記錄行到來與被丟棄時守住它的列；在底部與 `Esc` 時再次跟隨；標題（`Logs`、`Logs · scrolled`，帶有篩選的各部分）；每個狀態的視窗訊息；列按鍵無作用
- **AC10** — 篩選：記錄頁上的 `/` 依畫出的文字篩選記錄行，不分大小寫，帶有 0012 的輸入、`Enter`、`Esc` 與標題；`Esc` 先清除保留的篩選才再次跟隨；到來的記錄行也被篩選
- **AC11** — 繪製：在 80 × 24 與 50 × 20，具有每個等級的記錄（顏色：`error` 元件、調色盤黃色）、換行的長行與 CJK 行、捲動後的標題、篩選，以及每個視窗訊息的快照；任何大小都不會 panic
- **AC12** — 端對端：對以狀態目錄啟動的常駐程式，連上的客戶端的 `Logs` 請求以常駐程式的記錄行回答，`tidal-player.log` 存在、模式為 `0600` 並持有它們，常駐程式的 stderr 持有它們但沒有時間，而且客戶端行程沒有在狀態目錄中建立任何檔案
- **AC13** — 文件：`docs/tui.md`（頁面、它的按鍵、跟隨、篩選、匯出）、`docs/config.md`（`log_level`、`OpenLogs`、不含它的略過清單）、`docs/daemon.md`（記錄檔、journal）、`README.md`、它們的 zh-TW 副本、`examples/app.toml` 與 `examples/keymap.toml` 與本規格相符；0001 的「logging」一列與 0008 的略過清單指向這裡
- **AC14** — 匯出，模型（純邏輯）：在記錄頁上，`e` 送出一次持有的記錄行的匯出，由保留的篩選篩選，依頁面的順序，不論捲動位置，也不論有沒有連線；沒有這樣的行時它不送出任何東西，而訊息列顯示 `No log lines to export`；在任何其他頁面它什麼也不做；結果設定訊息列（`Exported N log lines to <path>`，一行時用 `1 log line`，或 `Could not export the log: <reason>`）；`ExportLogs` 在預設按鍵對應與按鍵說明中（在記錄頁的那一節），並可被重新綁定或解除綁定
- **AC15** — 匯出，檔案：匯出寫到快取目錄中，名稱如上（針對給定的本地時間），模式 `0600`，採用記錄檔的行格式；被佔用的名稱得到下一個 `-N`；目錄缺少時建立；無法寫入的目錄給出失敗，而且不留下檔案
- **AC16** — 一次執行的生命週期：「一次執行的生命週期」的每一列，每次出現都記錄一次，帶著它的等級、訊息與依序的欄位，而兩行 `DEBUG` 在 `DEBUG` 時記錄（表：每一列，接縫的輸入 → 被擷取的事件）；`playing` 每個開始的項目記錄一次，包含無縫的前進，而在從暫停繼續或跳轉時不記錄；這些測試所擷取的行中沒有任何一行包含權杖、URL 或帳號欄位（測試資料帶有可辨認的那些）
- **AC17** — 使用者被告知了什麼：播放器的每個 `Err` 回答記錄一行 `request failed`，帶有請求的名稱與原因，沒有酬載（以 `Reply`、`LibraryReply`、`DevicesReply` 為列的表；`LogsReply` 不記錄任何東西，AC5）；快照的 `message` 在它的文字改變時記錄一次，維持不變時不再記錄；TUI 的各列依表所說記錄；獨立模式的 TUI 對失敗的請求記錄一次
- **AC18** — panic：針對某個位置與訊息的那一行的文字（表：`&str` 與 `String` 酬載，其他酬載 → `message=<non-string panic>`）；安裝了記錄時，hook 在 `restore` 與前一個 hook 執行之前，依此順序把它寫入檔案與緩衝區（0001 AC10 仍然成立）；panic 的執行緒持有記錄的鎖時，hook 略過那一行，仍然還原並回報，而且不阻塞
- **AC19** — 文件：`docs/tui.md` 的「What is logged and where」依群組（一次執行的生命週期、使用者被告知了什麼、panic）列出這些行，並說明什麼絕不記錄；它的 zh-TW 副本相符；`CLAUDE.md` 的 Logs 項目說明記錄會記下一次執行的生命週期與顯示過的失敗

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 兩次請求之間有超過 1000 行的暴增 | 頁面取得最新的 1000 行；其餘只在檔案中。不畫出標記 |
| 頁面開啟時播放器重新啟動 | 失去連線的訊息；重新連線（`Welcome`）時，舊播放器的記錄行被丟棄，並詢問新播放器的 |
| 舊的常駐程式與新的 TUI（或反過來） | 0005 的版本訊息；絕不對另一個版本的播放器送出 `Logs` |
| 比視窗長的行 | 換行。捲動以記錄行為單位：視野最上面那一列永遠是某一行的第一列。跟隨時，不論各行如何換行，最新一行的最後一列都在視窗的最後一列 |
| 含有控制字元的行 | 不含它們地畫出（0012 對輸入文字的規則適用於畫出的文字） |
| 對視窗來說太小的終端機 | 0006 的規則：不會 panic（AC11） |
| 狀態目錄不存在 | 如 0009 那樣建立；若失敗，檔案被放棄（AC4） |
| 時鐘倒退 | 記錄行維持播放器給的 `seq` 順序；只有與 TUI 自己的記錄行的合併使用時間 |
| panic | 0001 的 panic hook 不變；它不會向記錄寫任何東西 |
| 忙碌的播放器上的 `log_level = "trace"` | 其他 crate 維持在 `WARN`；檔案輪替更頻繁。其他一切不變 |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的驗收條件命名（`ac4_…`）。紅燈是針對存根型別與函式、其本體為存根（沒有 `todo!()`、沒有編譯錯誤）的斷言失敗。檔案測試寫入 `tempfile` 目錄；時間、偏移與大小上限是純邏輯部分的參數，所以沒有任何測試會等待，或透過真實的時鐘寫入一個百萬位元組。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | 來回轉換；解碼 | 存根 `Serialize` 寫出 `null` |
| AC2 | `crates/core/src/log.rs` :: `ac2_kept` (table), `ac2_text` (table) | 哪些行被保留；文字 | 存根保留每一行；存根文字只有訊息 |
| AC3 | `crates/core/src/log.rs` :: `ac3_buffer` (table) + `crates/app/src/logging.rs` :: `ac3_roles` (table) | 順序、上限、`seq`、`after`；各模式的 sink | 存根緩衝區什麼都不保留；存根角色讓每個模式都有三個 sink |
| AC4 | `crates/app/src/logging.rs` :: `ac4_file_format` (table), `ac4_rotation`, `ac4_gives_up_once`, `ac4_off_creates_nothing` | 行格式與模式；輪替；一次警告然後沉默 | 存根寫入器什麼也不寫 |
| AC5 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new row) + `crates/app/src/player/runtime.rs` :: `ac5_logs_reply` | 一個回應，給送出者；`after` 之後的記錄行；`off`；緩衝區不變 | 存根回答空的批次 |
| AC6 | `crates/app/src/settings/tests.rs` :: `ac26_settings_precedence` (new rows) + `crates/app/src/config/tests.rs` :: `ac10_app_toml` (new rows) + `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` | 優先順序、大小寫、預設值、錯誤的值；範例 | 存根忽略該設定 |
| AC7 | `crates/core/src/ui/logs/tests.rs` :: `ac7_open_logs` (table) + `crates/core/src/ui/keymap/tests.rs` :: `ac7_logs_keys`, `ac12_unsupported_notice` (rows moved to another skipped command) + `crates/core/src/ui/help/tests.rs` :: `ac7_help_keys_and_texts` (new row) | 推入、請求、歷史；預設按鍵、說明、提示、解除綁定 | 存根讓 `OpenLogs` 維持不支援 |
| AC8 | `crates/core/src/ui/logs/tests.rs` :: `ac8_asking` (table) + `crates/core/src/ui/browse/tests.rs` :: `ac16_disconnected_and_login` (new rows) | 請求何時到期、帶著什麼 | 存根每個 tick 都詢問 |
| AC9 | `crates/core/src/ui/logs/tests.rs` :: `ac9_display_state` (table) | 合併、上限、跟隨、捲動、標題、訊息、無作用的按鍵 | 存根永遠跟隨 |
| AC10 | `crates/core/src/ui/logs/tests.rs` :: `ac10_filter` (table) | 篩選後的記錄行、標題、`Esc` 的順序 | 存根顯示每一行 |
| AC11 | `crates/app/src/tui/render.rs` :: `ac11_logs_80x24` (one snapshot per row), `ac11_logs_narrow_50x20`, `ac17_no_panic_any_size` (new page) | `contains` + 快照 | 存根繪製畫出佇列 |
| AC12 | `crates/app/tests/daemon.rs` :: `ac12_daemon_log` | 回應、檔案與模式、stderr、客戶端沒有建立任何東西 | 存根沒有安裝 subscriber，所以回應是空的 |
| AC13 | — reviewed at acceptance | 文件與本規格相符、有連結、zh-TW 同步 | — |
| AC14 | `crates/core/src/ui/logs/tests.rs` :: `ac14_export` (table) + `crates/core/src/ui/keymap/tests.rs` :: `ac14_export_key` | 匯出的記錄行；訊息；在別處無作用；按鍵與說明 | 存根匯出每一行，沒有篩選 |
| AC15 | `crates/app/src/tui/logs_export.rs` :: `ac15_export_file` (table) | 名稱、模式、內容、`-N`、建立的目錄、失敗 | 存根什麼也不寫 |
| AC16 | `crates/app/src/logging/events.rs` :: `ac16_run_life` (table), `ac16_playing_once_per_entry`, `ac16_no_secrets` | 每一列的等級、訊息、欄位；每個項目一次；沒有祕密 | 存根函式什麼也不記錄 |
| AC17 | `crates/app/src/logging/events.rs` :: `ac17_request_failed` (table), `ac17_message_once`, `ac17_tui_rows` (table) + `crates/app/src/player/runtime.rs` :: `ac17_err_answers_are_logged` | 每個 `Err` 回答一行，名稱與原因；每個文字一次；TUI 的記錄行；獨立模式下一次 | 存根什麼也不記錄 |
| AC18 | `crates/app/src/logging.rs` :: `ac18_panic_line` (table), `ac18_panic_skips_when_busy` + `crates/app/tests/panic_hook.rs` :: `ac18_panic_is_logged_first` | 文字；不阻塞；檔案與緩衝區，然後還原，然後報告 | 存根的行是空的；存根 hook 什麼也不寫 |
| AC19 | — reviewed at acceptance | 文件與本規格相符、zh-TW 同步 | — |

在驗收時以真實帳號手動檢查（結果寫在 PR 的手動檢查中）：在獨立模式的 TUI 與連上常駐程式的 TUI 中按 `g o`，都顯示播放器的記錄行；第二個連上的 TUI 顯示相同的內容；跟隨時新的記錄行在約一秒內出現，而且不會移動已捲動的視野；`/` 縮小範圍，而 `e` 恰好把縮小後的記錄行寫入訊息列所指出的檔案；健康的啟動與播放的曲目在 `info` 顯示 `starting`、`device opened` 與 `playing`，而 `log_level = "debug"` 加上客戶端的記錄行；`"off"` 顯示該訊息；失敗的請求（不存在的曲目 ID）顯示 `request failed`；`journalctl --user -u tidal-player`（維護者的，手動）顯示常駐程式的記錄行；在 `debug` 時，截圖或檔案中都沒有出現權杖或帳號名稱。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`log`（新：`LogLevel`、`LogLine`、`LogBatch`、緩衝區、保留什麼、行格式）、`protocol`（那兩個訊息）、`ui`（記錄頁：`PageKind::Logs`、詢問、跟隨、篩選）、`ui::keymap`（`OpenLogs`、`ExportLogs` 及其預設值）。沒有 I/O，沒有新的相依套件
- `tidal-player`（執行檔）：`logging.rs`（新，最上層：每個模式都使用它）：餵給緩衝區、檔案與 stderr 的 `tracing` subscriber，以及 panic 那一行；`logging/events.rs`：「程式記錄什麼」的每一行一個函式，在接縫處呼叫，因此每一行的等級、訊息與欄位都在同一個地方測試；`settings.rs`/`config.rs`（`log_level`）；`player/runtime.rs` 與 `ipc/`（處理 `Logs`）；`client.rs`（詢問）；`tui/`（繪製、`HintTimer` 旁的輪詢計時器、匯出檔案用的 `logs_export.rs`）。新的相依套件，宣告在根 manifest：`tracing-subscriber`（registry 與 layer，預設功能關閉）與一個處理本地時間的 crate
- `tidal-player-api`、`tidal-player-audio`：不變（它們的 `tracing` 呼叫現在會到達 subscriber）

<a id="out-of-scope"></a>
## 不在範圍內

- 印出播放器記錄的一次性指令（對檔案用 `tail -f`，或用 `journalctl`，就能做到）
- 結構化（JSON）記錄、各模組的等級、`RUST_LOG`、緩衝區或檔案大小的設定
- 記錄頁上的游標、選取，或複製到剪貼簿（剪貼簿是 #57 的）；選擇匯出的路徑或名稱
- 給記錄等級用的新佈景主題元件
- 改寫或移除在本規格之前就存在的記錄呼叫；連上的 TUI 的記錄檔（它的記錄行活在它的緩衝區中：`e` 匯出它們）
