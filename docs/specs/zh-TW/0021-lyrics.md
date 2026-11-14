[English](../0021-lyrics.md) | 正體中文

<a id="0021--lyrics"></a>
# 0021 — 歌詞

- **狀態**：implemented（2026-11-03；approved 2026-11-03；「測試計畫」下的手動檢查在使用者的機器上執行）；API 的形狀已由即時探測驗證（2026-11-03，見「事實」）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#54](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/54)
- **相依於**：0004（implemented：佇列、目前項目、`Position` 事件）、0005（implemented：客戶端、`Library`/`LibraryReply`、AC14）、0006（implemented：頁面、歷史、頁面中的失敗、失去連線的規則）、0008（implemented：按鍵對應、按鍵說明）、0013（implemented：按鍵提示）、0017（implemented：佈景主題、元件、略過的 spotify-player 元件）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Lyrics」一節與按鍵；[`docs/config.md`](../../zh-TW/config.md) 新增 `LyricsPage` 與 `lyrics_played`/`lyrics_playing` 元件，並把它們從略過清單中移除；以及它們的 zh-TW 副本（AC12）

<a id="context"></a>
## 背景

spotify-player 有歌詞頁（`LyricsPage`，`l` 與 `g L`）：顯示正在播放曲目的歌詞，同步歌詞會反白目前的那一行並讓它保持在視野內。本播放器在複製來的 `keymap.toml` 中略過 `LyricsPage`，在複製來的 `theme.toml` 中略過 `lyrics_played`/`lyrics_playing`（0008、0017）。本規格加上這個頁面。

tidalt 沒有歌詞。Tidal 從 `GET /v1/tracks/{id}/lyrics` 提供歌詞，已由探測驗證（「事實」）：來自 Musixmatch 的純文字與 LRC 計時的行、一個由右至左的旗標，以及曲目沒有歌詞時的 `404`。

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及由哪個驗收條件負責

1. **持有工作階段的客戶端**：由 TUI 自己取得歌詞，會需要每個客戶端都有工作階段，違反 0005 AC14 → 客戶端像每個頁面一樣，以 `Library` 請求向播放器詢問，由播放器取得（決策 1；AC3、AC4、AC11）
2. **錯誤曲目的歌詞**：請求進行中曲目改變了，遲到的回答被顯示在新的曲目上 → 回答只在它的曲目仍是頁面的曲目時才顯示（AC6）
3. **每次按鍵或每個位置事件就一個請求** → 客戶端在頁面位於最上層時，每次曲目變更只詢問一次，而播放器保留最近曲目的回答，包含「沒有歌詞」（決策 2；AC4、AC6）
4. **`404` 被顯示為失敗**：Tidal 對沒有歌詞的曲目（純樂器）回答 `404` → `No lyrics for this track`，而不是錯誤（AC3）
5. **LRC 的各種變化**（時間戳後沒有空格、`[mm:ss]` 或 `[mm:ss.xxx]`、一行有多個時間戳、中繼資料標籤、CRLF、空白的計時行）→ 一個純邏輯、以表驅動測試的解析器（AC2）
6. **反白偏離音樂**：目前的行由客戶端的位置算出，而播放器每 200 毫秒以及每次跳轉都會送出位置 → 一個以行與位置為輸入的純函式（AC2、AC7）
7. **由右至左的文字、長行、CJK、極小的終端機** → Tidal 標示歌詞為由右至左時靠右對齊，依顯示寬度換行，任何大小都不會 panic（AC9）

<a id="behaviour"></a>
## 行為

<a id="the-page"></a>
### 頁面

| 開啟方式 | 顯示 | 標題列 |
|---|---|---|
| 從任何地方按 `l` 或 `g L`（`LyricsPage`） | 播放器目前項目的歌詞 | `Lyrics · <title> · <artists>` |

- 這個頁面是歷史（0006）中的一個頁面：它被推到最上層；已在最上層時，按鍵什麼也不做；返回會離開它；返回到它會再次顯示它（歷史沒有「前進」）
- **它跟隨目前項目。** 每當目前項目的曲目與頁面顯示的曲目不同，而且頁面在最上層時，頁面顯示 `Loading…` 並詢問新曲目的歌詞（每次變更一個請求）。頁面不在最上層時什麼都不問；回到它時，若曲目在此期間改變就詢問，否則顯示保留的歌詞而不發出請求
- 目前項目不論是播放中、已暫停或已停止（0009 會還原已停止的項目）都算數：頁面顯示它的歌詞，以及它的位置處的反白。沒有目前項目時：`Nothing playing`，而且沒有請求
- 視窗的標題是 `Lyrics`，同步歌詞則是 `Lyrics (synced)`（不跟隨時為 `Lyrics (synced) · scrolled`，見下）
- 視窗中的訊息：`Loading…`、`No lyrics for this track`、`Nothing playing`；失敗為 `Could not load the lyrics: <reason>`（0006 的原因：`Tidal answered 429: try again in a moment`、工作階段過期的訊息）；失去連線時，原樣顯示 0006 失去連線的訊息，不加前綴，如 mix 與電台頁面（0011）

<a id="synced-and-plain-lyrics"></a>
### 同步與純文字歌詞

- **同步**（Tidal 的 `subtitles` 至少有一個計時的行）：每一行都會畫出；**目前行**是時間在位置之時或之前的最後一行；它之前的行以 `lyrics_played` 畫出，目前行以 `lyrics_playing` 畫出，其餘的用畫面的樣式。在第一行的時間之前，沒有行是目前行，每一行都未播放。時間相同的行維持 Tidal 的順序，目前行是其中的最後一個。沒有文字的計時行（器樂間奏、結尾的時間戳）是空白列，成為目前行時畫成 `♪`（決策 7）
- **跟隨**：頁面*跟隨*時，視窗會捲動，使目前行的第一列一旦超過中間，就停在視窗的中間那一列，如 spotify-player；在歌詞的最上方則不捲動。跳轉會隨下一個 `Position` 移動反白與視野
- **手動捲動**（決策 5）：清單移動鍵（`j`/`k`/`down`/`up`、`C-f`/`C-b`/`page_down`/`page_up`、`g g`/`G`）也能捲動同步歌詞，從視野所在處起，限制在歌詞之內。其中第一個按下就會停止跟隨：視野停在被放置的位置，而反白繼續移動，視窗的標題顯示 `Lyrics (synced) · scrolled`。**`Esc`**（`ClosePopup`，沒有開啟彈出視窗或輸入提示時）再次跟隨：視野跳回目前行；已經在跟隨時，什麼也不做。開啟頁面與新的曲目開始跟隨
- **純文字**（沒有計時行，`lyrics` 不為空）：依 Tidal 給的文字，以清單移動鍵（`j`/`k`/`down`/`up`、`C-f`/`C-b`/`page_down`/`page_up`、`g g`/`G`）捲動，從最上方開始；沒有任何東西被反白
- **兩者皆無**（兩者都空或缺少）：`No lyrics for this track`
- **長行**依視窗寬度以顯示寬度換行（CJK 算兩倍）；換行的目前行在它所有的列上都被反白
- **由右至左**（`isRightToLeft: true`）：每一列都靠右對齊；字元依 Tidal 的（邏輯）順序送出，由終端機排序（決策 6）
- 作用於列的按鍵（`Enter`、`Z`、對列的動作彈出視窗、`/`、`s …`、`d …`）在歌詞頁上什麼也不做；全域按鍵（播放、對正在播放曲目的動作 `a`、其他頁面、`?`）與別處一樣有作用

<a id="talking-to-the-player"></a>
### 與播放器溝通

0006 的 `Library` 請求，新增項目（`tidal_player_core::library`）：

- `PageRequest::Lyrics(u64)`（曲目 ID）→ `PageData::Lyrics { track: u64, lyrics: Option<Lyrics> }`；`None` 表示「沒有歌詞」
- `Lyrics { synced: Vec<LyricLine>, plain: String, right_to_left: bool }`、`LyricLine { at: Duration, text: String }`（`at` 在傳輸上以毫秒計）。播放器解析 `subtitles`（見下），因此客戶端收到的是行，不是 LRC

<a id="what-the-player-fetches-and-keeps"></a>
### 播放器取得與保留什麼

| 請求 | API 呼叫 | 保留 |
|---|---|---|
| `Page(Lyrics(id))` | `GET /tracks/{id}/lyrics?countryCode=…` | `subtitles` 解析成 `synced`、`lyrics` 解析成 `plain`（CRLF → LF，丟棄結尾的空行）、`isRightToLeft`（缺少 → `false`）。`404`（任何本文）→ `lyrics: None` |

- **歌詞快取**（決策 2）：播放器在記憶體中保留最近被詢問的 **50** 首曲目的回答，包含「沒有歌詞」；被詢問的曲目若已在其中，就不呼叫 API 直接回答，並移到最新的位置。失敗（`LoginRequired`、`429`、傳輸、`5xx`、格式錯誤）不保留。快取與播放器行程同壽（常駐程式的客戶端共用它）；不寫入磁碟，也沒有設定
- 其他錯誤依 0006，用上述頁面的措辭

<a id="parsing-lrc-tidal_player_corelyrics-pure"></a>
### 解析 LRC（`tidal_player_core::lyrics`，純邏輯）

- 一行是零個或多個**時間戳** `[mm:ss]`、`[mm:ss.x]`、`[mm:ss.xx]` 或 `[mm:ss.xxx]`（分鐘為任意位數；小數部分讀作秒的十進位小數），接著是去掉前後空格的文字。有多個時間戳的行，每個時間戳產生一個 `LyricLine`，文字相同
- 第一個方括號不是時間戳的行（`[ar:Adele]`、`[offset:…]`、任何其他標籤）與沒有時間戳的行會被略過。`\r` 被丟棄。秒數 ≥ 60 的不算時間戳
- 這些行依時間穩定排序（時間相同的維持檔案順序）
- `current_line(lines, position) -> Option<usize>`：最後一個 `at <= position` 的行的索引；在第一行之前與沒有行時為 `None`

<a id="keys"></a>
### 按鍵

| 指令 | 預設按鍵 | 作用於 | 說明文字 |
|---|---|---|---|
| `LyricsPage` | `l`、`g L` | 任何地方 | lyrics of the playing track |
| `ClosePopup`（既有） | `esc` | 歌詞頁，沒有開啟彈出視窗或輸入提示時（新） | follow the current line again |

`l` 與 `g L` 不是任何綁定的前綴，也不開啟任何前綴（0008 的衝突檢查維持乾淨）；`g L` 出現在 `g` 的按鍵提示中（0013）。`LyricsPage` 不再被回報為不支援的 spotify-player 指令。

<a id="theme-components"></a>
### 佈景主題元件

| 元件 | 畫在 | 預設（`default` 佈景主題） | spotify-player 的預設 |
|---|---|---|---|
| `lyrics_played` | 目前行之前的同步行 | `Dim` | `Dim` |
| `lyrics_playing` | 目前的同步行 | `fg = "Cyan"`、`Bold` | `fg = "Green"`、`Bold` |

它們離開 0017 的略過清單（清單仍保留 `playback_genres` 與 `visualization`）。`examples/theme.toml` 中的 `spotify_player` 佈景主題新增兩者，設定方式如 0017 AC14 對該佈景主題的設定。歌詞頁上不畫游標（`selection`）。

<a id="rendering"></a>
### 繪製

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Paper Lanterns · The Example Band                                       100%│
│  Night Market                                                                │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────────  1:10 / 3:42│
│Lyrics · Paper Lanterns · The Example Band                                    │
│┌Lyrics (synced)──────────────────────────────────────────────────────────────┐│
││We hung the lanterns on the wire                                     (played)││
││And watched the river carry them away                                (played)││
││…                                                                            ││
││Paper lanterns, burning slow                                        (playing)││
││Lighting up the road back home                                               ││
│…                                                                             │
```

（`(played)`/`(playing)` 的註記標示樣式；它們不會被畫出。曲目與它的歌詞是虛構的，如測試中那樣。）

<a id="acceptance-criteria"></a>
## 驗收條件

協定與型別（`tidal_player_core`，純邏輯）：

- **AC1** — `Lyrics`、`LyricLine`、`PageRequest::Lyrics` 與 `PageData::Lyrics`（有歌詞與沒有）在 JSON 來回轉換與 0005 的編解碼器下都能保存（0001 AC11 與 0005 AC2 的測試，新增的列）
- **AC2** — LRC（表）：探測的形式 `[00:06.32] text`；沒有空格；`[mm:ss]`、`[m:ss.x]`、`[mm:ss.xxx]`、`[123:00.00]`；一行有兩個時間戳 → 兩行；中繼資料標籤與沒有時間戳的行被略過；`[00:75.00]` 不是時間戳；CRLF；沒有文字的時間戳 → 一個空行；未排序的輸入被穩定排序。`current_line`（表）：沒有行；在第一行之前；恰好在一個時間戳上；兩個之間；最後一個之後；相同的時間戳 → 其中的最後一個

API（`tidal-player-api`，依探測的形狀做的 wiremock 測試資料）：

- **AC3** — `Page(Lyrics(id))` 送出帶有 `countryCode` 與 bearer 權杖的 `GET /tracks/{id}/lyrics`；帶有 `subtitles` 的 `200` → 解析出 `synced`、`plain`、`right_to_left`；`subtitles` 為空的 `200` → 只有 `plain`；`isRightToLeft: true` → `right_to_left`；帶有探測本文的 `404` → `lyrics: None`；`401`/`LoginRequired`、`429` 與 `5xx` 依 0006（表）

播放器（`tidal-player`）：

- **AC4** — 歌詞快取（表，會計算呼叫次數的假 API）：對同一首曲目的第二個請求不產生呼叫；「沒有歌詞」被保留；失敗不保留（下一個請求會再次呼叫）；第 51 首不同的曲目淘汰最久沒被詢問的；詢問已保留的曲目會讓它成為最新的。每個請求都以一個 `LibraryReply` 回答給它的送出者（0006 AC8 的測試框架，新增一列）

客戶端模型（`tidal_player_core::ui`，純邏輯）：

- **AC5** — 開啟（表）：`l` 與 `g L` 推入歌詞頁並送出 `Library { Page(Lyrics(current track)) }`；已在最上層時，什麼也不做；沒有目前項目時，頁面顯示 `Nothing playing`，也不送出任何東西；以相同曲目返回到該頁面時不送出任何東西
- **AC6** — 跟隨曲目（表）：頁面在最上層時，帶有新目前曲目的快照送出一個請求並顯示 `Loading…`；頁面不在最上層時的新曲目不送出任何東西，回到它時送出一個；`Position` 事件不送出任何東西；對已不是頁面曲目的曲目的回答被丟棄（頁面對目前的曲目維持 `Loading…`）；曲目相同的快照不送出任何東西
- **AC7** — 顯示狀態（表）：對同步歌詞與一個位置，哪一行是目前行、哪些是已播放的；往回跳轉會讓它回去；純文字歌詞以 `j`/`k`/`C-f`/`C-b`/`g g`/`G` 捲動並受限制；在同步歌詞上，移動鍵從視野所在處捲動並停止跟隨（視野不再隨 `Position` 移動），`Esc` 再次跟隨（視野回到目前行；開啟彈出視窗時，`Esc` 關閉它而仍不跟隨；已在跟隨時，什麼也不做），而新曲目或重新開啟頁面會跟隨；頁面上的 `Enter`、`Z`、`/`、`s c`、`d d` 不送出任何東西
- **AC8** — 按鍵對應（0008 AC2 的表，新增的列）：預設包含 `l` 與 `g L` → `LyricsPage`，出現在按鍵說明（`lyrics of the playing track`）與 `g` 的按鍵提示中；在歌詞頁上，按鍵說明列出 `esc` 為 `follow the current line again`；`LyricsPage` 不再是不支援的；`key_sequence = "l"`、`command = "None"` 會移除它；預設的按鍵對應沒有前綴衝突。失去連線與工作階段過期：0006 AC16 對歌詞頁的列

TUI 繪製（`tidal-player`）：

- **AC9** — 繪製（`insta`，80×24，以肉眼檢視，外加 `contains` 檢查）：在第一行之前、中間（目前行在中間那一列）、結尾，以及手動捲動後（`· scrolled`，目前行在視野外）的同步歌詞；捲動後的純文字歌詞；由右至左的歌詞（靠右對齊）；反白出現在每一列的換行長行；畫成 `♪` 的空白目前行；`Loading…`；`No lyrics for this track`；`Nothing playing`；失敗。50×20：同步歌詞。從 0×0 到 120×40，在有 CJK 與由右至左行的情況下都不會 panic
- **AC10** — 佈景主題：`lyrics_played` 與 `lyrics_playing` 是元件（`default`：`Dim`；`fg = "Cyan"`、`Bold`），不再被略過（0017 AC6 的通知測試，列已更新）；設定了它們的佈景主題會為已播放行與目前行上色（緩衝區檢查）；`examples/theme.toml` 能載入，而它的 `spotify_player` 佈景主題設定了兩者（0017 AC14 的測試）

接線：

- **AC11** — 連上的客戶端按 `l` 由測試播放器的 wiremock API 回答，而且客戶端不開啟任何工作階段儲存（0005 AC14 的測試，一個 `l` 步驟）

文件：

- **AC12** — `docs/tui.md` 記載歌詞頁、`l`/`g L`、同步與純文字歌詞，以及各訊息；`docs/config.md` 列出 `LyricsPage` 與兩個元件，並把它們從不支援與略過的清單中移除；`examples/keymap.toml` 列出 `l` 與 `g L`；zh-TW 副本與 `README.md`/`README.zh-TW.md` 相符；`CLAUDE.md` 的「Features」有這個項目；0008 與 0017 不再把 `LyricsPage`、`lyrics_played` 或 `lyrics_playing` 稱為略過的

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 純樂器（Tidal `404`） | `No lyrics for this track`；保留在快取中 |
| 未知的曲目 ID（同樣是 `404`） | 相同：Tidal 不區分它們 |
| 同步歌詞的 `subtitles` 只有標籤或亂碼 | 沒有計時行：顯示為純文字歌詞（來自 `lyrics`），或 `No lyrics for this track` |
| 請求進行中曲目改變 | 遲到的回答被丟棄；送出新曲目的請求（AC6） |
| 自動播放移到建議的曲目 | 目前項目改變了：頁面跟隨（AC6） |
| 單曲重複：曲目重新開始 | 同一首曲目：沒有請求；位置回去，反白也跟著回去 |
| 已暫停或已停止 | 歌詞留著，反白在保留的位置 |
| 同一個常駐程式上的兩個 TUI 開啟頁面 | 兩個請求；第二個由快取回答 |
| `429`、工作階段過期、失去連線 | 如 0006 在頁面中顯示（`Could not load the lyrics: …`；失去連線的訊息不加前綴）；失去連線時，在最上層的頁面於下一個 `Welcome` 重新取得（AC8） |
| 比視窗寬的行 | 換行；目前行在每一列都被反白（AC9） |
| 在不支援 bidi 的終端機中的由右至左歌詞 | 靠右對齊，字元為邏輯順序：終端機可能顯示成反的（決策 6） |
| 手動捲動後曲目改變 | 新曲目的歌詞，再次跟隨 |
| 手動捲動後，在動作彈出視窗開啟時按 `Esc` | 彈出視窗關閉；仍不跟隨；第二個 `Esc` 才跟隨 |
| 1 列高的視窗，或 0×0 | 目前行的第一列（或什麼都沒有）；不會 panic（AC9） |
| 登出 | 播放器行程結束或失去它的工作階段；快取隨行程而去 |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的驗收條件命名（`ac4_…`）。紅燈是針對存根型別與函式、其本體為存根（沒有 `todo!()`、沒有編譯錯誤）的斷言失敗。API 測試使用 `wiremock`，測試資料放在 `crates/api/tests/fixtures/lyrics/`，依「事實」下的形狀寫成，帶有**虛構的歌詞文字**（歌詞受著作權保護；探測的輸出只保留前幾行，而且不提交）。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | 來回轉換；解碼 | 存根 `Serialize` 寫出 `null` |
| AC2 | `crates/core/src/lyrics.rs` :: `ac2_parse_lrc` (table), `ac2_current_line` (table) | 行、時間、順序；目前索引 | 存根解析器回傳沒有行；存根 `current_line` 回傳 `None` |
| AC3 | `crates/api/tests/lyrics.rs` :: `ac3_lyrics` (table) | 路徑、參數、對應、`404` → `None`、錯誤 | 存根對每個請求都回答 `NotFound` |
| AC4 | `crates/app/src/player/runtime.rs` :: `ac4_lyrics_cache` (table) + `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new row) | API 呼叫次數；淘汰順序；一個回應 | 存根快取什麼都不保留，所以呼叫是 2 次而不是 1 次 |
| AC5 | `crates/core/src/ui/lyrics/tests.rs` :: `ac5_open_lyrics` (table) | 推入、請求、`Nothing playing`、歷史 | 存根 `LyricsPage` 什麼也不做 |
| AC6 | `crates/core/src/ui/lyrics/tests.rs` :: `ac6_follow_track` (table) | 每次變更的請求；遲到的回答被丟棄 | 存根從不重新請求 |
| AC7 | `crates/core/src/ui/lyrics/tests.rs` :: `ac7_display_state` (table) | 目前／已播放；捲動；跟隨與 `Esc`；無作用的按鍵 | 存根從不停止跟隨，所以移動鍵讓視野停在目前行 |
| AC8 | `crates/core/src/ui/keymap/tests.rs` :: `ac8_lyrics_keys` + 0008's help and unsupported tests (rows updated) + `crates/core/src/ui/browse/tests.rs` :: `ac16_disconnected_and_login` (new rows) | 預設按鍵、說明文字、提示、解除綁定；失去連線 | 存根讓 `LyricsPage` 維持不支援 |
| AC9 | `crates/app/src/tui/render.rs` :: `ac9_lyrics_80x24` (one snapshot per row), `ac9_lyrics_narrow_50x20`, `ac17_no_panic_any_size` (new page) | `contains` + 快照 | 存根繪製畫出佇列 |
| AC10 | `crates/core/src/ui/theme/tests.rs` :: `ac10_lyrics_components`, `ac6_theme_notice` (rows updated) + `crates/app/tests/examples.rs` :: `ac14_example_theme_toml_loads` | 預設值；沒有被略過；緩衝區樣式；範例 | 存根讓它們維持被略過 |
| AC11 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (an `l` step) | 已回答；沒有工作階段 | 存根客戶端丟掉請求 |
| AC12 | — reviewed at acceptance | 文件與本規格相符、有連結、zh-TW 同步 | — |

在驗收時以真實帳號手動檢查（結果寫在 PR 說明中）：在播放英文、中文與阿拉伯文曲目時按 `l`，顯示它們的歌詞（阿拉伯文靠右對齊）；反白在約四分之一秒內跟上歌聲，並隨跳轉而跳動；`n` 把頁面移到下一首曲目的歌詞；純樂器顯示 `No lyrics for this track`；有兩個 TUI 的常駐程式顯示相同的內容。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`lyrics`（新：`LyricLine`、`Lyrics`、`parse_lrc`、`current_line`）、`library`（`PageRequest::Lyrics`、`PageData::Lyrics`）、`ui`（歌詞頁：`PageKind::Lyrics`、跟隨曲目、捲動狀態）、`ui::keymap`（`LyricsPage` 及其預設值）、`ui::theme`（兩個元件）。沒有 I/O，沒有新的相依套件
- `tidal-player-api`：歌詞呼叫及其 DTO 對應（`library.rs`，或它旁邊新的 `lyrics.rs`）
- `tidal-player`：`player::runtime`（在 `Library` trait 的歌詞請求前面的快取）、`ui.rs`/`ui/pages.rs`（頁面）
- `xtask layering`：沒有變更

<a id="facts-vs-assumptions"></a>
## 事實與假設

於 2026-11-03 由 `scripts/tidal-lyrics-probe.sh` 對**即時 API** 驗證（國家 NG，帶有 `HI_RES` 的 `PREMIUM` 訂閱；唯讀；該腳本在規格實作完成後已刪除；它的輸出不提交）。測試資料依這些形狀寫成：

- `GET /v1/tracks/{id}/lyrics?countryCode=NG` → `200`、`content-type: application/json`、`{trackId, lyricsProvider: "MUSIXMATCH", providerCommontrackId, providerLyricsId, lyrics, subtitles, isRightToLeft}`，對象為 Adele *Hello*（62300893）、Fairuz *Kifak Inta*（1558491，`isRightToLeft: true`，阿拉伯文）、周杰倫 青花瓷（337502711）與 Queen *Bohemian Rhapsody*（534050211）
- `lyrics`：以 LF 分隔的純文字，段落之間有空行（*Hello* 為 60 行、1965 位元組）。`subtitles`：每句唱詞一行，`[mm:ss.xx] text`（分鐘一律兩位數、小數兩位數、時間戳後**一個空格**），段落之間沒有空行，有些時間戳沒有文字（`[01:55.15] ` 的間奏；*Hello* 有 4 個這樣的行），最後一行是接近曲目結尾的單獨時間戳（4:56 的曲目為 `[04:40.31]`）。沒有 CRLF。每首有歌詞的曲目兩者都有
- 播放器可能持有的兩種權杖都能用：裝置流程客戶端的與 PKCE 客戶端的（0002 AC19）。`deviceType`/`locale` 不改變任何事。沒有權杖 → `400 {"error":"invalid_request","description":"Token is missing","subStatus":11002}`
- 沒有歌詞（Brian Eno *1/1*，1403577，純樂器）與未知的曲目（1）→ `404 {"timestamp", "status": 404, "error": "Not Found", "path": "/lyrics/public/<id>"}`（不是 Tidal 一貫帶有 `subStatus` 的本文）
- 曲目（`GET /v1/tracks/{id}`、搜尋結果）沒有任何欄位說明它有沒有歌詞：唯一的辦法是去問
- `listen.tidal.com/v1/tracks/{id}/lyrics` → `301`；不使用
- `openapi.tidal.com/v2/tracks/{id}/relationships/lyrics` 與 `?include=lyrics` → 一個 `lyrics` 資源，帶有 `text`、`lrcText`（時間戳後沒有空格，段落之間有空行）、`technicalStatus: "OK"`、`provider {source: "THIRD_PARTY", name: "MUSIXMATCH", …}`、`direction: "LEFT_TO_RIGHT"`。不使用：v1 帶有相同的內容，而且形狀與播放器其他部分使用的一致
- 客戶端的位置：引擎每 200 毫秒回報一次（`POSITION_EVERY`，`crates/audio/src/engine.rs`），以 `Event::Position` 轉送給客戶端，由客戶端模型在它屬於目前項目時套用（`crates/core/src/ui/mod.rs`）
- spotify-player（`/home/df/CLionProjects/spotify-player`，`f1522b8`，2026-11-03 閱讀）：`LyricsPage` 綁定到 `g L` 與 `l`（`src/config/keymap.rs`）；`render_lyrics_page`（`src/ui/page.rs`）把頁面標題訂為 `<track> by <artists>`，把目前行之前的行設為 `lyrics_played`（預設 `Dim`），目前行設為 `lyrics_playing`（預設 `Green`、`Bold`），並捲動，使目前行一旦超過中間就停在中間；頁面不接受任何指令（`src/event/page.rs`）；沒有歌詞時顯示 `Lyrics not found`
- 目前的程式碼：`l` 與 `g L` 未綁定（`examples/keymap.toml`）；`LyricsPage` 被回報為不支援（`crates/core/src/ui/keymap.rs`）；`lyrics_played`、`lyrics_playing` 在 `SKIPPED_COMPONENTS` 中（`crates/core/src/ui/theme.rs`）

未驗證：

- 其他訂閱等級與國家：只探測了一個帳號。沒有歌詞的等級或國家，很可能回答 `404`（顯示為 `No lyrics for this track`）或 `403`（顯示為失敗）；不論哪一種都不會壞掉
- 有純文字歌詞但沒有 `subtitles` 的曲目：沒有找到；解析器與頁面都能處理（AC3、AC7）
- 時間戳領先或落後音訊多遠；在驗收時手動檢查

<a id="decisions"></a>
## 決策

由使用者回答（2026-11-03；已併入上面的本文）：

1. **誰來取得**：*客戶端按需詢問，播放器取得*（`Library` 請求，如 0006 的頁面；頁面不在最上層時什麼都不取得）。0005 AC14 維持：客戶端不持有工作階段。備選：TUI 以自己的工作階段取得（會推翻 AC14）
2. **快取**：*在記憶體中，在播放器裡*，最近 50 首曲目，包含「沒有歌詞」，不含失敗。備選：沒有；放在磁碟上
3. **`tidal-player playback lyrics`**：*不在範圍內*
4. **按鍵**：*`l` 與 `g L`*，如 spotify-player

5. **捲動同步歌詞**（使用者：「捲動時要有一個按鍵可以跟隨回來」；按鍵：`Esc`）：移動鍵捲動同步歌詞並停止跟隨；`Esc` 再次跟隨；新曲目或重新開啟頁面會跟隨。備選：不能捲動（spotify-player 的頁面不接受任何指令）；在下一行時彈回的捲動；以 `Enter` 或新的 `FollowLyrics` 指令作為按鍵
6. **由右至左**：*靠右對齊，字元順序留給終端機*（播放器在其他任何地方都不替文字排序，而且支援 bidi 的終端機，例如 VTE 與 Konsole，會把重新排序過的行再反轉一次）。備選：像 spotify-player 那樣以 `unicode-bidi` 為顯示重新排序每一行，這在不支援 bidi 的終端機中讀起來正確，在支援的終端機中則是錯的
7. **空白的目前行**（器樂間奏）：*畫成 `♪`*，讓反白看得見。備選：空白列，反白看不見
8. **預設值**：*如 spotify-player 的*，盡可能讓 0017 AC14 容許它的 `spotify_player` 佈景主題照著它們：`lyrics_played` 為 `Dim`、`lyrics_playing` 為 `fg = "Cyan"`、`Bold`。備選：不上色（`Dim`、`Bold`）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-11-03"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-11-03）

- **核心**：`tidal_player_core::lyrics` 有 `Lyrics`、`LyricLine`（`at` 在傳輸上為整數毫秒）、`parse_lrc` 與 `current_line`（`partition_point`）。`PageKind::Lyrics` 沒有酬載；頁面帶有 `Page.lyrics: Option<LyricsView>`（`track`、`lyrics`、`following`、`scroll`），而 `Page::request()` 在 `fetch_page` 中取代 `PageKind::request()`。`apply_snapshot` 回傳效果，並以 `follow_track` 結尾，它只對位於最上層的歌詞頁作用，並比較曲目 ID（同一首曲目在新的項目 ID 下不會詢問任何東西）；`browse::back` 也呼叫它，回到頁面時只詢問一次就是這樣來的；在 `Welcome` 時它在 `reconnected` 之前執行，所以重新連線只詢問一次。回答在它的請求 ID 過時，或它的 `track` 不是頁面的曲目時被丟棄。位於最上層的失敗歌詞頁上按 `l` 會再次取得，如其他頁面。兩段文字都是空白 → `No lyrics for this track`
- **捲動**：模型以*行*為單位捲動（它不知道寬度）；TUI 以 `pages.rs` 的 `wrap`（顯示寬度）把行偏移轉成列。跟隨時，目前行的第一列位於列 `h / 2`（絕不在 0 之上）。手動捲動限制在 `len - height`，或在被跟隨的偏移更遠時限制在該偏移；同步歌詞上的任何移動鍵都會停止跟隨，即使偏移沒有移動；純文字歌詞從不顯示 `· scrolled`。視窗的內部高度是 0006 的 `list_height`，因此 `Resize` 不變
- **按鍵說明**：`Where::Lyrics` 一節把 `ClosePopup` 列為 `follow the current line again`（跟隨時為淡色），與 Lists、Pages、Playback 與 App 各節並列。`COMMANDS` 有 51 個指令，`UNSUPPORTED_COMMANDS` 有 26 個
- **API**：這個呼叫位於 `crates/api/src/library.rs`，在 `page()` 旁邊。它自己讀取本文，因此任何 `404` 不論本文為何都是 `lyrics: None`；其他狀態走 0006 的 `check`。不是 JSON 的 `200` 是 `malformed lyrics response from Tidal`。每個欄位都是選用的；`plain` 把 CRLF 轉成 LF 並丟棄結尾的空行
- **播放器**：`LyricsCache`（`crates/app/src/player/runtime.rs`）包裝給 `TokioJobs::with_library` 的 `Library`：一個 `LYRICS_CACHE_SIZE` = 50 的 `Mutex<VecDeque>`，命中的移到後面，插入從前面淘汰，只保留 `Ok(Page(Lyrics))`。兩個客戶端同時詢問同一首未快取的曲目時，兩者都會呼叫 API（回答是相同的）
- **TUI**：視窗中的訊息為淡色，失敗用 `error`；純文字歌詞不套用樣式，也不畫游標。由右至左的歌詞使用 `Alignment::Right`
- **佈景主題**：`COMPONENTS` 有 21 個；`SKIPPED_COMPONENTS` 保留 `playback_genres` 與 `visualization`，需要略過名稱的測試現在使用它們
- **測試資料**（`crates/api/tests/fixtures/lyrics/`）：依探測的形狀寫成的虛構文字；`lyrics_synced.json` 也帶有 CRLF 與結尾的空行，這是探測沒有看到的

<a id="out-of-scope"></a>
## 不在範圍內

- `tidal-player playback lyrics`（決策 3）
- 磁碟上的歌詞、預先擷取下一首曲目的歌詞
- 逐字（卡拉 OK）計時；提供者的署名行
- 選擇某一行來跳轉（行上的 `Enter`）；自行彈回的捲動
- MPRIS 中繼資料中的歌詞（`xesam:asText`）
- 歌詞的翻譯

<a id="bugs"></a>
## 錯誤

- **測試計畫的紅燈原因（驗收時發現，2026-11-03）。** 有些測試沒有在計畫所寫的紅燈原因上失敗。AC6 與 AC7：紅燈的存根讓 `l` 維持未綁定，所以 `ac6_follow_track` 與 `ac7_*` 測試是在開啟頁面時失敗（測試輔助函式中的索引越界），而不是在「從不重新請求」或「從不停止跟隨」上；在一次用完即丟的執行中把開啟做成真的之後，它們就依計畫的原因失敗。AC9：存根什麼都沒畫，不是畫出佇列（`"Lyrics (synced)" missing`）。AC11：切片 A 的客戶端已經送出請求，所以紅燈是存根 API 的 `Track 1001 was not found`，不是被丟掉的請求。新的 `ac8_reply_to_sender_lyrics` 列與 `ac17_no_panic_any_size` 在存根上就通過了，並留作回歸防護。測試是對的；這個條目修正的是計畫
