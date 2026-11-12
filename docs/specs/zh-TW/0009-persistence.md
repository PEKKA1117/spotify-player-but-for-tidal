[English](../0009-persistence.md) | 正體中文

<a id="0009--persistence-resume-where-you-left-off"></a>
# 0009 — 持久化：從上次停下的地方繼續

- **狀態**：implemented（2026-10-08；「測試計畫」中的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0002（implemented：狀態目錄、`logout`）、0004（implemented：播放器狀態）、0005（implemented：每位使用者一個播放器，客戶端不在磁碟上擁有任何東西）、0008（implemented：`app.toml` 與它的優先順序）
- **使用者文件**：[`docs/playback.md`](../../zh-TW/playback.md) 新增「Resuming the last session」；[`docs/daemon.md`](../../zh-TW/daemon.md)（重新啟動的常駐程式會接續）、[`docs/config.md`](../../zh-TW/config.md)（`remember_playback`）、[`docs/login.md`](../../zh-TW/login.md)（`logout` 也會忘記它）與它們的 zh-TW 版本（AC14）

<a id="context"></a>
## 背景

每次執行都從空白開始：播放器結束時，佇列、位置、隨機播放、重複播放、自動播放與音量都會遺失，所以 `systemctl --user restart tidal-player`、重新開機或離開獨立模式的 TUI，都會丟掉原本正在播放的內容。0003、0004、0005、0006 與 0007 都把「記住」延後給本規格，0001 則把它列為「Persistence: last session, volume, device, metadata cache」。

本規格讓**播放器行程**記住它的播放狀態，並在啟動時還原，停在儲存的位置。0001 清單中的其餘項目（裝置、中繼資料快取、TUI 自己的狀態）也在此明確決定：見「決策」。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 tidalt 的 `internal/store/store.go`、`internal/ui/model.go`、`keys.go`、`library_sections.go`，以及 commit `0ab2897`、`f904af2`、`8ee1989`、`d3827b7`、`c3a59c3`、`1d3faed`、`a8b5e7e`：

- 一個 **bbolt** 資料庫 `~/.local/share/tidalt/tidal-cache.db`，有 `Tracks`、`Settings`、`Cache` 這幾個 bucket，以獨占鎖開啟，逾時 5 秒。鍵有：`volume`、`device`、`theme`、播放清單（佇列的曲目）、最後一首曲目的 ID、最後的位置、最近播放的歷史、看過的每一首曲目（`CacheTrack`），以及每一次純文字搜尋的結果（`search:<query>`）
- 這一切都**從 UI model** 寫入，大多是 `_ = m.store.Save…(…)`，所以每個寫入錯誤都被丟掉
- 位置在播放時**每一秒的 tick** 都寫入，離開時也寫
- 啟動時播放清單是**非同步**載入的；同時進行的收藏載入會取代它，直到 `f904af2` 加了 `playlistRestored` 旗標。最後一首曲目是用**曲目 ID**（第一個符合的）重新找到的，只把游標移到那裡，儲存的位置則在下一首曲目**開始**後以跳轉的方式套用（`restorePosition`）
- 還原的播放清單會重新隨機排序（`applyShuffle`），而不是保留它的播放順序
- 搜尋結果**永遠**從快取提供，沒有到期；`CacheTrack` 在每次播放時寫入，卻從未被讀取

<a id="what-went-wrong-there-and-the-criterion-that-covers-it-here"></a>
### 當時出了什麼問題，以及這裡由哪個條件涵蓋

1. **兩個行程爭用同一個資料庫**（`1d3faed`、`a8b5e7e`、tidalt #8 中 bolt 的鎖逾時）：父行程與客戶端開啟同一個儲存 → 只有播放器行程讀寫這個檔案；客戶端不在磁碟上開啟任何東西（0005 AC14，在此擴充：AC10）
2. **寫入錯誤看不見**（每次儲存都是 `_ =`）：磁碟已滿或目錄唯讀會讓工作階段默默遺失 → 失敗的寫入會記錄下來，並在播放器的訊息中顯示一次；播放繼續（AC7）
3. **寫到一半當機或被終止，可能損毀儲存**（bbolt 能抵禦當機；手寫的檔案不行）→ 檔案以原子方式取代（寫一個暫存檔、`fsync`、重新命名）；無法讀取或損毀的檔案絕不會讓播放器停止：它從空白開始、說明這件事，並把壞檔案另外保留（AC5、AC6）
4. **還原與啟動時的載入競爭**（`f904af2`）：收藏取代了還原的佇列 → 狀態是在**播放器處理第一個輸入之前同步讀取**的；第一個 `Welcome` 就已帶著它（AC8）
5. **最後一首曲目是用 ID 找的**（第一個含有該曲目的項目，有重複時就錯了），**而且只移動了游標** → 佇列連同它的項目 ID、目前項目 ID 與播放順序一起儲存，並完全照原樣還原（AC2）
6. **還原時重新套用隨機播放**，所以重新啟動後的下一首曲目與之前的不同 → 兩種順序都儲存；不重新隨機排序（AC2）
7. **儲存的位置在下一首曲目開始後才變成跳轉**（`restorePosition`，以及 `c3a59c3`：在長度為零的曲目上殘留的還原位置畫出了 100 % 的進度條），所以音訊會先從 0:00 播放才跳過去 → 播放器還原為**停在目前項目的儲存位置**；播放時串流直接從該位置開始（0004 的 `start_at`），沒有跳轉（AC3）
8. **播放時每秒寫入一次** → 只有位置的變化，在播放時最多每 30 秒儲存一次，並在暫停、停止、跳轉、換曲目與結束時立刻儲存；其他每一種變化在 2 秒內合併（AC4）
9. **從未失效或從未被讀取的快取**（永遠有效的搜尋結果、`CacheTrack`）→ 本規格不加入中繼資料快取（決策 3）；佇列自己的曲目中繼資料是唯一儲存的中繼資料，因為還原不可以依賴網路

<a id="behaviour"></a>
## 行為

<a id="what-is-remembered"></a>
### 記住什麼

**播放狀態**，由播放器行程（獨立模式的 TUI 或常駐程式）記住，存在一個檔案中：

| 記住的內容 | 還原為 |
|---|---|
| 佇列：每個項目連同它的項目 ID、它的曲目中繼資料（加入時的樣子：標題、版本、藝人、專輯、長度、streamable）與它的 `suggested` 標記 | 相同的項目、相同的 ID；新項目的 ID 從還原的最大 ID 往上 |
| 原始順序與播放順序 | 兩者都原封不動（不重新隨機排序） |
| 目前項目 | 同一個項目，**停止**（`■`） |
| 目前項目中的位置 | 顯示該位置；播放從那裡開始 |
| 隨機播放、重複播放、自動播放 | 相同的模式（自動播放：見「自動播放的優先順序」） |
| 音量與靜音 | 相同的音量與靜音 |

不記住的有：播放中／暫停的狀態（還原的播放器一律是停止的，所以開機時啟動的常駐程式是安靜的）、播放器的訊息、正在播放的詳細資訊（音質、輸出：它們來自下一次啟動）、預先載入、失敗、待處理的自動播放建議、登入工作階段（0002 保管它）、客戶端持有的任何東西（頁面歷史、游標、搜尋查詢：決策 4）、輸出裝置（決策 2）。

<a id="starting-from-the-remembered-state"></a>
### 從記住的狀態啟動

- 播放器**在處理第一個輸入之前**（以及在常駐程式綁定它的 socket 之前）**只讀取一次**檔案，所以第一個 `Welcome` 就已帶著還原的狀態。還原不需要擷取任何東西，在使用者播放之前，不解析任何串流，也不開啟任何裝置
- `space`（在有目前項目的停止播放器上的 `TogglePause`，0004）從**還原的位置**播放目前項目；`Next`、`Previous`、`PlayEntry` 的行為與任何停止的播放器相同
- 位於或超過項目已知長度的還原位置，還原為 `0:00`。不在佇列中的目前項目 ID，或不是項目排列的播放順序，會被修復：沒有目前項目，位置為 `0:00`；播放順序由原始順序重建，隨機播放**關閉**。修復是靜默的（檔案是我們寫的；手動編輯過的檔案能還原多少就還原多少）
- 超過 100 的音量會被限制為 100
- 還原的音量與靜音就是聽到的：播放器在啟動時、任何輸入之前，就把它們的增益交給音訊引擎，所以第一首曲目以還原的音量播放（AC15）
- 沒有播放器在執行時，`tidal-player ITEM…` 會先還原，再照現在的方式處理它的 `Open`（0005）：這些項目取代佇列並開始播放；隨機播放、重複播放、自動播放與音量維持還原的值
- `tidal-player play ITEM…`（無介面的前景指令，0004）既不讀也不寫這個檔案：它只播放被給予的內容，使用自己的旗標

<a id="saving"></a>
### 儲存

- **何時**：「記住什麼」中除了位置以外的任何內容有變動，都在 **2 秒**內儲存（期間的變動合併成一次寫入：狂按 `+` 只寫一次）。**播放**時，只有位置的變化最多每 **30 秒**儲存一次。暫停、停止、跳轉、換曲目，以及播放器的**結束**（來自 `q`、`daemon stop`、`SIGTERM`/`SIGINT` 的 `Shutdown`）都會立刻以確切的位置儲存。沒有任何記住的內容變動時不寫入
- **方式**：整個檔案先以模式 `0600` 寫入同一目錄中的 `playback.json.tmp`，`fsync` 之後再重新命名覆蓋 `playback.json`，所以當機後留下的是舊檔案或新檔案，絕不會是半個檔案。目錄不存在時會建立（`0700`）
- **當機或 `SIGKILL`**：最多遺失最後 30 秒的位置與 2 秒的變動
- **寫入失敗**（磁碟已滿、唯讀目錄、權限）：記錄下來，而播放器的訊息（0004「Failures」）會說一次 `Could not save the playback state: <error>`，直到有一次寫入再次成功；播放不受影響。下一次變動會重試

<a id="where"></a>
### 位置

`playback.json` 在**狀態目錄**中（0002：`$TIDAL_PLAYER_STATE_DIR`，否則 `$XDG_STATE_HOME/tidal-player`，否則 `~/.local/state/tidal-player`），與 `session.age` 並列。它是我們的資料，不是設定：絕不會從設定資料夾讀取，也絕不會被記載為可以編輯的東西。

```json
{
  "version": 1,
  "entries": [ { "id": 7, "track": { "id": 77640617, "title": "…", … }, "suggested": false } ],
  "play_order": [7, 3, 9],
  "current": 7,
  "position_ms": 83000,
  "shuffle": true,
  "repeat": "Queue",
  "autoplay": false,
  "volume": 80,
  "muted": false
}
```

<a id="a-missing-unreadable-or-corrupt-file"></a>
### 遺失、無法讀取或損毀的檔案

| 檔案 | 啟動 | 訊息 |
|---|---|---|
| 遺失（第一次執行、`logout` 之後） | 空白，與現在相同 | 無 |
| 無法讀取（權限、是目錄） | 空白 | `Could not restore the playback state: <path>: <io error>` |
| 不是有效的 JSON、缺少欄位，或 `version` 不是 1 | 空白；檔案被重新命名為 `playback.json.bad`（取代較舊的 `.bad`），讓下一次儲存不會毀掉證據 | `Could not restore the playback state (kept as playback.json.bad): <reason>` |

訊息是播放器的（0004），所以每個客戶端都會顯示它，直到第一首曲目開始。絕不使用結束碼：壞掉的狀態檔不可以讓常駐程式無法啟動（不像壞掉的 `app.toml`，那是使用者寫的，0008 決策 2）。

<a id="turning-it-off"></a>
### 關閉

`app.toml` 中的 `remember_playback`（預設 `true`），或 `TIDAL_PLAYER_REMEMBER_PLAYBACK`（`on`/`off`），優先順序依 0008（環境變數 > 檔案 > 預設值）。關閉時，播放器既不讀也不寫 `playback.json`，並保持現有的檔案不動。

<a id="precedence-of-autoplay"></a>
### 自動播放的優先順序

0004 把 `autoplay` 當作啟動設定（`--autoplay`、`TIDAL_PLAYER_AUTOPLAY`、`app.toml`）。有記住的狀態時：**旗標 > 環境變數 > 記住的值 > `app.toml` > 預設值**：檔案中的 `autoplay` 是全新佇列的預設值，記住的模式勝過它，而每次執行時明確指定的選擇勝過兩者（決策 5）。沒有其他設定與記住的值重疊：音量、隨機播放與重複播放沒有設定。

<a id="logout"></a>
### `logout`

`tidal-player logout` 也會刪除 `playback.json`（與 `.bad`），讓下一個帳號不會繼承上一個帳號的佇列；它的輸出與結束碼不變（0002）。正在執行的播放器會保留它的佇列，並在下一次儲存時再次寫入該檔案（已記載；`logout` 不會停止常駐程式，0002）。

<a id="acceptance-criteria"></a>
## 驗收條件

核心（`tidal_player_core::player`，純邏輯；儲存的型別 derive `Serialize`/`Deserialize`，`serde_json` 只在測試中使用）：

- **AC1** — `PlayerState::saved() -> SavedPlayback` 確切擷取記住的欄位（涵蓋各種狀態的表格：空的；停止；隨機播放開啟並播放中；曲目中途暫停；載入中且有起始位置；40 % 靜音；標為 suggested 的自動播放項目）；它不會隨訊息、正在播放的詳細資訊、預先載入或失敗而改變。`SavedPlayback` 經過 JSON 來回轉換後不變
- **AC2** — `PlayerState::restore(config, seed, saved)`（表格）：快照等於依播放順序排列的儲存佇列，項目 ID、目前項目、位置、模式、音量與靜音相同，狀態為 `Stopped`；下一次 `AddToQueue` 得到的 ID 高於還原的最大 ID；位於或超過已知長度的位置 → `0:00`；未知的目前 ID → 沒有目前項目，`0:00`；不是排列的播放順序 → 原始順序，隨機播放關閉；音量 150 → 100；還原空的儲存狀態等於 `PlayerState::new`。對 AC1 的每一列，`restore(…, s.saved()) .saved() == s.saved()`
- **AC3** — 播放還原的狀態：`TogglePause` 發出 `Resolve`，然後是 `EnginePlay { start_at: <restored position> }`（沒有 `EngineSeek`）；`Next`、`Previous`（相對於還原位置的重新開始門檻）與 `ToggleShuffle` 的行為，與經由指令抵達同一點的狀態相同（逐個效果比較）

儲存（`tidal-player::persist`，純邏輯，假時鐘）：

- **AC4** — 儲存排程（輸入序列隨時間 → 寫入 的表格）：一次音量變動 → 在 +2 秒寫一次；1 秒內十次音量變動 → 一次寫入；一次佇列編輯、一次模式切換 → 2 秒內一次寫入；只有位置、播放中 → 每 30 秒寫入一次，之間不寫；暫停、停止、跳轉、換曲目 → 立刻寫入；沒有變動 → 不寫；結束 → 立刻以確切位置寫入，之後不再寫入任何東西
- **AC5** — 寫入（暫存目錄）：檔案是狀態目錄中的 `playback.json`，模式 `0600`，它的目錄以 `0700` 建立；它經由 `playback.json.tmp` 與重新命名寫入（假的檔案系統接縫（seam）在寫入與重新命名之間失敗：舊檔案完好）

讀取（`tidal-player::persist`，暫存目錄）：

- **AC6** — 載入（表格）：遺失 → 空白，沒有訊息；目錄或模式 `000` → 空白，無法讀取的訊息；截斷的 JSON、缺少欄位、`"version": 2` → 空白，檔案被重新命名為 `playback.json.bad`（取代較舊的 `.bad`），損毀訊息指出原因；有效的檔案 → 它的 `SavedPlayback`。每則訊息如「遺失、無法讀取或損毀的檔案」所述
- **AC7** — 失敗的寫入（假的檔案系統先回傳 `ENOSPC`，然後成功）：播放器的訊息變成 `Could not save the playback state: …` 一次（下一次失敗不重複），播放的效果不受影響，而下一次成功的寫入會清除失敗旗標（訊息本身會保留到下一首曲目開始，如 0004 的訊息一樣）

執行階段與模式（`tidal-player`）：

- **AC8** — 常駐程式重新啟動（0005 的常駐程式測試框架、模擬 API、暫存狀態目錄）：載入一個佇列，隨機播放開啟，重複播放佇列，音量 70，跳轉到 1:23，`daemon stop`；再次啟動常駐程式：新客戶端的第一個 `Welcome` 等於儲存的快照，狀態為 `Stopped`（位置在測試框架的容許範圍內），而且在客戶端的 `TogglePause` 之前，模擬 API 沒有收到任何請求，假引擎除了還原的增益（AC15）之外也沒有收到任何指令；之後，引擎的 `Play` 從儲存的位置開始
- **AC9** — 獨立模式與一次性指令：獨立播放器（0004 的行程內測試框架）在第一個畫面之前還原；對記住的狀態執行 `tidal-player ITEM` 會取代佇列，並保留還原的模式與音量；`tidal-player play ITEM` 讓現有的 `playback.json` 逐位元組不變，也不會在空的狀態目錄中建立它
- **AC10** — 只有播放器碰這個檔案：0005 的 `ac14` 測試（客戶端的狀態目錄維持空白）原封不動地保持綠燈，並新增一列，其中播放器的狀態目錄放著一個 `playback.json`，客戶端執行後它仍逐位元組不變
- **AC11** — `remember_playback`（0008 的 `app.toml` 與優先順序表格新增它的列：`true`、`false`、錯誤的型別、`TIDAL_PLAYER_REMEMBER_PLAYBACK=off` 勝過檔案的 `true`、無效的變數是錯誤）：關閉 → 不讀、不寫、現有的檔案不動
- **AC12** — 自動播放的優先順序（表格）：預設值；`app.toml`；記住的值勝過 `app.toml`；環境變數勝過記住的值；`--autoplay` 勝過全部
- **AC13** — `logout` 刪除 `playback.json` 與 `playback.json.bad`，並印出與先前相同的內容（0002 的 `logout` 測試不變，新增一列）

TUI 與文件：

- **AC14** — 繪製（`insta`，80×24，用眼睛審閱，+ `contains`）：還原的狀態顯示 `■`、目前曲目的標題、`1:23 / 4:56` 與在該點的進度條，以及標出目前項目的佇列；訊息列上顯示損毀檔案的訊息。`docs/playback.md` 的「Resuming the last session」（記住什麼、何時儲存、在哪裡、如何關閉、損毀的檔案會怎樣）、`docs/daemon.md`、`docs/config.md`、`docs/login.md`、它們的 zh-TW 版本、`README.md`/`README.zh-TW.md` 與 `CLAUDE.md` 的「Features」都有說明；0004、0005 與 0006–0008 中「until 0009」的註記指向這裡。一個測試檢查 `remember_playback` 出現在 `docs/config.md` 中（0008 AC17 的測試，新增一列）
- **AC15** — 還原的播放器的引擎以還原的增益啟動（表格：音量 70 → `SetGain(0.343)`；40 時靜音 → `SetGain(0.0)`；沒有檔案 → `SetGain(1.0)`）：`PlayerRuntime::restored` 在第一個輸入之前確切送出那個 `SetGain`，不送別的，而 `TogglePause` 之後的 `Play` 接在它後面

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 第一次執行，沒有狀態目錄 | 從空白啟動；目錄在第一次儲存時建立 |
| 還原的目前曲目已不可串流，或已從 Tidal 移除 | 照儲存的樣子還原；播放時它和任何曲目一樣失敗（0004「Failures」：連同它的訊息被跳過） |
| 在登出狀態下還原（`LoginRequired`） | 照常還原；播放會失敗並顯示 0002 的訊息，直到 `login` |
| 10 000 個項目的佇列 | 幾 MB 的檔案；編輯時最多每 2 秒寫入一次，播放時每 30 秒一次 |
| 兩個播放器 | 不可能（0005：每位使用者一個播放器）；`play` 不寫入任何東西，所以不會蓋掉常駐程式的檔案 |
| 播放器執行中 `logout` | 檔案被刪除；執行中的播放器在下一次儲存時再次寫入 |
| 兩次執行之間 `TIDAL_PLAYER_STATE_DIR` 改變 | 每個目錄有自己的檔案；不遷移任何東西 |
| 狀態目錄在唯讀檔案系統上 | 可以啟動（空白或還原）；儲存失敗的訊息一次；正常播放 |
| 還原的位置在曲目的最後幾秒內 | 照原樣還原；播放會播完曲目並自動前進（0004） |
| 時鐘跳變（暫停／繼續） | 30 秒的排程使用單調時鐘；從暫停繼續後，在下一次超過 30 秒的位置變動時寫入 |
| `SIGKILL`、斷電 | 最後一個完整的檔案（AC5）；最多遺失 30 秒的位置 |
| 當機留下的 `.tmp` | 被下一次儲存覆蓋；絕不讀取 |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名（`ac4_…`）。紅燈是對存根型別與函式本體存根的斷言失敗（沒有 `todo!()`、沒有編譯錯誤），如同 0001–0008。

| AC | 測試（檔案 :: 名稱） | 驗證內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/player/tests/persistence.rs` :: `ac1_saved_fields` (table), `ac1_saved_round_trip` | 擷取的欄位；JSON 來回轉換 | 存根 `saved()` 回傳空的 `SavedPlayback` |
| AC2 | `crates/core/src/player/tests/persistence.rs` :: `ac2_restore` (table), `ac2_restore_saved_identity` | 還原的快照、ID、修復、限制；restore∘saved 恆等 | 存根 `restore` 回傳 `PlayerState::new` |
| AC3 | `crates/core/src/player/tests/persistence.rs` :: `ac3_play_restored`, `ac3_restored_equals_reached` (table) | 在該位置的 `EnginePlay`，沒有跳轉；與抵達的狀態有相同的效果 | 存根 restore 丟掉位置，所以 `start_at` 是 0 |
| AC4 | `crates/app/src/player/persist.rs` :: `ac4_save_schedule` (table, fake clock) | 每個輸入序列的寫入時間 | 存根排程器在每次變動都寫入 |
| AC5 | `crates/app/src/player/persist.rs` :: `ac5_write_file`, `ac5_write_is_atomic` | 路徑、模式、暫存檔 + 重新命名、重新命名失敗時舊檔案完好 | 存根就地寫入 `playback.json` |
| AC6 | `crates/app/src/player/persist.rs` :: `ac6_load` (table) | 結果、`.bad` 的重新命名、訊息 | 存根回傳空白，且不重新命名任何東西 |
| AC7 | `crates/app/src/player/persist.rs` :: `ac7_write_failure` | 訊息一次、重試、播放不受影響 | 存根忽略寫入錯誤（tidalt 的 `_ =`） |
| AC8 | `crates/app/tests/daemon.rs` :: `ac8_daemon_resumes` | 重新啟動後的 `Welcome`、播放之前沒有請求、`Play` 在該位置 | 存根 runtime 從不載入檔案 |
| AC9 | `crates/app/src/player/runtime.rs` :: `ac9_standalone_restores`, `ac9_items_replace_queue`; `crates/app/tests/cli.rs` :: `ac9_play_leaves_state_alone` | 在第一個畫面之前還原；項目取代佇列；`play` 不寫入任何東西 | 存根什麼都不還原；存根 `play` 在結束時儲存 |
| AC10 | `crates/app/tests/daemon.rs` :: the 0005 `ac14` test (unchanged) + `ac10_client_leaves_playback_file` | 檔案位元組不被客戶端改變 | —現有的測試是安全網；新增的列會對一個會儲存的存根客戶端失敗 |
| AC11 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows) + `crates/app/src/player/persist.rs` :: `ac11_remember_off` | 解析、優先順序、關閉時沒有 I/O | 存根忽略這個設定 |
| AC12 | `crates/app/src/settings.rs` :: `ac12_autoplay_precedence` (table) | 每個來源勝出的自動播放 | 存根讓 `app.toml` 勝過記住的模式 |
| AC13 | `crates/app/tests/cli.rs` :: the 0002 `logout` tests (unchanged) + `ac13_logout_forgets_playback` | 檔案消失、輸出不變 | 存根 `logout` 留下檔案 |
| AC14 | `crates/app/src/tui/render.rs` :: `ac14_restored_80x24`, `ac14_restore_message_80x24`; `crates/app/tests/docs.rs` :: `ac17_every_command_documented` (new row) + reviewed at acceptance | `contains` + 快照；文件 | 存根快照沒有目前項目 |
| AC15 | `crates/app/src/player/runtime.rs` :: `ac15_restored_gain_reaches_engine` (table) | 假引擎的第一個指令是在還原的增益下的 `SetGain`，在 `Play` 之前 | 沒有送出 `SetGain`：引擎維持在 1.0 |

驗收時在使用者的機器上親手檢查（結果寫在 PR 說明中）：`systemctl --user restart tidal-player` 會接續佇列，暫停在該位置，而 `space` 從那裡在 DAC 上播放，聽不到從 0:00 跳過來的聲音；離開並重新啟動獨立模式的 TUI 也一樣；重新開機後接續，常駐程式在播放之前都是安靜的。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::player`：`SavedPlayback`（+ `SavedEntry`）、`PlayerState::saved`、`PlayerState::restore` 與它的修復。沒有 I/O，沒有新的相依套件（`serde` 已經在那裡）
- `tidal-player`：`player::persist`（路徑、連同 `.bad` 重新命名的載入、位於一個小型檔案系統接縫（seam）之後的原子寫入、以先前儲存的狀態、新的狀態、播放中旗標與單調時刻為輸入的純函式儲存排程），接到播放器 runtime（獨立模式與常駐程式；不含 `play`）；`config` 中的 `remember_playback`；刪除這些檔案的 `logout`。`serde_json` 已經是這個執行檔的相依套件
- `xtask layering`：不變

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-08，從程式碼）：

- tidalt：如「tidalt 的做法」所述（檔案與 commit 都在那裡列出）
- 本儲存庫：`PlayerState` 把佇列保存為項目加上播放順序，並有項目 ID 計數器（`crates/core/src/player.rs`、`player/queue.rs`）；停止的播放器上的 `TogglePause` 透過 `EnginePlay { start_at }` 從 `position` 播放目前項目；`Track` 與 `QueueEntry` derive `Serialize`/`Deserialize`；狀態目錄與 `session.age` 來自 `store_setup.rs`；常駐程式把 `SIGTERM`/`SIGINT` 轉成 `Shutdown`（`daemon.rs`）；0005 的 `ac14` 測試斷言客戶端的狀態目錄維持空白
- spotify-player（README 與 `docs/config.md`，master，2026-10-08 閱讀）：一個快取資料夾（`-C/--cache-folder`，`$HOME/.cache/spotify-player`）存放記錄、憑證與選用的音訊快取；`enable_cover_image_cache`；`device.volume` 是啟動設定（70）；`pause_on_startup` 是以暫停啟動，而不是接續上一次的工作階段（接續是 Spotify Connect 在伺服器端做的）

假設，在驗收時檢查：狀態目錄內的 `rename(2)` 在使用者的檔案系統上是原子的（每個本機 Linux 檔案系統都是如此；某些網路檔案系統不保證，文件有提到）。

<a id="decisions-answered-by-the-user-2026-10-08-all-as-proposed"></a>
## 決策（使用者於 2026-10-08 回答：全部照提議）

1. **還原為暫停／停止，絕不播放中**：還原的播放器停在該位置，所以開機時啟動的常駐程式是安靜的，在 `space` 之前不會碰網路或裝置。替代方案：若結束時正在播放就繼續播放（`pause_on_startup = false` 時 spotify-player 的預設行為）
2. **不記住輸出裝置**：目前還沒有裝置選擇器（0004 延後了），所以裝置只會由 `--device`/`output_device` 選擇，而記住它會替一個設定加上第二個來源（0008 的「two sources of truth」）。等到有選擇器的規格時，它的選擇會記在這個檔案中。替代方案：記住上一次執行使用的裝置，並讓它勝過 `app.toml`（規格 0014 保持不這麼做：它的執行時選擇只持續到這次執行結束。）
3. **沒有中繼資料快取**：*已回答（使用者，2026-10-08）：「forget about caching lib」*：音樂庫與搜尋頁面持續擷取最新資料（0006）；tidalt 的快取不是過時就是從未被讀取，而這個播放器沒有封面需要快取。佇列儲存它的曲目的中繼資料，所以還原離線也行。替代方案：帶有到期時間的音樂庫頁面磁碟快取
4. **客戶端沒有任何東西**（頁面歷史、搜尋查詢、游標）：0005 禁止客戶端在磁碟上開啟 socket 以外的任何東西，而連上的 TUI 是客戶端。替代方案：由播放器儲存一個依客戶端區分、透過協定傳送的 blob
5. **自動播放的優先順序**：旗標 > 環境變數 > 記住的值 > `app.toml` > 預設值（檔案是全新佇列的預設值；每次執行時的選擇仍然勝出）。替代方案：`app.toml` 勝過記住的模式
6. **損毀的檔案**：從空白啟動，保留為 `playback.json.bad`，並說明這件事；絕不拒絕啟動。替代方案：拒絕啟動（如同 `app.toml`）
7. **`logout` 會忘記播放狀態**（下一個帳號不應該繼承上一個帳號的佇列）。替代方案：保留它
8. **預設開啟**，以 `remember_playback = false` 關閉。替代方案：預設關閉
9. **儲存時機**：變動以 2 秒合併，播放時的位置 30 秒，暫停／跳轉／停止／換曲目／結束時立刻

<a id="out-of-scope"></a>
## 不在範圍內

- 裝置選擇器與記住它的選擇（決策 2）
- 快取音樂庫頁面、搜尋結果或圖片（決策 3）
- 記住 TUI 的頁面歷史、游標或搜尋查詢（決策 4）
- 最近播放的歷史（tidalt 有；它會是自己的規格，連同它的頁面）
- 從 tidalt 的 `tidal-cache.db` 遷移任何東西
- MPRIS（[0010](0010-mpris.md)，implemented；它的封面快取是除了狀態以外唯一的磁碟快取）、mix 與電台（0011）、篩選與排序（0012）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-08"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-08）

- **還原**：重複的項目 ID 保留它的第一個項目，然後播放順序必須是排列（長度相同、沒有重複、ID 皆已知）；沒有已知長度的位置會保留；沒有目前項目表示 `0:00`；隨機播放關閉但播放順序不是原始順序時，照儲存的樣子還原。`restore` 取用儲存的自動播放；執行檔先套用 `start_autoplay`（旗標／環境變數勝過記住的值）。下一個項目 ID 是還原的最大 ID 加一
- **排程**：緊急變動（暫停、停止、`SeekBy`/`SeekTo`、改變的目前項目、開始的曲目）與結束，在狀態等於上一次寫入時不寫任何東西；非播放中的只有位置的變動會合併（2 秒）；播放中的位置寫入在上一次寫入之後最多每 30 秒一次，帶著最新的位置。runtime 把*播放中*推導為 `Playing`、`Buffering` 或 `Loading`，並在每次迴圈迭代（`POLL`，10 毫秒）檢查排程
- **檔案**：損毀的原因是 `unsupported version N` 或 serde 的訊息；失敗的 `.bad` 重新命名只會記錄下來。儲存失敗的訊息沒有路徑；失敗以 `tracing::warn!` 記錄。`logout` 在刪除工作階段之前先刪除播放檔案；若失敗，它在 stderr 印出 `Could not forget the playback state: …`，並保持它的輸出與結束碼
- **訊息**：核心新增了 `PlayerInput::Notice(String)`，它設定播放器的訊息並廣播；還原訊息在任何客戶端存在之前就套用，所以第一個 `Welcome` 帶著它
- **接線**：`PlayerRuntime::restored(…)`（常駐程式與獨立模式，在 socket 綁定之前建立）載入與儲存；`PlayerRuntime::new` 什麼都不記（`play` 與較舊的測試）
- **測試**：AC8 的「`Play` 在儲存的位置」是以假引擎在 `player_runtime.rs :: ac8_play_at_saved_position` 證明的（受測的常駐程式沒有音訊裝置）；`ac8_daemon_resumes` 檢查 `Welcome`、播放之前沒有模擬請求，以及之後的串流請求。`ac9_play_leaves_state_alone` 是一個在紅燈階段就通過的防護（`play` 從不持久化；要讓它變紅需要刻意編造的程式碼）。AC14 的快照是 `ac14_restored_80x24` 與 `ac14_restore_message_80x24`；`runtime_saves_on_schedule` 檢查排程與失敗寫入的接線
- **成本**：每個處理的輸入（包括引擎的位置）都為 `saved()` 複製佇列；在一般大小下沒問題，若 10 000 個項目的佇列出現在剖析（profile）中，則是可以精簡的候選

<a id="bugs"></a>
## 錯誤

- **還原的音量有顯示但聽不到（issue #41，使用者於 2026-10-18 回報）。** 預期（「從記住的狀態啟動」）：還原音量 70 % 的常駐程式或獨立播放器以 70 % 播放。實際：TUI、`playback` 與 MPRIS 顯示 70 %，但音訊以 100 % 播放，直到音量被改變一次。根本原因：`PlayerState::restore` 只在狀態中設定音量與靜音；音訊引擎以增益 1.0 啟動，並且只從音量與靜音指令發出的 `EngineSetGain` 效果接收 `SetGain`，所以沒有任何東西告訴它還原的增益。AC8 說「假引擎在 `TogglePause` 之前沒有任何指令」，而這個錯誤符合這句話。修正：`PlayerRuntime::restored` 在任何輸入之前，把還原狀態的增益送給引擎（`SetGain` 既不碰網路也不碰裝置，所以 AC8 的重點，即在播放之前不擷取也不開啟任何東西，仍然成立；它的措辭現在允許這一點）。新的條件：AC15。測試：`crates/app/src/player/runtime.rs` :: `ac15_restored_gain_reaches_engine`（紅燈：假引擎的記錄中沒有 `SetGain`）；`ac8_play_at_saved_position` 與 `ac9_standalone_restores` 預期在 `TogglePause` 之前有那個 `SetGain`
