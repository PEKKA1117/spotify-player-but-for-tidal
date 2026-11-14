[English](../0004-queue-and-controls.md) | 正體中文

<a id="0004--queue--playback-controls"></a>
# 0004 — 佇列與播放控制

- **狀態**：implemented（2026-10-07）
- **負責人**：tech-lead（主要 session）
- **相依於**：0001（implemented）、0002（implemented）、0003（approved；其引擎在此擴充，見「引擎新增項目」）
- **使用者文件**：[`docs/playback.md`](../../zh-TW/playback.md)（擴充）與全新的 [`docs/tui.md`](../../zh-TW/tui.md)（由本規格的實作撰寫，AC24）

<a id="context"></a>
## 背景

0003 只播放一首曲目。本規格加入把它變成播放器的各項功能：佇列、播放／暫停／跳轉／下一首／上一首、隨機播放、重複播放、自動前進（透過 0003 的 `Preload` 做到無縫播放）、音量，以及第一個真正的 TUI 畫面（正在播放列與佇列）。它建立規格 0001 預留的播放器狀態機 `tidal_player_core::player`（`PlayerState`、`PlayerInput`、`PlayerEffect`、`update`），並在 `protocol::Command`／`Event` 中填入播放相關的訊息。播放器與 TUI 在同一個行程中執行（0001 的*獨立模式*），也在 `tidal-player play` 之後執行；常駐程式與其客戶端屬於 0005，這裡的任何內容都不得假設客戶端與播放器共用記憶體。

瀏覽（音樂庫、搜尋）隨 0006／0007 加入。在那之前，佇列由 Tidal 連結或 ID 填入，從命令列給定或貼進 TUI（決策 1）。佇列播完時，**自動播放**可以接著播放 Tidal 建議的曲目，如同 Tidal 的各個應用程式（決策 4）。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 `internal/ui/model.go`、`internal/ui/queue.go`、`internal/player/mpv.go`，以及它們的 `fix` commit 歷史：

- 佇列、目前索引、隨機播放與「正在前進」旗標都放在 BubbleTea 的 UI model 中；播放器 goroutine 一次只播放一個 URL，結束時關閉一個「done」channel
- 隨機播放有兩種模式：*Random*（每次前進時挑一個尚未播放的索引，並以已播放索引堆疊支援*上一首*）與 *Fisher–Yates*（預先打亂的副本，原順序另外保留）。沒有重複播放
- 上一首是「游標 − 1」，或從隨機播放歷史堆疊彈出
- 音量是播放器中的浮點數，以縮放每個取樣的方式套用；UI 預設為 50 %（`e925071`）
- 按鍵（spotify 預設組）：`Space` 播放／暫停，`n`/`p` 下一首／上一首，`>`/`<` 跳轉 ±10 秒，`+`/`-` 音量 ±5 %，`C-s` 隨機播放，`z` 佇列

<a id="what-went-wrong-there-and-the-criterion-that-covers-it-here"></a>
### 當時出了什麼問題，以及這裡由哪個條件涵蓋

1. **跳過會在播放清單中連鎖發生**（`5e0e06b`）：舊曲目的「done」訊息在新曲目開始之後才被處理，觸發了又一次自動前進。當時在 UI 中以世代計數器修正 → 這裡每個屬於曲目的引擎事件都帶有啟動該曲目的指令的標記，播放器忽略過時的事件（AC6、AC14）
2. **錯誤看起來像曲目結束**，所以 UI 自動前進到同一個壞掉的狀態，一首接一首（`2afa977`，以及 0003「背景」第 2 點）→ 0003 AC16 讓 `Error` 與 `TrackEnded` 分開；這裡由播放器依錯誤種類決定要跳過或停止（AC7）
3. **緊密的跳過迴圈**：每首曲目都失敗時，狂打 API 而得到 `429`（`f6b90be` 加入跳過無法播放的曲目，`fac01ca` 加入 2 秒延遲以防迴圈）→ `429` 或任何暫時性錯誤絕不跳過，連續失敗達到一定次數後即停止跳過（AC7）
4. **跳過沒有中斷目前的曲目**（`efbf527`）：`PlayNext` 只是把 URL 放進佇列，串流迴圈從不去看它 → `Next` 送出 `Play`，由 0003 的引擎立即處理（AC1）
5. **客戶端把常駐程式已隨機播放的佇列再隨機播放一次**，顯示的順序與實際播放的不同（`fb4721c`，issue #19–#21）→ 隨機播放只存在於播放器；快照以播放順序攜帶佇列，客戶端照送來的樣子顯示（AC3、AC22）
6. **每首曲目開頭的進度條都是 100 %**（`78f841f`）：長度在設定之前就被讀成 0 → 長度隨佇列項目的中繼資料一起來，長度未知時不畫進度條，而不是畫滿（AC23）
7. **音量悄悄破壞了位元完美**：取樣被縮放，而 UI 仍然說「bit-perfect」，且預設音量是 50 % → 預設是 100 %，增益為 1 時取樣原封不動（逐位元相同，AC15），其他任何音量都會清除 `bit_perfect` 並附上原因（AC9；0003「不在範圍內」）
8. **播放清單中的影片被當成曲目排進佇列**：tidalt 從 `/v1/playlists/{uuid}/tracks` 載入播放清單，該端點會把音樂影片與曲目一起回傳，且沒有類型欄位（見「事實」），所以影片項目被當成曲目排進佇列並送去做曲目串流請求（由程式碼推斷，並無使用者回報）。tidalt 在那裡的 `limit=1000` 也可能被拒絕，如同 `/items` 拒絕它（未驗證）→ 改用 `/items`，只取 `type == "track"`，每頁 100 筆（AC17）
9. **從清單播放時只播放那一首曲目**（`59c610d`）：沒有東西被載入佇列，所以自動前進無從接續 → 載入清單一律載入整份清單，並從所選項目開始（AC1）

<a id="behaviour"></a>
## 行為

<a id="the-queue"></a>
### 佇列

佇列是由**項目**組成的有序清單。每個項目包含一首曲目（ID、標題、藝人、專輯、已知時的長度、是否可串流）、是否由自動播放**建議**（見下）、以及一個在播放器存活期間唯一的**項目 ID**，因此同一首曲目可以排入兩次，仍能被明確指定。其中一個項目是**目前**項目（佇列為空時則沒有）。

佇列有兩種順序：**原始**順序（載入與編輯後的順序）與**播放順序**（關閉隨機播放時與原始順序相同）。下一首、上一首、自動前進與 TUI 都使用播放順序。快照只送出播放順序。

| 操作 | 效果 |
|---|---|
| 載入*曲目*，從第 *i* 個開始 | 取代佇列。第 *i* 個項目成為目前項目並從頭播放。開啟隨機播放時，第 *i* 個項目排在播放順序的最前面，其餘打亂 |
| 將*曲目*加為下一首 | 依給定順序，在兩種順序中都插入目前項目之後（佇列為空：成為佇列，第一首成為目前項目，不開始播放） |
| 將*曲目*加到尾端 | 附加到兩種順序的尾端 |
| 移除*項目* | 從兩種順序中移除。在播放中或暫停時移除目前項目，其後續行為與**下一首**相同（沒有下一首則停止）；已停止時，下一個項目（或沒有）成為目前項目 |
| 清除 | 移除除目前項目以外的所有項目，目前項目繼續播放 |
| 播放*項目* | 使它成為目前項目並從頭播放 |

<a id="playback-state"></a>
### 播放狀態

`Stopped` → （`Loading` → `Playing` ⇄ `Paused`），其中 `Buffering` 是 `Playing` 的子狀態，在引擎回報時出現（0003 AC23）。

- **Loading**：串流正在解析與開啟；尚未播放任何聲音。位置是起始位置
- **播放／暫停**（`TogglePause`）：Playing → Paused；Paused → Playing；有目前項目的 Stopped → 從頭播放；Loading → 串流繼續載入但被保留：只有再切換一次才會開始，因此在載入期間暫停絕不會漏出一小段聲音
- **下一首**：依下方的重複播放表，播放播放順序中目前項目之後的項目（暫停時也一樣）。若沒有，播放停在目前項目的 0:00
- **上一首**：若位置**超過重新開始門檻**（預設 3 秒，是一項設定；`0` 表示上一首一律往前一個），就跳轉到 0:00；否則播放播放順序中目前項目之前的項目（重複播放 *queue* 時會從第一個繞到最後一個）；在沒有繞回的第一個項目上，則跳轉到 0:00
- **跳轉**依 ± 跳轉間隔（預設 5 秒，是一項設定；`SeekBy`）或跳到指定位置（`SeekTo`）：下限為 0:00；目標位置等於或超過結尾會結束該曲目，之後如同自然結束般自動前進。維持 Playing 或 Paused。Loading 時，目標成為起始位置。Stopped 時忽略
- **自動前進**：曲目自然結束時（引擎的 `TrackEnded`，或無縫的 `Transitioned`），依重複播放表播放下一個項目

<a id="shuffle-and-repeat"></a>
### 隨機播放與重複播放

- **隨機播放**（`ToggleShuffle`）：*開啟*時把目前項目放在播放順序的最前面，並打亂其他所有項目（Fisher–Yates，種子由執行階段交給播放器；測試時固定）。*關閉*時還原原始順序；兩種情況下目前項目都繼續播放。隨機播放期間加入的項目依佇列表所述放在兩種順序中的位置，不會被洗進去。只有單一隨機播放模式（捨棄 tidalt 的 *Random* 模式：播放順序固定，上一首不需要歷史堆疊）
- **重複播放**（`CycleRepeat`）：`off` → `queue` → `track` → `off`

| 重複播放 | 目前曲目自然結束 | `Next` | 最後一個項目結束（播放順序） |
|---|---|---|---|
| `off` | 下一個項目 | 下一個項目 | 自動播放開啟：建議的曲目（見下）；否則，或沒有建議時，停在最後一個項目的 0:00 |
| `queue` | 下一個項目 | 下一個項目 | 第一個項目（相同的播放順序；不重新隨機播放） |
| `track` | 同一個項目再來一次 | 下一個項目（同 `queue`） | —（曲目重複） |

<a id="autoplay"></a>
### 自動播放

當**自動播放**開啟（`ToggleAutoplay`；預設關閉，是一項設定）且重複播放為 `off` 時，佇列不會結束：當最後一個項目（依播放順序）到達預先載入點（剩 30 秒，或長度未知時為 `Started`），播放器會要求以該項目的曲目為種子的建議（效果 `FetchSuggestions { seed, tag }`）。結果會扣除佇列中已有的曲目後，以**建議**項目的身分附加到佇列，其中第一首照常預先載入，因此轉換是無縫的。建議項目之後就是普通項目（可以播放、移除、隨機播放），TUI 會標示它們。重複播放 `queue` 或 `track` 優先：自動播放只在佇列本來會停止的地方作用。

- 每個種子項目最多一個請求；關閉自動播放會丟棄進行中請求的結果，再次開啟後在預先載入點會重新要求
- 發生錯誤，或沒有新曲目：不附加任何東西，佇列如同自動播放關閉時一樣停在尾端，訊息為 `Autoplay: no suggestions (<reason>)`。絕不在迴圈中重試
- 建議來自 `GET {api_base}/tracks/{seed}/radio?countryCode=…&limit=100`：一個請求，不分頁（Tidal 上限為 100 首曲目，種子排第一）。該曲目的 mix（`mixes.TRACK_MIX`）以相同順序回傳相同的 100 首曲目（見「事實」），因此使用只需要曲目 ID 的電台。`404`（`Track radio cannot be generated…`）視為「沒有建議」

<a id="gapless-and-preloading"></a>
### 無縫播放與預先載入

當目前曲目剩 **30 秒或更少**（長度未知時則是一開始播放就算），且依重複播放表存在下一個項目時，播放器會解析該項目的串流並對引擎送出 `Preload`。格式相符時，引擎隨後會無縫轉入它（0003 AC17）。若之後下一個項目改變了（佇列編輯、隨機播放、重複播放），播放器會解析並預先載入新的那個，若已沒有下一個項目則送出 `CancelPreload`。預先載入的解析失敗，在該曲目本應開始播放之前不會回報：屆時播放器把它當成該項目的失敗處理（見下）。

<a id="failures"></a>
### 失敗處理

| 失敗的是什麼 | 範例 | 播放器的處理 |
|---|---|---|
| 只有這首曲目 | `NotFound`、`NotAvailable`、`PreviewOnly`、`Unsupported`（解析）；`Decode`、`Unsupported`（引擎） | 顯示 0003 的訊息，然後如同自動前進般繼續（重複播放 `track` 也會繼續）。連續失敗 **5** 次，或項目數更少時失敗項目數那麼多次之後，停在失敗的項目上並顯示 `Stopped: N tracks in a row could not be played`（N 為 1，也就是只有一個項目的佇列時，保留該失敗本身的訊息） |
| 暫時性的問題 | 重試後仍發生的網路錯誤（`SourceError::Network`）、`429`、`5xx` | 停止（狀態 `Stopped`）在該項目上，停在已到達的位置，並顯示 0003 的訊息。**絕不跳過。**播放／暫停會重試 |
| 輸出 | `Busy`、`NotFound`、`Lost` | 停止在該項目上並顯示 0003 的訊息。絕不跳過（下一首也會以同樣方式失敗） |
| 工作階段 | `LoginRequired` | 停止；TUI 顯示「session expired」狀態（0002 AC14） |

訊息會保留在快照中，直到下一首曲目開始，或下一次失敗取代它。成功的 `Started` 會重設失敗計數。

<a id="volume"></a>
### 音量

- `0`–`100` %，以**音量間隔**為單位（預設 5，是一項設定，1–25；`ChangeVolume(±step)`），有上下限；**預設 100 %**。靜音（`ToggleMute`）把增益設為 0 並保留音量，因此取消靜音會還原；靜音時改變音量會取消靜音。自 0009 起會跨次執行保存
- 引擎中的軟體增益：`gain = (volume / 100)³`（一條感知曲線：50 % ≈ −18 dB；決策 2），靜音時為 0。增益為 1 時引擎不碰取樣
- `bit_perfect`（0003「輸出種類」）多了最後一個條件，*而且增益為 1*：低於 100 % 時快照回報 `bit_perfect: false`，原因為 `volume below 100%`，靜音時為 `muted`

<a id="engine-additions-tidal-player-audio"></a>
### 引擎新增項目（`tidal-player-audio`）

| 變更 | 原因 |
|---|---|
| `Play` 與 `Preload` 帶有 `tag: u64`；`Started`、`Transitioned`、`TrackEnded` 與 `Error` 攜帶它們所屬曲目的標記 | 播放器可以分辨剛被取代的曲目的事件與剛啟動的曲目的事件（tidalt 錯誤 1） |
| `CancelPreload` | 忘掉預先載入（關閉其來源）；在目前曲目結束時，引擎排空並送出 `TrackEnded`，如同從未送出過預先載入 |
| `SetGain(f32)`（0.0–1.0） | 在封裝前套用到每個取樣，從下一次寫入起生效（在一個 50 毫秒的寫入片段之內）；跨曲目、轉換與 `SetDevice` 保持。增益恰為 1.0 時取樣原封不動通過。否則以 `f64` 計算 `round(s × gain)`，不加抖動（dither） |

<a id="player-and-protocol-tidal-player-core"></a>
### 播放器與協定（`tidal-player-core`）

`tidal_player_core::player::update(&mut PlayerState, PlayerInput) -> Vec<PlayerEffect>`，純函式，如 0001 所規劃：

- `PlayerInput`：客戶端的 `Command`；引擎事件，由執行階段對應成 core 型別（標記、來源格式、輸出資訊、錯誤種類與訊息）；串流解析結果（項目、標記、取得的音質，或錯誤種類與訊息）；建議結果（標記、曲目或錯誤訊息）
- `PlayerEffect`：`Resolve { entry, tag, purpose: Play | Preload }`、`EnginePlay { tag, start_at }`、`EnginePreload { tag }`、`EngineCancelPreload`、`EnginePause`、`EngineResume`、`EngineSeek`、`EngineStop`、`EngineSetGain(f32)`、`FetchSuggestions { seed, tag }`、`Broadcast(Event)`。執行階段保管某個標記已解析的串流，並在 `EnginePlay`/`EnginePreload` 時交給引擎；core 從不看到 URL
- 每個 `Resolve` 取得一個新的標記。解析結果或引擎事件的標記若不是目前曲目的（或等待中的預先載入的），就什麼都不改變

新的 `protocol::Command` 變體（客戶端 → 播放器）：`LoadQueue { tracks, start }`、`AddToQueue { tracks, at: Next | End }`、`RemoveFromQueue(EntryId)`、`ClearQueue`、`PlayEntry(EntryId)`、`TogglePause`、`Next`、`Previous`、`SeekBy(milliseconds, signed)`、`SeekTo(Duration)`、`ToggleShuffle`、`CycleRepeat`、`ToggleAutoplay`、`ChangeVolume(i8)`、`SetVolume(u8)`、`ToggleMute`。用切換而非「設定」變體，讓依據過時畫面行動的兩個客戶端（0005）不會互相拉扯；MPRIS（[0010](0010-mpris.md)，implemented）加入了設定型指令 `Play`、`Pause`、`Stop`、`SetShuffle`、`SetRepeat` 與 `SetPosition`，由播放器依自己的狀態決定。

新的 `protocol::Event` 變體（播放器 → 客戶端）：

- `Player(PlayerSnapshot)`：整個播放器狀態，在每個改變了位置以外任何東西的輸入之後送出一次：佇列（播放順序，含項目 ID 與建議標記）、目前項目 ID、狀態、位置、隨機播放、重複播放、自動播放、音量、是否靜音、正在播放的詳細資訊（取得的音質、0003 的「Track」與「Output」說明、如上所述的 `bit_perfect` 及其原因），以及訊息（若有）。客戶端以它取代自己的副本，從不重新排序
- `Position { entry, position }`：轉送自引擎的 `Position` 事件（播放時每 ≤ 250 毫秒一次），不附完整快照

<a id="filling-the-queue-until-00060007"></a>
### 填入佇列（在 0006/0007 之前）

從命令列（`tidal-player [ITEM]...`、`play <ITEM>...`）或從 TUI 的**開啟**輸入提示（`o` / `O`，見下）。**項目**是 Tidal 曲目 ID（純數字，如 0003）或指向曲目、專輯或播放清單的 Tidal 連結：

| 項目 | 成為 |
|---|---|
| `77640617` | 曲目 77640617 |
| `https://tidal.com/browse/track/77640617`、`https://tidal.com/track/77640617`、`https://listen.tidal.com/track/77640617`、`tidal://track/77640617`，以及以上任何一種加上分享選單的 `/u` 後綴（`https://tidal.com/track/77640617/u`）、結尾斜線或查詢字串 | 曲目 77640617 |
| `https://tidal.com/browse/album/123`、`…/album/123/`、`https://tidal.com/album/123/u` | 專輯 123 的每一首曲目，依專輯順序 |
| `https://tidal.com/browse/playlist/<uuid>`、`…/playlist/<uuid>/u` | 播放清單的每一首曲目，依播放清單順序；影片會被排除 |
| 其他任何東西（藝人或 mix 連結、影片、打錯字） | 在播放任何東西之前就拒絕：`Not a Tidal track, album or playlist: <item>`（結束碼 2） |

項目依給定順序展開成一份清單；中繼資料（標題、藝人、專輯、長度）同時從 API 取得：

- `GET {api_base}/tracks/{id}?countryCode=…`
- `GET {api_base}/albums/{id}/tracks?countryCode=…&limit=100&offset=…`，每一頁
- `GET {api_base}/playlists/{uuid}/items?countryCode=…&limit=100&offset=…`，每一頁，只保留 `type == "track"` 的項目（`item` 內是曲目）

請求以 `limit=100`（播放清單端點接受的最大值，見「事實」）分頁，`offset` 依收到的項目數往前推進，直到 `offset ≥ totalNumberOfItems` 或某一頁回傳空的為止。每首曲目對應到：`id`、`title`、`artists[].name`（依順序）、`album.title`、`duration`（整數秒），以及 **streamable** = `allowStreaming && streamReady`。不可串流的曲目仍會排入佇列並顯示，但播放器把它當成只涉及該曲目的失敗處理（`Track 123 is not available in <country>`），不去解析它。

<a id="settings-until-spec-0008-adds-apptoml"></a>
### 設定（在規格 0008 加入 `app.toml` 之前）

與 0003 的一樣，在 0008 把它們移進 `app.toml`（並保持這些變數仍可使用）之前，來自環境變數。空的變數視為未設定；無效的值會以結束碼 2 結束，並在任何東西啟動之前指出變數名稱與可接受的範圍。

| 設定 | 環境變數 | 可接受的值 | 預設值 |
|---|---|---|---|
| 音量間隔（%） | `TIDAL_PLAYER_VOLUME_STEP` | 整數 1–25 | `5` |
| 跳轉間隔（秒） | `TIDAL_PLAYER_SEEK_STEP` | 整數 1–600 | `5` |
| 超過幾秒後上一首會重新開始曲目（秒） | `TIDAL_PLAYER_PREVIOUS_RESTART` | 整數 0–60；`0` = 上一首一律往前一個 | `3` |
| 啟動時的自動播放 | `TIDAL_PLAYER_AUTOPLAY` | `on`、`off` | `off` |

播放器在啟動時以 `PlayerConfig` 取得它們；`ToggleAutoplay` 只改變執行中播放器的自動播放（0009 會記住它，優先順序低於明確的旗標或變數）。

<a id="commands"></a>
### 指令

| 指令 | 作用 |
|---|---|
| `tidal-player [--add-to-queue \| --play-next] [ITEM]...` | 啟動 TUI（獨立模式：播放器與 TUI 在同一個行程中）。有項目時，把它們載入佇列並開始播放第一個；沒有時，以空佇列啟動。`--add-to-queue` 把項目加到佇列尾端，`--play-next` 加到目前項目之後（`AddToQueue { at: End \| Next }`），而不是取代佇列；兩者互斥（結束碼 2），且至少需要一個項目（結束碼 2）。在 0005 讓第二次呼叫能連上執行中的播放器之前，啟動時佇列一律為空，所以兩者的行為都與一般形式相同（加入的第一首曲目會開始播放）；0005 會把同樣的指令送給執行中的常駐程式。音質與裝置來自環境變數，與 `play` 相同（0003「設定」） |
| `tidal-player play <ITEM>... [--shuffle] [--repeat off\|queue\|track] [--autoplay] [--quality Q] [--device PCM] [--start SECONDS]` | 0003 的無介面 `play`，現在接受多個項目，並透過同一套播放器程式碼把它們當成一個佇列播放。`--start` 套用於第一首曲目。「Track」與「Output」兩行在每首曲目開始時再印一次（進度行也會為目前曲目重繪）。加上 `--autoplay`（或 `TIDAL_PLAYER_AUTOPLAY=on`）時，會一直以建議的曲目播下去，直到 Ctrl-C。佇列播完且至少有一首曲目播放到結束時，結束碼為 `0`；因失敗而停止（訊息輸出到 stderr，如 0003）或沒有任何曲目能播放時為 `1`；參數或項目錯誤為 `2`；Ctrl-C 為 `130`。只有一個曲目 ID 且沒有新旗標時，其行為與輸出和 0003 完全相同 |

<a id="tui"></a>
### TUI

畫面如現在一樣，是一個標題為 `tidal-player` 的外框。**播放視窗**（spotify-player 的，在頂端）在框內佔 4 列；**佇列**填滿其餘部分。兩者絕不共用列：任何畫在正在播放文字旁邊的東西（封面，在之後的規格加入時）都留在播放視窗的列之內，而佇列清單永遠橫跨它自己那幾列的完整寬度（決策 6：tidalt 把封面面板放在佇列清單旁邊，清單重繪時會出現故障）

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
││  1  King For A Day         Pierce The Veil            3:52                  ││
││▶ 2  Hell Above             Pierce The Veil            3:32                  ││
│…                                                                             │
```

- 狀態符號：`▶` 播放中、`⏸` 已暫停、`…` 載入中或緩衝中、`■` 已停止；佇列為空時顯示 `Nothing playing`
- 指示項：`shuffle` 只在開啟時顯示，`repeat: queue`/`repeat: track` 只在不是 off 時顯示，`autoplay` 只在開啟時顯示，然後是音量，靜音時以 `muted` 取代音量
- 第三列是正在播放的詳細資訊；有訊息（失敗）時，訊息取代它
- 長度未知時，進度列不畫進度條（`1:23 / ?:??`）
- 佇列列出播放順序，目前項目標記為 `▶`，變動時保持在畫面內；游標獨立移動。建議的項目接在 `Suggested` 分隔列之後，並以淡色繪製
- **開啟輸入提示**（`o`：加到佇列尾端；`O`：下一首播放，即緊接在目前項目之後）：在佇列最上面一列之上的單列輸入框，`Add to queue: ` 或 `Play next: `，接受曲目 ID 或 Tidal 曲目／專輯／播放清單連結，可輸入或貼上（括號式貼上）。`Enter` 把它展開（如同在命令列）並把曲目加到佇列尾端（`o`）或目前項目之後（`O`）；若目前沒有播放任何東西，加入的第一首曲目會開始播放。`Esc` 關閉它。開啟期間，按鍵都輸入到其中；無效的項目或取得失敗會關閉它，並在播放視窗中顯示訊息
- 窄或矮的終端機：欄位以 `…` 截斷，先截專輯欄，再截藝人欄；內部列數低於 6 時只畫播放視窗；在任何大小下都不會 panic（包括 0 × 0）

按鍵（spotify-player 的預設；在 0008 讓它們可設定之前寫死）：

| 按鍵 | 作用 |
|---|---|
| `Space` | 播放／暫停 |
| `n` / `p` | 下一首／上一首 |
| `>` / `<` | 依跳轉間隔往前／往後跳轉 |
| `^` | 跳轉到開頭 |
| `C-s` | 切換隨機播放 |
| `C-r` | 循環切換重複播放 |
| `A` | 切換自動播放 |
| `o` | 把連結或 ID 加到佇列尾端（輸入提示） |
| `O` | 把連結或 ID 加為下一首播放（輸入提示） |
| `+` / `-` | 依音量間隔調高／調低音量 |
| `_` | 靜音／取消靜音 |
| `j`/`↓`、`k`/`↑`、`g g`、`G` | 移動佇列游標（下、上、最上、最下） |
| `Enter` | 播放游標所在的項目 |
| `q`、`Esc` | 離開（不變）；播放器先停止並釋放裝置 |

<a id="acceptance-criteria"></a>
## 驗收條件

播放器狀態機（`tidal_player_core::player`，純函式；測試以輸入驅動 `update`，並斷言狀態與效果）：

- **AC1** — `LoadQueue { tracks, start }` 取代佇列，使項目 `start` 成為目前項目，為它發出帶有新標記的 `Resolve { purpose: Play }` 並進入 `Loading`；該標記的解析成功後發出 `EnginePlay { tag, start_at: 0 }`；帶有該標記的引擎 `Started` 進入 `Playing`。`Next`、`Previous`（在重新開始門檻以內或等於門檻）、`PlayEntry` 與自動前進，從任何狀態都走同一條路徑，因此跳過絕不等目前曲目結束（tidalt 錯誤 4 與 8）
- **AC2** — 下一首、上一首與自然結束遵循「播放狀態」與「隨機播放與重複播放」下的表格（針對重複播放 `off`/`queue`/`track` × 目前項目在第一個／中間／最後一個 × 隨機播放關／開 × 上一首時位置在重新開始門檻以內或等於／超過門檻的表格測試，門檻為 3 秒與 0）。在結尾停止時，最後一個項目維持為目前項目，位置 0:00、狀態 `Stopped`，之後 `TogglePause` 從頭播放它
- **AC3** — `ToggleShuffle` 開啟：目前項目排在播放順序最前面，每個項目恰好出現一次，順序是種子的函式（兩個種子對 10 個項目的佇列給出不同順序；同一種子給出相同順序），且沒有任何效果碰到引擎（目前曲目繼續播放）。關閉：原始順序，目前項目不變。快照攜帶播放順序。開啟隨機播放時載入，會從所選項目開始
- **AC4** — `CycleRepeat` 依 `off → queue → track → off` 循環；每次改變都出現在下一個快照中
- **AC5** — 預先載入：`Position` 剩 30 秒或更少（或長度未知時的 `Started`）時，依重複播放表為下一個項目發出一次 `Resolve { purpose: Preload }`；其結果發出帶有其標記的 `EnginePreload`；帶有該標記的 `Transitioned` 使它成為目前項目，不發出任何 `EnginePlay`。之後若佇列編輯、隨機播放或重複播放的變更改變了下一個項目，就發出新的 `Resolve`/`EnginePreload`，沒有下一個項目時發出 `EngineCancelPreload`；沒有改變它的則什麼都不發出。沒有下一個項目時不請求預先載入
- **AC6** — 過時的輸入什麼都不改變也不發出任何效果：標記既不是目前曲目的、也不是等待中的預先載入的解析結果、`Started`、`Transitioned`、`TrackEnded` 或 `Error`（表格測試；tidalt 錯誤 1 的情形：`Next`，然後是被取代曲目的 `TrackEnded`，結果恰好只前進一次）
- **AC7** — 失敗遵循「失敗處理」下的表格（針對每種錯誤種類 × 解析／引擎的表格測試）：只涉及該曲目的失敗會前進並顯示訊息；暫時性、輸出或工作階段的失敗停在該項目上、顯示訊息，且不為其他項目發出 `Resolve`；連續 5 次只涉及該曲目的失敗（或佇列長度，若較小；包括重複播放 `queue` 與 `track`）會以 `N tracks in a row could not be played` 停止；`Started` 重設計數；曲目不可串流的項目是只涉及該曲目的失敗，且不為它送出 `Resolve`。`Error` 絕不被當成 `TrackEnded` 處理，反之亦然
- **AC8** — `TogglePause` 與跳轉遵循「播放狀態」：Playing ⇄ Paused 發出 `EnginePause`/`EngineResume`；在 `Loading` 時切換，成功的解析在再次切換之前不發出 `EnginePlay`；在 0:03 的 `SeekBy(-5000)` 跳轉到 0:00（客戶端送出 `SeekBy(±seek step)`；AC20 檢查它使用設定的間隔）；`Loading` 時的 `SeekBy`/`SeekTo` 改變即將發出的 `EnginePlay` 的 `start_at`；`Stopped` 時它們不發出任何東西
- **AC9** — 音量：預設 100，未靜音；`ChangeVolume(±n)` 限制在 0–100（n = 1、5 與 25）；`SetVolume` 設定它；每次改變發出 `EngineSetGain((v/100)³)`（靜音時為 0）；`ToggleMute` 兩次會還原先前的增益；改變音量會取消靜音。快照的 `bit_perfect` 在音量 100 且未靜音時是引擎的，否則為 `false`，原因為 `volume below 100%` / `muted`
- **AC10** — 佇列編輯遵循「佇列」下的表格（針對每種操作 × 目前項目在編輯之前／位置上／之後 × 隨機播放關／開的表格測試）：項目 ID 絕不重複使用，同一首曲目可以排入兩次，播放中移除目前項目會前進（沒有下一個時停止），`ClearQueue` 只保留目前項目且不打斷它，編輯空佇列不會啟動任何東西
- **AC11** — 每個改變了位置以外任何東西的輸入，恰好發出一個 `Broadcast(Event::Player(snapshot))`，作為最後一個效果，內含新的狀態；引擎的 `Position` 只發出 `Broadcast(Event::Position { entry, position })`；什麼都沒改變的輸入什麼都不發出
- **AC12** — 每個新的 `Command` 與 `Event` 變體都能通過 JSON 往返（0001 AC11 的測試，已擴充）

引擎（`tidal-player-audio`，使用 0003 的假實作）：

- **AC13** — `Started`、`Transitioned`、`TrackEnded` 與 `Error` 攜帶隨其曲目的 `Play` 或 `Preload` 給定的標記，`Play` 取代正在播放的曲目時，以及預先載入的曲目在轉換之後失敗時也一樣
- **AC14** — `CancelPreload`：被取消的來源會關閉；在目前曲目結束時，引擎排空並送出 `TrackEnded`（帶有它的標記），且接收端（sink）絕不會收到被取消曲目的任何一個影格。沒有預先載入任何東西時的 `CancelPreload` 什麼都不做
- **AC15** — `SetGain`：1.0 時，sink 收到的位元組與完全沒有 `SetGain` 時收到的完全相同（逐位元相同，16 位元與 24 位元的測試資料）；0.5 時每個取樣是 `round(s × 0.5)`；0 時為靜音；新的增益套用到指令之後寫入的影格，最多晚一個寫入片段（50 毫秒的音訊）；增益在無縫轉換、新的 `Play` 與 `SetDevice` 之間保持不變

項目與中繼資料：

- **AC16** — `parse_item(&str) -> Result<Item, ItemError>`（在 `tidal-player-core`）對應「填入佇列」下表格中的每一種形式（表格測試，包括結尾斜線、分享選單的 `/u` 後綴、查詢字串、`www.`、大寫主機名稱、純數字），並拒絕藝人、mix 與影片連結、其他主機與無意義的字串
- **AC17** — `tidal-player-api` 取得 `get_track`、`get_album_tracks` 與 `get_playlist_tracks`，回傳 `tidal_player_core::Track`（ID、標題、藝人、專輯標題、存在時的長度、streamable），透過 `Authenticator`，`countryCode` 來自工作階段（來自探測的 wiremock 測試資料）：專輯與播放清單的取得會走遍每一頁直到 `totalNumberOfItems`（`limit=100`；一頁時一個請求，250 個項目的清單時三個，空頁會提早結束走訪）；`track` 以外的播放清單項目會被丟棄；`allowStreaming: false` 或 `streamReady: false` 給出 `streamable: false`；`404`（`subStatus` 2001）對應到 `MetadataError::NotFound`；`LoginRequired` 與暫時性錯誤原樣回傳

CLI 與執行階段（`tidal-player`）：

- **AC18** — 有多個項目的 `play`：依序展開它們（假的中繼資料來源），把它們載入成一個佇列，並透過 `tidal_player_core::player` 播放（假的引擎）：每首曲目開始時印出「Track」/「Output」行；`--shuffle` 與 `--repeat` 在載入之前設定播放器；結束碼如「指令」之下（表格測試：全部播放 → 0；三首中有一首只涉及該曲目的失敗 → 0，訊息在 stderr；輸出忙碌 → 1；每首曲目都失敗 → 1；錯誤的項目 → 2）。0003 的 `play` 測試不經修改即通過
- **AC19** — 播放器執行階段依序執行效果，並把引擎事件與解析結果連同它們的標記對應回輸入（假的引擎與假的解析器：載入期間連按 5 次 `Next` 會解析全部 5 次但只播放最後一個；TUI 離開時裝置會停止）。`tidal-player [ITEM]...` 啟動時就已載入佇列的 TUI；沒有項目時則為空
- **AC20** — 按鍵 → 動作 → 指令表（`tidal_player_core::ui` 與 `tidal-player` 的繪製）：「按鍵」之下的每個按鍵對應到它的 `Command`（或游標移動）；`Enter` 送出帶有游標下項目 ID 的 `PlayEntry`（不是它的索引）；在空佇列上，除了音量與模式按鍵之外，沒有任何按鍵會送出任何東西
- **AC21** — `g g` 是兩個按鍵的序列：在同一個序列中 `g` 之後再按 `g` 移到最上面；`g` 之後接任何其他按鍵，就執行那個按鍵的動作
- **AC22** — 客戶端狀態原樣接受每個 `Event::Player` 快照（顯示的佇列順序與快照的完全相同；客戶端從不隨機播放：tidalt 錯誤 5）；對應到快照目前項目以外項目的 `Event::Position` 會被忽略；游標在各快照之間停在同一個項目 ID 上（它被移除時則夾回範圍內）
- **AC23** — 繪製快照（`insta`，80×24）：沒有播放；播放中，帶有隨機播放、重複播放 `queue`、80 % 與 `volume below 100%` 原因；已暫停；載入中；失敗訊息；長度未知（沒有進度條，`?:??`：tidalt 錯誤 6）；比視窗更長的佇列，目前項目保持在畫面內。40×12：截斷順序。從 0×0 到 120×40 都不 panic（針對各種大小的表格測試）
- **AC24** — `docs/playback.md` 記載項目、新的 `play` 旗標與設定；`docs/tui.md` 記載畫面、按鍵、開啟輸入提示、隨機播放／重複播放／自動播放／音量的語意與失敗時的行為；本規格連結到兩者

隨決策加入（2026-10-07）：

- **AC25** — 設定：`resolve_player_config(env)`（純函式）在什麼都沒設定時給出預設值，每個值都在其範圍內，超出範圍、非數字與未知的值會給出指出變數名稱的結束碼 2 錯誤（表格測試，包括空值 = 未設定）。`play --autoplay` 優先於環境變數。TUI 的 `+`/`-` 與 `>`/`<` 送出設定的間隔；播放器的上一首使用設定的門檻
- **AC26** — 自動播放（播放器）：自動播放開啟且重複播放為 `off` 時，最後一個項目到達預先載入點會發出一次 `FetchSuggestions { seed: its track, tag }`；該標記的結果會把佇列中尚未有的曲目以建議項目的身分附加上去，並預先載入第一首；錯誤、空的（或全部重複的）結果不附加任何東西，設定 `Autoplay: no suggestions (…)` 訊息，且佇列停在尾端；過時的標記，或在 `ToggleAutoplay` 關閉之後才到達的結果，什麼都不改變；重複播放 `queue`/`track` 或自動播放關閉時，絕不發出 `FetchSuggestions`（表格測試）
- **AC27** — `tidal-player-api` 取得 `get_suggestions(track)`：恰好一個帶有 `countryCode` 與 `limit=100` 的 `GET /tracks/{id}/radio`（來自探測的 wiremock 測試資料）；曲目依順序，如 AC17 對應（包括種子：播放器的去重會丟掉它）；`404`/`2001` → 空清單；`LoginRequired` 與暫時性錯誤原樣回傳
- **AC28** — 開啟輸入提示（客戶端狀態，純函式）：`o` 以加到佇列模式開啟它，`O` 以下一首播放模式開啟；輸入的字元與貼上會編輯它；`Backspace` 刪除；`Esc` 關閉它且沒有任何效果；`Enter` 搭配有效的項目時關閉它，並依模式發出 `Effect::Expand { item, at: End | Next }`（自 0005 起：`Send(Command::Open { items, at })`，而下方的開始規則在播放器中，0005 AC7）；搭配無效的項目時，關閉它並設定 `Not a Tidal track, album or playlist` 訊息。客戶端執行階段把展開後的曲目轉成 `AddToQueue { at }`，接著在播放器為 `Stopped` 且沒有目前項目，或佇列原本是空的時候，對第一個加入的項目送出 `PlayEntry`（假的中繼資料）。提示開啟期間沒有任何按鍵會到達播放器（`Space` 輸入一個空格）。CLI：`--add-to-queue` 與 `--play-next` 解析成對應的 `at`，互斥且需要一個項目（結束碼 2，`crates/app/tests/cli.rs`）

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 空佇列 | 每個播放按鍵都不作用；音量與隨機播放／重複播放／自動播放模式仍會改變（並套用到下一次載入）；`o`/`O` 開啟輸入提示 |
| 只有一個項目的佇列，重複播放 `queue` | 曲目重複（如 `track`），而且是無縫的 |
| 專輯的每一首曲目在該國家都無法取得 | 5 次失敗（或更少，專輯的長度），然後是 `Stopped: N tracks in a row could not be played`；沒有 API 風暴（tidalt 錯誤 3） |
| 解析期間 `429` 或網路中斷 | 停在該項目上並顯示訊息；播放／暫停會重試。絕不跳過 |
| 佇列播放到一半 DAC 被拔掉 | `Output hw:1,0 was lost`；停止；佇列保留；播放／暫停在同一個裝置上重試 |
| 佇列播放到一半工作階段過期 | 停止；TUI 顯示工作階段過期的狀態；在另一個終端機執行 `tidal-player login` 之後（0002 AC8），播放／暫停就會繼續 |
| 載入期間連按 Next | 每次按下都取代上一次；只有最後一個項目會播放（過時的解析結果被丟棄，AC6、AC19） |
| 已預先載入的下一個項目被從佇列移除 | `CancelPreload` 或新的預先載入（AC5）；被移除的曲目絕不會播放 |
| 跳轉超過結尾，重複播放 `track` | 曲目重新開始 |
| 預先載入的串流 URL 在播放時已過期 | 0003 在 `403`/`410` 時的重新解析涵蓋這種情況（預先載入發生在使用前 ≤ 30 秒） |
| 中繼資料中沒有長度的曲目 | 進度顯示 `?:??` 且沒有進度條；預先載入在 `Started` 時開始 |
| 在沒有硬體暫停的裝置上暫停 | 同 0003「邊界情況」（約 0.1 秒會繼續播放） |
| 有超過一頁曲目的專輯或播放清單 | 播放之前會取得所有頁面（AC17） |
| 不存在的項目（`404`/`2001`） | 不播放任何東西：`Track 1 was not found` / `Album 1 was not found` / `Playlist <uuid> was not found`（`play` 以 1 結束；TUI 以空佇列啟動並顯示該訊息） |
| 中繼資料標示為不可串流的曲目 | 排入佇列並顯示；以只涉及該曲目的失敗跳過，不做串流請求（AC7） |
| 含有影片的播放清單 | 影片被排除；全是影片的播放清單會被拒絕：`Playlist <uuid> has no tracks`（結束碼 2 ／ TUI 啟動時的訊息） |
| 自動播放，但 Tidal 對該種子沒有建議 | `Autoplay: no suggestions (…)`，停在尾端（AC26） |
| 以藝人或影片連結使用開啟輸入提示 | `Not a Tidal track, album or playlist: …`；佇列不變（AC28） |
| 終端機比版面還小 | 截斷，然後只顯示播放視窗；絕不 panic（AC23） |

<a id="test-plan"></a>
## 測試計畫

每個自動化測試以其條件命名（`ac5_…`）。紅燈是對存根型別與函式（存根本體，沒有 `todo!()`，沒有編譯錯誤）的失敗斷言，如 0001–0003。播放器測試以手工建立輸入，從不執行執行緒；時間只是 `Position` 輸入所說的。API 測試使用 `wiremock`，測試資料在 `crates/api/tests/fixtures/metadata/` 之下，依探測記錄的形狀撰寫（ID 與名稱換成假的）。引擎測試重用 0003 的測試資料與假實作。

| AC | 測試（檔案 :: 名稱） | 驗證內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/player.rs` :: `ac1_load_and_skip_paths`（表格：載入、下一首、上一首、播放項目、自動前進；從每個狀態出發） | 效果 `Resolve` → `EnginePlay` → 狀態 `Playing`；每次都是新的標記 | 存根的 `update` 不回傳任何效果 |
| AC2 | `crates/core/src/player.rs` :: `ac2_next_previous_end`（表格，如 AC2） | 接下來播放的項目，或停止狀態 | 存根一律播放索引 + 1，從不停止也不繞回 |
| AC3 | `crates/core/src/player.rs` :: `ac3_shuffle` | 第一個項目、排列、與種子的相依性、沒有引擎效果、還原原順序 | 存根不改變順序 |
| AC4 | `crates/core/src/player.rs` :: `ac4_repeat_cycle` | 循環與快照欄位 | 存根維持 `off` |
| AC5 | `crates/core/src/player.rs` :: `ac5_preload`（表格：31 秒 → 剩 30 秒、長度未知、沒有下一個、每種編輯／模式改變了下一個、下一個沒變） | 一次 `Resolve{Preload}`、`EnginePreload`、`Transitioned` 的處理、`EngineCancelPreload` | 存根從不預先載入，所以 `Transitioned` 是未知的，曲目結束時有空隙 |
| AC6 | `crates/core/src/player.rs` :: `ac6_stale_inputs_ignored`（表格） | 沒有效果，狀態不變；tidalt 的順序中恰好前進一次 | 存根比對事件時不看標記，所以過時的 `TrackEnded` 前進了兩次 |
| AC7 | `crates/core/src/player.rs` :: `ac7_failure_policy`（針對錯誤種類 × 來源的表格）+ `ac7_failure_run_stops` | 每一列依規則跳過或停止；訊息；佇列長度 3 與 20 的連續失敗上限 | 存根每次錯誤都跳過（tidalt 的迴圈） |
| AC8 | `crates/core/src/player.rs` :: `ac8_pause_and_seek`（表格） | 各狀態的效果；保留的播放；夾限；`start_at` | 存根忽略狀態，一律送出 `EnginePause` |
| AC9 | `crates/core/src/player.rs` :: `ac9_volume`（表格） | 音量、增益值、靜音往返、`bit_perfect` 與原因 | 存根從不發出 `EngineSetGain`，並保留引擎的 `bit_perfect` |
| AC10 | `crates/core/src/player.rs` :: `ac10_queue_edits`（表格） | 順序、目前項目、ID、效果 | 存根把所有東西都附加在尾端 |
| AC11 | `crates/core/src/player.rs` :: `ac11_one_snapshot_per_change`（涵蓋 AC1–AC10 表格的每一列） | 恰好一個 `Broadcast(Player)`，在最後；位置只有 `Position` 事件；沒有改變就什麼都不發 | 存根每次輸入後都廣播 |
| AC12 | `crates/core/src/protocol.rs` :: `ac11_round_trip`（0001 的，已擴充；`all_commands`/`all_events` 列出新的變體） | 往返 | 新變體手寫的存根 `Serialize` 寫出 `null` |
| AC13 | `crates/audio/tests/engine.rs` :: `ac13_events_carry_tags` | 每個事件上的標記，包括取代與失敗的預先載入 | 存根引擎送出標記 0 |
| AC14 | `crates/audio/tests/engine.rs` :: `ac14_cancel_preload` | 排空、`TrackEnded`、被取消來源沒有任何影格、來源已關閉 | 存根忽略 `CancelPreload`，所以 sink 收到第二首曲目 |
| AC15 | `crates/audio/tests/engine.rs` :: `ac15_gain`（表格：16/24 位元的 1.0、0.5、0、曲目途中改變、跨轉換／播放／裝置） | 每一列的位元組；以影格計的延遲 | 存根忽略 `SetGain` |
| AC16 | `crates/core/src/item.rs` :: `ac16_parse_item`（表格） | 每一列的項目或錯誤 | 存根只解析純數字 |
| AC17 | `crates/api/tests/metadata.rs` :: `ac17_get_track`、`ac17_album_pages`、`ac17_playlist_pages_and_videos`、`ac17_errors` | 請求路徑與參數（`expect(n)`）、對應的曲目、錯誤 | 存根只讀第一頁並保留影片 |
| AC18 | `crates/app/src/cli/play.rs` :: `ac18_queue_exit_codes`（表格，假的中繼資料 + 假的引擎）+ `crates/app/tests/cli.rs` :: `ac18_bad_item` | 印出的行、結束碼；結束碼 2 搭配 `Not a Tidal…` | 存根只播放第一個項目 |
| AC19 | `crates/app/src/player/runtime.rs` :: `ac19_effects_and_mash`、`ac19_quit_stops_engine` | 效果執行順序；連按之後只有一次 `Play`；離開前有 `Stop` | 存根執行階段播放它收到的每個解析結果 |
| AC20 | `crates/core/src/ui/mod.rs` :: `ac20_keys_to_commands`（表格）+ `crates/app/src/tui/input.rs` :: `ac18_key_events`（由 0006 AC18 改名，它把自己的按鍵加進了表格） | 每個按鍵的指令；`Enter` 的項目 ID | 存根只對應 `q`/`Esc` |
| AC21 | `crates/core/src/ui/mod.rs` :: `ac21_key_sequence` | `g g` → 最上；`g` `j` → 下 | 存根把單獨的 `g` 當成最上 |
| AC22 | `crates/core/src/ui/mod.rs` :: `ac22_snapshot_applied_as_is` | 顯示的順序等於快照的；過時的位置被忽略；游標跟隨項目 ID | 存根依原始位置排序佇列 |
| AC23 | `crates/app/src/tui/render.rs` :: `ac23_playback_window_*`、`ac23_queue_scrolls`、`ac23_truncation_40x12`、`ac23_no_panic_any_size` | 每個狀態的 `contains` 斷言（符號、`?:??`、原因文字），以及驗收時人工檢視的快照 | 存根的 `render` 只畫外框 |
| AC24 | — 驗收時檢視 | 文件存在、與本規格相符、已連結 | — |
| AC25 | `crates/app/src/settings.rs` :: `ac25_player_config`（表格）+ `crates/core/src/ui/mod.rs` :: `ac25_steps_from_config` | 值、錯誤與優先順序；指令攜帶設定的間隔 | 存根忽略環境變數並回傳預設值 |
| AC26 | `crates/core/src/player.rs` :: `ac26_autoplay`（表格：開／關 × 重複播放 × 結果 ok/空/重複/錯誤/過時/關閉之後） | 一次 `FetchSuggestions`、附加的項目及其標記、預先載入、訊息、停止 | 存根從不取得，所以自動播放開啟時佇列停在尾端 |
| AC27 | `crates/api/tests/metadata.rs` :: `ac27_suggestions` | 路徑與參數、對應的曲目、錯誤 | 存根不發請求就回傳空清單，所以 `expect(1)` 失敗 |
| AC28 | `crates/core/src/ui/mod.rs` :: `ac28_open_prompt`（表格）+ `crates/app/src/player/runtime.rs` :: `ac28_open_adds_and_starts` | 輸入提示的狀態與模式、效果；展開之後送出的指令（`End` 與 `Next`） | 存根的 `o`/`O` 什麼都不做 |

刻意不由自動化測試涵蓋，驗收時以真實帳號手動檢查（結果寫在 PR 說明中）：

- 在 DAC 上透過 `play <album link>`，憑耳朵檢查現場演出或古典專輯兩首曲目之間的無縫播放（0003 開放中的手動檢查），並檢查 `/proc/asound/cardN/pcm0p/sub0/hw_params` 在轉換前後沒有改變
- 音量確實以階梯式改變；在 100 % 時，「Output」行在 `hw:` 上仍然寫 `bit-perfect`
- 在串流 hi-res 曲目時，TUI 中的下一首／上一首／跳轉立即回應
- 從 Tidal 應用程式分享選單取得的專輯與播放清單連結，原樣貼上（在命令列與貼進開啟輸入提示）
- 自動播放：專輯播到結尾後，無縫地接著建議的曲目，而且它們看起來相關

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`player`（狀態機）、`item`（`parse_item`）、`Track`/`EntryId` 型別、`protocol` 變體、`ui`（客戶端狀態、按鍵序列、佇列游標）。隨機播放使用 `core` 中寫的小型帶種子 PRNG（幾行 xorshift/PCG）；沒有新的相依套件
- `tidal-player-api::metadata`：三個取得函式、`get_suggestions`，以及它們的 DTO → `core::Track` 對應
- `tidal-player-audio`：標記、`CancelPreload`、`SetGain`
- `tidal-player`：播放器執行階段（`player_runtime.rs`：執行 `update`，針對引擎、解析器與廣播 channel 執行效果）、建構於其上的 `play`、設定、TUI 的串接與繪製（開啟括號式貼上）、`input.rs` 按鍵對應
- `xtask layering`：沒有變更

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-07，來自程式碼）：

- 0003 的引擎在播放中收到 `Play` 時，會先停止舊曲目，保留一個預先載入（`Play`、`Stop` 與失敗的曲目都會讓它被忘掉），而且沒有辦法取消預先載入或為曲目加標記（`crates/audio/src/engine.rs`）：因此有了引擎新增項目
- 「背景」之下列出的 tidalt 行為與錯誤（讀自其原始碼與歷史）

2026-10-07 針對**真實 API** 驗證，由使用者執行 `scripts/tidal-metadata-probe.sh`（與 0003 的探測相同的帳號，裝置流程客戶端「Android Automotive HiRes」），使用曲目 33695188、一張 17 首曲目的專輯，以及一份有 39 首曲目與 2 部影片的使用者播放清單。`crates/api/tests/fixtures/metadata/` 之下的測試資料依這些形狀撰寫，ID 與名稱已替換：

- `GET /v1/tracks/{id}` → `200`，內有 `id`、`title`、`duration`（整數秒）、`version`（此處為 null）、`trackNumber`、`volumeNumber`、`allowStreaming`、`streamReady`、`audioQuality`、`mediaMetadata.tags`、`artist {id, name, type}`、`artists[] {id, name, type: MAIN…}`、`album {id, title, cover}`，以及本規格不使用的欄位（`replayGain`、`peak`、`isrc`、`bpm`、`mixes`……）。未知的 ID → `404 {"status":404,"subStatus":2001,"userMessage":"Track [1] not found"}`
- `GET /v1/albums/{id}/tracks` → `{limit, offset, totalNumberOfItems, items: [track…]}`，依專輯順序的純曲目（`trackNumber` 1、2……）。不帶 `limit` 時它回傳全部 17 首（`limit: 17`）；`limit=3&offset=2` 回傳第 3–5 首曲目，`totalNumberOfItems: 17`；`limit=1000` 在這個端點被接受。未知的專輯 → `404`/`2001`（`Album [1] not found`）。`GET /v1/albums/{id}/items` 也存在，內有 `{item, type: "track"}` 項目；不使用
- `GET /v1/playlists/{uuid}/items` → `{limit, offset, totalNumberOfItems, items: [{item, type, cut}]}`；`item` 是曲目，另外有 `dateAdded`、`index`、`itemUuid`、`description` 與 `album.releaseDate`。**預設頁面大小為 10**（所以不分頁的客戶端只會看到 10 個項目）；`totalNumberOfItems` 也把影片算進去（41 = 39 首曲目 + 2 部影片，依播放清單的 `numberOfTracks`/`numberOfVideos`）；`limit=1000` → `400 {"subStatus":1001,"userMessage":"Too big page, max page size is [100]"}`。未知的 UUID → `404`/`2001`（`Playlist not found`）
- `/items` 中的影片項目（第二次執行，`limit=50`，全部 41 個項目）：`{"item": {…}, "type": "video", "cut": null}`，其中 `item` 有 `id`、`title`、`duration`、`artists`、`streamReady`/`allowStreaming`（都是 `true`）、`quality: "MP4_1080P"`、`type: "Music Video"`、`trackNumber: 0`、`volumeNumber: 0`，以及 **`album: null`**。各類數量：39 個 `track`、2 個 `video`，與 `numberOfTracks`/`numberOfVideos` 相符
- `GET /v1/playlists/{uuid}/tracks`（tidalt 的端點）有相同的封套，內容是純項目，並且**也會回傳影片**（39 首曲目 + 2 部影片得到 41 個項目），除了它們的形狀（`album: null`、`quality`）之外沒有任何東西標示它們。因此本規格使用 `/items` 並只保留 `type == "track"`。`/tracks` 是否也拒絕 `limit=1000`（tidalt 的值）未驗證

2026-10-07 針對**真實 API** 驗證，由使用者執行 `scripts/tidal-autoplay-probe.sh`（同一個帳號），種子曲目 33695188：

- `GET /v1/tracks/{id}/radio` → 專輯曲目的封套，內容是純曲目（`{limit, offset, totalNumberOfItems, items: [track…]}`）。預設頁面大小為 10；`totalNumberOfItems: 100`；`limit=100` 回傳全部 100 首。**種子曲目是第一個項目**
- `GET /v1/mixes/{TRACK_MIX}/items`（ID 來自曲目的 `mixes.TRACK_MIX`）→ `{item, type: "track"}` 項目，預設頁面大小 100，`totalNumberOfItems: 100`，而且**與電台相同的 100 個曲目 ID、相同的順序**。所以曲目電台*就是*曲目 mix；使用電台，是因為它不需要額外的查詢
- 未知的曲目 → `404 {"subStatus":2001,"userMessage":"Track radio cannot be generated for trackId [1]"}`；未知的 mix → `404`/`2001`（`Mix items for [...] not found`）
- Tidal 應用程式自己的自動播放呼叫哪個端點，目前未知

未驗證（探測沒有涵蓋到）：

- `allowStreaming: false` 或 `streamReady: false` 的曲目（看過的每首曲目兩者都是 `true`）
- 超過 100 首曲目的專輯（分頁以測試資料驗證）

<a id="decisions-answered-by-the-user-2026-10-07"></a>
## 決策（由使用者回答，2026-10-07）

1. **在 0006/0007 之前填入佇列**：*兩者都要*——命令列上的項目（`tidal-player [ITEM]...`、`play <ITEM>...`）以及貼進 TUI 輸入提示的項目：`o` 加到佇列尾端，`O` 下一首播放；命令列上的 `--add-to-queue` / `--play-next`（AC28）
2. **音量**：引擎中的軟體增益，三次方曲線，預設 100 %（預設就是位元完美）。*合理，但可設定*：間隔是一項設定（預設 5 %，AC25）。（ALSA 硬體混音器是另一個選項；未採用）
3. **上一首**在超過門檻時重新開始曲目，否則往前一個。*合理，但可設定*：預設 3 秒，`0` 停用重新開始（AC25）
4. **佇列結尾**：停止，除非**自動播放**開啟，此時如同 Tidal 應用程式般接著播放 Tidal 的建議；*可切換*（`A`、`--autoplay`、一項設定；AC26）。預設關閉（提議；Tidal 應用程式預設為開啟）
5. **失敗政策**：如「失敗處理」。*合理*
6. **版面**：*兩種都可以*，但封面（出現時）絕不能與曲目清單共用列，如 tidalt 的那樣，那會使圖片出現故障。播放視窗在頂端；規則在「TUI」中
7. **跳轉間隔**：5 秒。*合理，但可設定*（AC25）

<a id="out-of-scope"></a>
## 不在範圍內

- 常駐程式、透過傳輸層的客戶端、一次性的 `playback` 指令、多個客戶端（0005）
- 音樂庫與搜尋頁面、從它們「加到佇列」（0006、0007）；它們會使用本規格的 `AddToQueue`
- 在 `app.toml`/`keymap.toml` 中可設定的按鍵與設定（0008）
- 跨次執行記住佇列、位置與音量（已在 0009 完成）
- MPRIS 與媒體鍵，以及為其而設的設定型指令（[0010](0010-mpris.md)，implemented）
- 作為可瀏覽頁面的 mix 與電台（0011）；佇列結尾的自動播放在本規格中
- 封面（其放置規則在此固定，見「TUI」）
- 從 TUI 切換輸出裝置（引擎支援；選擇器是之後的規格；0009 略過了它）
- 以按鍵重新排序佇列、把佇列存成播放清單（tidalt 兩者都有）、以按鍵移除項目（指令存在；按鍵隨 0006 而來）
- 淡入淡出（crossfade）、ReplayGain、音量漸變、抖動（dither）
- 閒置之後讓游標跟隨正在播放的曲目（tidalt `4b1199d`）：改為讓目前項目保持在畫面內

<a id="bugs"></a>
## 錯誤

- **只有一首失敗曲目的佇列，結尾是 `Stopped: 1 tracks in a row could not be played`（於切片 D 驗收時發現，2026-10-07）。** 預期：對無法播放的曲目執行 `play <id>`，印出的恰好是 0003 的訊息（`Track 404 is not available in NO`），如「指令」對單一曲目形式的要求；TUI 同樣顯示該曲目為什麼失敗。實際：連續失敗上限是 `min(5, queue length)` = 1，所以第一次失敗就達到上限，播放器把失敗的訊息換成連續失敗的摘要，而它沒有指出任何原因（還寫成「1 tracks」）。根本原因：「失敗處理」沒有說明只有一次的情況下摘要是什麼。修正：只有在 N > 1 時，摘要才取代訊息；N = 1 時保留失敗本身的訊息（表格列已更正）。AC7 新增一列：只有 1 個項目的佇列（重複播放 `off` 與 `queue`）在 1 次失敗之後停止，並帶有該次失敗的訊息。測試：`crates/core/src/player/tests.rs` :: `ac7_failure_run_stops`（列 `(1, 0, 1)` 與 `(1, 1, 1)`；紅燈：訊息是連續失敗的摘要）。移除了 `play` 的 reporter 針對此情況的變通做法
- **分享選單的連結被拒絕（使用者回報，2026-10-07）。** 預期：Tidal 應用程式分享選單複製的連結可以播放，例如 `https://tidal.com/track/145060431/u` 與 `https://tidal.com/album/145060429/u`。實際：`Not a Tidal track, album or playlist: …`。根本原因：規格的表格說分享連結帶有 `?u` 查詢字串；它們帶的其實是 `/u` 路徑區段（使用者的回報；從網址列複製的連結沒有後綴），而 `parse_item` 恰好只接受兩個路徑區段。表格與 AC16 已更正：曲目、專輯或播放清單 ID 之後接受結尾的 `/u` 區段（之後可再接 `/`、查詢字串或片段）；任何其他多出來的區段仍被拒絕。測試：`crates/core/src/item.rs` :: `ac16_parse_item`，新增的列包括曲目、專輯與播放清單上的 `/u`、`/u/`、`/u?x`、`/browse/…/u`，以及對 `/x` 與 `/u/u` 的拒絕（紅燈：`/u` 的列被拒絕）
