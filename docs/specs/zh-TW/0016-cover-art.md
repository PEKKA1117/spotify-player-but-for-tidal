[English](../0016-cover-art.md) | 正體中文

<a id="0016--album-cover-in-the-playback-window"></a>
# 0016 — 播放視窗中的專輯封面

- **狀態**：implemented（2026-10-12；2026-10-12 核准）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#19](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/19)
- **相依於**：0004（implemented：畫面與它的播放視窗）、0005（implemented：連上的 TUI）、0008（implemented：`app.toml` 與它的優先順序）、0010（implemented：`AlbumRef::cover`、`cover_url`、封面快取與 `max_cover_arts`）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 的「Playback window」新增「Album cover」；[`docs/config.md`](../../zh-TW/config.md) 新增 `cover_img`、`cover_img_rows` 與 `cover_img_protocol`；[`docs/mpris.md`](../../zh-TW/mpris.md) 的「Album covers」（MPRIS 關閉時也會快取）；它們的 zh-TW 版本（AC10）

<a id="context"></a>
## 背景

播放視窗（0004）以四行文字顯示目前的曲目。spotify-player 把專輯封面畫在它們的左側，tidalt 也畫過（Kitty 圖形協定，後來是 sixel：`internal/ui/coverart.go`、`becf508`）。0010 已經讓每首曲目帶有封面 ID、建立它的 URL（`cover_url`），並為 MPRIS 維護一個封面快取，而把 TUI 的封面留給本規格。

終端機透過三種協定之一繪製圖片：**sixel**（foot、WezTerm、以 `-ti vt340` 啟動的 xterm、mlterm、Konsole）、**kitty** 圖形協定（kitty、Ghostty、部分的 WezTerm）與 **iTerm2** 行內圖片協定（iTerm2、WezTerm）。許多終端機三種都不支援（GNOME Terminal 與其他沒有內建 sixel 的 VTE 終端機、Linux 主控台）。`ratatui-image` crate（11.1，對應 ratatui 0.30）會偵測終端機回應哪一種，為它編碼圖片，並讓圖片穿過 tmux。

<a id="behaviour"></a>
## 行為

<a id="when-there-is-a-cover"></a>
### 何時有封面

當 `cover_img` 開啟、終端機有圖形協定（sixel、kitty、iTerm2，偵測到的或強制指定的，見下方「協定」），且終端機夠大（「版面」）時，封面是**作用中**的。作用中時，播放視窗是 **`cover_img_rows` 列高**（預設 6），而不是 4，且不論播放什麼，封面區域都會保留，所以版面不會在曲目之間或封面載入時跳動：

- 目前的項目有封面且已就緒：畫出封面
- 它在載入中、載入失敗、曲目沒有封面（`album.cover` 為 none），或沒有目前項目：封面區域是空白的

不作用時：播放視窗與現在完全相同（4 列，沒有保留區域），它下方的頁面也與現在一樣高。

<a id="layout"></a>
### 版面

在外框內，封面作用中時：

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│████████████ ▶ Hell Above · Pierce The Veil        shuffle  repeat: queue  80%│
│████████████   Collide With The Sky                                           │
│████████████   LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: …    │
│████████████                                                                  │
│████████████                                                                  │
│████████████   ━━━━━━━━━━━━━━━━━━━───────────────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
```

在 80 × 24、`cover_img_rows = 6`、儲存格 10 × 20 px 時：封面（`█`）是 12 × 6，文字寬 65 欄。詳細資料列依 0004 的規則截斷（「測試計畫」下的快照固定了確切的文字）。

- **封面區域**：在播放視窗的左上角，`cover_img_rows` 列高、**`cols` 欄寬**，`cols` 是讓它在像素上呈正方形的寬度：`round(rows × cell height / cell width)`，由終端機的儲存格像素大小而來（協定查詢會給出）；不知道儲存格大小時，每列 2 欄（6 列就是 `12`）。封面會縮放以符合該區域，保持長寬比（Tidal 的封面是正方形），並置中
- **文字**：封面之後一個空白欄，然後是和現在一樣的播放視窗各行，使用剩餘的寬度（`width − cols − 1`）：標頭（`▶ Title · Artists` 與各指示）、專輯、詳細資料或訊息在前三列，進度條在**最後**一列；中間的列是空白的。0004 中所有關於截斷文字的規則都套用在這個較窄的寬度上
- **太小**：當外框的內部區域少於 `cover_img_rows + 2` 列（讓頁面保留 2 列，0004 的最小值）或少於 `cols + 1 + 20` 欄（0004 最窄的標題側，`MIN_LEFT`）時，封面不作用（視窗是 4 列，與現在相同）。一旦調整大小給了它空間，它就會再次作用
- **頁面高度**：頁面區域從較高的視窗下方開始，清單所顯示的列數（`Resize`，0006）也依此計算

彈出視窗、動作選單與按鍵提示都畫在頁面區域中（0004–0013），絕不畫在播放視窗上。涵蓋整個外框的按鍵說明只在沒有頁面區域時才出現，而那時封面不作用。如果有任何東西畫在封面區域上，該畫面不畫封面。

<a id="protocol"></a>
### 協定

啟動時，在終端機進入 raw mode 與備用螢幕之後、讀取第一個事件之前，`cover_img` 開啟的 TUI 會詢問終端機支援哪一種協定以及它的儲存格像素大小（`ratatui-image` 的 `Picker::from_query_stdio`，最多等待它的逾時時間 2 秒來取得回答）。`cover_img` 關閉時，它什麼都不送出。`cover_img_protocol` 決定：

| `cover_img_protocol` | 使用的協定 |
|---|---|
| `auto`（預設） | 偵測到的那一種：sixel、kitty 或 iTerm2；終端機三種都不回答，或沒有回答時**不使用**（不退回文字方塊） |
| `sixel`、`kitty`、`iterm2` | 該協定，不論終端機如何回答（給回答有誤的終端機）；儲存格大小在終端機有回答時取自查詢，否則每列 2 欄 |
| `halfblocks` | 彩色的 `▀` 儲存格：在任何彩色終端機上都能運作的低解析度封面 |

在 tmux 之下（`ratatui-image` 由 `TERM` 以 `tmux` 開頭或 `TERM_PROGRAM=tmux` 辨識，不是由 `$TMUX`），圖片會包在 tmux 的 passthrough 中。該 crate 在啟動時為 TUI 自己的窗格開啟 passthrough（`tmux set -p allow-passthrough on`）；只有在那失敗時（例如 tmux 太舊、不支援窗格層級的選項），才需要在 `tmux.conf` 中加上 `set -g allow-passthrough on`，這已寫入文件。協定是外層終端機的。結果不能運作的協定（終端機畫出亂碼或什麼都沒有）由使用者以 `cover_img_protocol` 或 `cover_img = false` 覆寫：TUI 看不到終端機實際畫出了什麼。

<a id="where-the-image-comes-from"></a>
### 圖片從哪裡來

**TUI 絕不下載封面。** 播放器是唯一下載封面的行程：它把目前項目的封面放進封面快取（0010「封面快取」），而 TUI 只從那個快取讀取檔案。TUI 與它的播放器永遠在同一台機器、同一位使用者之下（獨立模式：一個行程；連上的：0005 的 Unix socket），所以它們共用快取目錄，且播放器與客戶端之間的協定不變。

- **只要快取開著，播放器就會快取**：0010 只在 MPRIS 開啟且可用時才下載目前的封面。有了本規格，只要 `max_cover_arts > 0`，播放器的封面 worker 就會執行，不論 MPRIS 是開啟、關閉（`mpris = false`）還是不可用（沒有 session bus），所以連上的 TUI 能從任何常駐程式取得封面。worker 的規則是 0010 的（最新的優先、10 秒、5 MB、`.tmp` 與 rename、保留目前封面的上限、每個失敗的封面一則 `warn`）；MPRIS 開啟時，從同一個 worker 取得它的 `file://` URL
- 當目前項目的封面改變（另一個項目成為目前項目，或佇列清空）時，TUI 的**封面載入器**會尋找 `<cache dir>/covers/<cover>.jpg`。它絕不寫入快取、碰觸檔案或套用上限（那是播放器做的，0010）
- **還沒有**（播放器仍在下載）：只要那個封面是目前的，載入器每秒再找一次，並在檔案出現或該封面不再是目前的時候停止。檔案永遠是完整的（播放器會把它 rename 到位）
- **一直沒有**（播放器的下載失敗、`max_cover_arts = 0`、播放器使用另一個快取目錄）：區域維持空白；載入器每秒持續尋找一次，成本是一次 `stat`
- 圖片在載入器的執行緒上讀取、解碼（JPEG），並依封面區域的大小為協定編碼；UI 只繪製已就緒的圖片。改變封面區域大小的調整，會讓圖片重新編碼；在那之前區域是空白的
- 對已不是目前的封面所得到的結果會被丟棄
- **失敗**（無法讀取的檔案、無法解碼的圖片）：該項目的區域維持空白，以 `warn` 記錄一次，每個封面只記一次，只有在該封面再次成為目前的時才重試。絕不顯示在播放器的訊息或狀態列

<a id="config"></a>
### 設定

`app.toml` 新增下列項目，只由 TUI 讀取（如同 `key_hints`：常駐程式接受並忽略它們，每個連上的 TUI 使用自己的）：

| 鍵 | 可接受的值 | 預設值 | 可被覆寫於 |
|---|---|---|---|
| `cover_img` | `true`、`false` | `true` | `TIDAL_PLAYER_COVER_IMG`（`on`、`off`） |
| `cover_img_rows` | 整數 4–20 | `6` | `TIDAL_PLAYER_COVER_IMG_ROWS` |
| `cover_img_protocol` | `auto`、`sixel`、`kitty`、`iterm2`、`halfblocks` | `auto` | `TIDAL_PLAYER_COVER_IMG_PROTOCOL` |

`max_cover_arts` 仍是播放器的（0010）：`0` 關閉快取，TUI 的封面也隨之關閉（區域是空白的）。快取目錄在兩邊以相同方式解析（`$TIDAL_PLAYER_CACHE_DIR`、`$XDG_CACHE_HOME/tidal-player`、`~/.cache/tidal-player`）；解析出的目錄與它的常駐程式不同的 TUI，不會顯示封面。

<a id="acceptance-criteria"></a>
## 驗收條件

純邏輯（`tidal-player`）：

- **AC1** — 版面（`ui::cover_layout`，表格：外框內部大小 × `cover_img_rows` × 儲存格大小 × 是否作用 → 播放視窗矩形、封面矩形、文字矩形、頁面矩形）：78 × 22 的內部（80 × 24 的終端機）、6 列、儲存格 10 × 20 px → 封面 12 × 6 在左上角，文字從第 13 欄開始、寬度 65，頁面從第 6 列開始、16 列；儲存格 8 × 18 px → `round(6 × 18 / 8)` = 14 欄；沒有儲存格大小 → 12；4 列 → 8 欄；少於 `rows + 2` 列或少於 `cols + 21` 欄，或不作用 → 現在的 4 列版面，沒有封面矩形
- **AC2** — `ui::list_height` 計入較高的視窗：封面作用時，清單比沒有封面時少 `cover_img_rows − 4` 列；在大小限制以下則與現在相同
- **AC3** — 協定的選擇（`cover::protocol`，表格：`cover_img` × `cover_img_protocol` × 查詢的回答（sixel、kitty、iTerm2、只有 halfblocks、沒有回答）→ 協定或無，以及是否有查詢終端機）：`auto` 接受 sixel/kitty/iTerm2，對 halfblocks 或沒有回答給出無；強制指定的協定勝過任何回答；`halfblocks` 給出 halfblocks；`cover_img = false` 給出無，且**不查詢**
- **AC4** — 封面載入器的決策（`cover::Slot`，純狀態，事件表格：目前的封面改變、結果抵達、區域調整大小、佇列清空、`cover_img` 不作用）：要找哪個封面（只找最新的）、何時再找一次（目前封面的檔案缺少時每秒一次，封面不再是目前的之後絕不）、保留哪個結果（目前大小的目前封面）與丟棄哪個（過期的封面、過期的大小）、區域何時是空白的，以及解碼失敗的封面在它再次成為目前的之前不會重試

繪製（`tidal-player::ui`，`insta`，`TestBackend`：圖片本身在接縫（seam）之後，快照顯示保留的區域是空白的）：

- **AC5** — 封面作用時，播放視窗是 `cover_img_rows` 列，文字從封面區域與一個空白欄之後開始，進度條在它的最後一列，0004 其餘的文字規則在較窄的寬度上仍然成立。快照：80 × 24 播放中（6 列）、120 × 30 暫停（8 列）、80 × 24 沒有播放（區域空白，視窗仍然很高）、80 × 24 的一則訊息、40 × 12（作用中，25 欄的文字側）、34 × 12（太窄：現在的視窗）
- **AC6** — 不作用時（`cover_img = false`、協定為無、太小）：0004–0014 的每個既有快照都不變（本規格不重新接受任何快照）。封面作用時，以從 1 × 1 到 200 × 60 的每個終端機大小繪製都不會 panic（0004 的不 panic 測試，新增列）
- **AC7** — 接縫：`render` 把已就緒的圖片（一個記錄自己拿到的矩形的假協定）恰好畫在封面矩形中，而當圖片的大小不是封面矩形的大小（調整大小之後）或沒有已就緒的圖片時，什麼都不畫

I/O（`tidal-player`，暫存目錄，`wiremock`，如同 0010 AC21）：

- **AC8** — TUI 的載入器（暫存的快取目錄、一個必須看到**沒有任何請求**的模擬圖片伺服器：`wiremock` `expect(0)`）：已快取的封面被讀取、解碼並產生一張圖片；缺少的封面什麼也不產生，然後在檔案被 rename 到位之後產生（假時鐘，每秒一次的尋找）；載入器執行之後，快取目錄中的檔案、名稱與修改時間和之前相同；無法解碼的檔案 → 空白，一則 `warn`；讀取一個封面時出現較新的封面 → 較舊的結果被丟棄
- **AC11** — 播放器在沒有 MPRIS 時也會快取：`mpris = false` 的常駐程式（0010 `tests/mpris.rs` 的測試架構，模擬圖片伺服器），以及沒有 session bus 的常駐程式，會為目前的項目寫入 `covers/<cover>.jpg`；`max_cover_arts = 0` 時它什麼也不寫，也不發出請求。0010 的 AC20/AC21 測試維持綠燈、不更動
- **AC9** — 設定：`cover_img`、`cover_img_rows`、`cover_img_protocol` 在 0008 的 `app.toml` 與優先順序表格中（預設值；`cover_img_rows = 3` 與 `21` 是指出鍵名的錯誤；`cover_img_protocol = "ascii"` 是列出可接受值的錯誤；每個變數優先於設定檔；無效的變數是錯誤；結束碼 2）；常駐程式接受並忽略它們；`examples/app.toml` 列出這三個鍵與它們的預設值（0008 的範例測試）

文件：

- **AC10** — `docs/tui.md` 的「Playback window」（一個「Album cover」小節：何時顯示、協定與哪些終端機、tmux 的 `allow-passthrough`、強制指定或關閉、快取）、`docs/config.md`（三個鍵）、`docs/mpris.md` 的「Album covers」（播放器在 MPRIS 關閉時也會快取封面）、它們的 zh-TW 版本、`README.md`/`README.zh-TW.md`、`CLAUDE.md` 的「Features」，以及 0010 的「不在範圍內」與「封面快取」（*When*）指向這裡。一個測試檢查這三個鍵出現在兩份 `config.md` 中，且 "allow-passthrough" 出現在兩份 `tui.md` 中（0013 AC7 的測試，新增列）

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 終端機對查詢沒有回答，或回答很慢 | 啟動時最多等 `ratatui-image` 的逾時時間（2 秒）一次，然後沒有封面（`auto`） |
| 終端機回答了但沒有儲存格大小（不允許視窗操作的 xterm） | `ratatui-image` 退回 halfblocks，所以在 `auto` 下沒有封面；已寫入文件：強制指定 `cover_img_protocol = "sixel"` |
| `cover_img = false` | 不送出查詢；TUI 與以前完全相同 |
| 不會回答查詢的終端機（例如序列主控台、`TERM=linux`） | 沒有回答 → 無 |
| tmux 拒絕窗格層級的 `allow-passthrough` | tmux 丟掉圖片；區域維持空白。已寫入文件（`set -g allow-passthrough on`）；`cover_img = false` 會隱藏保留的區域 |
| 從兩個不同的終端機連上 tmux | 協定是 TUI 啟動時偵測到的那一種 |
| 調整大小 | 下一個畫面依新的大小排版；圖片為新的封面大小重新編碼，在那之前是空白的 |
| 終端機變得太小 | 封面不作用（4 列的視窗）；不畫圖片 |
| 沒有封面的曲目、空的佇列、還原後已停止的播放器 | 區域空白，視窗很高 |
| 快取目錄無法寫入，或 `max_cover_arts = 0` | 播放器無法快取（0010：MPRIS 得到 `https://` URL）；TUI 的區域維持空白 |
| 播放器的下載失敗或很慢 | 空白，直到檔案出現；TUI 每秒找一次 |
| 網路中斷 | 空白，每個封面一則 `warn`，在它再次成為目前的時重試 |
| 與常駐程式中斷連線（0005） | 顯示最後狀態期間保留最後的封面，如同文字 |
| 前一個封面仍在編碼時換了曲目 | 舊的結果被丟棄；改要求新的 |
| 兩個連上的 TUI | 各自讀取同一個快取檔案 |
| 只在它的 unit 中設定了 `TIDAL_PLAYER_CACHE_DIR` 的常駐程式 | TUI 在另一個目錄中尋找，不顯示封面；已寫入文件（兩邊都設定，或都不設定） |
| 調低 `max_cover_arts` | 如同 0010：在下一次寫入之後與播放器下次啟動時套用 |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名。紅燈是針對存根型別與函式、其本體為存根（`cover_layout` 回傳現在的版面、`protocol` 回傳無、載入器從不產生圖片、設定忽略這些鍵）的斷言失敗，沒有 `todo!()`、沒有編譯錯誤，如同 0001–0014。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/app/src/tui/render.rs` :: `ac1_cover_layout` (table) | 每種大小的矩形、列數、儲存格大小 | 存根回傳沒有封面的 4 列版面 |
| AC2 | `crates/app/src/tui/render.rs` :: `ac2_list_height_with_cover` (table) | 有封面與沒有封面時的清單列數 | 存根忽略封面 |
| AC3 | `crates/app/src/tui/cover.rs` :: `ac3_protocol` (table) | 協定、是否查詢 | 存根回傳無，且永遠查詢 |
| AC4 | `crates/app/src/tui/cover.rs` :: `ac4_slot` (table of event sequences) | 請求、保留與丟棄的結果、空白、不重試 | 存根保留每個結果 |
| AC5 | `crates/app/src/tui/render.rs` :: `ac5_cover_window` (snapshot per row) | 視窗高度、文字欄、進度列 | 存根畫出現在的視窗 |
| AC6 | the existing snapshots (unchanged); `crates/app/src/tui/render.rs` :: `ac17_no_panic_any_size` (new rows) | 不作用時沒有任何改變；不會 panic | 在紅燈時通過（防護）；新的列只有在存根 panic 時才會失敗 |
| AC7 | `crates/app/src/tui/render.rs` :: `ac7_cover_drawn_in_rect` | 假協定的矩形、大小不符、無 | 存根從不繪製 |
| AC8 | `crates/app/src/tui/cover.rs` :: `ac8_cached`, `ac8_appears_later`, `ac8_read_only`, `ac8_decode_failure`, `ac8_newest_wins` | 圖片、沒有請求、快取未被碰觸、記錄 | 存根載入器從不產生；`ac8_read_only` 在紅燈時通過（防護） |
| AC11 | `crates/app/tests/mpris.rs` :: `ac11_cache_without_mpris` (rows: `mpris = false`, no bus, `max_cover_arts = 0`) | 檔案是否寫入、請求 | 存根只在有 MPRIS 時啟動 worker |
| AC9 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows); `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` | 值、錯誤、範例 | 存根設定忽略這些鍵 |
| AC10 | `crates/app/tests/docs.rs` :: `ac10_cover_documented` + zh-TW copies reviewed at acceptance | 文件提到這些鍵與 tmux | 存根文件沒有它們 |

驗收時在使用者的機器上手動檢查（結果寫在 PR 說明中），因為它們需要真實的終端機，沒有測試能看到終端機畫了什麼：封面在 kitty（kitty 協定）、foot 或 WezTerm（sixel / iTerm2）中顯示並隨曲目改變，在 `allow-passthrough on` 的 tmux 之下也是；GNOME Terminal（或其他沒有協定的終端機）顯示現在的 4 列視窗；`cover_img_protocol = "halfblocks"` 畫出方塊；調整大小會乾淨地重繪、沒有殘留的像素；開啟與關閉彈出視窗與按鍵說明，封面上不會留下亂碼；連上的 TUI 顯示常駐程式曲目的封面；`cover_img = false` 給出現在的畫面。

結果（2026-10-12，sway，記錄在 PR #37）：

| 終端機 | 協定 | 檢查項目 |
|---|---|---|
| foot 1.28 | sixel | 封面依規格繪製與排版；換曲目時它跟著專輯；`?` 與動作彈出視窗不留下亂碼；縮小到限制以下時得到 4 列視窗，沒有殘留的 sixel，封面也會回來 |
| kitty 0.49.2 | kitty | 封面已繪製；`?` 不留下亂碼；調整大小時乾淨地重繪 |
| WezTerm 20260716，在 XWayland 之下 | iTerm2 | 封面已繪製；縮小到限制以下再放大。WezTerm 在 Wayland 下啟動時會 panic（`no keymap`，在它自己的鍵盤程式碼中，TUI 執行之前），所以以 `enable_wayland=false` 執行；按鍵無法送到那個視窗，所以沒有檢查彈出視窗 |
| foot 中的 tmux 3.8 | 穿過 passthrough 的 sixel | 封面已繪製，不需更動 `tmux.conf`（該窗格的 `allow-passthrough` 開啟，全域選項關閉）；`?` 不留下亂碼 |
| foot 中的 tmux，連上執行中的常駐程式 | sixel | 連上的 TUI 從常駐程式的快取顯示常駐程式目前曲目的封面 |

**尚未手動檢查**：

- GNOME Terminal 或其他沒有協定的終端機（預期：現在的 4 列視窗）
- `cover_img_protocol = "halfblocks"`
- 真實終端機上的 `cover_img = false`（只由 AC3 與 AC9 的測試涵蓋）
- kitty、WezTerm 與 tmux 中的換曲目（只檢查了 foot）；WezTerm 中的彈出視窗
- Ghostty、iTerm2、xterm（`-ti vt340`，以及沒有儲存格大小時強制指定的 `sixel`）、mlterm 與 Konsole，這些是「行為」與使用者文件依 `ratatui-image` 的相容性說明所列出的
- Linux 主控台，以及已知的限制：永遠不回答查詢的終端機可能會吞掉第一個按鍵
- 沒有窗格層級 `allow-passthrough` 的 tmux（較舊的 tmux），以及 `TERM=screen-256color` 的 tmux

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player`（執行檔）：新的 `cover.rs`（協定的選擇與 `Slot`（純邏輯）、載入器執行緒：唯讀的快取查找、解碼、編碼，位於測試會用假實作替代的接縫之後；它絕不下載，也沒有 HTTP 客戶端）；`ui.rs`（`cover_layout`、較高的播放視窗、接收已就緒圖片的 `render`、含封面列數的 `list_height`）；`main.rs`（啟動時的查詢、與狀態及重繪接線的載入器）；`config.rs`（三個鍵）。0010 的 `mpris/covers.rs` 搬出 `mpris`（現為 `crate::player::cover_cache`）：只要 `max_cover_arts > 0`，播放器就啟動它的 `CoverWorker`，與 MPRIS 無關，而 MPRIS 配接器在執行時取用它的結果；TUI 只使用它的唯讀路徑與目錄函式。它的測試原封不動地一起搬走
- 新的工作區相依套件：`ratatui-image` 11（`default-features = false`、`features = ["crossterm"]`：沒有 `chafa`，所以沒有新的系統函式庫）與 `image` 0.25（`default-features = false`、`features = ["jpeg"]`），只給執行檔
- `tidal-player-core`：沒有變更（`cover` 與 `cover_url` 已存在）。`xtask layering`：沒有變更（圖片留在執行檔中）

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-12，來自程式碼與 crates.io）：

- 播放視窗是 4 列（`PLAYBACK_ROWS`），頁面需要 6 個內部列（`MIN_ROWS_WITH_QUEUE`），標題側最窄是 20 欄（`MIN_LEFT`），都在 `crates/app/src/tui/render.rs`；`areas` 為外框排版，`list_height` 使用它；`main.rs` 從 `list_height` 送出 `Resize`
- 彈出視窗、動作選單與按鍵提示畫在頁面區域之內（`pages::render_popup`、`render_hint` 接收 `areas.page`）
- 封面快取：`crates/app/src/player/cover_cache.rs`（`CoverCache::{lookup, download, fetch}`、`CoverWorker`、`process_cache_dir`、`evict`），由 MPRIS 配接器啟動（`mpris/hub.rs`，`CoverWorker::spawn`），所以只在 MPRIS 開啟且可用時才下載（0010）
- `ratatui-image` 11.1.0 相依於 `ratatui ^0.30.1`（我們的：0.30.2）與 `image ^0.25.6`；它的預設功能會拉進 `chafa-dyn`（pkg-config）；它有 `Picker::from_query_stdio`（要在進入備用螢幕之後、讀取事件之前呼叫）、sixel、kitty、iTerm2 與 halfblocks 協定，以及 tmux passthrough（從環境變數得到 `is_tmux`）。它在 WezTerm 與 Konsole 之下把 kitty 與 sixel 列入黑名單，並在那裡使用 iTerm2。12.0 只是候選版本
- （來自它的原始碼，11.1.0）`from_query_stdio` 最多等待 2 秒（`STDIN_READ_TIMEOUT_MILLIS`），它的查詢執行緒每個「busy」步驟都會重新計時；會回答的終端機立刻回答（查詢以每個終端機都會回答的狀態回報結尾），所以只有不回答的才要付出這個代價。tmux 由 `TERM`/`TERM_PROGRAM` 偵測，不是 `$TMUX`，且該 crate 會為它的窗格執行 `tmux set -p allow-passthrough on`。回答了但沒有儲存格大小的終端機，會得到該 crate 預設的 halfblocks picker

假設，於驗收時檢查：

- 「背景」下列出的終端機，如同 `ratatui-image` 的相容性矩陣所說的那樣回答查詢
- 重繪 sixel 圖片底下的儲存格（調整大小、彈出視窗）會在 foot、WezTerm 與 xterm 中把它清掉；kitty 的圖片跟著它們的儲存格一起消失
- spotify-player（憑記憶，需再確認）：`cover_img_length`、`cover_img_width`、`cover_img_scale` 在它的 `image` 功能之後，以 `viuer` 繪製

<a id="decisions"></a>
## 決策

使用者已回答，2026-10-12（隨規格一併核准）：

1. **高度**：封面作用時，播放視窗會長高（`cover_img_rows`，預設 6），而不是把 8 × 4 的封面塞進現在的 4 列
2. **沒有協定**：沒有封面（現在的視窗），不自動退回文字方塊；`cover_img_protocol = "halfblocks"` 可強制使用
3. **圖片從哪裡來**：TUI 讀取共用的快取；協定不變
4. **TUI 絕不下載**（2026-10-12 回答，取代了讓它下載缺少項目的提案）：它只檢查播放器（常駐程式或獨立模式，MPRIS 的 worker）是否已快取該檔案。所以只要 `max_cover_arts > 0`，播放器就會快取目前的封面，不論有沒有 MPRIS（對 0010 的變更）
5. **預設值**：開啟（`cover_img = true`）
6. **穩定的版面**（2026-10-12 回答：是）：只要封面作用，視窗就是高的，即使沒有播放或曲目沒有封面也一樣，所以頁面不會在曲目之間改變高度
7. **進度條在較高視窗的最後一列**，文字在前三列（隨規格一併核准）

<a id="out-of-scope"></a>
## 不在範圍內

- 其他任何地方的封面（頁面、佇列、全螢幕的封面檢視）
- 預先擷取下一個項目的封面；`640x640` 以外的圖片大小
- 播放器告訴客戶端它快取的封面在哪裡（決策 3）
- `halfblocks` 以外的文字方塊（`chafa`/ASCII）繪製器
- 選擇封面的位置（右側）或與高度無關的寬度

<a id="implementation-notes"></a>
## 實作備註

- **切片 A（AC11）**：封面 worker 屬於 MPRIS 配接器（`mpris::hub::Adapter`），它是一個普通的行程內 hub 客戶端（attach、subscribe），跟隨目前的項目並驅動唯一的 `CoverWorker`。`mpris::start` 現在只要 `max_cover_arts > 0` 或 MPRIS 開啟就會建立它；匯流排只在 MPRIS 開啟時連接，MPRIS 關閉或不可用時就是永遠不呼叫 `set_bus`，所以不存在第二個下載器。`cache.start()`（清理與上限）在每次播放器啟動時執行一次，在嘗試匯流排之前。對只做快取的跟隨者，`Mpris::name()` 是 `None`
- 0010 的 `ac11_no_bus` 與 `ac13_mpris_off` 斷言「沒有 MPRIS 時什麼都不快取」，而本規格推翻了這一點；那兩行已移除（`ac11_cache_without_mpris` 涵蓋新的行為）
- **切片 B（AC1–AC10）**：
  - **執行緒**：UI 執行緒擁有 `cover::Cover`（一個 `Slot` 與一個 `Loader`），每個畫面（0004 的 100 ms tick）呼叫一次 `update(current cover, cover area size, now)`，從不阻塞：它以 `try_recv` 取得載入器的結果、餵給 slot，並送出 slot 最新的請求。載入器是一個 `std::thread`（`cover-loader`），取得最新等待中的請求，讀取 `<cache dir>/covers/<cover>.jpg`（`cover_cache::cached_file`，它拒絕任何不是封面 ID 的東西）、解碼（`image`，只有 JPEG）並依區域的大小編碼（`Picker::new_protocol`、`Resize::Fit`），然後送回 `ratatui_image::protocol::Protocol`（它是 `Send`）。它沒有 HTTP 客戶端，只做 `std::fs::read`；每秒一次的尋找是 slot 的 `look_at`，由畫面的時鐘驅動。失敗是每次尋找一則 `tracing::warn!`，且 slot 在該封面再次成為目前的之前不會再找。`render_with_cover` 接收一個 `CoverFrame`（幾何，以及已就緒的圖片與它被編碼時的大小），最後才繪製圖片，在封面矩形中，且只在該大小就是矩形的大小、按鍵說明沒有蓋住它時才畫；`render` 是 `render_with_cover(…, None)`，所以每個既有的快照都由同一段程式碼在封面不作用下繪製
  - **查詢在哪裡執行**：`main.rs::start_cover`，在 raw mode、備用螢幕與括號貼上之後、建立 `Terminal` 之前、第一次 `event::poll` 之前；只在 `cover_img` 開啟時（`cover::protocol` 只在那時才呼叫查詢閉包）。被查詢的 `Picker` 保留下來用於編碼（它的儲存格大小與 tmux 旗標）；沒有回答且強制指定協定時，使用 `Picker::halfblocks()`（10 × 20 px 的儲存格，所以每列 2 欄）搭配該協定
  - **`cover_img_*` 的傳遞**：如同 `key_hints`（`AppConfig` → `play.rs` 中的 `PlayerSettings` → `cover::Settings`）。常駐程式解析並忽略它們；如同每項設定，常駐程式在工作階段檢查之後才讀取變數，所以無效的 `TIDAL_PLAYER_COVER_IMG*` 只有在常駐程式登入之後才會讓它停止。TUI 也讀取自己的 `max_cover_arts`：`0` 什麼都不找（快取中可能留有舊檔案，快取關閉時播放器不會清理它）
  - **更正規格中的數字**：AC1 的第一個例子是 78 × 22 的內部（80 × 24 的終端機），文字寬度 65（80 寬的內部給出 67，也是一個測試列）。AC5 的 40 × 12 依所述的限制*並非*太窄（38 ≥ 12 + 1 + 20、10 ≥ 6 + 2）：它的快照顯示封面作用、文字側 25 欄，而「太窄」的案例改在 34 × 12（32 < 33）檢查，「太矮」在 80 × 9 檢查，兩者都與現在的繪製相同
  - **按鍵說明**（至少 40 × 10）在頁面很矮時，即使有頁面區域，也可能蓋到播放視窗；「任何東西畫在封面區域上：不畫封面」的規則處理了這一點（`pages::help_rect`，AC7 的測試）
  - **`ratatui-image` 11.1 的事實**（來自它的原始碼）：`from_query_stdio` 的逾時是 2 秒（`STDIN_READ_TIMEOUT_MILLIS`，每個「busy」步驟都會重新計時），不是「遠低於一秒」；真實的終端機立刻回答（查詢以每個終端機都會回答的 DSR 結尾），所以只有永遠不回答的終端機才會等到逾時。回答了但沒有儲存格大小的終端機（不允許視窗操作的 xterm）會得到該 crate 預設的 halfblocks picker，所以 `auto` 在那裡給出無，解法是 `cover_img_protocol = "sixel"`；沒有回答時，它對儲存格退回到視窗大小的 ioctl，對協定退回到環境提示（iTerm2/WezTerm）。Konsole 與 WezTerm 把 kitty 與 sixel 列入黑名單（WezTerm 接著從 `TERM_PROGRAM` 使用 iTerm2；Konsole 得到 halfblocks，所以在 `auto` 下是無）。tmux 由 `TERM`（`tmux…`）或 `TERM_PROGRAM=tmux` 偵測，不是 `$TMUX`，且該 crate 本身會為它的窗格執行 `tmux set -p allow-passthrough on`。這些現在已寫入「事實與假設」、行為（協定）、邊界情況與文件；AC1 與 AC5 已就地更正。`default-features = false` 保留 `from_query_stdio`（它在 Unix 上只需要 `rustix`，永遠開啟），並捨棄 `chafa`
  - **已知的限制**：如果終端機永遠不回答，該 crate 的查詢執行緒在逾時之後可能仍卡在讀取 stdin，並吞掉第一個按下的按鍵（讀自它的原始碼，沒有重現）；驗收時要在 Linux 主控台上手動檢查
