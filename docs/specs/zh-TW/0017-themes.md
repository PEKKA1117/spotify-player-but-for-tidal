@docs/Eastereggs/67055c40c1e2.md

[English](../0017-themes.md) | 正體中文

<a id="0017--themes-and-colours"></a>
# 0017 — 佈景主題與顏色

- **狀態**：implemented（2026-10-24；2026-10-24 核准；2026-10-24 由使用者修訂：AC16–AC20、決策 8–16 以及 AC14 的一部分，都在第一行所指的檔案中，該檔案不納入儲存庫）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#20](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/20)
- **相依於**：0004（implemented：畫面）、0006/0007/0011/0012/0013（implemented：本規格要上色的頁面、彈出視窗、篩選與按鍵提示）、0008（implemented：`app.toml`、它的優先順序與錯誤、按鍵對應與它的通知、按鍵說明）、0014（implemented：Devices 彈出視窗，Themes 彈出視窗的範本）
- **使用者文件**：[`docs/config.md`](../../zh-TW/config.md) 新增 `theme.toml` 與 `theme` 鍵；[`docs/tui.md`](../../zh-TW/tui.md) 新增「Themes」；新的 [`examples/theme.toml`](../../../examples/theme.toml)；它們的 zh-TW 版本（AC15）

<a id="context"></a>
## 背景

TUI 以終端機自己的顏色繪製，只用三種文字屬性：粗體表示有焦點或正在播放的，淡色表示沒有的，反白表示游標，紅色表示錯誤（`crates/app/src/tui/render.rs`、`ui/pages.rs`）。0008 把佈景主題排除在範圍之外，`SwitchTheme` 是它的按鍵對應略過的 spotify-player 指令之一。

spotify-player（`spotify_player/src/config/theme.rs`、`docs/config.md` 的「Themes」、`examples/theme.toml`，master `7dc9d17`，2026-10-24 閱讀）從 `theme.toml` 讀取任意數量的 `[[themes]]`，每個有一個 `name`、一個選用的 `[themes.palette]` 與一個選用的 `[themes.component_style]`，並以 `app.toml` 中的 `theme = "<name>"` 或 `-t/--theme` 選擇其中一個。`SwitchTheme`（`T`）開啟一個 *Themes* 彈出視窗，在其中移動游標會立刻套用該佈景主題，`Enter` 保留它，`Esc` 回到先前使用中的佈景主題。它自己的預設佈景主題色彩繽紛（洋紅色的標題、青色的曲目、綠色的進度條）。它會默默忽略未知的欄位。

tidalt 沒有佈景主題。

<a id="what-could-go-wrong-here-and-the-criterion-that-covers-it"></a>
### 這裡可能出什麼問題，以及涵蓋它的條件

1. **預設外觀意外改變**：把每個樣式都搬進佈景主題，必須讓畫面維持現在的樣子 → 內建的 `default` 佈景主題就是現在的外觀，且每個既有的繪製與樣式測試都維持綠燈、不更動（AC3）
2. **複製來的 spotify-player 佈景主題被拒絕，或它的顏色落在別處** → 相同的檔案格式、相同的顏色名稱與解析，它的元件名稱畫在 spotify-player 畫的地方，我們不畫的則以通知略過（AC1、AC2、AC6）。佈景主題沒寫的部分保留現在的外觀，而不是 spotify-player 色彩繽紛的預設值（決策 1）；`examples/theme.toml` 把那些寫成一個可以複製的佈景主題（AC14）
3. **打錯字被默默忽略**（`blok_title`、`Purpel`）→ 未知的欄位、樣式、顏色與修飾符都是錯誤，指出檔案、佈景主題與欄位（AC2），與 0008 相同
4. **佈景主題隱藏了狀態**：設定游標列顏色的佈景主題，不能讓焦點、正在播放與淡色的提示消失 → 這些提示由 TUI 疊加在每個佈景主題之上（AC10）
5. **看不到顏色的繪製測試**：現在的快照只有文字 → 本規格的快照會記錄樣式（AC9）
6. **壞掉的 `theme.toml` 讓音樂停止**：常駐程式什麼都不畫 → 只有 TUI 會讀取 `theme.toml`（決策 5）（AC7）

<a id="behaviour"></a>
## 行為

<a id="themetoml"></a>
### `theme.toml`

位於設定目錄（0008「檔案位置」），與 `app.toml`、`keymap.toml` 並列；選用。採用 spotify-player 的格式，所以它的 `theme.toml` 可以原封不動複製過來：

```toml
# ~/.config/tidal-player/theme.toml
[[themes]]
name = "dracula"
[themes.palette]
background = "#1e1f29"
foreground = "#f8f8f2"
red = "#ff5555"
cyan = "#8be9fd"
bright_black = "#555555"

[themes.component_style]
block_title = { fg = "Magenta", modifiers = ["Bold"] }
selection = { fg = "Black", bg = "Cyan" }
playback_progress_bar = { fg = "#bd93f9" }
```

一個佈景主題有：

- **`name`**（必填，不可為空）：`theme =` 與 Themes 彈出視窗用它來稱呼這個佈景主題
- **`palette`**（選用）：`background`、`foreground`，以及十六種終端機顏色 `black`、`red`、`green`、`yellow`、`blue`、`magenta`、`cyan`、`white`、`bright_black`、`bright_red`、`bright_green`、`bright_yellow`、`bright_blue`、`bright_magenta`、`bright_cyan`、`bright_white`。值的解析方式與 spotify-player 相同（ratatui 的顏色名稱）：一個名稱，不分大小寫、空格、`-` 與 `_`，`bright` 與 `light`、`grey`、`gray` 與 `silver` 依 ratatui 的讀法（`red`、`Bright Black`、`light-red`、`dark_gray`、`reset`；注意 ratatui 的 `white` 是亮白色，`gray` 才是一般的白），一個 `0`–`255` 的數字（終端機的索引顏色），或 `#rrggbb`。沒寫的顏色就是終端機自己的（十六種 ANSI 顏色：`white` 是一般的白，ANSI 7）。`background` 與 `foreground` 沒寫就沿用終端機的
- **`component_style`**（選用）：每個元件（下方的表格）一個樣式，每個都是由 `fg`、`bg`（調色盤名稱、`Black` … `White`、`BrightBlack` … `BrightWhite`（取該調色盤項目的顏色），或 `#rrggbb`；名稱不分大小寫、`_`、`-` 與空格，所以 `bright_black` 就是 `BrightBlack`）與 `modifiers`（`Bold`、`Dim`、`Italic`、`Underlined`、`RapidBlink`、`Reversed`、`Hidden`、`CrossedOut` 的清單，不分大小寫）組成的行內表格。一個樣式會整個取代該元件的預設值：`selection = { fg = "Cyan" }` 是青色文字，沒有預設值的反白

佈景主題的順序是：先是內建的 **`default`**，然後是檔案中的佈景主題，依檔案順序。檔案中名為 `default` 的佈景主題會取代內建的（就地取代，仍排第一）；spotify-player 則是忽略它（決策 6）。

<a id="components"></a>
### 元件

每個元件畫在哪裡，以及它的預設值：內建 `default` 佈景主題的樣式，也就是現在的外觀，且是任何佈景主題對它沒寫的元件所得到的（決策 1）。另附 spotify-player 自己的預設值作為比較（`examples/theme.toml` 把它放在 `spotify_player` 佈景主題中，見 AC14）；`*` 標記本播放器新增的兩個：

| 元件 | 畫在 | 預設值（現在的外觀） | spotify-player 的預設值 |
|---|---|---|---|
| `block_title` | 每個邊框上的標題：外框的 `tidal-player`、視窗與它們的分頁、佇列、彈出視窗、按鍵說明、按鍵提示 | — | `fg = "Magenta"` |
| `border` | 每個邊框 | — | — |
| `playback_status` | 播放視窗的狀態符號（`▶ ⏸ … ■`）與它的指示（`shuffle`、`repeat: …`、`autoplay`、音量） | `Bold` | `Cyan`、`Bold` |
| `playback_track` | 曲名（或 `Nothing playing`） | `Bold` | `Cyan`、`Bold` |
| `playback_artists` | 曲名後面的 ` · ` 與藝人 | `Bold` | `Cyan`、`Bold` |
| `playback_album` | 專輯列 | — | `fg = "Yellow"` |
| `playback_metadata` | 詳細資料列（音質、格式、裝置、bit-perfect）與時間 `1:23 / 3:32` | — | `fg = "BrightBlack"` |
| `playback_progress_bar` | 進度條已播放的部分（`━`） | — | `fg = "Green"`、`bg = "BrightBlack"` |
| `playback_progress_bar_unfilled` | 進度條的其餘部分（`─`） | — | `bg = "BrightBlack"` |
| `current_playing` | 佇列中目前項目的列 | `Bold` | `Green`、`Bold` |
| `page_desc` | 頁面的標題列（`Album · …`、`Search`……） | `Bold` | `Cyan`、`Bold` |
| `playlist_desc` | mix 頁的副標題列 | `Dim` | `BrightBlack`、`Dim` |
| `table_header` | 按鍵說明的區段標題（`Lists`、`Playback`……） | `Bold` | `fg = "Blue"` |
| `selection` | 游標列：清單、佇列、彈出視窗、按鍵說明、搜尋的最佳結果；以及窗格標題中作用中分頁的名稱 | `Reversed` | `Reversed`、`Bold` |
| `secondary_row` | 清單或佇列的每隔一列（第 2、4……列，在清單中計算） | — | — |
| `like` | 已追蹤播放清單上的 `♥` | — | — |
| `error`* | 訊息列與載入失敗（頁面、視窗、失敗的 `Loading more…`） | `fg = "Red"` | `fg = "Red"` |
| `prompt`* | 開啟中的輸入提示（`Add to queue: …`、`Play next: …`）、播放清單名稱的輸入提示、搜尋輸入框 | `Bold` | `Bold` |

調色盤的 `background` 與 `foreground` 是整個畫面的樣式（spotify-player 的 `app` 樣式）：每個儲存格都從它們開始，上面列出的樣式再疊上去。

本播放器不畫的 spotify-player 元件（`playback_genres`、`visualization`）會被接受並略過：TUI 的訊息列在啟動時說一次 `theme.toml: 2 spotify-player styles not used here: playback_genres, visualization`（每個名稱一次，依檔案順序），如同 0008 對按鍵對應所做的。其他任何名稱都是錯誤。

<a id="state-cues-on-top-of-the-theme"></a>
### 疊加在佈景主題之上的狀態提示

TUI 在每個佈景主題中，都會在佈景主題給的樣式之上加上這些（一個樣式是*疊*在另一個之上，如同 ratatui 的 `patch`：它有設定的顏色會取代底下的顏色，它的修飾符則是疊加）：

- **列**：一列從畫面的樣式開始，或在每隔一列使用 `secondary_row`；佇列中目前的項目疊上 `current_playing`；無法播放的曲目（0006）、自動播放建議與 `── Suggested` 分隔線加上 `Dim`；游標列疊上 `selection`，且在它的視窗沒有焦點時加上 `Dim`
- **視窗**：有焦點的視窗標題加上 `Bold`；其他視窗的標題與邊框加上 `Dim`。彈出視窗、按鍵說明與按鍵提示的標題加上 `Bold`。佇列的標題在輸入它的篩選時加上 `Bold`（0012）。外框的標題不加任何東西
- **搜尋輸入框**：有焦點時是加上 `Bold` 的 `prompt`；沒有焦點時，是去掉 `Bold` 的 `prompt`，再加上 `Dim`
- **狀態文字**（`Loading…`、`Loading more…`、空的清單、`No rows match …`、彈出視窗的狀態、按鍵說明的頁尾與淡色列、按鍵提示的淡色列）：在該列的樣式上加上 `Dim`

使用 `default` 佈景主題時，這恰好就是現在的畫面。

<a id="choosing-the-theme"></a>
### 選擇佈景主題

`app.toml` 中的 `theme = "<name>"`（預設 `"default"`），被 `TIDAL_PLAYER_THEME` 與 TUI 上的 `-t NAME`/`--theme NAME`（`tidal-player -t dracula`）覆寫，優先順序同 0008（旗標 > 環境變數 > 設定檔 > 預設值）。空的變數視同未設定。每個 TUI 使用自己的：連上同一個常駐程式的兩個 TUI 可以使用不同的佈景主題；佈景主題絕不經由 `Command`/`Event` 傳送。

全彩（truecolor）：`#rrggbb` 顏色原樣送出（spotify-player 也是如此）；不支援 24 位元顏色的終端機會顯示它自己換算出的結果（決策 3）。

<a id="the-themes-popup-t"></a>
### Themes 彈出視窗（`T`）

`SwitchTheme`（spotify-player 的名稱與預設按鍵 `T`）從任何頁面開啟 **Themes** 彈出視窗，如同 `D` 開啟 Devices（0014）。它不再是略過的 spotify-player 指令之一；它的說明文字是 `choose a theme`，在按鍵說明的 `App` 區段中。

```
┌Themes──────────────────────────────────────────┐
│● default                                       │
│  dracula                                       │
│  gruvbox_dark                                  │
└────────────────────────────────────────────────┘
```

- 每個佈景主題，依順序（`default` 第一）。`●` 標記彈出視窗開啟時使用中的佈景主題；游標從它開始
- **預覽**：移動游標（清單指令，經由按鍵對應）會立刻以游標所在的佈景主題繪製整個 TUI
- `Enter` 保留游標所在的佈景主題並關閉彈出視窗；`Esc`（或 `q`）回到標記 `●` 的佈景主題並關閉它
- 這個選擇只持續到該 TUI 的這次執行：重新啟動的 TUI 會再使用 `theme`（決策 4）。只讀取 `theme.toml` 與 `app.toml`；不寫入任何東西
- 沒有 `theme.toml` 時，彈出視窗只列出 `default`
- 彈出視窗沿用 0006 的規則：在它裡面只有它自己的按鍵有作用；在文字輸入框中 `T` 會輸入 `T`；開著另一個彈出視窗時 `T` 什麼也不做

<a id="errors"></a>
### 錯誤

與 0008 相同（結束碼 2、什麼都不啟動、在 stderr 輸出一行），只從 TUI（獨立模式或連上的）發出（決策 5）：

| 問題 | 訊息 |
|---|---|
| TOML 語法 | `…/theme.toml: line 3, column 9: <parser message>` |
| 佈景主題或它的調色盤中有未知的欄位 | `…/theme.toml: themes[1]: unknown field "colour"` / `…/theme.toml: themes[1].palette: unknown colour "orange"`（項目從 0 開始計數） |
| 未知的元件 | `…/theme.toml: themes[1].component_style: unknown style "blok_title"` |
| 樣式中有未知的欄位 | `…/theme.toml: themes[1].component_style.border: unknown field "colour", expected fg, bg or modifiers` |
| 錯誤的顏色 | `…/theme.toml: themes[1].palette.red: invalid colour "#ff55"` / `…/theme.toml: themes[1].component_style.border.fg: invalid colour "Purpel" (a palette name such as BrightBlack, or #rrggbb)` |
| 錯誤的修飾符 | `…/theme.toml: themes[1].component_style.selection: unknown modifier "Blink"` |
| 沒有名稱、名稱是空的，或名稱已被使用 | `…/theme.toml: themes[3]: missing name` / `…: themes[3]: empty name` / `…: themes[3]: "dracula" is already themes[1]` |
| 未知的 `theme` | `…/app.toml: unknown theme "draculla" (themes: default, dracula, gruvbox_dark)`；來自變數時是 `TIDAL_PLAYER_THEME: unknown theme …`，來自旗標時是 `--theme: unknown theme …` |
| `theme` 的型別錯誤 | `…/app.toml: invalid theme: expected a string, got 5`（每個行程都會，如同任何 `app.toml` 的鍵） |
| 無法讀取的檔案 | `…/theme.toml: <io error>` |

缺少 `theme.toml` 不是錯誤。UTF-8 位元組順序標記（BOM）與 CRLF 換行都能正常讀取（0008）。

<a id="acceptance-criteria"></a>
## 驗收條件

佈景主題（`tidal_player_core::ui::theme`，純邏輯；這裡沒有 TOML，也沒有 ratatui：它接收已經反序列化的項目，並給出自己的顏色與樣式型別）：

- **AC1** — 顏色語法（表格）：調色盤的值：每個 ratatui 顏色名稱，包含大小寫、空格、`-`、`_`、`bright`/`light`、`grey`/`silver` 的各種變體、`reset`、`0` 與 `255`、大小寫的 `#rrggbb`；被拒絕的：`""`、`#fff`、`#ggg000`、`256`、`orange`。樣式的顏色：十六個調色盤名稱的每種可接受的拼法，以及 `#rrggbb`；被拒絕的：`Purpel`、`Reset`、`7`。修飾符：八個，不分大小寫；被拒絕的：`Blink`。`tidal-player` 中的一個測試會檢查，在每一列上，核心的調色盤解析得到的結果與 ratatui 的 `Color::from_str` 相同
- **AC2** — 建立佈景主題清單（表格）：沒有項目 → `[default]`；項目依檔案順序接在 `default` 之後；名為 `default` 的項目取代內建的並仍排第一；部分的調色盤對每個沒寫的項目保留 ANSI 顏色；元件顏色經由該佈景主題的調色盤解析，十六進位直接使用；沒寫的元件取「預設值」欄（現在的外觀，決策 1），而 `error` 預設值中的 `Red` 經由該佈景主題的調色盤解析；有設定的樣式整個取代預設值；`background`/`foreground` 給出畫面的樣式；「錯誤」下 `theme.toml` 的每個錯誤，附上它的索引與路徑；四個被略過的 spotify-player 元件 → 通知清單，各一次，依順序
- **AC3** — `default` 佈景主題就是現在的外觀：它解析為「預設值」欄（表格測試），且每個既有的繪製與樣式測試（`crates/app/src/tui/render.rs`、`ui/pages.rs` 與它們的快照）都**不更動**地執行，並在它之下維持綠燈
- **AC4** — 選擇佈景主題（表格）：預設 `default`；`app.toml`；變數優先於設定檔；旗標優先於兩者；空的變數往下一層；每個來源的未知名稱，以及空的名稱 → 指出來源與已知名稱的錯誤
- **AC5** — Themes 彈出視窗（模型，表格）：`T` 在每個頁面開啟它，游標在使用中的佈景主題上，`●` 也在它上面；清單指令移動並把 `State` 的佈景主題改成游標所在的佈景主題；`Enter` 保留它並關閉；`Esc` 與 `q` 還原先前的佈景主題並關閉；在它裡面沒有其他按鍵有作用；`T` 在每個文字輸入框中輸入 `T`，開著另一個彈出視窗時什麼也不做；在 `keymap.toml` 重新綁定 `SwitchTheme` 有效；`SwitchTheme` 不再在不支援的清單中；按鍵說明在 `App` 下列出 `T  choose a theme`，從彈出視窗開啟時有 `Popup · Themes` 區段
- **AC6** — 通知：含有被略過元件的 `theme.toml` 給出 TUI 的啟動通知；同時有按鍵對應的通知時兩者都顯示，按鍵對應的在前，以 ` · ` 連接

載入（`tidal-player`）：

- **AC7** — 檔案（`assert_cmd`，一個暫時的設定目錄）：沒有 `theme.toml` 就以 `default` 執行；壞掉的 `theme.toml`，或 `app.toml` 中未知的 `theme`，讓 `tidal-player` 在進入 raw mode 之前以結束碼 2 結束並把訊息輸出到 stderr（乾淨的 stderr，沒有跳脫序列）；`tidal-player daemon` 與 `tidal-player play …` 在相同壞掉的 `theme.toml` 與未知的 `theme` 下仍能啟動（它們不讀取它）；`theme = 5` 讓三者都以結束碼 2 結束
- **AC8** — 樣式對應（表格）：核心的顏色（每個 ANSI 顏色、索引、RGB、reset）與修飾符對應到 ratatui 的

繪製（`tidal-player`，**含樣式**的緩衝區 `insta` 快照，也就是 `TestBackend` 緩衝區的 `Debug`，人工檢視，另加儲存格斷言）：

- **AC9** — 元件：以一個讓每個元件與 `background`/`foreground` 各有不同顏色的測試佈景主題，在 80×24 下，各有一個帶樣式的快照：正在播放的佇列頁（播放視窗、目前項目、一個建議、游標、每隔一列）、焦點在 *Albums* 的音樂庫（有焦點與沒有焦點的標題與邊框、一個分頁名稱、`♥`、沒有焦點的游標）、一個 mix 頁（副標題）、輸入框有焦點與最佳結果被選取的搜尋頁、一個失敗的頁面、動作彈出視窗、按鍵說明、Themes 彈出視窗、按鍵提示。對每個元件，一個儲存格斷言會在表格所說它被繪製的地方找到它的顏色；沒有任何元件繪製的儲存格是 `background`/`foreground`
- **AC10** — 疊加的提示（表格，以 `selection`、`block_title`、`border`、`current_playing` 與 `prompt` 只設定顏色的佈景主題做儲存格斷言）：沒有焦點的視窗的標題與邊框變淡；無法播放的列變淡；沒有焦點的視窗中的游標列變淡；有焦點的視窗與彈出視窗的標題變粗；輸入佇列篩選時佇列的標題變粗；搜尋輸入框有焦點時是粗體，沒有焦點時是淡色、非粗體
- **AC11** — 預覽：在 Themes 彈出視窗中移動游標並繪製，得到游標所在佈景主題的畫面；`Esc` 並繪製，再次得到第一個佈景主題的畫面（儲存格斷言）
- **AC12** — 以 AC9 的測試佈景主題，從 0×0 到 120×40 都不會 panic（0004 AC17 的測試，新增列）

接線：

- **AC13** — TUI 的 `State` 從檔案取得它的佈景主題清單與使用中的佈景主題（`configured_tui_state`，表格：預設值；一個 `theme.toml` 與 `theme`；變數優先於設定檔）；連上的 TUI 與獨立模式的 TUI 以相同方式建立它，所以兩個使用不同設定目錄的連上的 TUI 會使用不同的佈景主題。`Command` 與 `Event`（`tidal_player_core::protocol`）不變（驗收時檢查：`protocol.rs` 的 `git diff` 是空的）

文件與範例：

- **AC14** — `examples/theme.toml` 內有一個 `spotify_player` 佈景主題，把每個元件設為「spotify-player 的預設值」欄（要複製來得到 spotify-player 外觀的區塊），範圍以 [`docs/Eastereggs/67055c40c1e2.md`](../../Eastereggs/67055c40c1e2.md) 允許的為限，然後是它允許的 spotify-player 範例佈景主題（它的 `examples/theme.toml`，MIT，附一行註明出處），並且載入時沒有錯誤也沒有通知（測試，同時也檢查 `spotify_player` 佈景主題與該欄相符）；`examples/app.toml` 新增 `theme = "default"`（0008 AC18 的測試維持通過）
- **AC16**–**AC20** — 在 [`docs/Eastereggs/67055c40c1e2.md`](../../Eastereggs/67055c40c1e2.md) 中
- **AC15** — `docs/config.md` 記載 `theme.toml`（格式、調色盤、每個元件與它畫在哪裡以及兩種預設值、顏色、修飾符、提示、錯誤、被略過的 spotify-player 名稱）、`theme` 鍵、`TIDAL_PLAYER_THEME` 與 `--theme`，並把 `SwitchTheme` 從不支援的指令中移除；`docs/tui.md` 新增「Themes」與 `T` 鍵；`README.md` 提到佈景主題；各自的 zh-TW 版本。一個測試檢查每個元件都出現在 `docs/config.md` 中（而 0008 AC17 的檢查 `SwitchTheme` 與 `T`）。`CLAUDE.md` 的「Features」新增條目

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 沒有 `theme.toml`，也沒有 `theme` | `default`：現在的畫面 |
| `theme.toml` 沒有 `[[themes]]`（空的，只有註解） | 只有 `default`；沒有錯誤 |
| 既沒有調色盤也沒有樣式的佈景主題 | 現在的外觀，如同 `default` |
| 只有調色盤的佈景主題（spotify-player 的 `dracula`） | 現在的外觀，只是 `background`、`foreground` 與 `red`（錯誤）改變：其他調色盤顏色只被點名它們的樣式使用（決策 1） |
| `selection` 與列的顏色相同的佈景主題 | 游標看不見；那是該佈景主題的問題，如同 spotify-player（不修正） |
| `theme.toml` 壞掉，常駐程式在執行 | 常駐程式繼續播放；TUI 拒絕啟動並顯示訊息（決策 5） |
| `theme` 指名了連上的 TUI 的 `theme.toml` 沒有的佈景主題 | 該 TUI 以結束碼 2 結束；常駐程式與其他 TUI 不受影響 |
| 只有 16 或 256 色的終端機 | 十六進位顏色原樣送出（決策 3） |
| 設定了 `NO_COLOR` | 不讀取（不在範圍內） |
| 專輯封面（0016） | 如先前繪製；佈景主題不影響它 |
| 極小的終端機中的 Themes 彈出視窗 | 與每個彈出視窗一樣被裁切；不會 panic（AC12） |
| 播放器中斷連線時 Themes 彈出視窗是開著的 | 彈出視窗保持；佈景主題是 TUI 自己的 |

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名（`ac5_…`）。紅燈是針對存根型別與函式、其本體為存根（沒有 `todo!()`、沒有編譯錯誤）的斷言失敗，如同 0008。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/theme/tests.rs` :: `ac1_palette_colours`, `ac1_style_colours`, `ac1_modifiers` (tables) + `crates/app/src/tui/render/theme.rs` :: `ac1_palette_matches_ratatui` | 解析出的值與拒絕；與 ratatui 一致 | 存根解析器只接受 `#rrggbb` |
| AC2 | `crates/core/src/ui/theme/tests.rs` :: `ac2_build_themes` (table) | 順序、取代、後備、錯誤、通知 | 存根 `build` 回傳 `[default]` 並忽略項目 |
| AC3 | `crates/core/src/ui/theme/tests.rs` :: `ac3_default_is_todays_look` + the existing tests in `crates/app/src/tui/render.rs` (unchanged) | 預設值欄；舊的畫面 | 存根 `default` 佈景主題是空的，所以 `ac3_default_is_todays_look` 失敗（舊的測試是重構的安全網，不是它的紅燈） |
| AC4 | `crates/app/src/config.rs` :: `ac4_theme_precedence` (table) | 選到的名稱或錯誤 | 存根永遠選 `default` |
| AC5 | `crates/core/src/ui/theme/tests.rs` :: `ac5_themes_popup` (table), `ac5_switch_theme_key`, `ac5_help_lists_switch_theme` | 彈出視窗狀態、使用中的佈景主題、按鍵 | 存根 `SwitchTheme` 什麼也不做 |
| AC6 | `crates/core/src/ui/theme/tests.rs` :: `ac6_theme_notice`, `ac6_both_notices` | 通知文字 | 存根丟掉佈景主題的通知 |
| AC7 | `crates/app/tests/config.rs` :: `ac7_broken_theme_exits_2`, `ac7_unknown_theme_exits_2`, `ac7_daemon_ignores_theme_toml`, `ac7_theme_wrong_type_exits_2`, `ac7_missing_theme_toml_runs` | 結束碼、stderr | 存根載入器忽略 `theme.toml` |
| AC8 | `crates/app/src/tui/render/theme.rs` :: `ac8_style_mapping` (table) | ratatui 的顏色與修飾符 | 存根把每個顏色對應到 `Reset` |
| AC9 | `crates/app/src/tui/render.rs` :: `ac9_theme_queue_80x24`, `ac9_theme_library_80x24`, `ac9_theme_mix_80x24`, `ac9_theme_search_80x24`, `ac9_theme_failed_80x24`, `ac9_theme_popups_80x24`, `ac9_theme_components` | 帶樣式的快照；每個元件被繪製處的顏色 | 存根繪製忽略佈景主題 |
| AC10 | `crates/app/src/tui/render.rs` :: `ac10_cues_on_top` (table) | 每個提示的淡色／粗體 | 存根繪製只套用佈景主題的樣式 |
| AC11 | `crates/app/src/tui/render.rs` :: `ac11_preview_draws_theme` | 預覽佈景主題中的儲存格 | 存根繪製讀取的是設定的佈景主題，不是預覽的佈景主題 |
| AC12 | `crates/app/src/tui/render.rs` :: `ac17_no_panic_any_size` (new rows) | 不會 panic | 無：既有安全網測試的新列，在存根上也會通過（存根不可能在真實程式碼可能 panic 的地方 panic）；它們防護綠燈的程式碼 |
| AC13 | `crates/app/src/main.rs` :: `ac13_tui_state_theme` (table) | `State` 的佈景主題與使用中的佈景主題 | 存根 `configured_tui_state` 忽略佈景主題 |
| AC14 | `crates/app/tests/examples.rs` :: `ac14_example_theme_toml_loads`, `ac18_example_app_toml_is_the_defaults` (existing) | 載入、沒有通知；`theme` 存在 | 範例檔案是空的、已 commit |
| AC16–AC20 | in [`docs/Eastereggs/67055c40c1e2.md`](../../Eastereggs/67055c40c1e2.md) | | |
| AC15 | `crates/app/tests/docs.rs` :: `ac15_every_component_documented` + `ac17_every_command_documented` (existing) + reviewed at acceptance | `docs/config.md` 中的名稱 | 文件缺少這些元件 |

驗收時在使用者的機器上手動檢查（結果寫在 PR 說明中）：複製 spotify-player 的 `examples/theme.toml`，在使用者的終端機中以 `T` 切換它的各個佈景主題；在連上常駐程式的 TUI 中以 `app.toml` 的 `theme =` 設定佈景主題，同時另一個連上的 TUI 維持 `default`。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::ui::theme`：`Color`（reset、十六種 ANSI 顏色、索引、RGB）、`Modifiers`、`Style`（`fg`、`bg`、`modifiers`、`patch`）、`Component`、`Theme`（名稱、畫面樣式、每個元件一個已解析的 `Style`）、項目型別（`ThemeEntry`、調色盤與樣式項目，衍生 `Deserialize` 讓執行檔直接把 TOML 反序列化成它們，未知的名稱保留給上述檢查使用）、`build(entries) -> Result<Themes, ThemeError>`（附通知）、顏色與修飾符的解析器；`State` 持有佈景主題、使用中的佈景主題與彈出視窗的佈景主題（`Popup::Themes`）；`UiCommand::SwitchTheme`，預設按鍵 `T`。沒有 I/O，沒有新的相依套件
- `tidal-player`：`config`（讀取 `theme.toml`、`theme` 鍵、`TIDAL_PLAYER_THEME`、`-t/--theme`）、`ui::theme`（核心樣式 → ratatui 樣式）、經由使用中的佈景主題繪製每個元件
- `xtask layering`：沒有變更

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-24）：

- spotify-player（如上，master `7dc9d17`）：`theme.toml` 的欄位、調色盤的預設值（十六種 ANSI 顏色；`white` 是 ratatui 的 `Gray`，`bright_black` 是 `DarkGray`，`bright_white` 是 `White`）、經由 ratatui 的 `Color::from_str` 取得的調色盤值、元件顏色為 `Black` … `BrightYellow`（大小寫須完全一致）或 `#rrggbb`、八個修飾符、每個元件與它的預設值如「spotify-player 的預設值」欄、未知的欄位被忽略、與既有佈景主題同名的檔案佈景主題被略過、`SwitchTheme` 的彈出視窗（目前的佈景主題在最前面、游標套用、`Enter` 保留、`Esc` 回到第一個）、`secondary_row` 只用於說明表格的交替列、以 `theme`/`-t` 選擇
- ratatui-core 0.1.2 的 `Color::from_str`（本工作區建置所用的版本）：轉成小寫，去掉空格、`-` 與 `_`，把 `bright` 對應到 `light`、`grey`/`silver` 對應到 `gray`、`lightblack` 對應到 `darkgray`、`lightwhite`/`lightgray` 對應到 `white`；然後是名稱、`u8` 索引，或 `#rrggbb`
- 本儲存庫：TUI 中的每個樣式都在 `crates/app/src/tui/render.rs` 與 `ui/pages.rs`（粗體、淡色、反白、紅色），快照只記錄文字，`T` 未綁定，`SwitchTheme` 在 `UNSUPPORTED_COMMANDS` 中，`tidal-player-core` 不可相依於 ratatui（`cargo xtask layering`）

假設：沒有測試所依賴的假設。使用者的終端機是否顯示十六進位顏色與 `RapidBlink`，於驗收時手動檢查。

<a id="decisions-answered-by-the-user-2026-10-24-1-as-the-alternative-27-as-proposed"></a>
## 決策（使用者已回答，2026-10-24：1 選了替代方案，2–7 照提案）

1. **`theme.toml` 佈景主題沒寫的部分**：*回答：替代方案*：每個佈景主題都是現在的外觀，所以只有調色盤的佈景主題（例如 spotify-player 的 `dracula`）只會改變背景、前景與錯誤的紅色；spotify-player 的預設元件樣式放在 `examples/theme.toml` 供複製。提案是對檔案佈景主題沒寫的元件使用 spotify-player 的預設值
2. **圖示、邊框與進度條的樣式**（`play_icon`、`pause_icon`、`liked_icon`、`border_type`、`progress_bar_type`）：*回答：照提案*：另立規格。替代方案：放在這一份
3. **全彩（truecolor）**：*回答：照提案*：十六進位顏色原樣送出（spotify-player 的行為）。替代方案：當 `COLORTERM` 沒有說 `truecolor`/`24bit` 時，把它們近似成 256 色
4. **記住以 `T` 選的佈景主題**：*回答：照提案*：不記，它只持續到這次執行，如同以 `D` 選的裝置（0014）；`app.toml` 的 `theme` 讓它成為永久的。替代方案：在狀態目錄中依各 TUI 記住它（0009）
5. **誰讀取 `theme.toml`**：*回答：照提案*：只有 TUI（常駐程式什麼都不畫，顏色上的拼字錯誤不該讓音樂停止）；`app.toml` 中的 `theme` 仍由每個行程檢查型別。替代方案：每個行程都驗證 `theme.toml`，如同 0008 對 `keymap.toml` 所做的
6. **名稱**：*回答：照提案*：名為 `default` 的檔案佈景主題取代內建的；兩個檔案佈景主題同名是錯誤；未知的名稱是錯誤，但我們不畫的那四個 spotify-player 元件除外。替代方案：spotify-player 的做法（同名的第一個勝出，未知的欄位被忽略）
7. **兩個我們自己的元件**：*回答：照提案*：`error`（現在是紅色）與 `prompt`（現在是粗體），因為 spotify-player 兩者都沒有；`table_header` 為按鍵說明的標題上色（我們沒有表格標題）。替代方案：讓錯誤與輸入提示不納入佈景主題

決策 8–16 在 [`docs/Eastereggs/67055c40c1e2.md`](../../Eastereggs/67055c40c1e2.md) 中

<a id="out-of-scope"></a>
## 不在範圍內

- 圖示、邊框類型、進度條類型（決策 2）
- 近似十六進位顏色（決策 3）；讀取 `NO_COLOR`
- 記住以 `T` 選的佈景主題（決策 4）
- 即時重新載入 `theme.toml`；列出佈景主題的指令
- 在佇列以外的清單中標記正在播放的曲目（spotify-player 在所有地方都有的 `current_playing`）
- 以 spotify-player 的預設元件樣式作為後備（決策 1）
- spotify-player 的 `theme_parse` 腳本，以及從終端機配色方案建立的佈景主題

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-24"></a>
## 實作備註（規格沒有說明之處所做的選擇，2026-10-24）

- **解析**：調色盤的值逐步遵循 ratatui-core 0.1.2 的 `Color::from_str`，包含它的怪癖（`+7` 是索引 7，`light gray` 是 `White`）；`ac1_palette_matches_ratatui` 以 ratatui 檢查每一列，以及每個調色盤鍵的每種拼法。樣式的顏色接受十六個名稱，忽略大小寫、空格、`-` 與 `_`，`#rrggbb` 以同樣方式讀取；修飾符只忽略大小寫（`rapid_blink` 被拒絕）。TOML 型別錯誤的值（`red = 5`、`modifiers = "Bold"`），以及 `theme.toml` 最上層未知的鍵，是解析器的錯誤，附行與欄，如同 `keymap.toml`
- **錯誤**：依這個順序逐一項目檢查：未知的欄位、名稱（缺少、空的、已被使用）、依檔案順序的調色盤，然後是依檔案順序的 `component_style`（每個樣式：未知的欄位、`fg`、`bg`、依序的修飾符）；回報第一個被拒絕的。被略過的 spotify-player 元件的值完全不讀取（`visualization` 是 `low`/`mid`/`high` 的表格）；未知的樣式的也不讀取
- **選擇**：`--theme`/`-t` 是 TUI 的選項（不是全域的，如同項目），所以 `tidal-player -t NAME daemon` 會被 CLI 拒絕。`app.toml` 中空的 `theme = ""` 與空的 `--theme ""` 是未知的名稱（`unknown theme ""`）；空的 `TIDAL_PLAYER_THEME` 視同未設定。`theme.toml` 的讀取與名稱的檢查在選擇角色之前於 `main` 中進行，所以拒絕發生在鎖、socket 與 raw mode 之前
- **提示**：有焦點的搜尋輸入框是加上 `Bold` 的 `prompt`（一個 TUI 的提示，如同有焦點的視窗標題；2026-10-24 由 tech-lead 確認），所以 `prompt` 只設定顏色的佈景主題仍會顯示焦點（AC10）；沒有焦點時是去掉 `Bold` 的 `prompt`，再加上 `Dim`。`secondary_row` 畫在清單視窗與佇列的第 2、4……列，在顯示出來的列中計算（篩選的符合項目；`Suggested` 分隔線與狀態列不計）；彈出視窗與按鍵說明沒有它。作用中分頁的名稱是疊上 `selection` 的視窗標題樣式。彈出視窗的列、按鍵說明、輸入提示與按鍵提示的區域，在繪製之前都會清成畫面的樣式
- **標頭**：標題側與指示之間的空隙以畫面的樣式繪製（它原本和該列其餘部分一起是粗體）；那裡只有空格，所以畫面看起來相同。被截斷的列保留各部分的樣式，`…` 使用保留的最後一部分的樣式。既有 UI 測試所繪製的每個緩衝區中的其他每個儲存格，符號與樣式都和以前相同（實作時以傾印並比對全部來檢查，共 1,963 個緩衝區）
- **Themes 彈出視窗**：`Popup::Themes { cursor, active }`；游標隨清單指令移動（`State::theme` 跟著它），`Enter` 保留，`Esc` 與 `q` 還原 `active`。它的按鍵說明區段是 `Popup · Themes`：`q` "back to the theme in use"、`enter` "keep the selected theme"、`esc` "close / cancel"。`SwitchTheme` 在指令表中排在 `ClosePopup` 之後，所以 `App` 依序列出 `T`、`?`、`q`。它永遠不會變淡（佈景主題不需要播放器）
- **範例**：`examples/theme.toml` 複製了一部分 spotify-player 的範例佈景主題，沒有被略過的元件，所以檔案載入時沒有通知（AC14）；`examples/keymap.toml` 新增 `SwitchTheme`/`T` 項目（0008 AC18）
- **測試**：`ac7_daemon_ignores_theme_toml` 與 `ac7_missing_theme_toml_runs` 在存根上也通過（忽略 `theme.toml` 的載入器不可能拒絕任何東西）；它們防護綠燈的程式碼。把 `SwitchTheme` 當作被略過的名稱或舊預設值的測試（`ac12_unsupported_notice`，它現在改為略過 `SortTrackByTitle`、`ac2_build_keymap`、按鍵說明的 `APP` 列、`ac14_help_custom_keymap_end` 快照，它現在顯示 `T  choose a theme`）已依本規格更新
