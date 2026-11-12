[English](../0011-mixes-and-radio.md) | 正體中文

<a id="0011--mixes-and-radio"></a>
# 0011 — Mix 與電台

- **狀態**：implemented（2026-10-09；「測試計畫」下的手動檢查在使用者的機器上執行）；API 的形狀已由即時探測驗證（2026-10-09，見「事實與假設」）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0004（implemented：佇列、自動播放的曲目電台）、0006（implemented：頁面、歷史、隨捲動載入的清單、動作彈出視窗、`Library`/`LibraryReply`）、0008（implemented：按鍵對應、`[[actions]]`、按鍵說明）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Mixes」與「Radio」兩節以及新按鍵；[`docs/config.md`](../../zh-TW/config.md) 新增 `MixesPage` 與 `GoToRadio`（AC13）

<a id="context"></a>
## 背景

Tidal 會為每位使用者建立 mix（*My Daily Discovery*、*My New Arrivals*、依曲風或年代的每日 mix），並為任何曲目或藝人建立電台。到目前為止，播放器只在自動播放背後使用它們（0004：佇列結束時的曲目電台）。本規格讓它們可以瀏覽，如同 spotify-player 的電台頁面：

- 一個列出使用者 mix 的 **mix 清單頁**，以及每個 mix 各自的 **mix 頁**，是與專輯頁相似的曲目清單
- 曲目或藝人的**電台頁**，以 spotify-player 的 *Go to radio* 動作（`r`）開啟

Mix 連結仍不被接受為項目（決策 5）。

凡是 0006 已有的機制，本規格都不另立新的：mix 頁與電台頁是歷史中的頁面，由播放器擷取，其列就是 0006 的列，使用 0006 的按鍵與動作。與 0006 的清單不同，它們是整份一次到齊：Tidal 以一個回應提供整個 mix，電台上限為 100 首曲目（見「事實與假設」）。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 tidalt 的 `internal/tidal/api.go`（`GetMixes`、`GetMixTracks`、`GetTrackRadio`）、`internal/ui/{keys,overlay}.go`、`docs/architecture.md` 的「Daily Mixes」，以及下列提到的 commit：

- **Mix 來自已移除的 API**（`b1eddc8`、`40bf41e`，然後是 `e253b75`）：v2 的 `openapi.tidal.com/v2/userRecommendations/me/relationships/myMixes` 只回傳裸曲目 ID，每首曲目再各自擷取一次 `/v1/tracks/{id}`，然後重新排序。Tidal 移除了該資源，畫面因此是空的。`e253b75` 改用 v1：清單用 `GET /v1/pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US`（每個模組的 `pagedList.items`，略過重複與沒有 ID 的項目，丟棄 `mixType` 含 `VIDEO` 的項目：在作者的帳號上保留 9 個 mix、丟棄 8 個影片 mix），曲目用 `GET /v1/mixes/{id}/items`（完整曲目，`{item, type}`，丟棄非 `track` 的項目）
- **對 mix 按 `Enter` 會立刻以它的曲目取代佇列**：播放之前無法看到 mix 裡有什麼，也無法在其中挑選曲目
- **Mix 清單只擷取一次**（啟動時，以及該區段第一次開啟時），之後從不更新，儘管每日 mix 每天都會變
- **電台立刻播放**（`r`，*Start radio from this*）：`GET /v1/tracks/{id}/radio?limit=100` 取代佇列，並標記為 `radio · unsaved`。只有曲目電台，沒有藝人電台
- **Mix 連結**（`tidal.com/mix/<id>`）在它的搜尋輸入框中被接受，並把 mix 載入佇列

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及涵蓋它的條件

1. **端點被移除或變更**（tidalt：v2 `userRecommendations` → `404`，畫面是空的、沒有任何訊息）→ 使用探測得到的 v1 端點，測試資料（fixture）依其形狀製作，擷取失敗時頁面會說明（AC2、AC10）
2. **影片 mix 與影片項目被當成曲目加入佇列** → 影片 mix 不列入清單；mix 與電台清單丟棄不是曲目的項目，以及沒有 `STEREO` 的曲目（AC2、AC3、AC4）
3. **每首曲目一個請求**（tidalt 的 v2 扇出）→ mix 的曲目以完整曲目的形式，在一個請求中從它的 items 取得（AC3）
4. **mix 還沒看過就被播放**（tidalt 的 `Enter`）→ 對 mix 按 `Enter` 開啟它的頁面；只有對曲目按 `Enter` 才會播放，並把 mix 的其餘部分排入佇列（0006 的規則）（AC7）
5. **過期的每日 mix**（tidalt：只擷取一次）→ 開啟 mix 清單頁一律會擷取，與每個 0006 頁面相同（AC6）
6. **把佇列丟掉的電台**（tidalt 的 `r`）→ `r`（*Go to radio*）開啟的是頁面；佇列只在 `Enter` 或 `Z` 時改變，與其他各處相同（AC6）
7. **沒有電台的種子**（`404`、「Track radio cannot be generated」，0004「事實與假設」）→ 一個說明此事的空電台頁，而不是失敗（AC4）
8. **對 mix 分頁會重複它**：`/mixes/{id}/items` 忽略 `limit` 與 `offset`，永遠回答整個 mix（探測：`offset=50` 與 `offset=100` 回傳相同的 10 首曲目），所以 0006 的捲動載入會重複顯示，或陷入迴圈 → 每個 mix 只發一個請求，絕不發第二個（AC2）
9. **看起來沒載完的清單**：電台回報 98 或 100 首曲目，被丟棄的列會讓客戶端一直要求更多 → mix 與電台清單以保留下來的列數作為總數，客戶端因此不會再要求更多（AC3、AC4）

<a id="behaviour"></a>
## 行為

<a id="pages"></a>
### 頁面

三種新的頁面，開啟後與 0006 的頁面一樣保存在歷史中：

| 頁面 | 開啟方式 | 視窗 | 標題列 |
|---|---|---|---|
| Mixes | `g m`（`MixesPage`*），從任何地方 | 使用者的 mix | `Mixes · 7 mixes` |
| Mix | 對 mix 按 `Enter`；它彈出視窗中的 *Open* | mix 的曲目 | `<title> · 10 tracks` |
| Radio | 對選取的曲目或藝人按 `r`（*Go to radio*），或彈出視窗中的項目 | 電台的曲目 | `<track title> Radio · <artists>` / `<artist name> Radio`（spotify-player 的 `<name> Radio`） |

- **Mix 列**：標題，接著是副標題（Tidal 的 `subTitle`：每日 mix 是 `<artist>, <artist>, <artist> and more`，*My Daily Discovery* 是一句話），與 0006 的列一樣以 `…` 截斷。依 Tidal 的順序
- Mix 頁的標題列是 mix 的標題；頁面至少有 8 列高時，副標題會以第二個淡色標題列顯示在它下方
- mix 頁與電台頁上的**曲目列**：0006 的曲目列（無法串流的以淡色顯示）
- 空的時候：`No mixes yet`、`This mix has no tracks`、`No radio for this track`、`No radio for this artist`。頁面中的失敗：`Could not load the mixes: <reason>`、`Mix <id> was not found`、`Track 1 was not found`、`Artist 1 was not found`
- 每個頁面在開啟時都會重新擷取（0006，0009 決策 3）；返回時顯示的是它原本的樣子

<a id="whole-lists-no-scroll-loading"></a>
### 整份清單，不隨捲動載入

這三個頁面都是整份到齊：播放器以每一列回答每個請求，清單的 `total` 就是它持有的列數，因此 0006 的捲動載入與整份清單擷取都不會要求更多（它們沒有 `More`，也沒有 `ListRef`）。Mix 清單最多只有幾十個（探測帳號上有 15 個，其中 7 個是音訊）；一個 mix 是一個回應（看過 10 到 100 首曲目）；一個電台最多 100 首曲目。`page_size` 不適用。

<a id="playing-queueing-and-actions"></a>
### 播放、加入佇列與動作

0006 的表格仍然成立，mix 是新的一種列：

| 按鍵 | 對象 | 作用 |
|---|---|---|
| `Enter` | mix | 開啟它的頁面 |
| `Enter` | mix 頁或電台頁上的曲目 | `LoadQueue { every track of the list, start: index }`（清單是整份的：不必先擷取任何東西） |
| `Z`、`C-z` | mix | 無作用，與藝人相同：mix 不是項目（決策 5）；請開啟它，再對它的曲目使用 `Enter` 或 `Z` |
| `r` | 曲目或藝人列（任何頁面，包括佇列） | 開啟它的電台頁（*Go to radio*） |

| 項目 | 動作，依此順序 |
|---|---|
| Mix | *Open* |
| 曲目（瀏覽頁、佇列項目、正在播放的曲目） | 0006 的清單，在 *Go to artist* 項目之後加上 ***Go to radio*** |
| 藝人 | *Open*、***Go to radio***、*Add to favorites*/*Remove from favorites* |

- 對曲目的 *Go to radio* 開啟 `Radio(track)`；對藝人則開啟 `Radio(artist)`；專輯、播放清單或 mix 沒有它（決策 4）。這是 spotify-player 的 `GoToRadio` 動作（它原本在 0008 略過的清單中），預設綁定在選取列的 **`r`**（決策 3；tidalt 的按鍵）。其他按鍵以 `[[actions]] action = "GoToRadio"` 綁定，針對正在播放的曲目則用 `target = "PlayingTrack"`；`[[keymaps]] key_sequence = "r"`、`command = "None"` 會解除綁定
- 收藏 mix 不在範圍內：mix 列的彈出視窗沒有收藏項目

<a id="talking-to-the-player"></a>
### 與播放器溝通

0006 的 `Library` 請求，新增項目：

- `PageRequest::Mixes` → `PageData::Mixes { mixes: ListPage<MixSummary> }`
- `PageRequest::Mix(String)`（mix ID，Tidal 給的 30 個十六進位數字）→ `PageData::Mix { mix: MixSummary, tracks: ListPage<Track> }`
- `PageRequest::TrackRadio(u64)` → `PageData::Radio { seed: RadioSeed::Track(Track), tracks: ListPage<Track> }`；`PageRequest::ArtistRadio(u64)` → `PageData::Radio { seed: RadioSeed::Artist(ArtistRef), tracks }`
- 沒有新的 `ListRef`：清單是整份的（見上）
- `MixSummary { id: String, title, subtitle: Option<String> }`

錯誤沿用 0006 的措辭；mix 上的 `404`/`2001` 是 `Mix <id> was not found`。

<a id="what-the-player-fetches"></a>
### 播放器擷取什麼

每次呼叫都帶 `countryCode`，與其他各處相同。形狀以探測為準（「事實與假設」）。

| 請求 | API 呼叫 | 保留 |
|---|---|---|
| `Page(Mixes)` | `GET /pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US`；若某個模組的 `pagedList` 持有的項目少於它的 `totalNumberOfItems`，其餘的從它的 `dataApiPath` 以 `limit`/`offset` 取得（用播放器自己的 offset：回應回報的是 `offset: 0`），直到達到總數 | 每個 `MIX_LIST` 模組的 `pagedList.items[]`：`id`、`title`、`subTitle`（空的 → `None`）；`mixType` 含 `VIDEO` 的丟棄；重複或空的 `id` 丟棄 |
| `Page(Mix(id))` | `GET /pages/mix?mixId={id}&deviceType=BROWSER&locale=en_US`（`MIX_HEADER` 模組的 `mix`：`title`、`subTitle`）與 `GET /mixes/{id}/items`（沒有 `limit`、沒有 `offset`：兩者都會被忽略），平行執行 | `MixSummary`；`type == "track"` 的 `items[]`，其 `item` 依 0004 的曲目對應，沒有 `STEREO` 的丟棄 |
| `Page(TrackRadio(id))` | `GET /tracks/{id}`（0004）與 `GET /tracks/{id}/radio?limit=100`（0004 AC27），平行執行 | 標頭曲目；沒有 `STEREO` 的裸曲目丟棄。種子是電台的第一首曲目，且**會保留**：對它按 `Enter` 會播放種子，電台接在它後面 |
| `Page(ArtistRadio(id))` | `GET /artists/{id}`（0006）與 `GET /artists/{id}/radio?limit=100`，平行執行 | 標頭藝人；沒有 `STEREO` 的裸曲目丟棄 |

- `/pages/mix` 或 `/mixes/{id}/items` 上的 `404`/`2001` → `Mix <id> was not found`；電台標頭上則是 0006 的 `Track 1 was not found` / `Artist 1 was not found`。**電台**的 `404`/`2001`（`Track radio cannot be generated…`、`Artist radio cannot be generated for artist [1]`）是空清單（`No radio for this track` / `… artist`），自動播放已是如此處理（0004 AC27）
- 任何一個呼叫失敗，整個頁面就失敗（0006）；`ListPage` 的 `total` 是保留下來的列數
- `/mixes/{id}` 不存在（`404`）；標頭只來自 `/pages/mix`。自動播放（0004）不變：它仍自己呼叫曲目電台

<a id="keys"></a>
### 按鍵

| 指令 | 預設按鍵 | 作用範圍 | 說明文字 |
|---|---|---|---|
| `MixesPage`* | `g m` | 任何地方 | your mixes |

| 動作（`[[actions]]`） | 預設按鍵 | 目標 | 說明文字 |
|---|---|---|---|
| `GoToRadio` | `r` | `SelectedItem` | the radio of the selected track or artist |

`g m` 與 `r` 都不是其他綁定的前綴，也不開啟任何前綴（0008 的衝突檢查維持乾淨）。`r` 是第一個預設的 `[[actions]]` 綁定：0008 的預設值多了一個動作部分，顯示在按鍵說明與 `examples/keymap.toml` 中（其 "none by default" 的註解隨之更改）。`GoToRadio` 在清單中作用（「作用範圍」同 `ShowActionsOnSelectedItem`）；在沒有電台的列上則什麼也不做。

<a id="rendering"></a>
### 繪製

與 0006 相同：每個頁面一個視窗，標題列在它上方，視窗中有 `Loading…`、失敗與空白訊息。任何大小下都不會 panic。

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                                            100%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────  1:23 / 3:32│
│Mixes · 7 mixes                                                               │
│┌Mixes (7)────────────────────────────────────────────────────────────────────┐│
││My Daily Discovery     Songs by new and familiar artists inspired by your li…││
││My Mix 1               Pierce The Veil, Sleeping With Sirens and more         ││
│…                                                                             │
```

<a id="acceptance-criteria"></a>
## 驗收條件

協定與型別（`tidal_player_core`，純邏輯）：

- **AC1** — `MixSummary`、`RadioSeed` 以及新的 `PageRequest` 與 `PageData` 變體，能通過 JSON 往返，也能通過 0005 的編解碼器（0001 AC11 與 0005 AC2 的測試，新增列）；`parse_item` 仍然拒絕 mix 連結（0004 AC16 既有的列）

API（`tidal-player-api::library`，來自探測的 wiremock 測試資料）：

- **AC2** — `Page(Mixes)`：當模組持有它的總數時（探測的 15 之 15），一個帶有 `deviceType`、`locale`、`countryCode` 的 `GET /pages/my_collection_my_mixes`；否則是 offset 遞增的 `dataApiPath` 請求，直到達到總數，儘管每個回應都說 `offset: 0`；mix 依 Tidal 的順序；`VIDEO_DAILY_MIX`（任何 `mixType` 含 `VIDEO` 的）、重複與空的 ID 丟棄；`total` = 保留的 mix 數；`LoginRequired` 與暫時性錯誤同 0006（表格）
- **AC3** — `Page(Mix(id))`：標頭來自 `/pages/mix`，曲目來自**恰好一個**既沒有 `limit` 也沒有 `offset` 的 `/mixes/{id}/items`（wiremock `expect(1)`），平行執行；`video` 項目與沒有 `STEREO` 的曲目丟棄；`total` = 保留的曲目數；任一者的 `404`/`2001` → `Mix <id> was not found`；任何呼叫失敗，頁面就失敗（表格）
- **AC4** — 電台：`TrackRadio(id)` 送出 `GET /tracks/{id}` 與 `GET /tracks/{id}/radio?limit=100`，`ArtistRadio(id)` 送出 `GET /artists/{id}` 與 `GET /artists/{id}/radio?limit=100`；種子曲目保留在第一位；沒有 `STEREO` 的曲目丟棄；`total` = 保留的曲目數（探測的 98 首就是 98）；電台的 `404`/`2001` → 空清單；標頭的 `404` → `NotFound`（表格）

播放器（`tidal-player`）：

- **AC5** — 新的 `Library` 請求與任何 0006 請求一樣被回答：對傳送者的一個 `LibraryReply`，與輸入迴圈並行（0006 AC8 的測試架構，新增列）

客戶端模型（`tidal_player_core::ui`，純邏輯）：

- **AC6** — `g m` 推入 mix 清單頁並送出 `Library { Page(Mixes) }`（已在最上層：什麼也不做）；對 mix 按 `Enter` 推入它的頁面並送出 `Page(Mix(id))`；返回時顯示保留的列，不發請求；捲動到這三個頁面中任何一個的結尾都不送出 `More`，對曲目按 `Enter` 立刻送出 `LoadQueue`（表格）
- **AC7** — 列（表格）：對 mix 頁或電台頁上第 *i* 首曲目按 `Enter`，送出 `LoadQueue { the whole list, start: i }`，之前不送 `More`；對 mix 按 `Z`/`C-z` 不送出任何東西；mix 的彈出視窗只列出 *Open*
- **AC8** — *Go to radio*（表格）：對瀏覽頁的曲目、佇列項目與正在播放的曲目（`a`）列在 *Go to artist* 項目之後，對藝人列在 *Open* 之後，專輯、播放清單與 mix 沒有；執行它，或在曲目列（瀏覽頁或佇列）或藝人列上按 `r`，會推入 `Radio` 並送出 `Page(TrackRadio(id))` / `Page(ArtistRadio(id))`；在專輯、播放清單或 mix 列，或在空清單上按 `r` 什麼也不做；沒有任何東西會改變佇列或送出指令
- **AC9** — 按鍵對應（0008 AC2 的表格，新增列）：預設值包含 `g m` → `MixesPage` 與 `r` → `GoToRadio`（`SelectedItem`），兩者都在按鍵說明中（`your mixes`、`the radio of the selected track or artist`）；使用者的 `[[actions]] action = "GoToRadio"` 兩種目標都能解析；`key_sequence = "r"`、`command = "None"` 會移除預設值；使用者的綁定 `r x` 與預設的 `r` 衝突（0008 的前綴規則），直到 `r` 被解除綁定；`GoToRadio` 不再被回報為不支援的 spotify-player 動作；預設按鍵對應沒有前綴衝突
- **AC10** — 中斷連線與工作階段過期：0006 AC16 的列，套用到三個新頁面（以中斷連線的訊息失敗，在最上層時於下一個 `Welcome` 重新擷取）

TUI 繪製與執行階段（`tidal-player`）：

- **AC11** — 繪製（`insta`，80×24，人工檢視，另加 `contains` 檢查）：已載入的 mix 清單頁（含數量的標題、副標題）；mix 頁；曲目電台頁；藝人電台頁；`No mixes yet`；`No radio for this track`；失敗的 mix 清單頁；曲目上顯示 *Go to radio* 的動作彈出視窗。50×20：mix 頁。這三個頁面在 0×0 到 120×40 之間任何大小都不會 panic
- **AC12** — 接線：已連上的客戶端按 `g m`，由測試播放器的 wiremock API 回答，且不開啟工作階段儲存區（0006 AC19 的測試，新增一個 `g m` 步驟）

文件：

- **AC13** — `docs/tui.md` 記載 mix 清單頁、mix 頁與電台頁、`g m`、`r`、*Go to radio* 與各訊息；`docs/config.md` 列出 `MixesPage` 與 `GoToRadio` 的預設值，並把 `GoToRadio` 從不支援的清單中移除；`examples/keymap.toml` 顯示 `g m` 與 `r` 動作；zh-TW 版本與 `README.md`/`README.zh-TW.md` 相符；`CLAUDE.md` 的「Features」有對應條目；本規格連結到它們

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 沒有任何 mix 的新帳號 | `No mixes yet` |
| 只有影片 mix | 丟棄：`No mixes yet` |
| 有 10 首曲目的 mix（*My Daily Discovery*） | 10 列，標題 `… · 10 tracks`；不再要求更多 |
| 把 mix 連結貼到 `o`/`O` 或交給 `play` | 與先前一樣被拒絕：`Not a Tidal track, album or playlist: <item>`（決策 5） |
| 在清單擷取之後消失的 mix（每日 mix 會輪替） | 頁面中顯示 `Mix <id> was not found` |
| Tidal 沒有電台的曲目 | 頁面中顯示 `No radio for this track`；佇列不受影響 |
| 對無法串流的曲目使用 *Go to radio* | 頁面照常開啟（電台仍可能存在） |
| 身為種子的電台曲目 | 保留在第一位，所以對它按 `Enter` 會播放種子，電台接在後面 |
| 頁面開啟之後每日 mix 變了 | 返回時顯示舊的列（0006 的歷史規則）；從其他頁面按 `g m` 則重新擷取 |
| `429` | 頁面顯示 `Tidal answered 429: try again in a moment`；不重試 |
| 工作階段過期、中斷連線 | 0006 的規則（AC10） |
| 窄的終端機 | 一個視窗，與 0006 相同（AC11） |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名（`ac4_…`）。紅燈是針對存根型別與函式、其本體為存根（沒有 `todo!()`、沒有編譯錯誤）的斷言失敗，如同 0001–0010。API 測試使用 `wiremock`，測試資料放在 `crates/api/tests/fixtures/mixes/` 下，依「事實與假設」下的形狀撰寫（目錄資料是公開的；使用者 ID 已取代；圖片 URL 與權杖已移除）。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) + `crates/core/src/item.rs` :: `ac16_parse_item` (existing mix-link row, unchanged) | 往返；解碼；仍被拒絕 | 存根的 `Serialize` 寫出 `null` |
| AC2 | `crates/api/tests/mixes.rs` :: `ac2_mixes_list` (table) | 路徑、參數、篩選、`dataApiPath` 分頁、錯誤 | 存根保留了影片 mix |
| AC3 | `crates/api/tests/mixes.rs` :: `ac3_mix_page` (table) | 標頭、一個 items 請求、丟棄、總數、`NotFound` | 存根以 `offset` 分頁，所以 `expect(1)` 失敗 |
| AC4 | `crates/api/tests/mixes.rs` :: `ac4_radio` (table) | 路徑、保留種子、`404` → 空 | 存根在電台 `404` 時讓頁面失敗 |
| AC5 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new rows) | 對傳送者的一個回覆 | 存根以錯誤回答新的請求 |
| AC6 | `crates/core/src/ui/browse/tests.rs` :: `ac6_mixes_pages` (table) | 推入、請求、保留的狀態、沒有 `More` | 存根的 `g m` 什麼也不做 |
| AC7 | `crates/core/src/ui/browse/tests.rs` :: `ac7_mix_rows` (table) | 每一列的確切指令；彈出視窗 | 存根對 mix 按 `Enter` 送出 `LoadQueue` |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_go_to_radio` (table) | 在彈出視窗中的位置；每種列的 `r`；頁面與請求；沒有指令 | 存根的彈出視窗省略 *Go to radio*，`r` 什麼也不做 |
| AC9 | `crates/core/src/ui/keymap.rs` :: `ac9_mixes_and_radio_keys` + 0008's help and unsupported-action tests (rows updated) | 預設按鍵、動作解析、解除綁定、說明文字 | 存根仍把 `GoToRadio` 視為不支援，也沒有綁定 `r` |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_disconnected` (table) | 沒有送出任何東西；一次重新擷取 | 存根在中斷連線時送出請求 |
| AC11 | `crates/app/src/tui/render.rs` :: `ac11_mixes_80x24` (one snapshot per row), `ac11_mixes_narrow_50x20`, `ac17_no_panic_any_size` (new pages) | `contains` + 快照 | 存根的繪製畫出了佇列 |
| AC12 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (a `g m` step) | 已回答，沒有工作階段 | 存根的客戶端丟掉請求 |
| AC13 | — reviewed at acceptance | 文件與本規格相符、已連結、zh-TW 同步 | — |

驗收時以真實帳號手動檢查（結果寫在 PR 說明中）：`g m` 列出的 mix 與 Tidal 應用程式的 *My Mixes* 相同（影片 mix 除外）；mix 頁以相同順序顯示相同的曲目；對曲目按 `Enter` 會播放它，mix 的其餘部分接在後面；對曲目與藝人按 `r`，以及對正在播放的曲目使用 *Go to radio*，都會開啟看起來相關的電台。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`library`（`MixSummary`、`RadioSeed`、新的請求與頁面）、`ui`（頁面、mix 列、*Go to radio*）、`ui::keymap`（`MixesPage`、預設的 `GoToRadio` 綁定）。沒有 I/O，沒有新的相依套件
- `tidal-player-api::library`：mix 清單、mix 頁與電台的呼叫，以及它們的 DTO 對應
- `tidal-player`：`player::runtime`（`Library` trait 回答新的請求）、`ui.rs`/`ui/pages.rs`（這三個頁面）
- `xtask layering`：沒有變更

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-09，來自程式碼與文件）：

- 0004 的探測（2026-10-07，即時 API）：`GET /v1/tracks/{id}/radio` → 裸曲目，預設每頁 10 首，最多 100 首，種子在第一位；`GET /v1/mixes/{TRACK_MIX}/items` → `{item, type: "track"}` 項目，預設每頁 100 首，相同的 100 首曲目、相同順序；未知的曲目 → `404`/`2001` `Track radio cannot be generated for trackId [1]`；未知的 mix → `404`/`2001` `Mix items for [...] not found`
- 上述 tidalt 的 v1 呼叫在即時 API 上使用與我們相同的 client ID 運作正常（`e253b75`，2026：9 個 mix，丟棄 8 個影片 mix；曲目帶有藝人與專輯）
- 0008 把 spotify-player 的 `GoToRadio` 列在略過的動作中（`docs/config.md`）
- spotify-player（`spotify_player/src/command.rs` 的 `construct_*_actions`、`src/event/mod.rs` 的 `handle_go_to_radio`，2026-10-09 閱讀）：`GoToRadio` 在曲目的動作中（位於 `GoToArtist`、`GoToAlbum` 之後）、專輯、藝人（第一個）與播放清單（第一個）；它開啟一個標題為 `<name> Radio`、以該項目為種子的曲目頁；它沒有預設按鍵（`src/config/keymap.rs`），也沒有 mix 清單頁
- 目前的程式碼：`Item` 有 `Track`、`Album`、`Playlist`，並拒絕 mix 連結（`crates/core/src/item.rs`）；`PageRequest`/`ListRef` 如 0006/0007（`crates/core/src/library.rs`）；0008 的預設值沒有綁定 `g m` 或 `r`，也沒有 `[[actions]]`（`examples/keymap.toml`："none by default"）

2026-10-09 以 `scripts/tidal-mixes-probe.sh` 對**即時 API** 驗證，由使用者執行（國家 NG；唯讀；腳本已在這次更新後刪除）。`crates/api/tests/fixtures/mixes/` 下的測試資料依這些形狀撰寫：

- `GET /v1/pages/my_collection_my_mixes?deviceType=BROWSER&locale=en_US` → `200 {id, title: "My Mix", rows: [{modules: [{type: "MIX_LIST", title: "", pagedList: {dataApiPath: "pages/data/<uuid>", limit: 20, offset: 0, totalNumberOfItems: 15, items}, supportsPaging: false, …}]}]}`：一列、一個模組，全部 15 個 mix 都在第一頁。項目：`{id, title, subTitle, description, graphic, images {SMALL, MEDIUM, LARGE}, sharingImages, mixType, contentBehavior, shortSubtitle}`；`mixType` 有 `DISCOVERY_MIX`（*My Daily Discovery*，`subTitle` 是一句話）、`DAILY_MIX`（*My Mix 1*…，`subTitle` 是 `<artist>, <artist>, <artist> and more`，`description` 是曲風）、`VIDEO_DAILY_MIX`（*My Video Mix 2*…）。Mix ID 是 30 個小寫十六進位數字
- 它的 `dataApiPath` 加上 `limit=10&offset=10` → `200 {limit: 10, offset: 0, totalNumberOfItems: 15, items}`，內容是**最後 5 個** mix：offset 有被套用，但回應回報的是 `offset: 0`
- `GET /v1/pages/mix?mixId={id}&deviceType=BROWSER&locale=en_US` → `{title, rows: [{modules: [{type: "MIX_HEADER", mix: {id, title, subTitle, mixType, mixNumber, images, detailImages, titleColor, …}, playbackControls}]}, {modules: [{type: "TRACK_LIST", pagedList: {dataApiPath: "pages/data/<uuid>?mixId={id}", limit: 10, totalNumberOfItems: 10, items: [bare tracks, with album.releaseDate]}}]}]}`。未知的 mix → `404 {"subStatus":2001,"userMessage":"Not found"}`
- `GET /v1/mixes/{id}` → `404`/`2001` `Resource not found`：沒有這個資源
- `GET /v1/mixes/{id}/items` → `{limit, offset, totalNumberOfItems, items: [{item, type}]}`，`item` 是 0004 測試資料中的完整曲目（帶有 `artist`、`artists[]`、`album {id, title, cover}`、`audioModes`、`mixes.TRACK_MIX`）。**`limit` 與 `offset` 被忽略**：對 *My Daily Discovery*，不帶參數、`limit` 50/100/200/1000、`offset=50` 與 `offset=100`，全都回答 `limit: 10, offset: 0, total: 10`，且是相同的 10 首曲目。帶 `limit=3` 的影片 mix 回答了全部 50 個項目，`type: "video"`，`album: null`。帶 `limit=100` 的藝人 mix 回答 100 之 100。未知的 mix → `404`/`2001` `Mix items for [<id>] not found`
- `GET /v1/tracks/{id}/radio?limit=100`（種子 145060431）→ 98 首裸曲目，**種子在第一位**，`totalNumberOfItems: 98`；`limit=50&offset=50` → 最後 48 首（這個端點確實會分頁）
- `GET /v1/artists/{id}/radio`（藝人 35361）→ 裸曲目，預設每頁 10 首，`totalNumberOfItems: 100`；`limit=100` → 全部 100 首；**`limit=1000` → `400`/`1001` `Too big page, max page size is [100]`**；未知的藝人 → `404 {"subStatus":2001,"userMessage":"Artist radio cannot be generated for artist [1]"}`。它的前 8 個曲目 ID 與該藝人 `mixes.ARTIST_MIX` 項目的相同：藝人電台*就是*藝人 mix，如同 0004 對曲目的發現
- `GET /v1/artists/{id}` 帶有 `mixes: {ARTIST_MIX: <id>}`；曲目帶有 `mixes: {TRACK_MIX: <id>}`
- 取樣的每個 mix 與電台曲目都是 `["STEREO"]`；為安全起見保留 Dolby-Atmos 的丟棄，與 0007 相同

未驗證：

- 超過第一頁的 mix 清單（`dataApiPath` 的逐頁走訪只以測試資料測試）
- 超過 100 首曲目的 mix（沒看過；單一請求照單全收 Tidal 的回答）
- Tidal 應用程式是否把種子保留在曲目電台的最上面

<a id="decisions-answered-by-the-user-2026-10-09-folded-into-the-body-above"></a>
## 決策（使用者已回答，2026-10-09；已併入上方正文）

1. **Mix 放在哪裡**：*自己的頁面，`g m`*（「gm is fine」）
2. **電台**：*一個頁面*（「as a page」），如同 spotify-player 的 *Go to radio*；在 `Enter` 之前不會播放任何東西
3. **為 *Go to radio* 設定預設按鍵**：*`r`*（「r can be fine」），tidalt 的按鍵，作用於選取的列
4. **哪些電台**：*只有曲目與藝人*（「y」）；沒有專輯、播放清單或 mix 電台
5. **把 mix 連結當作項目**：*不要*。Mix 連結仍被 `parse_item` 拒絕，所以 `play`、`o`/`O`、`playback load/add` 與 MPRIS `OpenUri` 都不變，也沒有 `Item::Mix`。因此，mix 列無法整個加入佇列（對它按 `Z` 沒有作用，它的彈出視窗只有 *Open*）：先開啟它，再對曲目按 `Enter` 就會把整個 mix 加入佇列

<a id="out-of-scope"></a>
## 不在範圍內

- 收藏 mix，以及「收藏的 mix」清單
- Tidal 的首頁、探索與曲風頁面，以及 *My Mixes* 以外的編輯精選 mix
- 影片 mix 與影片
- 搜尋結果中的 mix 與電台（Tidal 的搜尋在 0007 使用的清單中沒有 mix 類型）
- 把電台或 mix 儲存為播放清單
- 專輯與播放清單電台
- 變更自動播放（0004）：它仍使用曲目電台
- 一次性的 `playback radio …`
- 把 mix 連結當作項目，以及從 mix 的列把整個 mix 加入佇列（決策 5）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-09"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-09）

- **API**：`Subject::Mix(id)` 呈現為 `Mix <id>`；內部的 `Subject::Radio` 標記電台的 `404`/`2001`，並轉成空清單。Mix 清單只讀取 `MIX_LIST` 模組；它的 `dataApiPath` 走訪使用模組自己的 `limit`（`0` 時用第一頁的項目數），以播放器自己的 offset 請求，並送出 `deviceType`/`locale`，遇到空的回答就停止。空的或全是空白的 `subTitle` 是 `None`。無法讀取的曲目項目會被略過，與搜尋相同。曲目電台的標頭曲目不依音訊模式篩選。沒有預期模組的回應本體是 `malformed mixes/mix response from Tidal`
- **播放器**：沒有變更：`page_size_for` 退回到 `page_size`，這些頁面不使用它
- **客戶端模型**：`PageKind::{Mixes, Mix, TrackRadio, ArtistRadio}`、`Header::{Mix, Radio}`、`WindowKind::{Mixes, MixTracks, RadioTracks, ArtistRadioTracks}`（最後一個只用於它的空白訊息）。涵蓋整份清單的視窗有 `whole: true` 與一個不會被送出的惰性佔位 `ListRef`：`ask_more` 提早返回，且一知道總數視窗就算完成。只有 mix 清單頁會為它的失敗加上前綴（`Could not load the mixes: …`）；mix 頁與電台頁照原樣顯示播放器的訊息。空的 mix 清單頁標題是 `Mixes · 0 mixes`。對正在播放的曲目使用 *Go to radio* 時，使用它的曲目 ID。預設的 `r` → `GoToRadio` 是 `defaults()` 中的一個 `Binding::Action`；`GoToRadio` 接在 `ACTIONS` 中的 `GoToArtist` 之後；按鍵說明新增一個「Actions」區段
- **TUI**：mix 列是一行，`title  subtitle`，整體一起截斷（列是逐行套用樣式的，所以副標題沒有另外變淡）。「至少 8 列高」是以播放視窗下方的頁面區域來量測
- **建置**：共用同一個 `CARGO_TARGET_DIR` 的 worktree 會重複使用彼此的測試執行檔（一個音訊測試執行檔保留了已移除 worktree 的 fixture 路徑）；平行的切片需要各自的 target 目錄

<a id="bugs"></a>
## 錯誤

- **測試計畫的錯誤，紅燈的原因（驗收時發現，2026-10-09）。** 計畫為每個測試寫了各自的紅燈失敗，但共用的存根並不會產生那些失敗。AC2–AC4：紅燈的存根以 `Malformed("page")` 回答每個新頁面，所以每個測試都在第一個斷言就以該錯誤失敗，而不是在「保留影片 mix」、「以 `offset` 分頁」或「在電台 `404` 時讓頁面失敗」上。AC7：它的紅燈失敗在建立 mix 頁的存根 `g m`，還沒到 `Enter`/`Z` 的列。AC5 與 AC12 沒有紅燈：伺服器測試的假實作（fake）庫回答任何請求，而 常駐程式的 `g m` 步驟在切片 A 與 B 一併入後就通過了。兩者都保留作為回歸防護。測試是對的；本條目修正的是計畫
